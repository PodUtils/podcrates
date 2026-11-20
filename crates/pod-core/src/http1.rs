//! The head of an HTTP/1.1 message, with no I/O of its own: the bounds of
//! a head (its bytes and its lines), the end of a head in a buffer, the
//! parse of a request line, of a status line and of the header fields, and
//! the write of a head. A reader of a crate (async or not) reads bytes into
//! a buffer and gives them to these functions; then each reader has the
//! same bounds and refuses the same heads.
//!
//! A request head is read strictly ([`parse_request`]): a control byte, a
//! bare CR, a space before a colon and a folded line are refused, as a head
//! that two readers read two ways can smuggle a request past one of them.
//! The value of a field keeps its case and its inner spaces: a reader of
//! `Authorization` gets the credential as the client sent it.
//! A response head is read leniently ([`parse_head`]): it comes from the
//! server that the caller chose, and its reason phrase is made safe text.

mod range;
mod request;
mod response;
#[cfg(test)]
mod tests;
mod write;

pub use range::{Span, parse_content_range};
pub use request::{Request, RequestError, find_request, parse_request};
pub use response::{Framing, Head, keeps, parse_head, parse_status_line};
pub use write::{answer, established, reason, refusal, request_head, response_head};

use std::io;

/// The longest head that a reader of this module reads by default.
pub const MAX_HEAD: usize = 16 * 1024;

/// The bounds of one head.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// The most bytes of the head, its empty line with them.
    pub bytes: usize,
    /// The most lines of the head: the first line and the header fields,
    /// not the empty line at the end.
    pub lines: usize,
}

impl Bounds {
    /// [`MAX_HEAD`] bytes, in any number of lines.
    pub const DEFAULT: Bounds = Bounds { bytes: MAX_HEAD, lines: usize::MAX };
}

impl Default for Bounds {
    fn default() -> Bounds {
        Bounds::DEFAULT
    }
}

/// The first index of `needle` in `haystack`.
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// The end of a head whose lines end with CRLF: the index after its empty
/// line. A response is read so; [`find_request`] also takes a bare LF.
pub fn crlf_head_end(buf: &[u8]) -> Option<usize> {
    find(buf, b"\r\n\r\n").map(|at| at + 4)
}

/// Where a head is in `buf`: after the empty lines that may come first
/// (RFC 9112 section 2.2), up to and with the empty line that ends it. A
/// line ends with CRLF or with a bare LF.
pub fn head_bounds(buf: &[u8]) -> Option<(usize, usize)> {
    let start = leading_empty(buf);
    let mut at = start;
    while let Some(lf) = buf[at..].iter().position(|&b| b == b'\n') {
        let next = at + lf + 1;
        if buf[next..].starts_with(b"\n") {
            return Some((start, next + 1));
        }
        if buf[next..].starts_with(b"\r\n") {
            return Some((start, next + 2));
        }
        at = next;
    }
    None
}

/// The bytes of the empty lines at the start of `buf`.
fn leading_empty(buf: &[u8]) -> usize {
    let mut start = 0;
    loop {
        if buf[start..].starts_with(b"\r\n") {
            start += 2;
        } else if buf[start..].starts_with(b"\n") {
            start += 1;
        } else {
            return start;
        }
    }
}

/// A head read one byte at a time, to its empty line (CRLF CRLF) and not
/// a byte further: the bytes after it belong to a tunnel or to TLS. A
/// reader gives each byte to [`Exact::push`] until it says the end.
#[derive(Debug, Clone)]
pub struct Exact {
    head: Vec<u8>,
    max: usize,
}

impl Exact {
    /// A head of at most `max` bytes.
    pub fn new(max: usize) -> Exact {
        Exact { head: Vec::with_capacity(128.min(max)), max }
    }

    /// Add `byte`: true at the end of the head; an error past `max` bytes.
    pub fn push(&mut self, byte: u8) -> io::Result<bool> {
        self.head.push(byte);
        if self.head.ends_with(b"\r\n\r\n") {
            return Ok(true);
        }
        if self.head.len() > self.max {
            let why = format!("no end of headers within {} bytes", self.max);
            return Err(io::Error::new(io::ErrorKind::InvalidData, why));
        }
        Ok(false)
    }

    /// The error of a stream that ended before the head did.
    pub fn closed() -> io::Error {
        io::Error::new(io::ErrorKind::UnexpectedEof, "the connection closed before the response ended")
    }

    /// The bytes of the head, its empty line with them.
    pub fn into_bytes(self) -> Vec<u8> {
        self.head
    }

    /// The head as lossy UTF-8.
    pub fn into_text(self) -> String {
        String::from_utf8_lossy(&self.head).into_owned()
    }
}

/// Read a head from `r` one byte at a time, as [`Exact`] reads it, within
/// `max` bytes.
pub fn read_head_exact(r: &mut impl io::Read, max: usize) -> io::Result<Vec<u8>> {
    let mut head = Exact::new(max);
    let mut byte = [0u8; 1];
    loop {
        match r.read(&mut byte) {
            Ok(0) => return Err(Exact::closed()),
            Ok(_) if head.push(byte[0])? => return Ok(head.into_bytes()),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}
