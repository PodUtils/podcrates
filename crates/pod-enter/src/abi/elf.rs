use super::*;

impl Elf {
    /// Read `path`, or say why not.
    ///
    /// ⚠ Bounded at 128 MiB. `AGENTS.md`: a runtime whose audience is
    /// automated may not assume a file it was pointed at is the size it
    /// expected, and `libc.so.6` is two.
    pub fn read(path: &str) -> Result<Elf, Unreadable> {
        const CEILING: u64 = 128 * 1024 * 1024;
        let len = std::fs::metadata(path)
            .map_err(|e| Unreadable(format!("{path}: {e}")))?
            .len();
        if len > CEILING {
            return Err(Unreadable(format!(
                "{path} is {len} bytes, over the {CEILING}-byte ceiling podvm \
                 reads an ELF file within"
            )));
        }
        let bytes = std::fs::read(path).map_err(|e| Unreadable(format!("{path}: {e}")))?;
        Elf::parse(path, &bytes)
    }

    /// The same, from bytes already in hand, which is what the tests drive.
    pub fn parse(path: &str, b: &[u8]) -> Result<Elf, Unreadable> {
        let why = |s: String| Unreadable(format!("{path}: {s}"));
        if b.len() < 64 || &b[0..4] != b"\x7fELF" {
            return Err(why("not an ELF file: the magic is not \\x7fELF".into()));
        }
        let class64 = match b[4] {
            1 => false,
            2 => true,
            c => return Err(why(format!("unknown ELF class {c}"))),
        };
        let msb = match b[5] {
            1 => false,
            2 => true,
            d => return Err(why(format!("unknown ELF data encoding {d}"))),
        };
        let r = Bytes { b, msb, class64 };

        let machine = r.u16(18).ok_or_else(|| why("truncated header".into()))?;
        let interp = r.interp();
        let sections = r.sections().ok_or_else(|| {
            why(
                "no section table podvm could read. refused: podvm refuses rather \
                 than preloading an object it could not check"
                    .into(),
            )
        })?;

        let mut out = Elf {
            path: path.to_string(),
            machine,
            interp,
            needed: Vec::new(),
            soname: None,
            sections: sections.by_name.keys().cloned().collect(),
            defined: Vec::new(),
            imported: Vec::new(),
            declares: Vec::new(),
        };

        // `.dynamic`, for DT_NEEDED and DT_SONAME.
        if let Some(dynamic) = sections.get(".dynamic") {
            let strs = sections
                .by_index(dynamic.link as usize)
                .map(|s| s.slice(b))
                .unwrap_or_default();
            let step = if class64 { 16 } else { 8 };
            let body = dynamic.slice(b);
            let mut at = 0usize;
            while at + step <= body.len() {
                let d = Bytes {
                    b: &body[at..],
                    msb,
                    class64,
                };
                let (tag, val) = if class64 {
                    (d.u64(0).unwrap_or(0) as i64, d.u64(8).unwrap_or(0))
                } else {
                    (
                        d.u32(0).unwrap_or(0) as i32 as i64,
                        u64::from(d.u32(4).unwrap_or(0)),
                    )
                };
                match tag {
                    0 => break, // DT_NULL
                    1 => {
                        if let Some(s) = cstr(strs, val as usize) {
                            out.needed.push(s);
                        }
                    }
                    14 => out.soname = cstr(strs, val as usize),
                    _ => {}
                }
                at += step;
            }
        }

        // `.dynsym`, its strings, and the three version sections.
        if let Some(dynsym) = sections.get(".dynsym") {
            let strs = sections
                .by_index(dynsym.link as usize)
                .map(|s| s.slice(b))
                .unwrap_or_default();
            let versym = sections.get(".gnu.version").map(|s| s.slice(b));
            let need = sections
                .get(".gnu.version_r")
                .map(|s| r.verneed(s.slice(b), strs))
                .unwrap_or_default();
            let def = sections
                .get(".gnu.version_d")
                .map(|s| r.verdef(s.slice(b), strs))
                .unwrap_or_default();
            out.declares = def.values().cloned().collect();
            out.declares.sort();
            out.declares.dedup();

            let step = if class64 { 24 } else { 16 };
            let body = dynsym.slice(b);
            let mut i = 0usize;
            while (i + 1) * step <= body.len() {
                let s = Bytes {
                    b: &body[i * step..],
                    msb,
                    class64,
                };
                let name_off = s.u32(0).unwrap_or(0) as usize;
                // ⚠ `st_info` and `st_shndx` sit at different offsets in the two
                // classes, which is the whole reason this reader is class-aware
                // rather than a 64-bit one with a comment.
                let (info_at, shndx_at) = if class64 { (4, 6) } else { (12, 14) };
                let shndx = s.u16(shndx_at).unwrap_or(0);
                // `STB_WEAK` is binding 2, in the high nibble of `st_info`.
                let weak = s.b.get(info_at).map(|i| i >> 4) == Some(2);
                let ndx = versym
                    .and_then(|v| Bytes { b: v, msb, class64 }.u16(i * 2))
                    .unwrap_or(1);
                let Some(name) = cstr(strs, name_off) else {
                    i += 1;
                    continue;
                };
                if name.is_empty() {
                    i += 1;
                    continue;
                }
                // ⚠ 0 is VER_NDX_LOCAL and 1 is VER_NDX_GLOBAL; neither names a
                // version. The high bit is the "hidden" flag and not part of the
                // index.
                let key = ndx & 0x7fff;
                let (version, file) = if key < 2 {
                    (None, None)
                } else if shndx == 0 {
                    match need.get(&key) {
                        Some((v, f)) => {
                            (Some(v.clone()), Some(f.clone()).filter(|f| !f.is_empty()))
                        }
                        None => (None, None),
                    }
                } else {
                    (def.get(&key).cloned(), None)
                };
                let v = Versioned {
                    name,
                    version,
                    weak,
                    file,
                };
                if shndx == 0 {
                    out.imported.push(v);
                } else {
                    out.defined.push(v);
                }
                i += 1;
            }
        }
        Ok(out)
    }

    /// Which libc this file belongs to.
    ///
    /// ⭐ For a **library**, the answer is its own SONAME: glibc's declares
    /// `libc.so.6` and musl's declares none. For anything else it is what the
    /// file asks for in `DT_NEEDED`, and for an executable its `PT_INTERP`
    /// settles it first, because the loader named there is the one that will
    /// actually run it.
    pub fn flavour(&self) -> Flavour {
        if let Some(i) = &self.interp {
            match Flavour::of_libc_name(i) {
                Flavour::Unknown => {}
                f => return f,
            }
        }
        if let Some(s) = &self.soname {
            match Flavour::of_libc_name(s) {
                Flavour::Unknown => {}
                f => return f,
            }
        }
        // ⚠ musl's libc declares no SONAME, so a file with none, no interpreter
        // and no libc in DT_NEEDED is identified by the name it was read under.
        match Flavour::of_needed(&self.needed) {
            Flavour::Unknown => match Flavour::of_libc_name(&self.path) {
                // ⭐ And a last answer from the CONTENT rather than the name,
                // for a libc copied out of an image under some other filename.
                // glibc's declares `GLIBC_*` version definitions; musl's
                // declares none at all and is its own program interpreter, so
                // it defines the entry point every C program starts at.
                Flavour::Unknown => {
                    if self.declares.iter().any(|d| d.starts_with("GLIBC_")) {
                        Flavour::Glibc
                    } else if self.declares.is_empty()
                        && self.defines(&Versioned {
                            name: "__libc_start_main".into(),
                            version: None,
                            weak: false,
                            file: None,
                        })
                    {
                        Flavour::Musl
                    } else {
                        Flavour::Unknown
                    }
                }
                f => f,
            },
            f => f,
        }
    }

    pub(super) fn defines(&self, want: &Versioned) -> bool {
        self.defined.iter().any(|d| d.name == want.name)
    }
}
