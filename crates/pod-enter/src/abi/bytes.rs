use super::*;

// The bytes. Each read checks its bounds and gives an `Option`, and each
// offset of the file goes through checked arithmetic: this module reads
// files of an image of another party, and a short or hostile file must
// give a refusal, not a panic, also on a 32-bit target where an offset of
// 64 bits does not fit a `usize`.

pub(super) struct Bytes<'a> {
    pub(super) b: &'a [u8],
    pub(super) msb: bool,
    pub(super) class64: bool,
}

/// `base + i * size` as an index into the file, or `None` where it does not
/// fit a `usize`.
pub(super) fn offset(base: u64, i: usize, size: u16) -> Option<usize> {
    usize::try_from(base).ok()?.checked_add(i.checked_mul(usize::from(size))?)
}

impl Bytes<'_> {
    pub(super) fn u16(&self, at: usize) -> Option<u16> {
        let s: [u8; 2] = self.b.get(at..at.checked_add(2)?)?.try_into().ok()?;
        Some(if self.msb { u16::from_be_bytes(s) } else { u16::from_le_bytes(s) })
    }

    pub(super) fn u32(&self, at: usize) -> Option<u32> {
        let s: [u8; 4] = self.b.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(if self.msb { u32::from_be_bytes(s) } else { u32::from_le_bytes(s) })
    }

    pub(super) fn u64(&self, at: usize) -> Option<u64> {
        let s: [u8; 8] = self.b.get(at..at.checked_add(8)?)?.try_into().ok()?;
        Some(if self.msb { u64::from_be_bytes(s) } else { u64::from_le_bytes(s) })
    }

    /// A field of 8 bytes in ELF64 and of 4 bytes in ELF32.
    fn word(&self, at: usize) -> Option<u64> {
        if self.class64 { self.u64(at) } else { self.u32(at).map(u64::from) }
    }

    /// A field of 32 bits as an index into the file.
    fn index(&self, at: usize) -> Option<usize> {
        usize::try_from(self.u32(at)?).ok()
    }

    /// The contents of `PT_INTERP`, where the file has one.
    pub(super) fn interp(&self) -> Option<String> {
        let (phoff, phentsize, phnum) = if self.class64 {
            (self.u64(32)?, self.u16(54)?, self.u16(56)?)
        } else {
            (u64::from(self.u32(28)?), self.u16(42)?, self.u16(44)?)
        };
        for i in 0..usize::from(phnum) {
            let p = Bytes { b: self.b.get(offset(phoff, i, phentsize)?..)?, msb: self.msb, class64: self.class64 };
            if p.u32(0)? != 3 {
                continue; // PT_INTERP
            }
            let off = if self.class64 { p.u64(8)? } else { u64::from(p.u32(4)?) };
            return cstr(self.b, usize::try_from(off).ok()?);
        }
        None
    }

    pub(super) fn sections(&self) -> Option<Sections> {
        let (shoff, shentsize, mut shnum, mut shstrndx) = if self.class64 {
            (self.u64(40)?, self.u16(58)?, u64::from(self.u16(60)?), usize::from(self.u16(62)?))
        } else {
            (u64::from(self.u32(32)?), self.u16(46)?, u64::from(self.u16(48)?), usize::from(self.u16(50)?))
        };
        if shoff == 0 || shentsize == 0 {
            return None;
        }
        let one = |i: usize| -> Option<Section> {
            let s = Bytes { b: self.b.get(offset(shoff, i, shentsize)?..)?, msb: self.msb, class64: self.class64 };
            let (off_at, size_at, link_at) = if self.class64 { (24, 32, 40) } else { (16, 20, 24) };
            Some(Section {
                name_off: s.index(0)?,
                offset: s.word(off_at)?,
                size: s.word(size_at)?,
                link: s.u32(link_at)?,
            })
        };
        // `e_shnum` 0 and `e_shstrndx` SHN_XINDEX both mean "the real value
        // is in section 0": the form of an object with more than 0xff00
        // sections. A reader that took the fields of the header as they are
        // would read no section of such an object.
        let zero = one(0)?;
        if shnum == 0 {
            shnum = zero.size;
        }
        if shstrndx == 0xffff {
            shstrndx = usize::try_from(zero.link).ok()?;
        }
        // Each header must be inside the file, so the file bounds the
        // count, and a hostile count cannot make a large allocation.
        let shnum = usize::try_from(shnum).ok()?;
        if shnum > self.b.len() / usize::from(shentsize) {
            return None;
        }
        let all = (0..shnum).map(one).collect::<Option<Vec<_>>>()?;
        let names = all.get(shstrndx)?.slice(self.b);
        let mut by_name = BTreeMap::new();
        for s in &all {
            if let Some(n) = cstr(names, s.name_off) {
                by_name.insert(n, s.clone());
            }
        }
        Some(Sections { all, by_name })
    }

    /// `.gnu.version_d`: the index of a version and the name that it
    /// declares.
    pub(super) fn verdef(&self, body: &[u8], strs: &[u8]) -> BTreeMap<u16, String> {
        let mut out = BTreeMap::new();
        let mut at = 0usize;
        // A bound on the steps, and not only the `vd_next` chain: a hostile
        // file can make that chain a loop.
        for _ in 0..1024 {
            let d = Bytes {
                b: match body.get(at..) {
                    Some(s) if s.len() >= 20 => s,
                    _ => break,
                },
                msb: self.msb,
                class64: self.class64,
            };
            let (Some(flags), Some(ndx), Some(aux), Some(next)) = (d.u16(2), d.u16(4), d.index(12), d.index(16)) else {
                break;
            };
            // `VER_FLG_BASE` is the own SONAME of the file in the form of a
            // version entry, and no file imports it. With it, `libc.so.6`
            // would be in `declares` beside `GLIBC_2.39`, and sort after
            // each real version.
            if flags & 0x1 == 0 {
                // The first verdaux is the name of the version; the next
                // ones are the versions that it inherits.
                if let Some(name) = d.index(aux).and_then(|off| cstr(strs, off)).filter(|s| !s.is_empty()) {
                    out.insert(ndx & 0x7fff, name);
                }
            }
            match (next, at.checked_add(next)) {
                (0, _) | (_, None) => break,
                (_, Some(n)) => at = n,
            }
        }
        out
    }

    /// `.gnu.version_r`: the index of a version, the name that the file
    /// needs at it, and the file (`vn_file`) that the reference names as
    /// the provider.
    pub(super) fn verneed(&self, body: &[u8], strs: &[u8]) -> BTreeMap<u16, (String, String)> {
        let mut out = BTreeMap::new();
        let mut at = 0usize;
        for _ in 0..1024 {
            let n = Bytes {
                b: match body.get(at..) {
                    Some(s) if s.len() >= 16 => s,
                    _ => break,
                },
                msb: self.msb,
                class64: self.class64,
            };
            let (Some(cnt), Some(aux), Some(next)) = (n.u16(2), n.index(8), n.index(12)) else {
                break;
            };
            let file = n.index(4).and_then(|off| cstr(strs, off)).unwrap_or_default();
            let mut a = aux;
            for _ in 0..usize::from(cnt).min(1024) {
                let x = Bytes {
                    b: match n.b.get(a..) {
                        Some(s) if s.len() >= 16 => s,
                        _ => break,
                    },
                    msb: self.msb,
                    class64: self.class64,
                };
                let (Some(other), Some(name), Some(anext)) = (x.u16(6), x.index(8), x.index(12)) else {
                    break;
                };
                if let Some(s) = cstr(strs, name) {
                    out.insert(other & 0x7fff, (s, file.clone()));
                }
                match (anext, a.checked_add(anext)) {
                    (0, _) | (_, None) => break,
                    (_, Some(n)) => a = n,
                }
            }
            match (next, at.checked_add(next)) {
                (0, _) | (_, None) => break,
                (_, Some(n)) => at = n,
            }
        }
        out
    }
}

#[derive(Debug, Clone)]
pub(super) struct Section {
    name_off: usize,
    offset: u64,
    size: u64,
    pub(super) link: u32,
}

impl Section {
    /// The bytes of the section, or an empty slice where the section is not
    /// inside the file.
    pub(super) fn slice<'a>(&self, b: &'a [u8]) -> &'a [u8] {
        let range = usize::try_from(self.offset)
            .ok()
            .and_then(|start| Some(start..start.checked_add(usize::try_from(self.size).ok()?)?));
        range.and_then(|r| b.get(r)).unwrap_or_default()
    }
}

pub(super) struct Sections {
    all: Vec<Section>,
    pub(super) by_name: BTreeMap<String, Section>,
}

impl Sections {
    pub(super) fn get(&self, name: &str) -> Option<&Section> {
        self.by_name.get(name)
    }

    pub(super) fn by_index(&self, i: u32) -> Option<&Section> {
        self.all.get(usize::try_from(i).ok()?)
    }
}

/// A string that ends with NUL at `off` in `b`, where there is one.
pub(super) fn cstr(b: &[u8], off: usize) -> Option<String> {
    let rest = b.get(off..)?;
    let end = rest.iter().position(|c| *c == 0).unwrap_or(rest.len());
    Some(String::from_utf8_lossy(&rest[..end]).to_string())
}

/// [`cstr`] at an offset of 64 bits, which can be too large for a `usize`.
pub(super) fn cstr_at(b: &[u8], off: u64) -> Option<String> {
    cstr(b, usize::try_from(off).ok()?)
}
