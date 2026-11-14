//! The store of a base image: bounded, with a digest, resumable, and all or
//! nothing at the destination.
//!
//! The bytes go to `NAME.part` ([`partial`]). A break resumes with
//! `Range: bytes=N-`: a `206` continues the file, a `200` starts it again
//! from zero. The destination gets its name only after the size and the
//! sha256 match ([`store`]).
//!
//! Four bounds, and each is a refusal, never a truncation. Before the
//! transfer: a declared size over the ceiling. During the transfer: the
//! bytes past the ceiling (the limit of the caller and `RLIMIT_FSIZE`), and
//! the bytes past the size that the origin declared. At the end: a digest
//! that is not the pinned one. A refusal of the bytes removes the partial
//! file; a break keeps it, so that a later call resumes.
//!
//! The ceiling applies to the declared size and to the bytes that come: an
//! origin can declare 10 MB and send 10 GB, or declare nothing.
//!
//! The transport is not in this module: [`store`] takes an [`Origin`], so
//! the policy (the ranges, the ceilings, the digest, the cleanup) is tested
//! against a stand-in, and the HTTP client is the caller's.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use pod_core::Failure;
use sha2::{Digest, Sha256};

use crate::{Result, io_at};

mod range;
mod refusal;

pub use range::{Span, parse_content_range};
pub use refusal::{Refusal, refusal};

/// How much can be written, in bytes. `max_bytes` is the limit of the
/// caller for this entry and `fsize` is `RLIMIT_FSIZE`; both apply, and
/// `u64::MAX` is no limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ceiling {
    pub max_bytes: u64,
    pub fsize: u64,
}

impl Ceiling {
    /// The smaller of the two limits.
    pub fn effective(self) -> u64 {
        self.max_bytes.min(self.fsize)
    }
}

/// The answer of an origin to one GET: the status, two headers as the
/// origin wrote them, and the body.
#[derive(Debug)]
pub struct Reply<B> {
    pub status: u16,
    /// `Content-Length`.
    pub length: Option<u64>,
    /// `Content-Range`, unparsed.
    pub range: Option<String>,
    /// The bytes of the body. Its end, or an error, is the end of the reply.
    pub body: B,
}

/// The source of the bytes: the caller's HTTP client.
pub trait Origin {
    type Body: Read;
    /// Send one GET. With `from > 0`, the request carries
    /// `Range: bytes=FROM-`. An error is a break: [`store`] tries again.
    fn get(&mut self, from: u64) -> std::io::Result<Reply<Self::Body>>;
}

/// An origin of one stream that cannot resume: the first GET gives the
/// stream with the status `200` and the declared size; each later GET is an
/// error.
#[derive(Debug)]
pub struct Stream<R> {
    reader: Option<R>,
    declared: Option<u64>,
}

impl<R: Read> Stream<R> {
    pub fn new(reader: R, declared: Option<u64>) -> Stream<R> {
        Stream { reader: Some(reader), declared }
    }
}

impl<R: Read> Origin for Stream<R> {
    type Body = R;
    fn get(&mut self, _from: u64) -> std::io::Result<Reply<R>> {
        let body = self.reader.take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Unsupported, "a stream does not resume after a break")
        })?;
        Ok(Reply { status: 200, length: self.declared, range: None, body })
    }
}

/// Refuse a declared size over the ceiling. `None` is an origin that
/// declared nothing: the check during the transfer bounds it.
pub fn check_declared(declared: Option<u64>, ceiling: Ceiling) -> Result<()> {
    let limit = ceiling.effective();
    match declared {
        Some(n) if n > limit => Err(Refusal::DeclaredTooLarge { declared: n, ceiling: limit }.failure()),
        _ => Ok(()),
    }
}

/// The partial file of `dest`: `NAME.part` beside it. The name is fixed, so
/// that a later call resumes it; a lock on the file keeps a second writer
/// out ([`Refusal::Busy`]).
pub fn partial(dest: &Path) -> PathBuf {
    let name = dest.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "image".to_string());
    dest.with_file_name(format!("{name}.part"))
}

/// The statuses that are a break, not an answer: a later try can work.
fn transient(status: u16) -> bool {
    matches!(status, 408 | 429 | 500..=599)
}

/// The end of one transfer: a break keeps the partial file, a refusal
/// removes it.
enum End {
    Break(String),
    Refuse(Failure),
}

/// The state of one store: the open partial file, its digest so far, and
/// what the origin declared.
struct Part {
    path: PathBuf,
    file: File,
    hasher: Sha256,
    have: u64,
    /// The size of the whole object, when an answer declared it.
    total: Option<u64>,
    limit: u64,
}

impl Part {
    /// Start the file again from zero.
    fn restart(&mut self) -> Result<()> {
        self.file.set_len(0).map_err(|e| io_at(&self.path, e))?;
        self.file.seek(SeekFrom::Start(0)).map_err(|e| io_at(&self.path, e))?;
        self.hasher = Sha256::new();
        self.have = 0;
        self.total = None;
        Ok(())
    }

    /// Hash the first `len` bytes of the file, the bytes of an earlier call,
    /// and continue after them.
    fn rehash(&mut self, len: u64) -> Result<()> {
        let mut buf = vec![0u8; 1 << 20];
        let mut from = (&self.file).take(len);
        loop {
            match from.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => self.hasher.update(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(io_at(&self.path, e)),
            }
        }
        self.have = self.file.stream_position().map_err(|e| io_at(&self.path, e))?;
        Ok(())
    }

    /// Take the size of the object from an answer. A size that differs from
    /// an earlier answer is another object.
    fn declare(&mut self, total: Option<u64>, ceiling: Ceiling) -> std::result::Result<(), End> {
        check_declared(total, ceiling).map_err(End::Refuse)?;
        match (self.total, total) {
            (Some(was), Some(now)) if was != now => {
                Err(End::Refuse(Refusal::Size { declared: was, got: now }.failure()))
            }
            (None, Some(now)) => {
                self.total = Some(now);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Copy one body to the file, to its end or to `end` (the offset where
    /// the answer said that it stops). `Ok(true)`: the body ended where it
    /// said; `Ok(false)`: it ended with no size, the end of the object.
    fn copy(&mut self, body: &mut impl Read, end: Option<u64>) -> std::result::Result<bool, End> {
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = match body.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(End::Break(format!("read: {e}"))),
            };
            // `n` is at most the size of the buffer: it fits in a u64.
            let after = self.have.saturating_add(n as u64);
            if after > self.limit {
                return Err(End::Refuse(Refusal::OverCeiling { ceiling: self.limit }.failure()));
            }
            if let Some(e) = end.filter(|e| after > *e) {
                return Err(End::Refuse(Refusal::Size { declared: e, got: after }.failure()));
            }
            self.file.write_all(&buf[..n]).map_err(|e| End::Refuse(io_at(&self.path, e)))?;
            self.hasher.update(&buf[..n]);
            self.have = after;
        }
        match end {
            Some(e) if self.have < e => Err(End::Break(format!("the body ended at byte {} of {e}", self.have))),
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }
}

/// Fetch the object of `origin` to `dest` and return its sha256 in
/// lower-case hex.
///
/// The bytes go to [`partial`]`(dest)`. A break (an error of the origin, a
/// body that ends before its declared size, a status `408`, `429` or `5xx`)
/// takes one of `tries` and resumes with `Range: bytes=N-`; a `206` must
/// start at `N` ([`Refusal::Range`]), a `200` starts again from zero. A
/// partial file of an earlier call is resumed only when `want` pins the
/// digest: with no pin, a resume can join two versions of the object.
///
/// The destination gets its name only after the size of the file is the
/// declared size and the digest is `want`. A refusal of the bytes (a
/// ceiling, a size, a range, a digest) removes the partial file; a break
/// on the last try ([`Refusal::Broken`]) and another status
/// ([`Refusal::Status`]) keep it. `want` is the pinned digest, or `None`;
/// the digest comes back in both cases, so that the caller can record it.
pub fn store<O: Origin>(
    origin: &mut O,
    dest: &Path,
    want: Option<&str>,
    ceiling: Ceiling,
    tries: u32,
) -> Result<String> {
    let path = partial(dest);
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| io_at(&path, e))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => return Err(Refusal::Busy { part: path }.failure()),
        Err(std::fs::TryLockError::Error(e)) => return Err(io_at(&path, e)),
    }
    let mut part = Part { path, file, hasher: Sha256::new(), have: 0, total: None, limit: ceiling.effective() };
    match transfer(origin, &mut part, want.is_some(), ceiling, tries.max(1)) {
        Ok(()) => {}
        Err(end) => {
            let f = match end {
                End::Break(last) => {
                    Refusal::Broken { have: part.have, total: part.total, tries: tries.max(1), last }.failure()
                }
                End::Refuse(f) => f,
            };
            // A partial file with no byte is no resume.
            if part.have == 0 || !refusal(&f).is_some_and(Refusal::keeps_part) {
                let _ = std::fs::remove_file(&part.path);
            }
            return Err(f);
        }
    }
    // The output type of `sha2` 0.11 has no `LowerHex`, so the hex is
    // written here.
    let got: String = part.hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if let Some(w) = want.filter(|w| !w.eq_ignore_ascii_case(&got)) {
        let _ = std::fs::remove_file(&part.path);
        return Err(Refusal::Digest { want: w.to_string(), got }.failure());
    }
    part.file.sync_all().map_err(|e| io_at(&part.path, e))?;
    std::fs::rename(&part.path, dest).map_err(|e| io_at(dest, e))?;
    Ok(got)
}

/// The loop of the answers, to the end of the object or to the end of the
/// tries.
fn transfer<O: Origin>(
    origin: &mut O,
    part: &mut Part,
    resume: bool,
    ceiling: Ceiling,
    tries: u32,
) -> std::result::Result<(), End> {
    let refuse = End::Refuse;
    if resume {
        let len = part.file.metadata().map_err(|e| refuse(io_at(&part.path, e)))?.len();
        if len > part.limit {
            part.restart().map_err(refuse)?;
        } else {
            // The digest covers the whole file: hash the bytes of the
            // earlier call first.
            part.rehash(len).map_err(refuse)?;
        }
    } else {
        part.restart().map_err(refuse)?;
    }
    let mut breaks = 0u32;
    let mut last = String::new();
    while breaks < tries {
        if part.total.is_some_and(|t| part.have == t) {
            return Ok(());
        }
        let reply = match origin.get(part.have) {
            Ok(r) => r,
            Err(e) => {
                breaks += 1;
                last = format!("the origin: {e}");
                continue;
            }
        };
        let mut body = reply.body;
        let end = match reply.status {
            200 => {
                if part.have > 0 {
                    part.restart().map_err(refuse)?;
                }
                part.declare(reply.length, ceiling)?;
                reply.length
            }
            206 => {
                let span = reply.range.as_deref().and_then(parse_content_range);
                let Some(Span { start, end: Some(last_byte), total }) = span.filter(|s| s.start == part.have) else {
                    let got = reply.range.unwrap_or_default();
                    return Err(refuse(Refusal::Range { from: part.have, got }.failure()));
                };
                let size = last_byte.saturating_sub(start).saturating_add(1);
                if reply.length.is_some_and(|l| l != size) || total.is_some_and(|t| last_byte >= t) {
                    let got = reply.range.unwrap_or_default();
                    return Err(refuse(Refusal::Range { from: part.have, got }.failure()));
                }
                part.declare(total, ceiling)?;
                Some(last_byte.saturating_add(1))
            }
            416 => {
                // The range starts past the end: the object is whole when
                // its size is the size of the file, else start again.
                let total = reply.range.as_deref().and_then(parse_content_range).and_then(|s| s.total);
                if total == Some(part.have) && part.have > 0 {
                    part.declare(total, ceiling)?;
                    return Ok(());
                }
                part.restart().map_err(refuse)?;
                breaks += 1;
                last = "the origin answered 416".to_string();
                continue;
            }
            s if transient(s) => {
                breaks += 1;
                last = format!("the origin answered {s}");
                continue;
            }
            s => return Err(refuse(Refusal::Status { status: s }.failure())),
        };
        match part.copy(&mut body, end) {
            Ok(true) => {}
            // No size: the end of the body is the end of the object.
            Ok(false) => return Ok(()),
            Err(End::Break(why)) => {
                breaks += 1;
                last = why;
            }
            Err(e) => return Err(e),
        }
    }
    if part.total.is_some_and(|t| part.have == t) {
        return Ok(());
    }
    Err(End::Break(last))
}

#[cfg(test)]
mod tests;
