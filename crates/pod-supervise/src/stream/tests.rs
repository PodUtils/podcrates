//! The stream of a log against the whole read, and the bound of its memory.

use std::io::{Cursor, Read, Seek, SeekFrom, Write};

use super::*;

/// The texts of the tail: no line end, one at the end, empty lines, a
/// line longer than a piece, and lines that cross the edge of a piece.
const TEXTS: &[&[u8]] = &[
    b"",
    b"\n",
    b"\n\n\n",
    b"a",
    b"a\n",
    b"a\nb",
    b"a\nb\n",
    b"one\ntwo\nthree\n",
    b"one\n\ntwo\n\n",
    b"a-long-line-past-the-piece\nb\nc-also-long-past-it\n",
    b"x\ny\nz",
];

/// The rule of each count and each size of piece is the rule of the whole
/// read (`crate::tail_offset`).
#[test]
fn the_tail_from_the_end_is_the_tail_of_the_whole_read() {
    for text in TEXTS {
        for n in 0..6u64 {
            let want = crate::tail_offset(text, n);
            for chunk in [1usize, 2, 3, 5, 64, CHUNK] {
                let mut f = Cursor::new(text.to_vec());
                let got = tail_start_by(&mut f, text.len() as u64, n, chunk).unwrap();
                assert_eq!(got, want, "{:?} n={n} chunk={chunk}", String::from_utf8_lossy(text));
            }
        }
    }
}

/// A file shorter than the length of the open gives its start, never an
/// error and never a panic.
#[test]
fn a_file_that_became_shorter_gives_the_start() {
    let mut f = Cursor::new(b"a\nb\n".to_vec());
    assert_eq!(tail_start_by(&mut f, 100, 1, 3).unwrap(), 0);
}

/// The copy: each start, end and bound gives the same bytes as a slice of
/// the whole read.
#[test]
fn the_copy_is_the_slice_of_the_whole_read() {
    let text: Vec<u8> = (0..(CHUNK * 2 + 17)).map(|i| (i % 251) as u8).collect();
    let len = text.len() as u64;
    let cases: &[(u64, u64, Option<u64>)] = &[
        (0, len, None),
        (0, len, Some(1)),
        (0, len, Some(CHUNK as u64)),
        (0, len, Some(CHUNK as u64 + 1)),
        (0, len, Some(len + 5)),
        (5, len, Some(10)),
        (len, len, None),
        (0, len + 100, None), // the file ends first
        (len + 3, len + 9, None),
    ];
    for &(start, end, limit) in cases {
        let mut out = Vec::new();
        let n = copy_range(&mut Cursor::new(text.clone()), start, end, limit, &mut out).unwrap();
        let lo = start.min(len) as usize;
        let mut hi = end.min(len) as usize;
        if let Some(l) = limit {
            hi = hi.min(lo + l as usize);
        }
        assert_eq!(out, text[lo..hi.max(lo)], "{start} {end} {limit:?}");
        assert_eq!(n, out.len() as u64);
    }
}

/// A writer that refuses each write, as a closed pipe does.
struct Closed;

impl Write for Closed {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::ErrorKind::BrokenPipe.into())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A closed output is a fault of the write, so the caller names it as one.
#[test]
fn a_closed_output_is_a_fault_of_the_write() {
    let r = copy_range(&mut Cursor::new(b"abc".to_vec()), 0, 3, None, &mut Closed);
    assert!(matches!(r, Err(Fault::Write(e)) if e.kind() == std::io::ErrorKind::BrokenPipe));
}

/// The bound: the write that reaches it passes its bytes and stops the
/// writer; a write before it passes all.
#[test]
fn the_cap_passes_its_bytes_and_then_refuses() {
    for (bound, writes, want) in [
        (5u64, &[&b"ab"[..], b"cd", b"efgh"][..], &b"abcde"[..]),
        (2, &[b"ab", b"cd"], b"ab"),
        (10, &[b"ab", b"cd"], b"abcd"),
        (0, &[b"ab"], b""),
    ] {
        let mut sink = Vec::new();
        let mut cap = Capped::new(&mut sink, bound);
        let mut stopped = false;
        for w in writes {
            if cap.write_all(w).is_err() {
                stopped = true;
                break;
            }
        }
        assert_eq!(stopped, cap.full(), "{bound}");
        assert_eq!(cap.left(), bound - want.len() as u64, "{bound}");
        assert_eq!(sink, want, "{bound}");
    }
}

/// A file that counts each byte that a read gives and the largest read.
struct Counted {
    inner: Cursor<Vec<u8>>,
    read: u64,
    largest: usize,
}

impl Read for Counted {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.largest = self.largest.max(buf.len());
        let n = self.inner.read(buf)?;
        self.read += n as u64;
        Ok(n)
    }
}

impl Seek for Counted {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(to)
    }
}

/// A writer that keeps the count of the bytes and the largest write, not
/// the bytes.
#[derive(Default)]
struct Sink {
    written: u64,
    largest: usize,
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.largest = self.largest.max(buf.len());
        self.written += buf.len() as u64;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A log of 4 MiB: lines of 100 bytes, then 3 short lines.
fn large_log() -> Vec<u8> {
    let mut log = Vec::with_capacity(4 << 20);
    while log.len() < 4 << 20 {
        log.extend_from_slice(&[b'x'; 99]);
        log.push(b'\n');
    }
    log.extend_from_slice(b"one\ntwo\nthree\n");
    log
}

/// The memory bound: the tail of 3 lines of a log of 4 MiB reads at most
/// two pieces, and the copy reads and writes one piece at a time, never
/// the whole log.
#[test]
fn the_tail_and_the_copy_of_a_large_log_read_one_piece_at_a_time() {
    let log = large_log();
    let len = log.len() as u64;
    let mut f = Counted { inner: Cursor::new(log.clone()), read: 0, largest: 0 };
    let start = tail_start(&mut f, len, 3).unwrap();
    assert_eq!(start, crate::tail_offset(&log, 3));
    assert!(f.read <= 2 * CHUNK as u64, "the tail read {} bytes", f.read);
    assert!(f.largest <= CHUNK, "a read of {} bytes", f.largest);

    let mut f = Counted { inner: Cursor::new(log), read: 0, largest: 0 };
    let mut out = Sink::default();
    let n = copy_tail(&mut f, len, None, None, &mut out).unwrap();
    assert_eq!((n, out.written, f.read), (len, len, len));
    assert!(f.largest <= CHUNK && out.largest <= CHUNK, "a piece of {} or {} bytes", f.largest, out.largest);
}

/// `copy_tail` is the tail of the whole read, cut at the bound.
#[test]
fn the_copy_of_a_tail_is_the_tail_of_the_whole_read() {
    let log = large_log();
    let len = log.len() as u64;
    for (tail, limit) in [(Some(2), None), (Some(2), Some(3)), (None, Some(5)), (Some(0), None)] {
        let mut out = Vec::new();
        let n = copy_tail(&mut Cursor::new(log.clone()), len, tail, limit, &mut out).unwrap();
        let start = tail.map_or(0, |t| crate::tail_offset(&log, t)) as usize;
        let end = limit.map_or(log.len(), |l| (start + l as usize).min(log.len()));
        assert_eq!(out, log[start..end], "{tail:?} {limit:?}");
        assert_eq!(n, out.len() as u64);
    }
}
