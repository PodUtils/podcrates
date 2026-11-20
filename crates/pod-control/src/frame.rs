//! The line frame: one cap of length, a check of UTF-8, and a deadline for
//! each line. A slow peer that sends one byte at a time meets the deadline
//! of the whole line, not a new timeout for each byte.

use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

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
        std::os::unix::net::UnixStream::set_read_timeout(self, timeout)
    }

    fn set_write_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        std::os::unix::net::UnixStream::set_write_timeout(self, timeout)
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
    /// A whole line, with no line end.
    Text(String),
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
    buf: Vec<u8>,
    cap: usize,
}

const CHUNK: usize = 8 * 1024;

impl<S: Stream> Lines<S> {
    pub fn new(stream: S, cap: usize) -> Lines<S> {
        Lines { stream, buf: Vec::new(), cap }
    }

    pub fn stream(&mut self) -> &mut S {
        &mut self.stream
    }

    /// The next line, whole before `timeout` from now.
    pub fn next_line(&mut self, timeout: Duration) -> io::Result<Line> {
        let deadline = Instant::now() + timeout;
        let mut chunk = [0u8; CHUNK];
        loop {
            if let Some(end) = self.buf.iter().position(|b| *b == b'\n') {
                let mut line: Vec<u8> = self.buf.drain(..=end).collect();
                line.pop();
                if line.len() > self.cap {
                    return Ok(Line::TooLong);
                }
                return Ok(String::from_utf8(line).map_or(Line::NotUtf8, Line::Text));
            }
            if self.buf.len() > self.cap {
                return Ok(Line::TooLong);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(Line::Timeout);
            }
            self.stream.set_read_timeout(Some(left))?;
            match self.stream.read(&mut chunk) {
                Ok(0) => return Ok(Line::End),
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                    return Ok(Line::Timeout);
                }
                Err(e) => return Err(e),
            }
        }
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

    fn pair(cap: usize) -> (Lines<UnixStream>, UnixStream) {
        let (a, b) = UnixStream::pair().unwrap();
        (Lines::new(a, cap), b)
    }

    #[test]
    fn two_lines_in_one_read_are_two_lines() {
        let (mut lines, mut peer) = pair(16);
        peer.write_all(b"one\ntwo\n").unwrap();
        let t = Duration::from_secs(5);
        assert_eq!(lines.next_line(t).unwrap(), Line::Text("one".into()));
        assert_eq!(lines.next_line(t).unwrap(), Line::Text("two".into()));
        drop(peer);
        assert_eq!(lines.next_line(t).unwrap(), Line::End);
    }

    #[test]
    fn a_line_at_the_cap_is_read_and_one_byte_more_is_too_long() {
        let (mut lines, mut peer) = pair(4);
        peer.write_all(b"abcd\nabcde\n").unwrap();
        let t = Duration::from_secs(5);
        assert_eq!(lines.next_line(t).unwrap(), Line::Text("abcd".into()));
        assert_eq!(lines.next_line(t).unwrap(), Line::TooLong);
        // A line with no end that passes the cap is too long before its end.
        let (mut lines, mut peer) = pair(4);
        peer.write_all(b"abcdef").unwrap();
        assert_eq!(lines.next_line(t).unwrap(), Line::TooLong);
    }

    #[test]
    fn a_line_that_is_not_utf8_keeps_the_frame() {
        let (mut lines, mut peer) = pair(16);
        peer.write_all(b"\xff\xfe\nok\n").unwrap();
        let t = Duration::from_secs(5);
        assert_eq!(lines.next_line(t).unwrap(), Line::NotUtf8);
        assert_eq!(lines.next_line(t).unwrap(), Line::Text("ok".into()));
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
