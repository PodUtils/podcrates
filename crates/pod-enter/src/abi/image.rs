use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::os::unix::fs::FileExt;

use super::*;

// Where the reader gets the bytes of a file: a slice in hand, or a file
// that it reads in bounded pieces. A payload can be larger than the memory
// that a check may take, so [`Elf::read`] reads the header, the tables and
// the sections that the checks need, and never the whole file.

/// The most bytes that [`Elf::read`] holds for one file: the header, the
/// two header tables and the sections that the checks read (`.dynamic`,
/// `.dynsym`, their strings and the version sections).
///
/// It bounds the memory, not the size of the file: a file of any size
/// reads if its tables fit. `libc.so.6` needs less than 1 MiB. A file
/// whose tables need more is an [`Unreadable`] that names this bound.
pub const READ_CEILING: u64 = 64 * 1024 * 1024;

/// The bytes of the head of a payload that a caller reads to classify it
/// ([`interp_of`], [`crate::ladder::payload_head`]).
///
/// The ELF header and the program headers of a real executable are in its
/// first pages. Where a program header lies past the bytes read,
/// [`interp_of`] says "unknown", never "static".
pub const HEAD_BYTES: u64 = 64 * 1024;

/// The largest program header table that the reader accepts. The kernel
/// refuses a table over 64 KiB (`load_elf_phdrs`), so a larger one is a
/// hostile or broken file.
pub const PHDR_TABLE_MAX: u64 = 64 * 1024;

/// The longest `PT_INTERP` path that the reader takes: `PATH_MAX`.
const INTERP_MAX: u64 = 4096;

pub(super) enum Src<'a> {
    /// Bytes in hand: a whole file, or the head of one.
    Mem(&'a [u8]),
    /// A file, read with `pread` in the pieces that the parser asks for.
    File {
        file: &'a std::fs::File,
        len: u64,
        /// The bytes that the reader can still hold ([`READ_CEILING`]).
        left: Cell<u64>,
        /// Set when a read asked for more than `left`.
        over: Cell<bool>,
        /// The first error of a read.
        fault: RefCell<Option<std::io::Error>>,
    },
}

impl<'a> Src<'a> {
    pub(super) fn file(file: &'a std::fs::File, len: u64) -> Src<'a> {
        Src::File { file, len, left: Cell::new(READ_CEILING), over: Cell::new(false), fault: RefCell::new(None) }
    }

    pub(super) fn len(&self) -> u64 {
        match self {
            Src::Mem(b) => b.len() as u64,
            Src::File { len, .. } => *len,
        }
    }

    /// `len` bytes at `off`, or `None` where they are not all inside the
    /// bytes, or where a file read fails or goes past [`READ_CEILING`].
    pub(super) fn get(&self, off: u64, len: u64) -> Option<Cow<'a, [u8]>> {
        let end = off.checked_add(len)?;
        if end > self.len() {
            return None;
        }
        match self {
            Src::Mem(b) => Some(Cow::Borrowed(b.get(usize::try_from(off).ok()?..usize::try_from(end).ok()?)?)),
            Src::File { file, left, over, fault, .. } => {
                if len > left.get() {
                    over.set(true);
                    return None;
                }
                left.set(left.get() - len);
                let mut v = vec![0u8; usize::try_from(len).ok()?];
                if let Err(e) = file.read_exact_at(&mut v, off) {
                    fault.borrow_mut().get_or_insert(e);
                    return None;
                }
                Some(Cow::Owned(v))
            }
        }
    }

    /// Up to `len` bytes at `off`: fewer where the bytes end first.
    pub(super) fn upto(&self, off: u64, len: u64) -> Option<Cow<'a, [u8]>> {
        if off >= self.len() {
            return None;
        }
        self.get(off, len.min(self.len() - off))
    }

    /// Why a file read gave no bytes: the error of a read, or the bound.
    pub(super) fn trouble(&self) -> Option<String> {
        let Src::File { over, fault, .. } = self else {
            return None;
        };
        if let Some(e) = fault.borrow().as_ref() {
            return Some(format!("{e}"));
        }
        over.get().then(|| {
            format!(
                "its tables need more than the {READ_CEILING} bytes that the reader holds for one file. \
                 An object that cannot be checked is not preloaded"
            )
        })
    }
}

/// A file opened for the parser: its source and its class.
pub(super) struct Image<'a> {
    pub(super) src: Src<'a>,
    pub(super) msb: bool,
    pub(super) class64: bool,
    head: Cow<'a, [u8]>,
}

impl<'a> Image<'a> {
    /// Check the magic, the class and the data encoding.
    pub(super) fn open(src: Src<'a>) -> Result<Image<'a>, String> {
        let not_elf = || "not an ELF file: the magic is not \\x7fELF".to_string();
        let head = match src.get(0, 64) {
            Some(h) if &h[0..4] == b"\x7fELF" => h,
            _ => return Err(src.trouble().unwrap_or_else(not_elf)),
        };
        let class64 = match head[4] {
            1 => false,
            2 => true,
            c => return Err(format!("unknown ELF class {c}")),
        };
        let msb = match head[5] {
            1 => false,
            2 => true,
            d => return Err(format!("unknown ELF data encoding {d}")),
        };
        Ok(Image { src, msb, class64, head })
    }

    pub(super) fn view<'b>(&self, b: &'b [u8]) -> Bytes<'b> {
        Bytes { b, msb: self.msb, class64: self.class64 }
    }

    pub(super) fn machine(&self) -> Option<u16> {
        self.view(&self.head).u16(18)
    }

    /// The contents of `PT_INTERP`: `Ok(None)` for a file with no
    /// `PT_INTERP`, and an error where the program headers or the path lie
    /// past the bytes. A head read cannot tell static from dynamic there,
    /// so the answer is "unknown" and never "static".
    pub(super) fn interp(&self) -> Result<Option<String>, String> {
        let h = self.view(&self.head);
        let (phoff, phentsize, phnum, min) = if self.class64 {
            (h.u64(32), h.u16(54), h.u16(56), 56)
        } else {
            (h.u32(28).map(u64::from), h.u16(42), h.u16(44), 32)
        };
        let (Some(phoff), Some(phentsize), Some(phnum)) = (phoff, phentsize, phnum) else {
            return Err("truncated header".into());
        };
        if phnum == 0 {
            return Ok(None);
        }
        if phentsize < min {
            return Err(format!("e_phentsize {phentsize} is smaller than a program header ({min} bytes)"));
        }
        let size = u64::from(phentsize) * u64::from(phnum);
        if size > PHDR_TABLE_MAX {
            return Err(format!(
                "the program header table has {size} bytes, over the {PHDR_TABLE_MAX} that the kernel takes"
            ));
        }
        let past = |what: &str| {
            self.src.trouble().unwrap_or_else(|| {
                format!(
                    "{what} lies past the {} bytes read, so the reader cannot tell a static file from a dynamic one",
                    self.src.len()
                )
            })
        };
        let table = self.src.get(phoff, size).ok_or_else(|| past("the program header table"))?;
        for i in 0..usize::from(phnum) {
            let p = self.view(&table[i * usize::from(phentsize)..]);
            if p.u32(0) != Some(3) {
                continue; // PT_INTERP
            }
            let off = if self.class64 { p.u64(8) } else { p.u32(4).map(u64::from) };
            let s = off.and_then(|o| self.src.upto(o, INTERP_MAX)).ok_or_else(|| past("the PT_INTERP path"))?;
            return Ok(cstr(&s, 0));
        }
        Ok(None)
    }

    pub(super) fn sections(&self) -> Option<Sections> {
        let h = self.view(&self.head);
        let (shoff, shentsize, mut shnum, mut shstrndx, min) = if self.class64 {
            (h.u64(40)?, h.u16(58)?, u64::from(h.u16(60)?), usize::from(h.u16(62)?), 64)
        } else {
            (u64::from(h.u32(32)?), h.u16(46)?, u64::from(h.u16(48)?), usize::from(h.u16(50)?), 40)
        };
        if shoff == 0 || shentsize < min {
            return None;
        }
        let entry = u64::from(shentsize);
        let one = |b: &[u8]| -> Option<Section> {
            let s = self.view(b);
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
        let zero = one(&self.src.get(shoff, entry)?)?;
        if shnum == 0 {
            shnum = zero.size;
        }
        if shstrndx == 0xffff {
            shstrndx = usize::try_from(zero.link).ok()?;
        }
        // Each header must be inside the file, so the file bounds the
        // count, and a hostile count cannot make a large allocation.
        if shnum > self.src.len() / entry {
            return None;
        }
        let table = self.src.get(shoff, shnum * entry)?;
        let all = table.chunks_exact(usize::from(shentsize)).map(one).collect::<Option<Vec<_>>>()?;
        let names = self.bytes(all.get(shstrndx)?);
        let mut by_name = BTreeMap::new();
        for s in &all {
            if let Some(n) = cstr(&names, s.name_off) {
                by_name.insert(n, s.clone());
            }
        }
        Some(Sections { all, by_name })
    }

    /// The bytes of a section, or none where the section is not inside the
    /// file.
    pub(super) fn bytes(&self, s: &Section) -> Cow<'a, [u8]> {
        self.src.get(s.offset, s.size).unwrap_or_default()
    }
}
