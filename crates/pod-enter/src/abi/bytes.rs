use super::*;

// ------------------------------------------------------------------ the bytes
//
// ⚠ Every read is bounds-checked and returns `Option`. This module reads files
// out of somebody else's image, and a truncated or hostile one must produce a
// refusal rather than a panic inside `podvm run`.

pub(super) struct Bytes<'a> {
    pub(super) b: &'a [u8],
    pub(super) msb: bool,
    pub(super) class64: bool,
}

impl Bytes<'_> {
    pub(super) fn u16(&self, at: usize) -> Option<u16> {
        let s: [u8; 2] = self.b.get(at..at + 2)?.try_into().ok()?;
        Some(if self.msb {
            u16::from_be_bytes(s)
        } else {
            u16::from_le_bytes(s)
        })
    }

    pub(super) fn u32(&self, at: usize) -> Option<u32> {
        let s: [u8; 4] = self.b.get(at..at + 4)?.try_into().ok()?;
        Some(if self.msb {
            u32::from_be_bytes(s)
        } else {
            u32::from_le_bytes(s)
        })
    }

    pub(super) fn u64(&self, at: usize) -> Option<u64> {
        let s: [u8; 8] = self.b.get(at..at + 8)?.try_into().ok()?;
        Some(if self.msb {
            u64::from_be_bytes(s)
        } else {
            u64::from_le_bytes(s)
        })
    }

    /// A field that is 8 bytes in ELF64 and 4 in ELF32.
    fn word(&self, at: usize) -> Option<u64> {
        if self.class64 {
            self.u64(at)
        } else {
            self.u32(at).map(u64::from)
        }
    }

    /// `PT_INTERP`'s contents, where the file has one.
    pub(super) fn interp(&self) -> Option<String> {
        let (phoff, phentsize, phnum) = if self.class64 {
            (self.u64(32)?, self.u16(54)?, self.u16(56)?)
        } else {
            (u64::from(self.u32(28)?), self.u16(42)?, self.u16(44)?)
        };
        for i in 0..phnum as usize {
            let at = phoff as usize + i * phentsize as usize;
            let p = Bytes {
                b: self.b.get(at..)?,
                msb: self.msb,
                class64: self.class64,
            };
            if p.u32(0)? != 3 {
                continue; // PT_INTERP
            }
            let off = if self.class64 {
                p.u64(8)?
            } else {
                u64::from(p.u32(4)?)
            };
            return cstr(self.b, off as usize);
        }
        None
    }

    pub(super) fn sections(&self) -> Option<Sections> {
        let (shoff, shentsize, mut shnum, mut shstrndx) = if self.class64 {
            (
                self.u64(40)?,
                self.u16(58)?,
                self.u16(60)? as usize,
                self.u16(62)? as usize,
            )
        } else {
            (
                u64::from(self.u32(32)?),
                self.u16(46)?,
                self.u16(48)? as usize,
                self.u16(50)? as usize,
            )
        };
        if shoff == 0 || shentsize == 0 {
            return None;
        }
        let one = |i: usize| -> Option<Section> {
            let at = shoff as usize + i * shentsize as usize;
            let s = Bytes {
                b: self.b.get(at..)?,
                msb: self.msb,
                class64: self.class64,
            };
            let (off_at, size_at, link_at) = if self.class64 {
                (24, 32, 40)
            } else {
                (16, 20, 24)
            };
            Some(Section {
                name_off: s.u32(0)? as usize,
                offset: s.word(off_at)? as usize,
                size: s.word(size_at)? as usize,
                link: s.u32(link_at)?,
            })
        };
        // ⚠ `e_shnum` 0 and `e_shstrndx` SHN_XINDEX both mean "the real value is
        // in section 0", which is how an object with more than 0xff00 sections
        // is encoded. A reader that took the header fields at face value would
        // read no sections at all from one.
        let zero = one(0)?;
        if shnum == 0 {
            shnum = zero.size;
        }
        if shstrndx == 0xffff {
            shstrndx = zero.link as usize;
        }
        let mut all = Vec::with_capacity(shnum);
        for i in 0..shnum {
            all.push(one(i)?);
        }
        let names = all.get(shstrndx)?.slice(self.b);
        let mut by_name = BTreeMap::new();
        for s in &all {
            if let Some(n) = cstr(names, s.name_off) {
                by_name.insert(n, s.clone());
            }
        }
        Some(Sections { all, by_name })
    }

    /// `.gnu.version_d`: version index -> the name it declares.
    pub(super) fn verdef(&self, body: &[u8], strs: &[u8]) -> BTreeMap<u16, String> {
        let mut out = BTreeMap::new();
        let mut at = 0usize;
        // ⚠ Bounded by the section rather than by the `vd_next` chain alone: a
        // hostile file can make that chain a loop.
        for _ in 0..1024 {
            let d = Bytes {
                b: match body.get(at..) {
                    Some(s) if s.len() >= 20 => s,
                    _ => break,
                },
                msb: self.msb,
                class64: self.class64,
            };
            let (flags, ndx, aux, next) = match (d.u16(2), d.u16(4), d.u32(12), d.u32(16)) {
                (Some(f), Some(a), Some(b), Some(c)) => (f, a, b as usize, c as usize),
                _ => break,
            };
            // ⛔ `VER_FLG_BASE` is the file's OWN SONAME wearing a version
            // entry's clothes, and it is not a version anything imports.
            // Collecting it put `libc.so.6` in `declares` beside `GLIBC_2.39`,
            // and it sorts after every real one.
            if flags & 0x1 != 0 {
                if next == 0 {
                    break;
                }
                at += next;
                continue;
            }
            // The FIRST verdaux is the version's own name; the ones after it are
            // the versions it inherits.
            if let Some(name) = d
                .u32(aux)
                .and_then(|off| cstr(strs, off as usize))
                .filter(|s| !s.is_empty())
            {
                out.insert(ndx & 0x7fff, name);
            }
            if next == 0 {
                break;
            }
            at += next;
        }
        out
    }

    /// `.gnu.version_r`: version index -> the name required at it, and the
    /// file (`vn_file`) the reference names as its provider, T-1644.
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
            let (cnt, aux, next) = match (n.u16(2), n.u32(8), n.u32(12)) {
                (Some(a), Some(b), Some(c)) => (a as usize, b as usize, c as usize),
                _ => break,
            };
            let file = n
                .u32(4)
                .and_then(|off| cstr(strs, off as usize))
                .unwrap_or_default();
            let mut a = aux;
            for _ in 0..cnt.min(1024) {
                let x = Bytes {
                    b: match n.b.get(a..) {
                        Some(s) if s.len() >= 16 => s,
                        _ => break,
                    },
                    msb: self.msb,
                    class64: self.class64,
                };
                let (other, name, anext) = match (x.u16(6), x.u32(8), x.u32(12)) {
                    (Some(o), Some(m), Some(k)) => (o, m as usize, k as usize),
                    _ => break,
                };
                if let Some(s) = cstr(strs, name) {
                    out.insert(other & 0x7fff, (s, file.clone()));
                }
                if anext == 0 {
                    break;
                }
                a += anext;
            }
            if next == 0 {
                break;
            }
            at += next;
        }
        out
    }
}

#[derive(Debug, Clone)]
pub(super) struct Section {
    name_off: usize,
    offset: usize,
    size: usize,
    pub(super) link: u32,
}

impl Section {
    pub(super) fn slice<'a>(&self, b: &'a [u8]) -> &'a [u8] {
        b.get(self.offset..self.offset.saturating_add(self.size))
            .unwrap_or_default()
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

    pub(super) fn by_index(&self, i: usize) -> Option<&Section> {
        self.all.get(i)
    }
}

/// A NUL-terminated string at `off` in `b`, where there is one.
pub(super) fn cstr(b: &[u8], off: usize) -> Option<String> {
    let rest = b.get(off..)?;
    let end = rest.iter().position(|c| *c == 0).unwrap_or(rest.len());
    Some(String::from_utf8_lossy(&rest[..end]).to_string())
}
