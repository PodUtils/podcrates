//! The extension members of a layer, read by this crate before the `tar`
//! reader sees them.
//!
//! **Why.** The `tar` reader reads the data of a PAX header (`x`) and of a
//! GNU long name or long link (`L`, `K`) whole, at the size that its header
//! gives: a header that gives 8 GiB makes it allocate 8 GiB. It also cuts
//! the PAX records at each newline byte, not by the length of each record:
//! a binary `SCHILY.xattr.` value with a byte 0x0a does not parse, and the
//! bytes after it can parse as records that the writer did not write.
//!
//! **How.** [`Framed`] reads the layer between the stream and the `tar`
//! reader and follows the members of the archive. It refuses an `x` past
//! [`PAX_MAX`] and an `L` or a `K` past [`LONG_MAX`] before it reads their
//! data. It parses each record of an `x` by its length, as POSIX and Go's
//! `archive/tar` do. Each record is in one class:
//!
//! * The crate uses it: the attribute records (`SCHILY.xattr.`,
//!   `LIBARCHIVE.xattr.`, and the ACLs `SCHILY.acl.`), the records of a GNU
//!   sparse file (`GNU.sparse.`, [`crate::sparse`]) and `path` and
//!   `linkpath` go to [`Side`], one [`Member`] for each member that the
//!   `tar` reader gives. `GNU.sparse.name` is the path of its member.
//! * The `tar` reader needs it: `size`, `uid`, `gid`, `path` and
//!   `linkpath` go to the `tar` reader in a new `x` member. A `path` or a
//!   `linkpath` with no newline byte goes to both, so the two readers
//!   agree; one with a newline byte goes to the side only. A `uid` or a
//!   `gid` with a newline byte is refused: the `tar` reader cuts it.
//! * Neither uses it: each other record (`mtime`, `uname`, `SCHILY.fflags`,
//!   the other `LIBARCHIVE.` records, `RHT.`, a key of a vendor) is
//!   dropped, with any byte.
//!
//! The `tar` reader does not read a GNU sparse file of the PAX form: it
//! gives the data of the member with no holes and, from version 0.1, under
//! the name `GNUSparseFile.N/NAME`. The crate writes the file from its map
//! ([`crate::sparse`]), and a map that does not hold refuses the layer.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{self, Read};
use std::rc::Rc;

/// The data of one PAX header. The attributes of one entry are 1 MiB at
/// most ([`crate::xattr::VALUES_MAX`]), and a path is 4 KiB.
pub const PAX_MAX: u64 = 1024 * 1024;
/// The data of a GNU long name or long link: `PATH_MAX`, its NUL included.
pub const LONG_MAX: u64 = 4096;
const BLOCK: usize = 512;

/// The records of one member that the `tar` reader gives, from its `x`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Member {
    pub path: Option<Vec<u8>>,
    pub linkpath: Option<Vec<u8>>,
    /// The attribute records, the key with its prefix, in their order:
    /// `SCHILY.xattr.`, `LIBARCHIVE.xattr.` and the ACLs in their text form,
    /// `SCHILY.acl.`.
    pub xattrs: Vec<(Vec<u8>, Vec<u8>)>,
    /// The records of a GNU sparse file of the PAX form.
    pub sparse: Option<crate::sparse::Sparse>,
}

#[derive(Debug, Default)]
struct State {
    members: VecDeque<Member>,
    /// The member that [`Framed`] refused and why.
    refused: Option<(String, String)>,
}

/// The records that [`Framed`] took from the stream, shared with the loop
/// over the entries of the `tar` reader.
#[derive(Debug, Default, Clone)]
pub struct Side(Rc<RefCell<State>>);

impl Side {
    /// The records of the next entry that the `tar` reader gave. Call it once
    /// for each entry, in order.
    pub fn next(&self) -> Member {
        self.0.borrow_mut().members.pop_front().unwrap_or_default()
    }

    /// The member that [`Framed`] refused and why, after a read failed.
    pub fn refused(&self) -> Option<(String, String)> {
        self.0.borrow().refused.clone()
    }

    fn push(&self, m: Member) {
        self.0.borrow_mut().members.push_back(m);
    }
}

enum Mode {
    Header,
    /// The bytes of the current member left, its padding included.
    Data(u64),
    /// After an end of the archive, or a header that the `tar` reader
    /// refuses: each byte goes on, and the `tar` reader decides.
    Pass,
}

/// The stream of a layer as the `tar` reader reads it. See the module.
pub struct Framed<R> {
    inner: R,
    side: Side,
    mode: Mode,
    out: Vec<u8>,
    at: usize,
    /// The offset in the stream of the next header, for a refusal.
    offset: u64,
    /// The records of the last `x`, for the member after it.
    pending: Option<Member>,
    /// The `size` record of the last `x`.
    pending_size: Option<u64>,
}

impl<R: Read> Framed<R> {
    pub fn new(inner: R, side: Side) -> Framed<R> {
        Framed { inner, side, mode: Mode::Header, out: Vec::new(), at: 0, offset: 0, pending: None, pending_size: None }
    }

    fn refuse(&self, what: String, why: String) -> io::Error {
        self.side.0.borrow_mut().refused = Some((what, why.clone()));
        io::Error::new(io::ErrorKind::InvalidData, why)
    }

    /// Read up to `n` bytes; fewer only at the end of the stream.
    fn read_up_to(&mut self, n: usize) -> io::Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(n);
        (&mut self.inner).take(n as u64).read_to_end(&mut buf)?;
        Ok(buf)
    }

    /// Read the next header and put what the `tar` reader gets in `out`.
    fn header(&mut self) -> io::Result<()> {
        let block = self.read_up_to(BLOCK)?;
        let at = self.offset;
        self.offset += block.len() as u64;
        if block.len() < BLOCK || block.iter().all(|b| *b == 0) {
            // The end of the archive, or a cut stream that the `tar` reader
            // reports.
            self.out = block;
            self.mode = Mode::Pass;
            return Ok(());
        }
        let h = tar::Header::from_byte_slice(&block).clone();
        let Ok(size) = h.entry_size() else {
            self.out = block;
            self.mode = Mode::Pass;
            return Ok(());
        };
        let kind = h.entry_type();
        // The `tar` reader takes an extension member only with a header of
        // ustar or GNU; else it gives it as an entry.
        let known = h.as_ustar().is_some() || h.as_gnu().is_some();
        let what = |name: &str| format!("the {name} member at byte {at}");
        if known && kind.is_pax_local_extensions() {
            if size > PAX_MAX {
                let why = format!("a PAX header of {size} bytes, more than {PAX_MAX}");
                return Err(self.refuse(what("PAX header"), why));
            }
            let data = self.read_up_to(padded(size) as usize)?;
            self.offset += data.len() as u64;
            let Some(records) = data.get(..size as usize) else {
                // A cut stream: the `tar` reader reports it.
                self.out = [block, data].concat();
                self.mode = Mode::Pass;
                return Ok(());
            };
            let (member, kept, size_record) = split(records).map_err(|why| self.refuse(what("PAX header"), why))?;
            self.pending = Some(member);
            self.pending_size = size_record;
            let mut h = h;
            h.set_size(kept.len() as u64);
            h.set_cksum();
            self.out =
                [h.as_bytes().as_slice(), &kept, &vec![0; padded(kept.len() as u64) as usize - kept.len()]].concat();
            return Ok(());
        }
        if known && (kind.is_gnu_longname() || kind.is_gnu_longlink()) {
            if size > LONG_MAX {
                let why = format!("a GNU long name of {size} bytes, more than {LONG_MAX}");
                return Err(self.refuse(what("GNU long name"), why));
            }
            return self.data(block, size);
        }
        // A member that the `tar` reader gives: the records of the `x`
        // before it are its records. An extension type that it gives as an
        // entry (`g`, or one with no ustar header) takes no `size` record.
        self.side.push(self.pending.take().unwrap_or_default());
        let extension = kind.is_pax_local_extensions()
            || kind.is_pax_global_extensions()
            || kind.is_gnu_longname()
            || kind.is_gnu_longlink();
        let size_record = self.pending_size.take();
        let size = if extension { size } else { size_record.unwrap_or(size) };
        let mut head = block;
        // The extension blocks of a GNU sparse header come before its data
        // and are not in its size.
        if kind.is_gnu_sparse() && h.as_gnu().is_some_and(|g| g.is_extended()) {
            loop {
                let ext = self.read_up_to(BLOCK)?;
                self.offset += ext.len() as u64;
                let more = ext.len() == BLOCK && ext[504] != 0;
                let cut = ext.len() < BLOCK;
                head.extend_from_slice(&ext);
                if cut {
                    self.out = head;
                    self.mode = Mode::Pass;
                    return Ok(());
                }
                if !more {
                    break;
                }
            }
        }
        self.data(head, size)
    }

    fn data(&mut self, head: Vec<u8>, size: u64) -> io::Result<()> {
        self.out = head;
        let left = padded(size);
        self.offset = self.offset.saturating_add(left);
        // A size past `u64` is refused by the `tar` reader.
        self.mode = if left < size { Mode::Pass } else { Mode::Data(left) };
        Ok(())
    }
}

/// `n` rounded up to a block, or `n` when that overflows.
fn padded(n: u64) -> u64 {
    n.checked_add(BLOCK as u64 - 1).map_or(n, |x| x & !(BLOCK as u64 - 1))
}

impl<R: Read> Read for Framed<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.at < self.out.len() {
                let n = buf.len().min(self.out.len() - self.at);
                buf[..n].copy_from_slice(&self.out[self.at..self.at + n]);
                self.at += n;
                return Ok(n);
            }
            self.out.clear();
            self.at = 0;
            match self.mode {
                Mode::Pass => return self.inner.read(buf),
                Mode::Data(0) => self.mode = Mode::Header,
                Mode::Data(left) => {
                    let n = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
                    let got = self.inner.read(&mut buf[..n])?;
                    self.mode = if got == 0 { Mode::Pass } else { Mode::Data(left - got as u64) };
                    return Ok(got);
                }
                Mode::Header => {
                    self.header()?;
                    if self.out.is_empty() {
                        return Ok(0);
                    }
                }
            }
        }
    }
}

/// The records of an `x`: the member's own records, the bytes of the
/// records that go on to the `tar` reader, and the `size` record. See the
/// module for the class of each key.
fn split(data: &[u8]) -> Result<(Member, Vec<u8>, Option<u64>), String> {
    let (mut m, mut kept, mut size) = (Member::default(), Vec::new(), None);
    let mut sparse = crate::sparse::Records::default();
    for (key, value) in records(data)? {
        let newline = value.contains(&b'\n');
        match key {
            k if k.starts_with(crate::xattr::PAX_PREFIX.as_bytes())
                || k.starts_with(crate::xattr::LIBARCHIVE_PREFIX.as_bytes())
                || k.starts_with(crate::acl::PAX_PREFIX.as_bytes()) =>
            {
                m.xattrs.push((k.to_vec(), value.to_vec()));
                continue;
            }
            k if k.starts_with(b"GNU.sparse.") => {
                sparse.push(&k[b"GNU.sparse.".len()..], value)?;
                continue;
            }
            // An empty value removes the record (POSIX).
            b"path" => m.path = (!value.is_empty()).then(|| value.to_vec()),
            b"linkpath" => m.linkpath = (!value.is_empty()).then(|| value.to_vec()),
            // The last `size` record frames the member, and the `tar` reader
            // gets only that one: it takes the first.
            b"size" if value.is_empty() => {
                size = None;
                continue;
            }
            b"size" => {
                let n = std::str::from_utf8(value).ok().and_then(|s| s.parse::<u64>().ok());
                size = Some(n.ok_or_else(|| format!("the PAX size record {:?} is not a number", shown(value)))?);
                continue;
            }
            b"uid" | b"gid" if newline => {
                return Err(format!(
                    "the PAX record {:?} has a newline byte, which the tar reader cuts",
                    String::from_utf8_lossy(key)
                ));
            }
            b"uid" | b"gid" => {}
            // Neither reader uses it.
            _ => continue,
        }
        // The `tar` reader cuts a value at a newline byte: this crate reads
        // such a `path` or `linkpath` from the member.
        if !newline {
            push_record(&mut kept, key, value);
        }
    }
    if let Some(n) = size {
        push_record(&mut kept, b"size", n.to_string().as_bytes());
    }
    // `GNU.sparse.name` holds over `path` in any order (Go's `archive/tar`).
    if let Some((s, name)) = sparse.finish()? {
        m.path = name.or(m.path);
        m.sparse = Some(s);
    }
    Ok((m, kept, size))
}

/// The failure of a read of the `tar` reader: a [`Refusal::Member`] of
/// `layer` when [`Framed`] refused a member, else the error of the read.
///
/// [`Refusal::Member`]: crate::safety::Refusal::Member
pub(crate) fn failed(side: &Side, layer: &str, e: io::Error) -> pod_core::Failure {
    match side.refused() {
        Some((what, why)) => crate::error::refuse(layer, &what, crate::safety::Refusal::Member(why)),
        None => crate::error::io(e),
    }
}

/// A key and its value.
pub type Record<'a> = (&'a [u8], &'a [u8]);

/// Each record of `data`, by its length: `LEN KEY=VALUE\n`.
pub fn records(mut data: &[u8]) -> Result<Vec<Record<'_>>, String> {
    let mut out = Vec::new();
    while !data.is_empty() {
        let digits = data.iter().take(20).take_while(|b| b.is_ascii_digit()).count();
        let len = std::str::from_utf8(&data[..digits]).ok().and_then(|s| s.parse::<usize>().ok());
        let bad = |why: &str| format!("a PAX record that does not parse: {why}, at {:?}", shown(data));
        let Some(len) = len.filter(|n| *n > digits + 1 && *n <= data.len()) else {
            return Err(bad("its length is not a number inside the header"));
        };
        let (rec, rest) = data.split_at(len);
        if rec[digits] != b' ' || rec[len - 1] != b'\n' {
            return Err(bad("no space after its length, or no newline at its end"));
        }
        let body = &rec[digits + 1..len - 1];
        let Some(eq) = body.iter().position(|b| *b == b'=').filter(|i| *i > 0) else {
            return Err(bad("no key"));
        };
        out.push((&body[..eq], &body[eq + 1..]));
        data = rest;
    }
    Ok(out)
}

pub(crate) fn push_record(out: &mut Vec<u8>, key: &[u8], value: &[u8]) {
    // The length counts its own digits.
    let rest = key.len() + value.len() + 3;
    let mut len = rest + 1;
    while len != rest + len.to_string().len() {
        len = rest + len.to_string().len();
    }
    out.extend_from_slice(format!("{len} ").as_bytes());
    out.extend_from_slice(key);
    out.push(b'=');
    out.extend_from_slice(value);
    out.push(b'\n');
}

/// The start of a value for a message.
fn shown(b: &[u8]) -> String {
    String::from_utf8_lossy(&b[..b.len().min(40)]).into_owned()
}

#[cfg(test)]
mod tests;
