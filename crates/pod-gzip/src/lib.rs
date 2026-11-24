//! A writer of gzip members.
//!
//! [`Members`] cuts the stream into blocks of [`BLOCK`] bytes and writes
//! each block as one gzip member. The concatenation of gzip members is one
//! gzip file (RFC 1952, section 2.2), which `gzip -d`, `tar -z` and
//! `flate2::read::MultiGzDecoder` read.
//!
//! The bytes do not depend on the count of threads or on the sizes of the
//! writes: the header of each member is fixed (no name, time 0, the XFL of
//! the level, OS 255), and each block is cut at the same offset. So the
//! SHA-256 of the stream is the same for the same input, level and version
//! of the backend. A change of `flate2` or `miniz_oxide` can change the
//! bytes: `tests/golden.rs` holds the SHA-256 of one input.

use std::io::{self, Write};
use std::sync::Arc;

use flate2::write::GzEncoder;
use flate2::{Compression, GzBuilder};

use crate::pool::Pool;

mod pool;

/// The bytes of one gzip member. Each member starts with no dictionary: a
/// block of 2 MiB loses less than 0.1 % of the ratio of one stream.
pub const BLOCK: usize = 2 << 20;

/// The most threads of one writer.
pub const MAX_THREADS: usize = 8;

/// The level of [`Members::new`]: the fast level of gzip (`gzip -1`).
pub const FAST: u32 = 1;

/// The threads of this host: its parallelism, at most [`MAX_THREADS`], 1
/// where the kernel does not say.
pub fn host_threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get().min(MAX_THREADS))
}

/// The compression of one block into one member, at a level. A test gives
/// its own, to hold a thread or to fail.
type Compress = Arc<dyn Fn(&[u8], u32) -> io::Result<Vec<u8>> + Send + Sync>;

/// A writer of gzip members into `inner`.
///
/// A pool of `threads` threads compresses the blocks while the caller
/// writes the next ones; the caller's thread writes the members to `inner`
/// in their order. At most [`Members::limit`] blocks are in flight, and one
/// more fills: a slow pool or a slow `inner` makes a fast producer wait.
/// Drop stops and joins the threads.
pub struct Members<W: Write> {
    inner: W,
    pool: Pool,
    limit: usize,
    cur: Vec<u8>,
    /// The blocks given to the pool, and the members written.
    sent: u64,
    written: u64,
    /// The first error: each later call gives it again.
    failed: Option<(io::ErrorKind, String)>,
}

impl<W: Write> Members<W> {
    /// A writer with `threads` threads (from 1 to [`MAX_THREADS`]) at the
    /// fast level of gzip.
    pub fn new(inner: W, threads: usize) -> Members<W> {
        Members::with_level(inner, threads, FAST)
    }

    /// A writer with `threads` threads at `level` (0 to 9; a higher value
    /// is 9).
    pub fn with_level(inner: W, threads: usize, level: u32) -> Members<W> {
        Members::with_compress(inner, threads, level, Arc::new(member))
    }

    fn with_compress(inner: W, threads: usize, level: u32, compress: Compress) -> Members<W> {
        let threads = threads.clamp(1, MAX_THREADS);
        // One block more than the threads: a thread that ends its block
        // finds the next one in the queue.
        let limit = threads + 1;
        Members {
            inner,
            pool: Pool::new(threads, limit, level.min(9), compress),
            limit,
            cur: Vec::with_capacity(BLOCK),
            sent: 0,
            written: 0,
            failed: None,
        }
    }

    /// The most blocks in flight: given to the threads, with no member
    /// written to the inner writer. It is `threads` + 1.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// The blocks in flight now.
    pub fn in_flight(&self) -> usize {
        (self.sent - self.written) as usize
    }

    /// Give a full block to the pool: first wait until fewer than the
    /// limit are in flight, then write each member that is there.
    fn send(&mut self, block: Vec<u8>) -> io::Result<()> {
        while self.in_flight() >= self.limit {
            self.write_next(true)?;
        }
        self.pool.send(self.sent, block)?;
        self.sent += 1;
        while self.written < self.sent && self.write_next(false)? {}
        Ok(())
    }

    /// Write the member of the next block; with `wait`, wait for it.
    /// False where it is not there.
    fn write_next(&mut self, wait: bool) -> io::Result<bool> {
        let Some(m) = self.pool.take(self.written, wait)? else { return Ok(false) };
        self.inner.write_all(&m)?;
        self.written += 1;
        Ok(true)
    }

    /// The error of an earlier call, if any.
    fn check(&self) -> io::Result<()> {
        match &self.failed {
            Some((kind, text)) => Err(io::Error::new(*kind, text.clone())),
            None => Ok(()),
        }
    }

    /// Keep the first error and stop the threads.
    fn fail(&mut self, e: io::Error) -> io::Error {
        self.failed = Some((e.kind(), e.to_string()));
        self.pool.stop();
        e
    }

    /// Write the last block and flush: the inner writer. A stream with no
    /// byte is one empty member, which is a valid gzip file.
    pub fn finish(mut self) -> io::Result<W> {
        self.check()?;
        let end = (|| {
            if !self.cur.is_empty() || self.sent == 0 {
                let last = std::mem::take(&mut self.cur);
                self.send(last)?;
            }
            while self.written < self.sent {
                self.write_next(true)?;
            }
            self.inner.flush()
        })();
        if let Err(e) = end {
            return Err(self.fail(e));
        }
        let Members { inner, pool, .. } = self;
        drop(pool);
        Ok(inner)
    }
}

/// One gzip member of `block` at `level`, with a fixed header.
fn member(block: &[u8], level: u32) -> io::Result<Vec<u8>> {
    let out = Vec::with_capacity(block.len() / 2 + 64);
    let mut e: GzEncoder<Vec<u8>> = GzBuilder::new().mtime(0).operating_system(255).write(out, Compression::new(level));
    e.write_all(block)?;
    e.finish()
}

impl<W: Write> Write for Members<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.check()?;
        let n = buf.len().min(BLOCK - self.cur.len());
        self.cur.extend_from_slice(&buf[..n]);
        if self.cur.len() == BLOCK {
            let block = std::mem::replace(&mut self.cur, Vec::with_capacity(BLOCK));
            if let Err(e) = self.send(block) {
                return Err(self.fail(e));
            }
        }
        Ok(n)
    }

    /// A flush writes nothing: a member ends only at its block's end, so
    /// that the bytes do not depend on where a caller flushes. It gives the
    /// error of an earlier call.
    fn flush(&mut self) -> io::Result<()> {
        self.check()
    }
}

#[cfg(test)]
mod tests;
