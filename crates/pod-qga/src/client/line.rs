//! The bytes of the socket: a write and a read of one delimited answer,
//! each within a deadline. A read keeps the bytes after the delimiter for
//! the next read.

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

/// Why a read or a write stopped.
#[derive(Debug)]
pub(crate) enum Stop {
    /// The deadline came first.
    Timeout,
    /// The agent closed the connection.
    Closed,
    /// The answer is longer than the limit.
    TooLong(usize),
    Io(io::Error),
}

/// One connection to the agent.
pub(crate) struct Line {
    stream: UnixStream,
    buf: Vec<u8>,
}

/// The time to the deadline, or `None` when it is past. A zero time is no
/// time: `set_read_timeout` refuses zero.
fn left(deadline: Instant) -> Option<Duration> {
    deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero())
}

fn stop_of(e: io::Error) -> Stop {
    match e.kind() {
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => Stop::Timeout,
        _ => Stop::Io(e),
    }
}

impl Line {
    pub(crate) fn new(stream: UnixStream) -> Line {
        Line { stream, buf: Vec::new() }
    }

    /// Write all of `bytes` before `deadline`.
    pub(crate) fn send(&mut self, bytes: &[u8], deadline: Instant) -> Result<(), Stop> {
        let mut at = 0;
        while at < bytes.len() {
            let wait = left(deadline).ok_or(Stop::Timeout)?;
            self.stream.set_write_timeout(Some(wait)).map_err(Stop::Io)?;
            match self.stream.write(&bytes[at..]) {
                Ok(0) => return Err(Stop::Closed),
                Ok(n) => at += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == io::ErrorKind::BrokenPipe => return Err(Stop::Closed),
                Err(e) => return Err(stop_of(e)),
            }
        }
        Ok(())
    }

    /// Read more bytes into the buffer.
    fn fill(&mut self, deadline: Instant) -> Result<(), Stop> {
        let wait = left(deadline).ok_or(Stop::Timeout)?;
        self.stream.set_read_timeout(Some(wait)).map_err(Stop::Io)?;
        let mut chunk = [0u8; 64 * 1024];
        loop {
            match self.stream.read(&mut chunk) {
                Ok(0) => return Err(Stop::Closed),
                Ok(n) => {
                    self.buf.extend_from_slice(&chunk[..n]);
                    return Ok(());
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => return Err(Stop::Closed),
                Err(e) => return Err(stop_of(e)),
            }
        }
    }

    /// The bytes up to `delim`, with no `delim`, at most `max` of them.
    pub(crate) fn read_until(&mut self, delim: u8, deadline: Instant, max: usize) -> Result<Vec<u8>, Stop> {
        let mut seen = 0;
        loop {
            if let Some(i) = self.buf[seen..].iter().position(|b| *b == delim) {
                let end = seen + i;
                if end > max {
                    return Err(Stop::TooLong(max));
                }
                let mut out: Vec<u8> = self.buf.drain(..=end).collect();
                out.pop();
                return Ok(out);
            }
            seen = self.buf.len();
            if seen > max {
                return Err(Stop::TooLong(max));
            }
            self.fill(deadline)?;
        }
    }

    /// Drop each byte up to and with `delim`. The bytes before it are old
    /// answers and the answer of the reset, and are not kept, so they have
    /// no size limit: only the deadline stops the skip.
    pub(crate) fn skip_past(&mut self, delim: u8, deadline: Instant) -> Result<(), Stop> {
        loop {
            if let Some(i) = self.buf.iter().position(|b| *b == delim) {
                self.buf.drain(..=i);
                return Ok(());
            }
            self.buf.clear();
            self.fill(deadline)?;
        }
    }

    /// Drop the bytes that the buffer holds: after a reset of the agent
    /// they are old.
    pub(crate) fn clear(&mut self) {
        self.buf.clear();
    }
}
