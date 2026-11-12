//! The SHA-256 credentials of RFC 8489: the password algorithms of a
//! challenge (PASSWORD-ALGORITHMS, PASSWORD-ALGORITHM, 14.11 and 14.12), the
//! key of each algorithm (9.2.2), MESSAGE-INTEGRITY-SHA256 (14.6), and the
//! security features that a nonce announces (9.2). Pure Rust: HMAC-SHA256 of
//! `hmac` and `sha2`.

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

use super::auth::{IntegrityError, MESSAGE_INTEGRITY, offsets, with_length};

/// The attributes of the SHA-256 credentials (RFC 8489, 18.3).
pub const MESSAGE_INTEGRITY_SHA256: u16 = 0x001c;
pub const PASSWORD_ALGORITHM: u16 = 0x001d;
pub const USERHASH: u16 = 0x001e;
pub const PASSWORD_ALGORITHMS: u16 = 0x8002;

/// The cookie at the start of a nonce that announces security features
/// (RFC 8489, 9.2): four base64 characters of 24 bits follow it. The order
/// of the bits is not clear in the RFC (the nonce of its example B.1 fits
/// neither order), so [`nonce_features`] gives the 24 bits as they are.
pub const NONCE_COOKIE: &[u8] = b"obMatJos2";

/// The bytes of a MESSAGE-INTEGRITY-SHA256 attribute that this codec writes:
/// the whole HMAC, not a truncated one.
const SHA256_BYTES: usize = 4 + 32;

/// A password algorithm of the long-term credentials (RFC 8489, 18.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PasswordAlgorithm {
    Md5,
    Sha256,
}

impl PasswordAlgorithm {
    /// The number of the algorithm in PASSWORD-ALGORITHM(S).
    pub const fn number(self) -> u16 {
        match self {
            PasswordAlgorithm::Md5 => 0x0001,
            PasswordAlgorithm::Sha256 => 0x0002,
        }
    }

    pub fn from_number(number: u16) -> Option<PasswordAlgorithm> {
        match number {
            0x0001 => Some(PasswordAlgorithm::Md5),
            0x0002 => Some(PasswordAlgorithm::Sha256),
            _ => None,
        }
    }

    /// The key of `user`, `realm` and `password` (RFC 8489, 9.2.2): the
    /// hash of `user:realm:password`, 16 bytes of MD5 or 32 of SHA-256. The
    /// caller gives each text after its preparation; this function does
    /// none.
    pub fn key(self, user: &str, realm: &str, password: &str) -> Vec<u8> {
        match self {
            PasswordAlgorithm::Md5 => super::long_term_key(user, realm, password).to_vec(),
            PasswordAlgorithm::Sha256 => sha256_key(user, realm, password).to_vec(),
        }
    }

    /// The value of a PASSWORD-ALGORITHM attribute: the number, and a
    /// length of 0, as neither algorithm takes a parameter.
    pub fn attribute(self) -> [u8; 4] {
        let n = self.number().to_be_bytes();
        [n[0], n[1], 0, 0]
    }
}

/// The SHA-256 key of the long-term credentials (RFC 8489, 9.2.2):
/// SHA-256 of `user:realm:password`.
pub fn sha256_key(user: &str, realm: &str, password: &str) -> [u8; 32] {
    Sha256::digest(format!("{user}:{realm}:{password}").as_bytes()).into()
}

/// The algorithms of a PASSWORD-ALGORITHMS value, in its order, each with
/// its parameters; an unknown algorithm is kept by its number. A parameter
/// that runs past the value is refused, and why.
pub fn password_algorithms(value: &[u8]) -> Result<Vec<(u16, &[u8])>, String> {
    let mut out = Vec::new();
    let mut rest = value;
    while !rest.is_empty() {
        if rest.len() < 4 {
            return Err(format!("a password algorithm cut at {} bytes", rest.len()));
        }
        let number = u16::from_be_bytes([rest[0], rest[1]]);
        let len = usize::from(u16::from_be_bytes([rest[2], rest[3]]));
        let whole = 4 + len + (4 - len % 4) % 4;
        if whole > rest.len() {
            return Err(format!("the password algorithm {number} says {len} bytes of parameters, past the value"));
        }
        out.push((number, &rest[4..4 + len]));
        rest = &rest[whole..];
    }
    Ok(out)
}

/// A PASSWORD-ALGORITHMS value of `algorithms`, each with no parameter.
pub fn write_password_algorithms(algorithms: &[PasswordAlgorithm]) -> Vec<u8> {
    algorithms.iter().flat_map(|a| a.attribute()).collect()
}

/// `message` (a header and its attributes) with a MESSAGE-INTEGRITY-SHA256
/// attribute added: the HMAC-SHA256 with `key` over the message whose
/// length already counts the attribute (RFC 8489, 14.6).
pub fn signed_sha256(message: &[u8], key: &[u8]) -> Vec<u8> {
    let mut out = with_length(message, SHA256_BYTES);
    let mac = hmac_sha256(key, &out);
    out.extend_from_slice(&MESSAGE_INTEGRITY_SHA256.to_be_bytes());
    out.extend_from_slice(&32u16.to_be_bytes());
    out.extend_from_slice(&mac);
    out
}

/// Check the MESSAGE-INTEGRITY-SHA256 of `bytes` (a whole message) with
/// `key`: the HMAC over the bytes before the attribute, with the length set
/// to end at the attribute. A value of 16 to 32 bytes, a multiple of 4, is
/// the HMAC cut to its length (RFC 8489, 14.6). The compare takes the same
/// time for each value.
pub fn integrity_sha256(bytes: &[u8], key: &[u8]) -> Result<(), IntegrityError> {
    let (_, start, len) = offsets(bytes)
        .map_err(IntegrityError::Missing)?
        .into_iter()
        .find(|(kind, _, _)| *kind == MESSAGE_INTEGRITY_SHA256)
        .ok_or_else(|| IntegrityError::Missing("the message has no MESSAGE-INTEGRITY-SHA256".to_string()))?;
    if !(16..=32).contains(&len) || len % 4 != 0 {
        return Err(IntegrityError::Missing(format!("a MESSAGE-INTEGRITY-SHA256 of {len} bytes")));
    }
    let covered = with_length(&bytes[..start], 4 + len);
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).map_err(|_| IntegrityError::Mismatch)?;
    mac.update(&covered);
    mac.verify_truncated_left(&bytes[start + 4..start + 4 + len]).map_err(|_| IntegrityError::Mismatch)
}

/// Which integrity a message carries: MESSAGE-INTEGRITY-SHA256 when it has
/// one, else MESSAGE-INTEGRITY, else none. A reader checks the one of the
/// algorithm that it chose, never the weaker one in its place.
pub fn integrity_of(attributes: &[(u16, &[u8])]) -> Option<PasswordAlgorithm> {
    if attributes.iter().any(|(k, _)| *k == MESSAGE_INTEGRITY_SHA256) {
        Some(PasswordAlgorithm::Sha256)
    } else if attributes.iter().any(|(k, _)| *k == MESSAGE_INTEGRITY) {
        Some(PasswordAlgorithm::Md5)
    } else {
        None
    }
}

/// The security features of `nonce` (RFC 8489, 9.2): the 24 bits of the
/// four base64 characters after [`NONCE_COOKIE`]; `None` when the nonce
/// does not start with the cookie and four characters of base64.
pub fn nonce_features(nonce: &[u8]) -> Option<u32> {
    let rest = nonce.strip_prefix(NONCE_COOKIE)?;
    let four = rest.get(..4)?;
    four.iter().try_fold(0u32, |bits, c| base64_value(*c).map(|v| (bits << 6) | u32::from(v)))
}

/// The value of one character of base64 (RFC 4648, 4).
fn base64_value(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn hmac_sha256(key: &[u8], bytes: &[u8]) -> [u8; 32] {
    // HMAC takes a key of any length; the error never comes.
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC takes a key of any length");
    mac.update(bytes);
    mac.finalize().into_bytes().into()
}

#[cfg(test)]
#[path = "stun_sha256_tests.rs"]
mod tests;
