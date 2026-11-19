//! The members of a compressed layer. A gzip stream of more than one member
//! (eStargz, bgzip, a concatenation) and a zstd stream of more than one frame
//! are each one layer: the tar archive goes on in the next member
//! (PodUtils/podrunner#317). A decoder of one member stops at the end of the
//! first, and the archive is then cut short with no error.
//!
//! After a member, the next one starts only when the bytes that follow begin
//! with its magic. Other bytes after the last member (zeros of a padded
//! blob) are no member: they stay for [`crate::layer::HashedStream::finish`],
//! which hashes the rest of the blob. A zstd skippable frame is skipped.

use std::io::{self, BufRead, Read};

use crate::layer::Compression;

const GZIP: &[u8] = &[0x1f, 0x8b];
const ZSTD: &[u8] = &[0x28, 0xb5, 0x2f, 0xfd];

/// A read-ahead that can look at the next `n` bytes, also across the end
/// of its buffer, with no read past them.
pub(crate) struct Ahead<R> {
    inner: R,
    buf: Vec<u8>,
    pos: usize,
}

impl<R: Read> Ahead<R> {
    fn new(inner: R) -> Self {
        Ahead { inner, buf: Vec::with_capacity(64 * 1024), pos: 0 }
    }

    /// The next `n` bytes, or fewer at the end of the stream.
    fn peek(&mut self, n: usize) -> io::Result<&[u8]> {
        if self.buf.len() - self.pos < n {
            self.buf.drain(..self.pos);
            self.pos = 0;
            let mut chunk = [0u8; 4096];
            while self.buf.len() < n {
                let got = self.inner.read(&mut chunk)?;
                if got == 0 {
                    break;
                }
                self.buf.extend_from_slice(&chunk[..got]);
            }
        }
        let end = (self.pos + n).min(self.buf.len());
        Ok(&self.buf[self.pos..end])
    }
}

impl<R: Read> BufRead for Ahead<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.pos == self.buf.len() {
            self.buf.clear();
            self.pos = 0;
            self.buf.resize(64 * 1024, 0);
            let got = self.inner.read(&mut self.buf)?;
            self.buf.truncate(got);
        }
        Ok(&self.buf[self.pos..])
    }

    fn consume(&mut self, amt: usize) {
        self.pos = (self.pos + amt).min(self.buf.len());
    }
}

impl<R: Read> Read for Ahead<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let avail = self.fill_buf()?;
        let n = avail.len().min(out.len());
        out[..n].copy_from_slice(&avail[..n]);
        self.consume(n);
        Ok(n)
    }
}

/// The decoder of one member.
enum Member<R: BufRead> {
    Gzip(flate2::bufread::GzDecoder<R>),
    Zstd(Box<ruzstd::decoding::StreamingDecoder<R, ruzstd::decoding::FrameDecoder>>),
}

impl<R: BufRead> Member<R> {
    fn into_inner(self) -> R {
        match self {
            Member::Gzip(d) => d.into_inner(),
            Member::Zstd(d) => d.into_inner(),
        }
    }
}

/// The decompressed bytes of each member of a gzip or a zstd layer, one
/// after the other.
pub(crate) struct Members<R: Read> {
    kind: Compression,
    cur: Option<Member<Ahead<R>>>,
}

impl<R: Read> Members<R> {
    /// The first member starts in each case: a blob that is not of `kind`
    /// fails at its first read, as with a decoder of one member.
    pub(crate) fn new(kind: Compression, r: R) -> io::Result<Self> {
        let mut m = Members { kind, cur: None };
        m.cur = m.start(Ahead::new(r), true)?;
        Ok(m)
    }

    /// The member at the start of `r`; `None` when the bytes there are no
    /// member and `first` is false.
    fn start(&self, mut r: Ahead<R>, first: bool) -> io::Result<Option<Member<Ahead<R>>>> {
        if self.kind == Compression::Zstd {
            skip_skippable_frames(&mut r)?;
        }
        let magic = if self.kind == Compression::Gzip { GZIP } else { ZSTD };
        if !first && r.peek(magic.len())? != magic {
            return Ok(None);
        }
        Ok(Some(match self.kind {
            Compression::Gzip => Member::Gzip(flate2::bufread::GzDecoder::new(r)),
            _ => Member::Zstd(Box::new(
                ruzstd::decoding::StreamingDecoder::new(r)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("a zstd frame: {e}")))?,
            )),
        }))
    }
}

impl<R: Read> Read for Members<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while let Some(member) = self.cur.as_mut() {
            let n = match member {
                Member::Gzip(d) => d.read(out)?,
                Member::Zstd(d) => d.read(out)?,
            };
            if n > 0 || out.is_empty() {
                return Ok(n);
            }
            let rest = self.cur.take().map(Member::into_inner).expect("a member");
            self.cur = self.start(rest, false)?;
        }
        Ok(0)
    }
}

/// Skip each zstd skippable frame at the start of `r` (magic
/// `0x184D2A50` to `0x184D2A5F`, then a length of four bytes).
fn skip_skippable_frames<R: Read>(r: &mut Ahead<R>) -> io::Result<()> {
    loop {
        let head = r.peek(8)?;
        if head.len() < 8 || head[0] & 0xf0 != 0x50 || head[1..4] != [0x2a, 0x4d, 0x18] {
            return Ok(());
        }
        let len = u64::from(u32::from_le_bytes([head[4], head[5], head[6], head[7]]));
        r.consume(8);
        let skipped = io::copy(&mut r.by_ref().take(len), &mut io::sink())?;
        if skipped != len {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "a zstd skippable frame ends early"));
        }
    }
}

#[cfg(test)]
mod tests;
