//! The answers of the store, and the small parts of the wire: the twirp
//! errors, the names, the digests, the compare of a signature and the time.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// The content type of a twirp answer.
pub const CONTENT_JSON: &str = "application/json";
/// The content type of the zip of an artifact.
pub const CONTENT_BLOB: &str = "application/octet-stream";
/// The content type of the text of a blob route (an error, or no body).
pub const CONTENT_TEXT: &str = "text/plain; charset=utf-8";

/// The body of an [`Answer`].
#[derive(Debug)]
pub enum Payload {
    /// Bytes in memory: a JSON document of the service, or a short text.
    Bytes(Vec<u8>),
    /// The zip of an artifact, open for read, and its length in bytes. The
    /// caller streams it: an artifact can be larger than the memory.
    File(File, u64),
}

/// The answer to one request of the service or of a blob.
#[derive(Debug)]
pub struct Answer {
    /// The HTTP status.
    pub status: u16,
    /// The value of `Content-Type`.
    pub content_type: &'static str,
    pub body: Payload,
}

impl Answer {
    /// The status and the body in memory. A file that cannot be read gives
    /// 500 with the error as text.
    pub fn into_bytes(self) -> (u16, Vec<u8>) {
        match self.body {
            Payload::Bytes(b) => (self.status, b),
            Payload::File(mut f, len) => {
                let mut out = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
                match f.read_to_end(&mut out) {
                    Ok(_) => (self.status, out),
                    Err(e) => (500, e.to_string().into_bytes()),
                }
            }
        }
    }
}

/// A twirp error: `code` is one of the error codes of twirp, with its
/// HTTP status.
pub(crate) fn twirp(status: u16, code: &str, msg: String) -> Answer {
    let body = json!({ "code": code, "msg": msg }).to_string().into_bytes();
    Answer { status, content_type: CONTENT_JSON, body: Payload::Bytes(body) }
}

pub(crate) fn ok(v: Value) -> Answer {
    Answer { status: 200, content_type: CONTENT_JSON, body: Payload::Bytes(v.to_string().into_bytes()) }
}

/// An answer of a blob route: no body, or the reason as text.
pub(crate) fn text(status: u16, msg: impl Into<Vec<u8>>) -> Answer {
    Answer { status, content_type: CONTENT_TEXT, body: Payload::Bytes(msg.into()) }
}

/// A name of an artifact: the characters that the client refuses, and no
/// name that is a path, are refused here too.
pub(crate) fn valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.chars().any(|c| c.is_control() || "\":<>|*?\\/".contains(c))
}

/// The value of a wrapper of protobuf (`google.protobuf.StringValue`,
/// `Int64Value`): its JSON is the plain value, and a request is read in the
/// form `{"value": ...}` too.
pub(crate) fn plain(v: &Value) -> &Value {
    match v.get("value") {
        Some(inner) => inner,
        None => v,
    }
}

/// An int64 of protobuf: a string in its JSON, or a number.
pub(crate) fn int64(v: &Value) -> Option<u64> {
    v.as_str().and_then(|s| s.parse().ok()).or(v.as_u64())
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// The digest of bytes, as the client sends it: `sha256:HEX`.
#[cfg(test)]
pub(crate) fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{}", hex(&Sha256::digest(bytes)))
}

/// The length and the digest of a file, read in parts: a blob can be
/// larger than the memory.
pub(crate) fn sha256_file(path: &Path) -> io::Result<(u64, String)> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut len = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        len += n as u64;
    }
    Ok((len, format!("sha256:{}", hex(&h.finalize()))))
}

/// Whether a signature is the expected one. The time does not depend on
/// where the first difference is: a client cannot find the signature byte
/// by byte. The length of a signature is public.
pub(crate) fn same(expected: &str, given: &str) -> bool {
    let (a, b) = (expected.as_bytes(), given.as_bytes());
    if a.is_empty() || a.len() != b.len() {
        return false;
    }
    let diff = a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y));
    std::hint::black_box(diff) == 0
}

/// RFC 3339 in UTC, from seconds of the Unix epoch.
pub(crate) fn rfc3339(secs: u64) -> String {
    pod_core::clock::rfc3339(UNIX_EPOCH + Duration::from_secs(secs))
}

/// The value of `key` in a query string, percent-decoded.
pub(crate) fn query(q: &str, key: &str) -> Option<String> {
    let v = q.split('&').find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))?;
    let mut out = Vec::new();
    let b = v.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let h = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok())?;
                out.push(h);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// The block ids of a block list, in order.
pub(crate) fn block_list(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find('<') {
        rest = &rest[at + 1..];
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[..end];
        rest = &rest[end + 1..];
        if matches!(tag, "Latest" | "Committed" | "Uncommitted") {
            let Some(close) = rest.find("</") else { break };
            out.push(rest[..close].trim().to_string());
            rest = &rest[close..];
        }
    }
    out
}
