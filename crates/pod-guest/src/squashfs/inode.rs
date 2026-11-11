//! The inodes, the directory listings, the lookup of a path and the read of
//! a file.

use std::io::{Read, Seek};

use super::meta::Place;
use super::{Fault, Image, MAX_FILE, MAX_LINK, MAX_LISTING, METADATA_SIZE, u16_at, u32_at, u64_at};
use crate::Result;

/// The fragment index of a file whose tail is in its last block.
const NO_FRAGMENT: u32 = u32::MAX;
/// The bit of a size word: the block is stored, not compressed.
const STORED: u32 = 1 << 24;
/// The most entries under one header of a listing.
const HEADER_ENTRIES: u32 = 256;
/// The longest name of an entry.
const NAME_LEN: usize = 256;

/// One inode: its number, its mode bits and what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inode {
    pub number: u32,
    pub mode: u16,
    pub kind: Kind,
}

/// What an inode is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Directory(Dir),
    File(File),
    /// A symbolic link and its target. The reader follows no link.
    Symlink(Vec<u8>),
    /// A device, a FIFO or a socket: its inode type.
    Other(u16),
}

/// A directory: the place of its listing and the length of the listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dir {
    block: u32,
    offset: u16,
    listing: u64,
}

/// A regular file: its size, the place of its first block, the place of its
/// list of block sizes, and its fragment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    size: u64,
    start: u64,
    list: Place,
    fragment: Option<(u32, u32)>,
}

impl File {
    /// The size of the file in bytes.
    pub fn size(&self) -> u64 {
        self.size
    }
}

/// One entry of a listing: its name and the reference of its inode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: Vec<u8>,
    pub inode: u64,
}

impl<R: Read + Seek> Image<R> {
    /// The inode of the root directory.
    pub fn root(&mut self) -> Result<Inode> {
        self.inode(self.sb.root)
    }

    /// The inode at `reference`: the place of its metadata block from the
    /// start of the inode table (the high bits), and the offset in the block
    /// (the low 16 bits).
    pub fn inode(&mut self, reference: u64) -> Result<Inode> {
        let block = self.sb.inode_table.checked_add(reference >> 16);
        let offset = (reference & 0xffff) as usize;
        let Some(block) = block.filter(|&b| b < self.sb.directory_table && offset < METADATA_SIZE) else {
            return Err(Fault::Reference(reference).into());
        };
        let mut at = Place { block, offset };
        let head: [u8; 16] = self.meta_array(&mut at)?;
        let (kind, mode, number) = (u16_at(&head, 0), u16_at(&head, 2), u32_at(&head, 12));
        let kind = match kind {
            1 => {
                let b: [u8; 16] = self.meta_array(&mut at)?;
                let size = u64::from(u16_at(&b, 8));
                Kind::Directory(Dir { block: u32_at(&b, 0), offset: u16_at(&b, 10), listing: size.saturating_sub(3) })
            }
            8 => {
                let b: [u8; 24] = self.meta_array(&mut at)?;
                let size = u64::from(u32_at(&b, 4));
                Kind::Directory(Dir { block: u32_at(&b, 8), offset: u16_at(&b, 18), listing: size.saturating_sub(3) })
            }
            2 => {
                let b: [u8; 16] = self.meta_array(&mut at)?;
                let start = u64::from(u32_at(&b, 0));
                Kind::File(self.file(start, u64::from(u32_at(&b, 12)), u32_at(&b, 4), u32_at(&b, 8), at))
            }
            9 => {
                let b: [u8; 40] = self.meta_array(&mut at)?;
                Kind::File(self.file(u64_at(&b, 0), u64_at(&b, 8), u32_at(&b, 28), u32_at(&b, 32), at))
            }
            3 | 10 => {
                let b: [u8; 8] = self.meta_array(&mut at)?;
                let len = u64::from(u32_at(&b, 4));
                if len > MAX_LINK {
                    return Err(Fault::TooLarge { what: "a link", size: len, limit: MAX_LINK }.into());
                }
                Kind::Symlink(self.meta_vec(&mut at, len as usize)?)
            }
            4..=7 | 11..=14 => Kind::Other(kind),
            other => return Err(Fault::InodeType(other).into()),
        };
        Ok(Inode { number, mode, kind })
    }

    fn file(&self, start: u64, size: u64, fragment: u32, offset: u32, list: Place) -> File {
        let fragment = (fragment != NO_FRAGMENT).then_some((fragment, offset));
        File { size, start, list, fragment }
    }

    /// The entries of a directory, in the order of its listing.
    pub fn entries(&mut self, dir: &Dir) -> Result<Vec<Entry>> {
        if dir.listing > MAX_LISTING {
            return Err(Fault::TooLarge { what: "a listing", size: dir.listing, limit: MAX_LISTING }.into());
        }
        if dir.listing == 0 {
            return Ok(Vec::new());
        }
        let block = self.sb.directory_table.checked_add(u64::from(dir.block));
        let block = block.ok_or(Fault::Past { what: "a listing", at: self.sb.directory_table })?;
        let raw = self.meta_vec(&mut Place { block, offset: usize::from(dir.offset) }, dir.listing as usize)?;
        listing(&raw).map_err(Into::into)
    }

    /// The inode at `path`, from the root. A part of the path that is not a
    /// directory is [`Fault::NotADirectory`]; a name that is not there is
    /// [`Fault::Missing`]. Empty parts are skipped; no link is followed.
    pub fn lookup(&mut self, path: &str) -> Result<Inode> {
        let mut inode = self.root()?;
        let mut walked = String::new();
        for part in path.split('/').filter(|p| !p.is_empty()) {
            let Kind::Directory(dir) = &inode.kind else {
                return Err(Fault::NotADirectory(walked).into());
            };
            let entries = self.entries(dir)?;
            if !walked.is_empty() {
                walked.push('/');
            }
            walked.push_str(part);
            let Some(entry) = entries.iter().find(|e| e.name == part.as_bytes()) else {
                return Err(Fault::Missing(walked).into());
            };
            inode = self.inode(entry.inode)?;
        }
        Ok(inode)
    }

    /// The regular file at `path` ([`Image::lookup`]).
    pub fn file_at(&mut self, path: &str) -> Result<File> {
        match self.lookup(path)?.kind {
            Kind::File(file) => Ok(file),
            _ => Err(Fault::NotAFile(path.to_string()).into()),
        }
    }

    /// The bytes of `file`, at most [`MAX_FILE`]. Each block decodes to the
    /// length that the file gives it; a block of size 0 is sparse (zeros);
    /// the tail is in the last block or in a fragment.
    pub fn read(&mut self, file: &File) -> Result<Vec<u8>> {
        if file.size > MAX_FILE {
            return Err(Fault::TooLarge { what: "a file", size: file.size, limit: MAX_FILE }.into());
        }
        let bs = u64::from(self.sb.block_size);
        let blocks = match file.fragment {
            Some(_) => file.size / bs,
            None => file.size.div_ceil(bs),
        };
        let mut out = Vec::with_capacity(file.size as usize);
        let (mut list, mut at) = (file.list, file.start);
        for i in 0..blocks {
            let word = u32::from_le_bytes(self.meta_array(&mut list)?);
            let want = (file.size - i * bs).min(bs) as usize;
            if word == 0 {
                out.resize(out.len() + want, 0);
                continue;
            }
            let disk = word & !STORED;
            if disk > self.sb.block_size {
                return Err(Fault::BlockLength { at, size: disk }.into());
            }
            let data = self.data_block(at, word, want)?;
            if data.len() != want {
                return Err(Fault::Length { at, want, got: data.len() }.into());
            }
            out.extend_from_slice(&data);
            at += u64::from(disk);
        }
        if let Some((index, offset)) = file.fragment {
            let tail = (file.size % bs) as usize;
            let (start, word) = self.fragment_entry(index)?;
            if (word & !STORED) > self.sb.block_size {
                return Err(Fault::BlockLength { at: start, size: word & !STORED }.into());
            }
            if self.fragment.as_ref().is_none_or(|(s, _)| *s != start) {
                let data = self.data_block(start, word, self.sb.block_size as usize)?;
                self.fragment = Some((start, data));
            }
            let block = self.fragment.as_ref().map(|(_, d)| d.as_slice()).unwrap_or_default();
            let bytes = block.get(offset as usize..).and_then(|b| b.get(..tail));
            out.extend_from_slice(bytes.ok_or(Fault::Fragment { index })?);
        }
        Ok(out)
    }

    /// A data or fragment block at `at` by its size word: stored, or
    /// decoded to at most `max` bytes.
    fn data_block(&mut self, at: u64, word: u32, max: usize) -> Result<Vec<u8>> {
        let disk = (word & !STORED) as usize;
        let raw = self.bytes_at("a data block", at, disk)?;
        if word & STORED != 0 { Ok(raw) } else { self.decode(at, &raw, max) }
    }
}

/// The entries of a listing: headers (a count less one, the block of the
/// inodes, a base inode number), each with up to 256 entries (an offset in
/// the block, a delta of the number, a type, a name size less one, the
/// name). A name is not empty, `.` or `..`, and holds no `/` and no NUL.
pub fn listing(raw: &[u8]) -> std::result::Result<Vec<Entry>, Fault> {
    let mut entries = Vec::new();
    let mut p = 0;
    while p < raw.len() {
        let head = raw.get(p..p + 12).ok_or(Fault::Listing("a header is cut short"))?;
        let (count, start) = (u32_at(head, 0), u32_at(head, 4));
        if count >= HEADER_ENTRIES {
            return Err(Fault::Listing("more than 256 entries under one header"));
        }
        p += 12;
        for _ in 0..=count {
            let e = raw.get(p..p + 8).ok_or(Fault::Listing("an entry is cut short"))?;
            let (offset, size) = (u16_at(e, 0), usize::from(u16_at(e, 6)) + 1);
            if size > NAME_LEN {
                return Err(Fault::Listing("a name of more than 256 bytes"));
            }
            let name = raw.get(p + 8..p + 8 + size).ok_or(Fault::Listing("a name is cut short"))?;
            if name == b"." || name == b".." || name.iter().any(|&c| c == b'/' || c == 0) {
                return Err(Fault::Listing("a name that is not one part of a path"));
            }
            entries.push(Entry { name: name.to_vec(), inode: (u64::from(start) << 16) | u64::from(offset) });
            p += 8 + size;
        }
    }
    Ok(entries)
}
