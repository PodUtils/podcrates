//! The bytes of the minimal HTTP/3 client: the frames (RFC 9114, section
//! 7) and QPACK with the static table only (RFC 9204). The dynamic table has
//! a capacity of 0, so an answer that uses it is refused.

use pod_core::{Class, Failure, Stage};

pub use crate::capsule::{get_varint, put_varint};

/// The HTTP/3 frame types that the client uses (RFC 9114, section 7.2).
pub const FRAME_DATA: u64 = 0x00;
pub const FRAME_HEADERS: u64 = 0x01;
pub const FRAME_SETTINGS: u64 = 0x04;

/// The most bytes of a frame that the client reads (a DATA frame of
/// capsules, or a HEADERS frame).
pub const MAX_FRAME: usize = 1 << 20;

fn bad(text: impl Into<String>) -> Failure {
    Failure::new(Stage::Warp, Class::Protocol, text)
}

/// One HTTP/3 frame.
pub fn frame(kind: u64, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 8);
    put_varint(&mut out, kind);
    put_varint(&mut out, payload.len() as u64);
    out.extend_from_slice(payload);
    out
}

/// The frames of a stream, as its bytes come.
#[derive(Default)]
pub struct Frames {
    buf: Vec<u8>,
}

impl Frames {
    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// The next whole frame: its type and its payload. A frame larger than
    /// [`MAX_FRAME`] is a failure.
    pub fn next_frame(&mut self) -> Result<Option<(u64, Vec<u8>)>, Failure> {
        let Some((kind, a)) = get_varint(&self.buf) else {
            return Ok(None);
        };
        let Some((len, b)) = get_varint(&self.buf[a..]) else {
            return Ok(None);
        };
        let len = usize::try_from(len).unwrap_or(usize::MAX);
        if len > MAX_FRAME {
            return Err(bad(format!("an HTTP/3 frame of {len} bytes (the most is {MAX_FRAME})")));
        }
        let start = a + b;
        if self.buf.len() < start + len {
            return Ok(None);
        }
        let payload = self.buf[start..start + len].to_vec();
        self.buf.drain(..start + len);
        Ok(Some((kind, payload)))
    }

    /// The bytes that are not yet a whole frame.
    pub fn into_rest(self) -> Vec<u8> {
        self.buf
    }
}

/// The payload of the DATA frames of a stream, as its bytes come, with no
/// limit on the size of a frame: a tunnel of CONNECT can carry a DATA frame
/// of any size (RFC 9114, section 4.4). A frame of another type is skipped.
#[derive(Debug, Default)]
pub struct DataFrames {
    /// The first bytes of a frame header that a chunk cut.
    head: Vec<u8>,
    /// The bytes of the payload of this frame still to come.
    left: u64,
    data: bool,
}

impl DataFrames {
    /// Add the DATA bytes of `input` to `out`, in order.
    pub fn feed(&mut self, mut input: &[u8], out: &mut Vec<u8>) {
        loop {
            if self.left > 0 {
                let n = usize::try_from(self.left).unwrap_or(usize::MAX).min(input.len());
                if self.data {
                    out.extend_from_slice(&input[..n]);
                }
                self.left -= n as u64;
                input = &input[n..];
            }
            if input.is_empty() {
                return;
            }
            // Two varints take at most 16 bytes.
            let had = self.head.len();
            let take = (16 - had).min(input.len());
            self.head.extend_from_slice(&input[..take]);
            let header = get_varint(&self.head)
                .and_then(|(kind, a)| get_varint(&self.head[a..]).map(|(len, b)| (kind, len, a + b)));
            match header {
                Some((kind, len, used)) => {
                    input = &input[used - had..];
                    self.head.clear();
                    self.left = len;
                    self.data = kind == FRAME_DATA;
                }
                None => input = &input[take..],
            }
        }
    }
}

/// A prefixed integer (RFC 7541, section 5.1) with `bits` bits in the first
/// byte, whose other bits are `flags`.
pub(crate) fn put_prefixed(buf: &mut Vec<u8>, flags: u8, bits: u8, mut v: u64) {
    let max = (1u64 << bits) - 1;
    if v < max {
        buf.push(flags | v as u8);
        return;
    }
    buf.push(flags | max as u8);
    v -= max;
    while v >= 128 {
        buf.push((v % 128) as u8 | 0x80);
        v /= 128;
    }
    buf.push(v as u8);
}

pub(crate) fn get_prefixed(buf: &[u8], bits: u8) -> Option<(u64, usize)> {
    let max = (1u64 << bits) - 1;
    let mut v = u64::from(*buf.first()?) & max;
    if v < max {
        return Some((v, 1));
    }
    let mut shift = 0;
    for (i, b) in buf.iter().enumerate().skip(1) {
        v = v.checked_add(u64::from(b & 0x7f).checked_shl(shift)?)?;
        if b & 0x80 == 0 {
            return Some((v, i + 1));
        }
        shift += 7;
        if shift > 56 {
            return None;
        }
    }
    None
}

/// A string literal with no Huffman code: the H bit and a length of `bits`
/// bits, after `flags`.
pub(crate) fn put_string(buf: &mut Vec<u8>, flags: u8, bits: u8, s: &str) {
    put_prefixed(buf, flags, bits, s.len() as u64);
    buf.extend_from_slice(s.as_bytes());
}

/// The field section of the extended CONNECT (RFC 9220) of CONNECT-IP.
pub fn connect_ip_headers(authority: &str, protocol: &str) -> Vec<u8> {
    // Required Insert Count 0, Base 0.
    let mut out = vec![0x00, 0x00];
    // :method CONNECT, :scheme https and :path / of the static table (15,
    // 23 and 1).
    out.extend_from_slice(&[0xc0 | 15, 0xc0 | 23, 0xc0 | 1]);
    // :authority (static name 0), a literal value.
    put_prefixed(&mut out, 0x50, 4, 0);
    put_string(&mut out, 0x00, 7, authority);
    for (name, value) in [(":protocol", protocol), ("capsule-protocol", "?1"), ("pq-enabled", "false")] {
        // A literal field line with a literal name (001NHxxx).
        put_string(&mut out, 0x20, 3, name);
        put_string(&mut out, 0x00, 7, value);
    }
    out
}

/// The `:status` of the static table, by index.
fn static_status(index: u64) -> Option<u16> {
    Some(match index {
        24 => 103,
        25 => 200,
        26 => 304,
        27 => 404,
        28 => 503,
        63 => 100,
        64 => 204,
        65 => 206,
        66 => 302,
        67 => 400,
        68 => 403,
        69 => 421,
        70 => 425,
        71 => 500,
        _ => return None,
    })
}

/// A string literal: its bytes, whether they are Huffman coded, and the
/// bytes that it took.
fn get_string(buf: &[u8], bits: u8) -> Option<(&[u8], bool, usize)> {
    let huffman = buf.first()? & (1 << bits) != 0;
    let (len, a) = get_prefixed(buf, bits)?;
    let len = usize::try_from(len).ok()?;
    let bytes = buf.get(a..a.checked_add(len)?)?;
    Some((bytes, huffman, a + len))
}

/// The status of a literal value; a Huffman coded value is not read (0).
fn literal_status(value: &[u8], huffman: bool) -> Option<u16> {
    if huffman {
        return Some(0);
    }
    std::str::from_utf8(value).ok()?.parse().ok()
}

/// The `:status` of a response field section. A status as a Huffman coded
/// literal reads as 0; the static table has the usual ones.
pub fn response_status(section: &[u8]) -> Result<u16, Failure> {
    let unreadable = || bad("an HTTP/3 field section that the client cannot read");
    let dynamic = || bad("the response uses the QPACK dynamic table, which has a capacity of 0");
    let (ric, a) = get_prefixed(section, 8).ok_or_else(unreadable)?;
    if ric != 0 {
        return Err(dynamic());
    }
    let (_, b) = get_prefixed(&section[a..], 7).ok_or_else(unreadable)?;
    let mut i = a + b;
    let mut status = None;
    while i < section.len() {
        let rest = &section[i..];
        let first = rest[0];
        if first & 0x80 != 0 {
            // An indexed field line (1Txxxxxx).
            let (index, n) = get_prefixed(rest, 6).ok_or_else(unreadable)?;
            if first & 0x40 == 0 {
                return Err(dynamic());
            }
            status = status.or(static_status(index));
            i += n;
        } else if first & 0xc0 == 0x40 {
            // A literal with a name reference (01NTxxxx).
            let (index, n) = get_prefixed(rest, 4).ok_or_else(unreadable)?;
            if first & 0x10 == 0 {
                return Err(dynamic());
            }
            let (value, huffman, m) = get_string(&rest[n..], 7).ok_or_else(unreadable)?;
            if static_status(index).is_some() {
                status = status.or_else(|| literal_status(value, huffman));
            }
            i += n + m;
        } else if first & 0xe0 == 0x20 {
            // A literal with a literal name (001NHxxx).
            let (name, name_huffman, n) = get_string(rest, 3).ok_or_else(unreadable)?;
            let (value, huffman, m) = get_string(&rest[n..], 7).ok_or_else(unreadable)?;
            if !name_huffman && name == b":status" {
                status = status.or_else(|| literal_status(value, huffman));
            }
            i += n + m;
        } else {
            return Err(bad("the response uses the QPACK dynamic table (post-base)"));
        }
    }
    status.ok_or_else(|| bad("the response has no :status"))
}
