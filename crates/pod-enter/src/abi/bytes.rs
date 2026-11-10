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
    pub(super) fn word(&self, at: usize) -> Option<u64> {
        if self.class64 { self.u64(at) } else { self.u32(at).map(u64::from) }
    }

    /// A field of 32 bits as an index into the file.
    pub(super) fn index(&self, at: usize) -> Option<usize> {
        usize::try_from(self.u32(at)?).ok()
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
    pub(super) name_off: usize,
    pub(super) offset: u64,
    pub(super) size: u64,
    pub(super) link: u32,
}

pub(super) struct Sections {
    pub(super) all: Vec<Section>,
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
