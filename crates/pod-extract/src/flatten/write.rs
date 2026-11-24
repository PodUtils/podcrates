//! The tar writer of the stream: a ustar header for each entry, a PAX
//! header before it where a value does not fit, the data padded to a
//! block, and the end blocks only after the last layer.
//!
//! The crate does not take `tar::Builder` for the stream: its drop writes
//! the end blocks, also after a failure, and a stream that ends with them
//! reads as a whole tree. A failure writes one block that is not a header
//! ([`Out::broken`]) in their place.

use std::io::{self, Read, Write};

use crate::error::{self, Result};
use crate::sparse::Layout;

const BLOCK: usize = 512;
/// The largest size and time in 11 octal digits, and the largest id in 7.
const OCTAL_11: u64 = 8u64.pow(11);
const OCTAL_7: u64 = 8u64.pow(7);

/// The kind of an entry of the stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    File,
    Dir,
    Symlink,
    Link,
}

/// The header of one entry, with the values of the image.
#[derive(Debug)]
pub(super) struct Head<'a> {
    pub(super) path: &'a str,
    pub(super) kind: Kind,
    pub(super) mode: u32,
    pub(super) uid: u64,
    pub(super) gid: u64,
    pub(super) mtime: u64,
    /// The size of the data: 0 for each kind but a file.
    pub(super) size: u64,
    pub(super) link: Option<&'a str>,
}

/// The writer of the caller, and the count of the bytes that went to it.
pub(super) struct Out<'a, W: Write> {
    w: &'a mut W,
    bytes: u64,
}

impl<'a, W: Write> Out<'a, W> {
    pub(super) fn new(w: &'a mut W) -> Out<'a, W> {
        Out { w, bytes: 0 }
    }

    pub(super) fn bytes(&self) -> u64 {
        self.bytes
    }

    fn put(&mut self, b: &[u8]) -> Result<()> {
        self.w.write_all(b).map_err(error::io)?;
        self.bytes += b.len() as u64;
        Ok(())
    }

    fn pad(&mut self, len: u64) -> Result<()> {
        let rest = (BLOCK - (len % BLOCK as u64) as usize) % BLOCK;
        self.put(&[0u8; BLOCK][..rest])
    }

    /// The header of one entry: a PAX header first where a name has more
    /// than 100 bytes or a byte that is not ASCII, or a number does not fit
    /// its field.
    pub(super) fn head(&mut self, h: &Head) -> Result<()> {
        let mut recs = Vec::new();
        let mut t = tar::Header::new_ustar();
        let u = t.as_ustar_mut().expect("a ustar header");
        fit(&mut u.name, h.path, b"path", &mut recs);
        if let Some(l) = h.link {
            fit(&mut u.linkname, l, b"linkpath", &mut recs);
        }
        for (key, value, max) in [(&b"size"[..], h.size, OCTAL_11), (b"uid", h.uid, OCTAL_7), (b"gid", h.gid, OCTAL_7)]
        {
            if value >= max {
                crate::pax::push_record(&mut recs, key, value.to_string().as_bytes());
            }
        }
        if !recs.is_empty() {
            let mut x = tar::Header::new_ustar();
            let name = b"././@PaxHeader";
            x.as_ustar_mut().expect("a ustar header").name[..name.len()].copy_from_slice(name);
            x.set_entry_type(tar::EntryType::XHeader);
            x.set_mode(0o644);
            x.set_size(recs.len() as u64);
            x.set_cksum();
            self.put(x.as_bytes())?;
            self.put(&recs)?;
            self.pad(recs.len() as u64)?;
        }
        t.set_entry_type(match h.kind {
            Kind::File => tar::EntryType::Regular,
            Kind::Dir => tar::EntryType::Directory,
            Kind::Symlink => tar::EntryType::Symlink,
            Kind::Link => tar::EntryType::Link,
        });
        t.set_mode(h.mode & 0o7777);
        t.set_uid(h.uid);
        t.set_gid(h.gid);
        t.set_mtime(h.mtime);
        t.set_size(h.size);
        t.set_cksum();
        self.put(t.as_bytes())
    }

    /// The data of a file: `size` bytes of `r`, then the padding. A reader
    /// that ends first fails the entry.
    pub(super) fn data(&mut self, r: &mut dyn Read, size: u64) -> Result<()> {
        let mut buf = vec![0u8; 128 * 1024];
        let mut left = size;
        while left > 0 {
            let want = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
            let n = match r.read(&mut buf[..want]) {
                Ok(0) => {
                    let e = io::Error::new(io::ErrorKind::UnexpectedEof, format!("the data ended {left} bytes early"));
                    return Err(error::io(e));
                }
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(error::io(e)),
            };
            self.put(&buf[..n])?;
            left -= n as u64;
        }
        self.pad(size)
    }

    /// The end of the archive: two zero blocks, then a flush.
    pub(super) fn end(&mut self) -> Result<()> {
        self.put(&[0u8; 2 * BLOCK])?;
        self.w.flush().map_err(error::io)
    }

    /// One block that is not a header, in place of the end blocks: a
    /// reader of the stream fails on it and does not take a short tree. An
    /// error of the writer here is dropped: the caller gets the first one.
    pub(super) fn broken(&mut self) {
        if self.put(&[0xff; BLOCK]).is_ok() {
            let _ = self.w.flush();
        }
    }
}

/// Put `value` in the field when it fits there, else in a PAX record and
/// its first bytes in the field.
fn fit(field: &mut [u8; 100], value: &str, key: &[u8], recs: &mut Vec<u8>) {
    let b = value.as_bytes();
    if b.len() <= field.len() && b.iter().all(|c| c.is_ascii() && *c != 0) {
        field[..b.len()].copy_from_slice(b);
        return;
    }
    crate::pax::push_record(recs, key, b);
    let cut = value.char_indices().map(|(i, c)| i + c.len_utf8()).take_while(|e| *e <= field.len()).last();
    let n = cut.unwrap_or(0);
    field[..n].copy_from_slice(&b[..n]);
}

/// The data of a GNU sparse file with its holes as zero bytes: the
/// regions of `inner` at their offsets, up to the real size.
pub(super) struct Expand<'a> {
    inner: &'a mut dyn Read,
    layout: &'a Layout,
    pos: u64,
    at: usize,
}

impl<'a> Expand<'a> {
    pub(super) fn new(inner: &'a mut dyn Read, layout: &'a Layout) -> Expand<'a> {
        Expand { inner, layout, pos: 0, at: 0 }
    }
}

impl Read for Expand<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let regions = &self.layout.regions;
        // `sparse::layout` checked the regions: in order, inside the size.
        while self.at < regions.len() && self.pos >= regions[self.at].offset + regions[self.at].len {
            self.at += 1;
        }
        let real = self.layout.real_size;
        if self.pos >= real || buf.is_empty() {
            return Ok(0);
        }
        let room = |end: u64, pos: u64| buf.len().min(usize::try_from(end - pos).unwrap_or(usize::MAX));
        let n = match regions.get(self.at) {
            Some(r) if self.pos >= r.offset => {
                let n = room(r.offset + r.len, self.pos);
                self.inner.read(&mut buf[..n])?
            }
            next => {
                let n = room(next.map_or(real, |r| r.offset), self.pos);
                buf[..n].fill(0);
                n
            }
        };
        self.pos += n as u64;
        Ok(n)
    }
}
