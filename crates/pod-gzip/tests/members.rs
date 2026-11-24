//! The members are one gzip file that gives the input back, with the same
//! bytes for each count of threads and each split of the writes; an error
//! of the inner writer comes out with its kind.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

use pod_gzip::{BLOCK, Members};

/// Bytes that gzip cannot shrink much: a xorshift stream.
fn noise(n: usize, seed: u64) -> Vec<u8> {
    let mut x = seed | 1;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

fn gunzip(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::MultiGzDecoder::new(b).read_to_end(&mut out).expect("one gzip file");
    out
}

/// Half noise, half one byte again.
fn input(size: usize) -> Vec<u8> {
    noise(size / 2, size as u64).into_iter().chain(std::iter::repeat_n(b'a', size - size / 2)).collect()
}

fn members(input: &[u8], threads: usize, split: usize) -> Vec<u8> {
    let mut w = Members::new(Vec::new(), threads);
    for chunk in input.chunks(split) {
        w.write_all(chunk).unwrap();
        w.flush().unwrap();
    }
    w.finish().unwrap()
}

/// One rule for each size and each count of threads: the members are one
/// gzip file that gives the input back, and the bytes do not depend on the
/// threads (the fingerprint of T-2081 stays the same for the same tarball).
#[test]
fn the_members_give_the_input_back_with_the_same_bytes_for_each_thread_count() {
    let sizes = [0, 1, 4096, BLOCK - 1, BLOCK, BLOCK + 1, 3 * BLOCK + 5];
    for size in sizes {
        let input = input(size);
        let mut first: Option<Vec<u8>> = None;
        for threads in [1, 2, 3, 8, 64] {
            // Writes of odd sizes, so that the cut of the blocks is the
            // writer's, not the caller's.
            let out = members(&input, threads, 7919);
            assert!(gunzip(&out) == input, "size {size}, {threads} threads");
            match &first {
                None => first = Some(out),
                Some(f) => assert!(&out == f, "size {size}: {threads} threads, other bytes"),
            }
        }
    }
}

/// The split of the writes does not change a byte: 1 byte, 4097 bytes and
/// one block for each write, at 1, 2, 4 and 8 threads.
#[test]
fn the_split_of_the_writes_does_not_change_the_bytes() {
    let input = input(2 * BLOCK + 3);
    let want = members(&input, 1, BLOCK);
    for threads in [1, 2, 4, 8] {
        for split in [1, 4097, BLOCK] {
            assert!(members(&input, threads, split) == want, "{threads} threads, writes of {split} bytes");
        }
    }
}

/// The count of members is the count of blocks: each member ends at the
/// end of its block, also where the caller flushes.
#[test]
fn each_block_is_one_member_with_a_fixed_header() {
    let out = members(&input(3 * BLOCK + 5), 4, 4097);
    // ID1 ID2 CM, FLG 0 (no name), MTIME 0, XFL 4 (the fast level), OS 255.
    let header = [0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 4, 255];
    assert_eq!(out[..10], header);
    let mut starts = 0;
    let mut rest = &out[..];
    while !rest.is_empty() {
        assert_eq!(rest[..10], header, "member {starts}");
        let mut d = flate2::bufread::GzDecoder::new(rest);
        let mut one = Vec::new();
        d.read_to_end(&mut one).unwrap();
        assert!(one.len() == BLOCK || rest.len() < 2 * BLOCK, "member {starts}: {} bytes", one.len());
        rest = d.into_inner();
        starts += 1;
    }
    assert_eq!(starts, 4);
}

/// A stream with no byte is one empty member: a valid gzip file.
#[test]
fn an_empty_stream_is_one_empty_member() {
    let out = Members::new(Vec::new(), 4).finish().unwrap();
    assert_eq!(out.len(), 20, "a header of 10 bytes, an empty block of 2, a trailer of 8");
    assert!(gunzip(&out).is_empty());
}

/// gzip of the host reads the stream back, where the host has it.
#[test]
fn gzip_of_the_host_reads_the_members_back() {
    let input = input(3 * BLOCK + 5);
    let out = members(&input, 4, 7919);
    let child = Command::new("gzip").arg("-dc").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn();
    let Ok(mut child) = child else {
        eprintln!("skip: no gzip on this host");
        return;
    };
    let mut stdin = child.stdin.take().unwrap();
    let feed = std::thread::spawn(move || stdin.write_all(&out));
    let got = child.wait_with_output().unwrap();
    feed.join().unwrap().unwrap();
    assert!(got.status.success(), "gzip -dc: {}", got.status);
    assert!(got.stdout == input, "gzip -dc gives other bytes");
}

/// A writer that fails after `left` bytes, as a pipe that the reader closed.
#[derive(Debug)]
struct Failing {
    left: usize,
}

impl Write for Failing {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.left == 0 {
            return Err(std::io::ErrorKind::BrokenPipe.into());
        }
        let n = buf.len().min(self.left);
        self.left -= n;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_reader_that_goes_gives_the_error_of_the_pipe() {
    let input = noise(3 * BLOCK, 9);
    for left in [0, 100, BLOCK] {
        let mut w = Members::new(Failing { left }, 4);
        let wrote = w.write_all(&input);
        let e = match wrote {
            Err(e) => e,
            Ok(()) => w.finish().expect_err("an error"),
        };
        assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe, "left {left}");
    }
}

/// A writer that keeps each member whole: one call for each member.
struct Counting {
    members: std::sync::Arc<std::sync::Mutex<usize>>,
}

impl Write for Counting {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        *self.members.lock().unwrap() += 1;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The members go out while the stream goes on: after each block, at most
/// the limit of blocks is not yet written to the inner writer.
#[test]
fn the_members_reach_the_inner_writer_while_the_stream_goes_on() {
    let count = std::sync::Arc::new(std::sync::Mutex::new(0));
    let mut w = Members::new(Counting { members: count.clone() }, 2);
    let block = input(BLOCK);
    for sent in 1..=12 {
        w.write_all(&block).unwrap();
        let wrote = *count.lock().unwrap();
        assert!(sent - wrote <= w.limit(), "{sent} blocks, {wrote} members written, limit {}", w.limit());
        assert!(w.in_flight() <= w.limit());
    }
    w.finish().unwrap();
    assert_eq!(*count.lock().unwrap(), 12);
}

/// A writer that fails at its first call only, as an error that a retry
/// of the caller would pass.
struct FailsOnce {
    failed: bool,
    got: Vec<u8>,
}

impl Write for FailsOnce {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if !self.failed {
            self.failed = true;
            return Err(std::io::ErrorKind::BrokenPipe.into());
        }
        self.got.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl std::fmt::Debug for FailsOnce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FailsOnce({} bytes)", self.got.len())
    }
}

/// After an error, each call fails: a stream with a lost member never
/// ends as a success.
#[test]
fn after_an_error_of_the_inner_writer_each_call_fails() {
    let block = input(BLOCK);
    let mut w = Members::new(FailsOnce { failed: false, got: Vec::new() }, 2);
    let mut first = None;
    for sent in 1..=8 {
        match w.write_all(&block) {
            Err(_) if first.is_none() => first = Some(sent),
            Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe, "block {sent}"),
            Ok(()) => assert!(first.is_none(), "block {sent} written after the error of block {first:?}"),
        }
    }
    assert!(first.is_some(), "no error");
    assert_eq!(w.finish().expect_err("an error").kind(), std::io::ErrorKind::BrokenPipe);
}
