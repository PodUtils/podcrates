//! Outline's salt prefix (developer.getoutline.org, "Disguise Connections
//! with Prefixes"): the first bytes of the salt of each stream, so that the
//! stream starts as a protocol that a network lets through (`POST `, a TLS
//! record). A key gives it as text: each code point U+0000 to U+00FF is one
//! byte, in the YAML of a dynamic key (`prefix: "\u0016\u0003\u0001"`) and,
//! URL-encoded as UTF-8, in an `ss://` link (`prefix=%16%03%01`). The server
//! reads the salt as any other: it needs no word of the prefix.
//!
//! Outline's documents give 16 bytes at most. A salt of 16 bytes
//! (`aes-128-gcm`) then has no random byte, and each stream would take the
//! same key: so the rest of the salt is [`MIN_RANDOM`] random bytes at least.

use super::Method;
use super::codec::ChunkError;
use pod_core::entropy::Entropy as _;

/// The longest prefix, as Outline's documents give it.
pub const MAX_PREFIX: usize = 16;

/// The random bytes of a salt after its prefix, at least: 64 bits, so that
/// two streams of one key take one salt only after some 2^32 streams.
pub const MIN_RANDOM: usize = 8;

/// The bytes of a prefix that a key gives as text. No message quotes it.
pub fn from_text(text: &str) -> Result<Vec<u8>, String> {
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).map_err(|_| "its prefix has a character past U+00FF".to_string()))
        .collect()
}

/// The prefix as an `ss://` link gives it: the UTF-8 of each byte's code
/// point, each byte of that a `%` escape.
pub fn to_url_text(prefix: &[u8]) -> String {
    prefix
        .iter()
        .map(|b| char::from(*b).to_string())
        .flat_map(|c| c.into_bytes())
        .map(|b| format!("%{b:02X}"))
        .collect()
}

/// Whether `prefix` fits the salt of `method`.
pub fn check(prefix: &[u8], method: Method) -> Result<(), String> {
    let most = MAX_PREFIX.min(method.salt_len() - MIN_RANDOM);
    if prefix.len() > most {
        return Err(format!(
            "its prefix has {} bytes; with {} it has {most} at most, so that {MIN_RANDOM} bytes of the salt stay random",
            prefix.len(),
            method.name()
        ));
    }
    Ok(())
}

/// A salt of `method` that starts with `prefix`, its other bytes from the
/// OS's random source.
pub fn salt(method: Method, prefix: &[u8]) -> Result<Vec<u8>, ChunkError> {
    // A node checked its prefix; the bound keeps the random bytes all the same.
    let n = prefix.len().min(method.salt_len() - MIN_RANDOM);
    let mut salt = vec![0u8; method.salt_len()];
    salt[..n].copy_from_slice(&prefix[..n]);
    pod_core::entropy::OsEntropy.fill(&mut salt[n..]).map_err(|_| ChunkError::Random)?;
    Ok(salt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prefix_is_a_byte_for_each_code_point_and_its_url_text_is_outline_s() {
        // Outline's TLS ClientHello: YAML "\u0016\u0003\u0001\u0000¨\u0001\u0001",
        // URL-encoded %16%03%01%00%C2%A8%01%01.
        let hello = from_text("\u{16}\u{3}\u{1}\u{0}\u{a8}\u{1}\u{1}").unwrap();
        assert_eq!(hello, [0x16, 0x03, 0x01, 0x00, 0xa8, 0x01, 0x01]);
        assert_eq!(to_url_text(&hello), "%16%03%01%00%C2%A8%01%01");
        assert!(from_text("POST\u{100}").unwrap_err().contains("past U+00FF"));
    }

    #[test]
    fn the_salt_starts_with_the_prefix_and_keeps_eight_random_bytes() {
        let salt = salt(Method::Chacha20IetfPoly1305, b"POST ").unwrap();
        assert_eq!((&salt[..5], salt.len()), (&b"POST "[..], 32));
        assert_ne!(salt[5..], super::salt(Method::Chacha20IetfPoly1305, b"POST ").unwrap()[5..], "random");
        assert!(check(&[0; 16], Method::Aes256Gcm).is_ok() && check(&[0; 17], Method::Aes256Gcm).is_err());
        assert!(check(&[0; 8], Method::Aes128Gcm).is_ok());
        let why = check(b"HTTP/1.1 ", Method::Aes128Gcm).unwrap_err();
        assert!(why.contains("with aes-128-gcm it has 8 at most"), "{why}");
    }
}
