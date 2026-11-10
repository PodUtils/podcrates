use super::*;

impl Elf {
    /// Read `path`, or say why not.
    ///
    /// A bounded read: the header, the two header tables and the sections
    /// that the checks read, each one with `pread`, and never the whole
    /// file. The size of the file has no bound; the bytes held have the
    /// bound [`READ_CEILING`].
    pub fn read(path: &str) -> Result<Elf, Unreadable> {
        let why = |e: std::io::Error| Unreadable(format!("{path}: {e}"));
        let file = std::fs::File::open(path).map_err(why)?;
        let len = file.metadata().map_err(why)?.len();
        let r = Image::open(Src::file(&file, len)).map_err(|s| Unreadable(format!("{path}: {s}")))?;
        Elf::from_image(path, r)
    }

    /// The same, from bytes in hand (the tests use it).
    pub fn parse(path: &str, b: &[u8]) -> Result<Elf, Unreadable> {
        let r = Image::open(Src::Mem(b)).map_err(|s| Unreadable(format!("{path}: {s}")))?;
        Elf::from_image(path, r)
    }

    fn from_image(path: &str, r: Image<'_>) -> Result<Elf, Unreadable> {
        let why = |s: String| Unreadable(format!("{path}: {}", r.src.trouble().unwrap_or(s)));
        let (msb, class64) = (r.msb, r.class64);
        let machine = r.machine().ok_or_else(|| why("truncated header".into()))?;
        let sections = r.sections().ok_or_else(|| {
            why("no section table could be read. refused: an object that cannot be \
                 checked is not preloaded"
                .into())
        })?;
        let interp = r.interp().map_err(why)?;

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
            let strs = sections.by_index(dynamic.link).map(|s| r.bytes(s)).unwrap_or_default();
            let step = if class64 { 16 } else { 8 };
            let body = r.bytes(dynamic);
            let mut at = 0usize;
            while at + step <= body.len() {
                let d = Bytes { b: &body[at..], msb, class64 };
                let (tag, val) = if class64 {
                    (d.u64(0).unwrap_or(0) as i64, d.u64(8).unwrap_or(0))
                } else {
                    (d.u32(0).unwrap_or(0) as i32 as i64, u64::from(d.u32(4).unwrap_or(0)))
                };
                match tag {
                    0 => break, // DT_NULL
                    1 => {
                        if let Some(s) = cstr_at(&strs, val) {
                            out.needed.push(s);
                        }
                    }
                    14 => out.soname = cstr_at(&strs, val),
                    _ => {}
                }
                at += step;
            }
        }

        // `.dynsym`, its strings, and the three version sections.
        if let Some(dynsym) = sections.get(".dynsym") {
            let strs = sections.by_index(dynsym.link).map(|s| r.bytes(s)).unwrap_or_default();
            let versym = sections.get(".gnu.version").map(|s| r.bytes(s));
            let need =
                sections.get(".gnu.version_r").map(|s| r.view(&[]).verneed(&r.bytes(s), &strs)).unwrap_or_default();
            let def =
                sections.get(".gnu.version_d").map(|s| r.view(&[]).verdef(&r.bytes(s), &strs)).unwrap_or_default();
            out.declares = def.values().cloned().collect();
            out.declares.sort();
            out.declares.dedup();

            let step = if class64 { 24 } else { 16 };
            let body = r.bytes(dynsym);
            let mut i = 0usize;
            while (i + 1) * step <= body.len() {
                let s = Bytes { b: &body[i * step..], msb, class64 };
                let name_off = s.u32(0).and_then(|o| usize::try_from(o).ok()).unwrap_or(0);
                // `st_info` and `st_shndx` are at other offsets in the two
                // classes, so this reader knows the class.
                let (info_at, shndx_at) = if class64 { (4, 6) } else { (12, 14) };
                let shndx = s.u16(shndx_at).unwrap_or(0);
                // `STB_WEAK` is binding 2, in the high nibble of `st_info`.
                let weak = s.b.get(info_at).map(|i| i >> 4) == Some(2);
                let ndx = versym.as_deref().and_then(|v| Bytes { b: v, msb, class64 }.u16(i * 2)).unwrap_or(1);
                let Some(name) = cstr(&strs, name_off) else {
                    i += 1;
                    continue;
                };
                if name.is_empty() {
                    i += 1;
                    continue;
                }
                // 0 is VER_NDX_LOCAL and 1 is VER_NDX_GLOBAL; neither names a
                // version. The high bit is the "hidden" flag, not a part of
                // the index.
                let key = ndx & 0x7fff;
                let (version, file) = if key < 2 {
                    (None, None)
                } else if shndx == 0 {
                    match need.get(&key) {
                        Some((v, f)) => (Some(v.clone()), Some(f.clone()).filter(|f| !f.is_empty())),
                        None => (None, None),
                    }
                } else {
                    (def.get(&key).cloned(), None)
                };
                let v = Versioned { name, version, weak, file };
                if shndx == 0 {
                    out.imported.push(v);
                } else {
                    out.defined.push(v);
                }
                i += 1;
            }
        }
        // A section that a read could not give is empty above. For a file
        // that is a refusal with the reason, never a file with fewer symbols.
        match r.src.trouble() {
            Some(t) => Err(Unreadable(format!("{path}: {t}"))),
            None => Ok(out),
        }
    }

    /// The C library of this file.
    ///
    /// For a library, the answer is its own SONAME: the libc of glibc
    /// declares `libc.so.6`, and the libc of musl declares none. For each
    /// other file it is what the file asks for in `DT_NEEDED`, and for an
    /// executable its `PT_INTERP` decides first, because the loader named
    /// there is the one that runs it.
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
        // The libc of musl declares no SONAME, so a file with none, no
        // interpreter and no libc in DT_NEEDED is known by its file name.
        match Flavour::of_needed(&self.needed) {
            Flavour::Unknown => match Flavour::of_libc_name(&self.path) {
                // A last answer from the content, not the name, for a libc
                // copied out of an image under another file name. The libc of
                // glibc declares `GLIBC_*` versions; the libc of musl declares
                // none and is its own program interpreter, so it defines the
                // entry point of each C program.
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
