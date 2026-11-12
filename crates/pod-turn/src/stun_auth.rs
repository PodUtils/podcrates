//! The integrity of a STUN message (RFC 8489, 9 and 14.5 to 14.7) and the
//! methods and attributes of TURN (RFC 8656, RFC 6062): a TURN client signs
//! each request with its key, and checks each answer. Pure Rust: HMAC-SHA1
//! of `hmac` and `sha1`, the MD5 of [`crate::md5`], and the CRC-32 here.

use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;

use super::HEADER;

/// The attributes of the integrity and of the long-term credentials.
pub const USERNAME: u16 = 0x0006;
pub const MESSAGE_INTEGRITY: u16 = 0x0008;
pub const REALM: u16 = 0x0014;
pub const NONCE: u16 = 0x0015;
pub const FINGERPRINT: u16 = 0x8028;

/// The methods of TURN (RFC 8656, 18; RFC 6062, 6.1).
pub const ALLOCATE: u16 = 0x0003;
pub const REFRESH: u16 = 0x0004;
pub const SEND: u16 = 0x0006;
pub const DATA_METHOD: u16 = 0x0007;
pub const CREATE_PERMISSION: u16 = 0x0008;
pub const CHANNEL_BIND: u16 = 0x0009;
pub const CONNECT: u16 = 0x000a;
pub const CONNECTION_BIND: u16 = 0x000b;
pub const CONNECTION_ATTEMPT: u16 = 0x000c;

/// The attributes of TURN (RFC 8656, 18; RFC 6062, 6.2).
pub const CHANNEL_NUMBER: u16 = 0x000c;
pub const LIFETIME: u16 = 0x000d;
pub const XOR_PEER_ADDRESS: u16 = 0x0012;
pub const DATA: u16 = 0x0013;
pub const XOR_RELAYED_ADDRESS: u16 = 0x0016;
pub const REQUESTED_ADDRESS_FAMILY: u16 = 0x0017;
pub const EVEN_PORT: u16 = 0x0018;
pub const REQUESTED_TRANSPORT: u16 = 0x0019;
pub const DONT_FRAGMENT: u16 = 0x001a;
pub const RESERVATION_TOKEN: u16 = 0x0022;
pub const CONNECTION_ID: u16 = 0x002a;
pub const ADDITIONAL_ADDRESS_FAMILY: u16 = 0x8000;
pub const ADDRESS_ERROR_CODE: u16 = 0x8001;
pub const ICMP: u16 = 0x8004;

/// The protocol number of UDP in REQUESTED-TRANSPORT (RFC 8656, 18.6).
pub const TRANSPORT_UDP: u8 = 17;

/// The classes of a message (RFC 8489, 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Request,
    Indication,
    Success,
    Error,
}

/// The type of a message of `method` and `class`: the two bits of the class
/// go between the bits of the method (RFC 8489, 5, figure 3).
pub fn message_type(method: u16, class: Class) -> u16 {
    let c = match class {
        Class::Request => 0b00,
        Class::Indication => 0b01,
        Class::Success => 0b10,
        Class::Error => 0b11,
    };
    let m = method & 0x0fff;
    (m & 0x000f) | ((m & 0x0070) << 1) | ((m & 0x0f80) << 2) | ((c & 1) << 4) | ((c & 2) << 7)
}

/// The class of a message's type: the bits 4 and 8 of [`message_type`].
pub fn class_of(kind: u16) -> Class {
    match ((kind >> 4) & 1) | ((kind >> 7) & 2) {
        0b00 => Class::Request,
        0b01 => Class::Indication,
        0b10 => Class::Success,
        _ => Class::Error,
    }
}

/// The method of a message's type: the bits of [`message_type`] without the
/// class.
pub fn method_of(kind: u16) -> u16 {
    (kind & 0x000f) | ((kind >> 1) & 0x0070) | ((kind >> 2) & 0x0f80)
}

/// The XOR of the CRC-32 of a FINGERPRINT (RFC 8489, 14.7).
const FINGERPRINT_XOR: u32 = 0x5354_554e;
/// The bytes of a MESSAGE-INTEGRITY attribute and of a FINGERPRINT one.
const INTEGRITY_BYTES: usize = 4 + 20;
const FINGERPRINT_BYTES: usize = 4 + 4;

/// The key of the long-term credentials (RFC 8489, 9.2.2): MD5 of
/// `user:realm:password`. The caller gives each text after SASLprep (or
/// OpaqueString); this function does no preparation.
pub fn long_term_key(user: &str, realm: &str, password: &str) -> [u8; 16] {
    crate::md5::md5(format!("{user}:{realm}:{password}").as_bytes())
}

/// `message` (a header and its attributes, as [`super::message`] writes
/// it) with a MESSAGE-INTEGRITY attribute added: the HMAC-SHA1 with `key`
/// over the message whose length already counts the attribute.
pub fn signed(message: &[u8], key: &[u8]) -> Vec<u8> {
    let mut out = with_length(message, INTEGRITY_BYTES);
    let mac = hmac_sha1(key, &out);
    out.extend_from_slice(&MESSAGE_INTEGRITY.to_be_bytes());
    out.extend_from_slice(&20u16.to_be_bytes());
    out.extend_from_slice(&mac);
    out
}

/// `message` with a FINGERPRINT attribute added, the last one: the CRC-32
/// of the message whose length already counts it, XOR `0x5354554e`.
pub fn fingerprinted(message: &[u8]) -> Vec<u8> {
    let mut out = with_length(message, FINGERPRINT_BYTES);
    let crc = crc32(&out) ^ FINGERPRINT_XOR;
    out.extend_from_slice(&FINGERPRINT.to_be_bytes());
    out.extend_from_slice(&4u16.to_be_bytes());
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

/// Why a message's integrity does not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrityError {
    /// No MESSAGE-INTEGRITY attribute, or not a whole message.
    Missing(String),
    /// The HMAC does not match the key: a wrong key or a changed message.
    Mismatch,
}

impl std::fmt::Display for IntegrityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IntegrityError::Missing(why) => write!(f, "no MESSAGE-INTEGRITY to check: {why}"),
            IntegrityError::Mismatch => f.write_str("the MESSAGE-INTEGRITY does not match the key"),
        }
    }
}

impl std::error::Error for IntegrityError {}

/// Check the MESSAGE-INTEGRITY of `bytes` (a whole message) with `key`:
/// the HMAC over the bytes before the attribute, with the length set to
/// end at the attribute; the attributes after it (a FINGERPRINT) are out
/// of it (RFC 8489, 14.5). The compare takes the same time for each value.
pub fn integrity(bytes: &[u8], key: &[u8]) -> Result<(), IntegrityError> {
    let at = offsets(bytes)
        .map_err(IntegrityError::Missing)?
        .into_iter()
        .find(|(kind, _, _)| *kind == MESSAGE_INTEGRITY)
        .ok_or_else(|| IntegrityError::Missing("the message has no such attribute".to_string()))?;
    let (_, start, len) = at;
    if len != 20 {
        return Err(IntegrityError::Missing(format!("an attribute of {len} bytes, not 20")));
    }
    let covered = with_length(&bytes[..start], INTEGRITY_BYTES);
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(key).map_err(|_| IntegrityError::Mismatch)?;
    mac.update(&covered);
    mac.verify_slice(&bytes[start + 4..start + 24]).map_err(|_| IntegrityError::Mismatch)
}

/// True when the last attribute of `bytes` is a FINGERPRINT that matches
/// the bytes before it.
pub fn fingerprint_ok(bytes: &[u8]) -> bool {
    let Ok(attributes) = offsets(bytes) else { return false };
    let Some(&(kind, start, len)) = attributes.last() else { return false };
    if kind != FINGERPRINT || len != 4 {
        return false;
    }
    let want = u32::from_be_bytes([bytes[start + 4], bytes[start + 5], bytes[start + 6], bytes[start + 7]]);
    crc32(&bytes[..start]) ^ FINGERPRINT_XOR == want
}

/// Each attribute of a whole message: its type, the offset of its header,
/// and its length; the checks of [`super::parse`].
pub(super) fn offsets(bytes: &[u8]) -> Result<Vec<(u16, usize, usize)>, String> {
    super::parse(bytes)?;
    let mut out = Vec::new();
    let mut at = HEADER;
    while at < bytes.len() {
        let kind = u16::from_be_bytes([bytes[at], bytes[at + 1]]);
        let len = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        out.push((kind, at, len));
        at += 4 + len + (4 - len % 4) % 4;
    }
    Ok(out)
}

/// `message` with the length of its header set to the bytes after the
/// header plus `more`.
pub(super) fn with_length(message: &[u8], more: usize) -> Vec<u8> {
    let mut out = message.to_vec();
    if out.len() >= HEADER {
        let len = (out.len() - HEADER + more).min(usize::from(u16::MAX)) as u16;
        out[2..4].copy_from_slice(&len.to_be_bytes());
    }
    out
}

fn hmac_sha1(key: &[u8], bytes: &[u8]) -> [u8; 20] {
    // HMAC takes a key of any length; the error never comes.
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(key).expect("HMAC takes a key of any length");
    mac.update(bytes);
    mac.finalize().into_bytes().into()
}

/// The CRC-32 of ISO-HDLC (IEEE 802.3), as FINGERPRINT takes it.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for b in bytes {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

#[cfg(test)]
#[path = "stun_auth_tests.rs"]
mod tests;
