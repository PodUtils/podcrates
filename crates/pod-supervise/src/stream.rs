//! The log as a stream: each read holds one buffer of [`CHUNK`] bytes.
//!
//! A copy of a range goes from the start, one piece at a time, and the
//! start of the last `n` lines is found by reads back from the end. Thus a
//! caller that reads the log of each job stops the read at any byte, and
//! the memory of a read is one piece, not the size of the log. The
//! functions work on any file, so the serial log of a machine takes them
//! too.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde::{Serialize, de::DeserializeOwned};

use crate::{Result, Settings, get, io_at, table};

/// The one buffer of a read of a log: the bound of its memory.
pub const CHUNK: usize = 64 * 1024;

/// Why a copy stopped before its end.
#[derive(Debug)]
pub enum Fault {
    /// The log could not be read.
    Read(std::io::Error),
    /// The output refused a write (a closed pipe, a full disk).
    Write(std::io::Error),
}

/// Open the file at `path` for a read, with no link followed: a link at a
/// log could name any file of the user. `None`: no file, an empty log.
pub fn open(path: &Path) -> std::io::Result<Option<File>> {
    use std::os::unix::fs::OpenOptionsExt;
    let opened = std::fs::OpenOptions::new().read(true).custom_flags(pod_hostprobe::sys::O_NOFOLLOW as i32).open(path);
    match opened {
        Ok(f) => Ok(Some(f)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// The log file of the container `want`, open for a read, and its length
/// at the open. `None`: the container wrote nothing yet.
pub fn open_log<X>(s: &Settings, want: &str) -> Result<Option<(File, u64)>>
where
    X: Clone + Serialize + DeserializeOwned,
{
    let c = get::<X>(s, want)?;
    open_sized(&table::log_path(s, &c.id))
}

/// [`open`] with the length at the open, and the path in each failure.
pub(crate) fn open_sized(path: &Path) -> Result<Option<(File, u64)>> {
    let Some(f) = open(path).map_err(|e| io_at(path, e))? else {
        return Ok(None);
    };
    let len = f.metadata().map_err(|e| io_at(path, e))?.len();
    Ok(Some((f, len)))
}

/// The byte offset where the last `n` lines of the first `len` bytes of
/// `f` start, by the rule of [`crate::tail_offset`], read from the end in
/// pieces of [`CHUNK`] bytes.
pub fn tail_start<F: Read + Seek>(f: &mut F, len: u64, n: u64) -> std::io::Result<u64> {
    tail_start_by(f, len, n, CHUNK)
}

/// [`tail_start`] with the size of its piece, so that a test crosses the
/// edge of a piece with a short file.
pub fn tail_start_by<F: Read + Seek>(f: &mut F, len: u64, n: u64, chunk: usize) -> std::io::Result<u64> {
    if n == 0 || len == 0 {
        return Ok(len);
    }
    let mut buf = vec![0u8; chunk.clamp(1, CHUNK)];
    // One `\n` at the end ends the last line and starts no empty one.
    let mut last = [0u8; 1];
    if !read_at(f, len - 1, &mut last)? {
        return Ok(0);
    }
    let mut hi = if last[0] == b'\n' { len - 1 } else { len };
    let mut seen = 0u64;
    while hi > 0 {
        let lo = hi.saturating_sub(buf.len() as u64);
        let piece = &mut buf[..(hi - lo) as usize];
        // A file that became shorter during the read: its start.
        if !read_at(f, lo, piece)? {
            return Ok(0);
        }
        for (i, b) in piece.iter().enumerate().rev() {
            if *b == b'\n' {
                seen += 1;
                if seen == n {
                    return Ok(lo + i as u64 + 1);
                }
            }
        }
        hi = lo;
    }
    Ok(0)
}

/// Fill `buf` from `at`; false where the file ends first.
fn read_at<F: Read + Seek>(f: &mut F, at: u64, buf: &mut [u8]) -> std::io::Result<bool> {
    f.seek(SeekFrom::Start(at))?;
    match f.read_exact(buf) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(e) => Err(e),
    }
}

/// Copy the bytes of `f` from `start` to `end` to `out`, at most `limit`
/// of them, one [`CHUNK`] at a time, and flush. A file that ends before
/// `end` ends the copy there. Gives the count of bytes written.
pub fn copy_range<F: Read + Seek>(
    f: &mut F,
    start: u64,
    end: u64,
    limit: Option<u64>,
    out: &mut dyn Write,
) -> std::result::Result<u64, Fault> {
    f.seek(SeekFrom::Start(start)).map_err(Fault::Read)?;
    let mut left = end.saturating_sub(start);
    if let Some(l) = limit {
        left = left.min(l);
    }
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    while left > 0 {
        let want = left.min(CHUNK as u64) as usize;
        let got = match f.read(&mut buf[..want]) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Fault::Read(e)),
        };
        out.write_all(&buf[..got]).map_err(Fault::Write)?;
        done += got as u64;
        left -= got as u64;
    }
    out.flush().map_err(Fault::Write)?;
    Ok(done)
}

/// The first part of a log of `len` bytes: from the start, or from its
/// last `tail` lines ([`tail_start`]), to `len`, at most `limit` bytes
/// ([`copy_range`]). Gives the count of bytes written.
pub fn copy_tail<F: Read + Seek>(
    f: &mut F,
    len: u64,
    tail: Option<u64>,
    limit: Option<u64>,
    out: &mut dyn Write,
) -> std::result::Result<u64, Fault> {
    let start = match tail {
        Some(n) => tail_start(f, len, n).map_err(Fault::Read)?,
        None => 0,
    };
    copy_range(f, start, len, limit, out)
}

/// A writer that passes `left` bytes, then refuses each write. The write
/// that reaches the bound passes its bytes and then gives an error, so a
/// loop that follows a log stops at once, also when no more bytes come.
pub struct Capped<'a> {
    inner: &'a mut dyn Write,
    left: u64,
    full: bool,
}

impl<'a> Capped<'a> {
    pub fn new(inner: &'a mut dyn Write, left: u64) -> Capped<'a> {
        Capped { inner, left, full: left == 0 }
    }

    /// True once the bound was reached: an error of a write is then the
    /// end of the copy, not a fault.
    pub fn full(&self) -> bool {
        self.full
    }

    /// The bytes that the bound still lets pass.
    pub fn left(&self) -> u64 {
        self.left
    }
}

impl Write for Capped<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.full {
            return Err(bound_reached());
        }
        let n = buf.len().min(usize::try_from(self.left).unwrap_or(usize::MAX));
        self.inner.write_all(&buf[..n])?;
        self.left -= n as u64;
        if self.left == 0 {
            self.full = true;
            self.inner.flush()?;
            return Err(bound_reached());
        }
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn bound_reached() -> std::io::Error {
    std::io::Error::other("the byte bound of the copy is reached")
}

#[cfg(test)]
mod tests;
