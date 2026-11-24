//! The refusal of a body over the bound of its reader. The buffered
//! reader ([`super::read_answer`]), the body reader ([`super::Body`]) and
//! [`super::decode_chunked`] give this one error, so that a refusal keeps
//! what the reader knows: the status of the answer, the size that the peer
//! declared or that the reader saw, the bound, and the first bytes of the
//! body. A caller finds it in an `io::Error` with [`TooLarge::of`].

use std::fmt;
use std::io;

use pod_core::text::one_line;

/// The most bytes of the start of a body that a refusal keeps.
pub const KEPT_HEAD: usize = 8 * 1024;

/// The characters of the start of the body in the text of a refusal.
const SHOWN_HEAD: usize = 160;

/// The size of a body over its bound, as far as the reader knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodySize {
    /// The `Content-Length` of the answer.
    Length(u64),
    /// The size of the chunk that passes the bound, and the bytes of the
    /// chunks before it.
    Chunk { before: u64, size: u64 },
    /// A chunk size line with more hex digits than a `u64` holds: larger
    /// than any bound. The count of its digits.
    PastAnyBound { digits: usize },
    /// A body to the close: the bytes that the reader saw when the body
    /// passed the bound. The body has this many bytes or more.
    Seen(u64),
}

impl BodySize {
    /// The bytes of the body that the peer declared or that the reader saw;
    /// `None` for a size larger than any bound.
    pub fn bytes(&self) -> Option<u64> {
        match *self {
            BodySize::Length(n) | BodySize::Seen(n) => Some(n),
            BodySize::Chunk { before, size } => Some(before.saturating_add(size)),
            BodySize::PastAnyBound { .. } => None,
        }
    }
}

impl fmt::Display for BodySize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            BodySize::Length(n) => write!(f, "a Content-Length of {n} bytes"),
            BodySize::Chunk { before: 0, size } => write!(f, "a chunk of {size} bytes"),
            BodySize::Chunk { before, size } => write!(f, "a chunk of {size} bytes after {before} bytes"),
            BodySize::PastAnyBound { digits } => {
                write!(f, "a chunk size of {digits} hex digits, larger than any bound")
            }
            BodySize::Seen(n) => write!(f, "{n} bytes or more"),
        }
    }
}

/// A body larger than the bound of its reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TooLarge {
    /// The status of the answer; `None` where the reader has no head.
    pub status: Option<u16>,
    /// The reason phrase of the answer; empty where the reader has no head.
    pub reason: String,
    pub size: BodySize,
    /// The bound of the reader, in bytes.
    pub bound: u64,
    /// The first bytes of the body, [`KEPT_HEAD`] at most: the bytes that
    /// the reader holds when it refuses, and the bytes of the declared size
    /// that come after them.
    pub head: Vec<u8>,
}

impl TooLarge {
    /// The refusal that `e` carries, where it carries one.
    pub fn of(e: &io::Error) -> Option<&TooLarge> {
        e.get_ref().and_then(|inner| inner.downcast_ref::<TooLarge>())
    }

    /// The start of the body as text for a line: lossy UTF-8 made safe,
    /// cut to `max` characters.
    pub fn head_text(&self, max: usize) -> String {
        let text = one_line(&String::from_utf8_lossy(&self.head));
        match text.char_indices().nth(max) {
            Some((at, _)) => format!("{}…", &text[..at]),
            None => text,
        }
    }

    /// The refusal as an `InvalidData` error of I/O.
    pub fn into_io(self) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, self)
    }
}

impl fmt::Display for TooLarge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the response body is larger than allowed")?;
        if let Some(status) = self.status {
            let reason = one_line(&self.reason);
            let reason: String = reason.chars().take(64).collect();
            write!(f, " (HTTP {status} {reason})")?;
        }
        write!(f, ": {}, over the bound of {} bytes", self.size, self.bound)?;
        if !self.head.is_empty() {
            write!(f, "; the body starts with {:?}", self.head_text(SHOWN_HEAD))?;
        }
        Ok(())
    }
}

impl std::error::Error for TooLarge {}

/// The size of a chunk from its line; its extensions are passed over.
/// More hex digits than a `u64` holds is [`BodySize::PastAnyBound`].
pub(crate) fn chunk_size(line: &[u8]) -> Result<Result<u64, usize>, String> {
    let line = std::str::from_utf8(line).map_err(|_| "a chunk size line is not text".to_string())?;
    let hex = line.split(';').next().unwrap_or_default().trim();
    match u64::from_str_radix(hex, 16) {
        Ok(size) => Ok(Ok(size)),
        Err(_) if !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()) => Ok(Err(hex.len())),
        Err(_) => Err(format!("bad chunk size {hex:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_names_the_status_the_size_the_bound_and_the_head() {
        let refusal = TooLarge {
            status: Some(200),
            reason: "OK".into(),
            size: BodySize::Chunk { before: 0, size: 108_198_406 },
            bound: 67_108_864,
            head: br#"{"type":"error","error":"Failed"}"#.to_vec(),
        };
        assert_eq!(
            refusal.to_string(),
            "the response body is larger than allowed (HTTP 200 OK): a chunk of 108198406 bytes, over the bound of \
             67108864 bytes; the body starts with \"{\\\"type\\\":\\\"error\\\",\\\"error\\\":\\\"Failed\\\"}\""
        );
        let e = refusal.clone().into_io();
        assert_eq!((e.kind(), TooLarge::of(&e)), (io::ErrorKind::InvalidData, Some(&refusal)));
        let past = TooLarge {
            status: None,
            reason: String::new(),
            size: BodySize::PastAnyBound { digits: 17 },
            bound: 10,
            head: Vec::new(),
        };
        assert_eq!(
            past.to_string(),
            "the response body is larger than allowed: a chunk size of 17 hex digits, larger than any bound, over the \
             bound of 10 bytes"
        );
        assert_eq!(chunk_size(b"10000000000000000"), Ok(Err(17)));
        assert_eq!(chunk_size(b"672fa06;ext"), Ok(Ok(0x672fa06)));
        assert!(chunk_size(b"zz").is_err());
    }
}
