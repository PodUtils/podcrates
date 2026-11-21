//! The line frame: one cap of length, a check of UTF-8, and a deadline for
//! each line. A slow peer that sends one byte at a time meets the deadline
//! of the whole line, not a new timeout for each byte.
//!
//! A line can hold a token: the frame wipes each byte of a line that it
//! gives or drops, in its buffer and in the old buffer at each growth.

use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use zeroize::{Zeroize, Zeroizing};

/// A connected byte stream of the control: a Unix stream or the end of a
/// named pipe.
pub trait Stream: Read + Write + Send {
    /// The longest wait of each later read; `None` waits with no limit.
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()>;
    /// The longest wait of each later write.
    fn set_write_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()>;
}

#[cfg(unix)]
impl Stream for std::os::unix::net::UnixStream {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        shut_needs_no_timeout(std::os::unix::net::UnixStream::set_read_timeout(self, timeout))
    }

    fn set_write_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        shut_needs_no_timeout(std::os::unix::net::UnixStream::set_write_timeout(self, timeout))
    }
}

/// Darwin refuses each socket option with `EINVAL` when both directions of
/// the socket are shut, as after the close of the peer. A read of such a
/// socket gives the bytes that it holds, then the end, and a write fails at
/// once: no call waits, so the socket needs no timeout.
#[cfg(unix)]
fn shut_needs_no_timeout(set: io::Result<()>) -> io::Result<()> {
    const EINVAL: i32 = 22;
    match set {
        Err(e) if cfg!(target_vendor = "apple") && e.raw_os_error() == Some(EINVAL) => Ok(()),
        set => set,
    }
}

impl<S: Stream + ?Sized> Stream for Box<S> {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        (**self).set_read_timeout(timeout)
    }

    fn set_write_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        (**self).set_write_timeout(timeout)
    }
}

/// What one read of a line gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// A whole line, with no line end. Its text is wiped at its drop.
    Text(Zeroizing<String>),
    /// The peer closed. A part of a line before the close is dropped.
    End,
    /// The line is longer than the cap. The frame is lost: close.
    TooLong,
    /// A whole line that is not UTF-8. The frame holds: the next line can
    /// be read.
    NotUtf8,
    /// No whole line came before the deadline.
    Timeout,
}

/// The lines of a stream, at most `cap` bytes each without the line end.
pub struct Lines<S> {
    stream: S,
    /// Each byte is written before it is read: the reads fill `buf[filled..]`,
    /// and the bytes past `filled` are zero.
    buf: Zeroizing<Vec<u8>>,
    filled: usize,
    cap: usize,
}

const CHUNK: usize = 8 * 1024;

impl<S: Stream> Lines<S> {
    pub fn new(stream: S, cap: usize) -> Lines<S> {
        Lines { stream, buf: Zeroizing::new(Vec::new()), filled: 0, cap }
    }

    pub fn stream(&mut self) -> &mut S {
        &mut self.stream
    }

    /// The next line, whole before `timeout` from now.
    pub fn next_line(&mut self, timeout: Duration) -> io::Result<Line> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(end) = self.buf[..self.filled].iter().position(|b| *b == b'\n') {
                return Ok(self.take(end));
            }
            if self.filled > self.cap {
                return Ok(Line::TooLong);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(Line::Timeout);
            }
            if self.filled == self.buf.len() {
                self.grow();
            }
            self.stream.set_read_timeout(Some(left))?;
            match self.stream.read(&mut self.buf[self.filled..]) {
                Ok(0) => return Ok(Line::End),
                Ok(n) => self.filled += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                    return Ok(Line::Timeout);
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// The line of `buf[..end]`, with its line end at `end`. The bytes after
    /// it move to the start, and each byte that they leave is wiped.
    fn take(&mut self, end: usize) -> Line {
        let line = (end <= self.cap).then(|| Zeroizing::new(self.buf[..end].to_vec()));
        let rest = self.filled - end - 1;
        self.buf.copy_within(end + 1..self.filled, 0);
        self.buf[rest..self.filled].zeroize();
        self.filled = rest;
        let Some(mut line) = line else { return Line::TooLong };
        match String::from_utf8(std::mem::take(&mut *line)) {
            Ok(text) => Line::Text(Zeroizing::new(text)),
            Err(e) => {
                drop(Zeroizing::new(e.into_bytes()));
                Line::NotUtf8
            }
        }
    }

    /// A buffer of twice the size, at most one byte past the cap. The old
    /// buffer is wiped at its drop: a growth in place of the allocator would
    /// leave a copy that no one wipes.
    fn grow(&mut self) {
        let size = (self.buf.len() * 2).clamp(CHUNK, self.cap.saturating_add(1).max(CHUNK));
        let mut bigger = Zeroizing::new(vec![0u8; size]);
        bigger[..self.filled].copy_from_slice(&self.buf[..self.filled]);
        self.buf = bigger;
    }

    /// Write `line` whole; it holds its line end.
    pub fn write_line(&mut self, line: &str) -> io::Result<()> {
        self.stream.write_all(line.as_bytes())?;
        self.stream.flush()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    fn text(t: &str) -> Line {
        Line::Text(Zeroizing::new(t.to_string()))
    }

    fn pair(cap: usize) -> (Lines<UnixStream>, UnixStream) {
        let (a, b) = UnixStream::pair().unwrap();
        (Lines::new(a, cap), b)
    }

    #[test]
    fn only_the_einval_of_darwin_needs_no_timeout() {
        let einval = || Err(io::Error::from_raw_os_error(22));
        assert_eq!(shut_needs_no_timeout(einval()).is_ok(), cfg!(target_vendor = "apple"));
        // An EINVAL of the standard library (a zero timeout) has no code of the system.
        assert!(shut_needs_no_timeout(Err(io::Error::from(io::ErrorKind::InvalidInput))).is_err());
        assert!(shut_needs_no_timeout(Err(io::Error::from_raw_os_error(9))).is_err());
        assert!(shut_needs_no_timeout(Ok(())).is_ok());
    }

    #[test]
    fn a_closed_peer_with_bytes_left_gives_the_bytes_then_the_end() {
        let (mut lines, mut peer) = pair(16);
        peer.write_all(b"one\n").unwrap();
        drop(peer);
        let t = Duration::from_secs(5);
        assert_eq!(lines.next_line(t).unwrap(), text("one"));
        assert_eq!(lines.next_line(t).unwrap(), Line::End);
        lines.stream().set_write_timeout(Some(t)).unwrap();
        assert!(lines.write_line("two\n").is_err(), "a write to a closed peer fails at once");
    }

    #[test]
    fn two_lines_in_one_read_are_two_lines() {
        let (mut lines, mut peer) = pair(16);
        peer.write_all(b"one\ntwo\n").unwrap();
        let t = Duration::from_secs(5);
        assert_eq!(lines.next_line(t).unwrap(), text("one"));
        assert_eq!(lines.next_line(t).unwrap(), text("two"));
        drop(peer);
        assert_eq!(lines.next_line(t).unwrap(), Line::End);
    }

    #[test]
    fn a_line_at_the_cap_is_read_and_one_byte_more_is_too_long() {
        let (mut lines, mut peer) = pair(4);
        peer.write_all(b"abcd\nabcde\n").unwrap();
        let t = Duration::from_secs(5);
        assert_eq!(lines.next_line(t).unwrap(), text("abcd"));
        assert_eq!(lines.next_line(t).unwrap(), Line::TooLong);
        // A line with no end that passes the cap is too long before its end.
        let (mut lines, mut peer) = pair(4);
        peer.write_all(b"abcdef").unwrap();
        assert_eq!(lines.next_line(t).unwrap(), Line::TooLong);
    }

    #[test]
    fn a_line_that_was_given_leaves_no_byte_in_the_buffer() {
        let holds = |buf: &[u8], text: &[u8]| buf.windows(text.len()).any(|w| w == text);
        let (mut lines, mut peer) = pair(64);
        peer.write_all(b"made-up-one\nmade-up-two\nrest").unwrap();
        let t = Duration::from_secs(5);
        assert_eq!(lines.next_line(t).unwrap(), text("made-up-one"));
        assert!(!holds(&lines.buf, b"made-up-one"), "the buffer keeps the line that it gave");
        assert_eq!(&lines.buf[..lines.filled], b"made-up-two\nrest");
        assert_eq!(lines.next_line(t).unwrap(), text("made-up-two"));
        assert!(!holds(&lines.buf, b"made-up"), "the buffer keeps a line that it gave");
        assert_eq!(&lines.buf[..lines.filled], b"rest");
        assert!(lines.buf[lines.filled..].iter().all(|b| *b == 0), "a byte past the rest is not wiped");
    }

    #[test]
    fn a_line_that_is_not_utf8_keeps_the_frame() {
        let (mut lines, mut peer) = pair(16);
        peer.write_all(b"\xff\xfe\nok\n").unwrap();
        let t = Duration::from_secs(5);
        assert_eq!(lines.next_line(t).unwrap(), Line::NotUtf8);
        assert_eq!(lines.next_line(t).unwrap(), text("ok"));
    }

    #[test]
    fn a_slow_line_meets_one_deadline_for_the_whole_line() {
        let (mut lines, mut peer) = pair(1024);
        let writer = std::thread::spawn(move || {
            // One byte each 20 ms: each read gets a byte before a timeout of
            // its own, but the line is not whole in 200 ms.
            for _ in 0..40 {
                if peer.write_all(b"x").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let started = Instant::now();
        assert_eq!(lines.next_line(Duration::from_millis(200)).unwrap(), Line::Timeout);
        assert!(started.elapsed() < Duration::from_millis(700), "{:?}", started.elapsed());
        drop(lines);
        writer.join().unwrap();
    }
}
