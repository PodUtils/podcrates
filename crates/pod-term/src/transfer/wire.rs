//! The bytes of the codec: the payload of each kind, the text of a frame,
//! and the check of a name from the peer.

use base64::Engine;
use base64::alphabet::STANDARD;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

use super::{COMPONENT_MAX, NAME_MAX, REASON_MAX};

/// Base64 of the standard alphabet: written with no padding, read with up
/// to two `=`, and canonical (zero unused bits, no length of 1 mod 4).
pub(super) const BASE64: GeneralPurpose = GeneralPurpose::new(
    &STANDARD,
    GeneralPurposeConfig::new()
        .with_encode_padding(false)
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(false),
);

/// The largest size, offset or count: below 2^63.
const SIZE_LIMIT: u64 = 1 << 63;

/// One message of the codec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Msg {
    Helo { version: u8, id: [u8; 8], chunk: u32 },
    Offr { size: u64, mode: u32, mtime: u64, name: Vec<u8> },
    Take { from: u64 },
    Skip { reason: Vec<u8> },
    Data { offset: u64, bytes: Vec<u8> },
    Ackn { of: u64, count: u64 },
    Done { size: u64, sha256: [u8; 32] },
    Kept { size: u64 },
    Stop { code: u8, reason: Vec<u8> },
}

/// Why a frame with a CRC that holds is no message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Bad {
    /// A kind that is not in the table.
    Kind,
    /// A field that is short, out of range, or bytes after the last field.
    Field,
}

impl Msg {
    pub(super) fn kind(&self) -> &'static [u8; 4] {
        match self {
            Msg::Helo { .. } => b"HELO",
            Msg::Offr { .. } => b"OFFR",
            Msg::Take { .. } => b"TAKE",
            Msg::Skip { .. } => b"SKIP",
            Msg::Data { .. } => b"DATA",
            Msg::Ackn { .. } => b"ACKN",
            Msg::Done { .. } => b"DONE",
            Msg::Kept { .. } => b"KEPT",
            Msg::Stop { .. } => b"STOP",
        }
    }

    pub(super) fn payload(&self) -> Vec<u8> {
        let mut p = Vec::new();
        match self {
            Msg::Helo { version, id, chunk } => {
                p.push(*version);
                p.extend_from_slice(id);
                p.extend_from_slice(&chunk.to_be_bytes());
            }
            Msg::Offr { size, mode, mtime, name } => {
                p.extend_from_slice(&size.to_be_bytes());
                p.extend_from_slice(&mode.to_be_bytes());
                p.extend_from_slice(&mtime.to_be_bytes());
                put_str(&mut p, name);
            }
            Msg::Take { from: n } | Msg::Kept { size: n } => p.extend_from_slice(&n.to_be_bytes()),
            Msg::Skip { reason } => put_str(&mut p, cut(reason)),
            Msg::Data { offset, bytes } => {
                p.extend_from_slice(&offset.to_be_bytes());
                p.extend_from_slice(bytes);
            }
            Msg::Ackn { of, count } => {
                p.extend_from_slice(&of.to_be_bytes());
                p.extend_from_slice(&count.to_be_bytes());
            }
            Msg::Done { size, sha256 } => {
                p.extend_from_slice(&size.to_be_bytes());
                p.extend_from_slice(sha256);
            }
            Msg::Stop { code, reason } => {
                p.push(*code);
                put_str(&mut p, cut(reason));
            }
        }
        p
    }

    pub(super) fn decode(kind: &[u8; 4], payload: &[u8]) -> Result<Msg, Bad> {
        let mut r = Fields(payload);
        let msg = match kind {
            b"HELO" => Msg::Helo { version: r.u8()?, id: r.array()?, chunk: r.u32()? },
            b"OFFR" => Msg::Offr { size: r.size()?, mode: r.u32()? & 0o7777, mtime: r.u64()?, name: r.str()? },
            b"TAKE" => Msg::Take { from: r.size()? },
            b"SKIP" => Msg::Skip { reason: r.str()? },
            b"DATA" => {
                let offset = r.size()?;
                return Ok(Msg::Data { offset, bytes: r.0.to_vec() });
            }
            b"ACKN" => Msg::Ackn { of: r.size()?, count: r.size()? },
            b"DONE" => Msg::Done { size: r.size()?, sha256: r.array()? },
            b"KEPT" => Msg::Kept { size: r.size()? },
            b"STOP" => Msg::Stop { code: r.u8()?, reason: r.str()? },
            _ => return Err(Bad::Kind),
        };
        if r.0.is_empty() { Ok(msg) } else { Err(Bad::Field) }
    }
}

/// The reader of the fields of a payload.
struct Fields<'a>(&'a [u8]);

impl Fields<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], Bad> {
        if self.0.len() < n {
            return Err(Bad::Field);
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Bad> {
        self.take(N)?.try_into().map_err(|_| Bad::Field)
    }

    fn u8(&mut self) -> Result<u8, Bad> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, Bad> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, Bad> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    /// A size, an offset or a count: below 2^63.
    fn size(&mut self) -> Result<u64, Bad> {
        let n = self.u64()?;
        if n < SIZE_LIMIT { Ok(n) } else { Err(Bad::Field) }
    }

    fn str(&mut self) -> Result<Vec<u8>, Bad> {
        let n = u16::from_be_bytes(self.array()?);
        Ok(self.take(usize::from(n))?.to_vec())
    }
}

fn put_str(p: &mut Vec<u8>, s: &[u8]) {
    let n = s.len().min(usize::from(u16::MAX));
    p.extend_from_slice(&(n as u16).to_be_bytes());
    p.extend_from_slice(&s[..n]);
}

/// The first [`REASON_MAX`] bytes of a reason, cut at a character.
pub(super) fn cut(reason: &[u8]) -> &[u8] {
    if reason.len() <= REASON_MAX {
        return reason;
    }
    let mut end = REASON_MAX;
    while end > 0 && (reason[end] & 0xc0) == 0x80 {
        end -= 1;
    }
    &reason[..end]
}

/// A reason as text: at most [`REASON_MAX`] bytes, each byte that is not
/// UTF-8 replaced.
pub(super) fn reason_text(reason: &[u8]) -> String {
    String::from_utf8_lossy(cut(reason)).into_owned()
}

/// CRC-32/ISO-HDLC of `KIND`, `:` and the payload.
pub(super) fn frame_crc(kind: &[u8; 4], payload: &[u8]) -> u32 {
    let mut h = crc32fast::Hasher::new();
    h.update(kind);
    h.update(b":");
    h.update(payload);
    h.finalize()
}

/// The text of a frame, `#KIND:<base64>!<crc>`, then `end`. A line longer
/// than `width` breaks with `end` inside the base64.
pub(super) fn frame(msg: &Msg, end: &[u8], width: usize) -> Vec<u8> {
    let payload = msg.payload();
    let text = BASE64.encode(&payload);
    let tail = format!("!{:08x}", frame_crc(msg.kind(), &payload));
    let width = width.max(16);
    let mut out = Vec::with_capacity(text.len() + 32);
    out.push(b'#');
    out.extend_from_slice(msg.kind());
    out.push(b':');
    let mut line = 6;
    for &c in text.as_bytes() {
        if line == width {
            out.extend_from_slice(end);
            line = 0;
        }
        out.push(c);
        line += 1;
    }
    if line + tail.len() > width {
        out.extend_from_slice(end);
    }
    out.extend_from_slice(tail.as_bytes());
    out.extend_from_slice(end);
    out
}

/// The name of a file from the peer: its last component after `/`. `None`
/// for a name that names no file of this directory: longer than
/// [`NAME_MAX`] bytes, not UTF-8, or a last component that is empty, `.`,
/// `..`, longer than [`COMPONENT_MAX`] bytes, or holds a control character
/// (C0, DEL, C1).
pub(super) fn file_name(wire: &[u8]) -> Option<String> {
    if wire.len() > NAME_MAX {
        return None;
    }
    let name = std::str::from_utf8(wire).ok()?;
    let last = name.rsplit('/').next().unwrap_or(name);
    let bad = last.is_empty()
        || last == "."
        || last == ".."
        || last.len() > COMPONENT_MAX
        || last.chars().any(|c| c.is_control());
    (!bad).then(|| last.to_string())
}
