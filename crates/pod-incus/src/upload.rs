//! A body that comes from a reader ([`Payload`]), and the bodies of an
//! upload: one payload, or the `multipart/form-data` body of a split image
//! (the part `metadata`, then `rootfs` or `rootfs.img`), which incus reads
//! in that order.

use pod_core::entropy::{Entropy, OsEntropy};
use tokio::io::AsyncRead;

/// The bytes of a body from a reader, and their number when the caller
/// knows it. With a length, the request has `Content-Length` and the crate
/// sends that many bytes: a reader that ends before is an error, and one
/// that has more is an error. With none, the body goes in chunks.
pub struct Payload {
    pub(crate) reader: Box<dyn AsyncRead + Send + Unpin>,
    pub(crate) length: Option<u64>,
}

impl std::fmt::Debug for Payload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Payload").field("length", &self.length).finish_non_exhaustive()
    }
}

impl Payload {
    /// The bytes of `reader`; `length`, when known.
    pub fn new(reader: impl AsyncRead + Send + Unpin + 'static, length: Option<u64>) -> Payload {
        Payload { reader: Box::new(reader), length }
    }

    /// Bytes in memory.
    pub fn bytes(data: impl Into<Vec<u8>>) -> Payload {
        let data = data.into();
        let length = data.len() as u64;
        Payload { reader: Box::new(std::io::Cursor::new(data)), length: Some(length) }
    }

    /// The number of bytes, when known.
    pub fn length(&self) -> Option<u64> {
        self.length
    }
}

/// One piece of a stream body.
pub(crate) enum Part {
    Bytes(Vec<u8>),
    Payload(Payload),
}

/// A stream body: its type and its pieces in order.
pub(crate) struct Upload {
    pub content_type: String,
    pub parts: Vec<Part>,
}

impl Upload {
    /// One payload of `content_type`.
    pub fn single(content_type: &str, payload: Payload) -> Upload {
        Upload { content_type: content_type.to_string(), parts: vec![Part::Payload(payload)] }
    }

    /// A `multipart/form-data` body of `fields` (a form name, a file name,
    /// a payload), each part of type `application/octet-stream`.
    pub fn multipart(boundary: &str, fields: Vec<(&str, &str, Payload)>) -> Upload {
        let mut parts = Vec::with_capacity(fields.len() * 2 + 1);
        for (i, (name, filename, payload)) in fields.into_iter().enumerate() {
            let lead = if i == 0 { "" } else { "\r\n" };
            let head = format!(
                "{lead}--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n\
                 Content-Type: application/octet-stream\r\n\r\n",
                quoted(name),
                quoted(filename)
            );
            parts.push(Part::Bytes(head.into_bytes()));
            parts.push(Part::Payload(payload));
        }
        parts.push(Part::Bytes(format!("\r\n--{boundary}--\r\n").into_bytes()));
        Upload { content_type: format!("multipart/form-data; boundary={boundary}"), parts }
    }

    /// The number of bytes of the body, when each piece has a known length.
    pub fn length(&self) -> Option<u64> {
        self.parts.iter().try_fold(0u64, |sum, part| match part {
            Part::Bytes(bytes) => sum.checked_add(bytes.len() as u64),
            Part::Payload(payload) => payload.length.and_then(|length| sum.checked_add(length)),
        })
    }
}

/// A value of a quoted parameter of `Content-Disposition`, as the multipart
/// writer of Go escapes it; a line break becomes a space.
fn quoted(text: &str) -> String {
    text.replace(['\r', '\n'], " ").replace('\\', "\\\\").replace('"', "\\\"")
}

/// A boundary of 60 hexadecimal digits from 30 random bytes: no payload
/// holds it by chance.
pub(crate) fn boundary(random: &mut dyn Entropy) -> Option<String> {
    let mut bytes = [0u8; 30];
    random.fill(&mut bytes).ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// [`boundary`] from the system.
pub(crate) fn system_boundary() -> Option<String> {
    boundary(&mut OsEntropy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_multipart_body_has_its_parts_in_order_and_its_length() {
        let fields = vec![
            ("metadata", "meta.tar", Payload::bytes(&b"META"[..])),
            ("rootfs", "root \"fs\".tar", Payload::bytes(&b"ROOT"[..])),
        ];
        let upload = Upload::multipart("b0", fields);
        assert_eq!(upload.content_type, "multipart/form-data; boundary=b0");
        let mut text = String::new();
        for part in &upload.parts {
            match part {
                Part::Bytes(bytes) => text.push_str(std::str::from_utf8(bytes).unwrap()),
                Part::Payload(_) => text.push_str("<payload>"),
            }
        }
        let want = "--b0\r\nContent-Disposition: form-data; name=\"metadata\"; filename=\"meta.tar\"\r\n\
                    Content-Type: application/octet-stream\r\n\r\n<payload>\r\n--b0\r\n\
                    Content-Disposition: form-data; name=\"rootfs\"; filename=\"root \\\"fs\\\".tar\"\r\n\
                    Content-Type: application/octet-stream\r\n\r\n<payload>\r\n--b0--\r\n";
        assert_eq!(text, want);
        let framing = want.len() as u64 - 2 * "<payload>".len() as u64;
        assert_eq!(upload.length(), Some(framing + 8));
    }

    #[test]
    fn a_part_of_unknown_length_makes_the_length_unknown() {
        let fields = vec![("metadata", "m", Payload::bytes(&b"M"[..])), ("rootfs", "r", Payload::new(&b"R"[..], None))];
        assert_eq!(Upload::multipart("b", fields).length(), None);
    }

    #[test]
    fn a_boundary_has_60_hex_digits_or_none_with_no_entropy() {
        let made = system_boundary().unwrap();
        assert_eq!(made.len(), 60);
        assert!(made.bytes().all(|b| b.is_ascii_hexdigit()));
        struct Empty;
        impl Entropy for Empty {
            fn fill(&mut self, _: &mut [u8]) -> Result<(), pod_core::entropy::NoRandom> {
                Err(pod_core::entropy::NoRandom)
            }
        }
        assert_eq!(boundary(&mut Empty), None);
    }
}
