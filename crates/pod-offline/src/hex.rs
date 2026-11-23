//! Lower-case hex and SHA-256, for the names and the digests of the crate.

use sha2::{Digest, Sha256};

/// The lower-case hex of `bytes`.
pub(crate) fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit(u32::from(byte >> 4), 16).expect("a digit under 16"));
        out.push(char::from_digit(u32::from(byte & 0x0f), 16).expect("a digit under 16"));
    }
    out
}

/// The bytes of lower-case or upper-case hex, or `None` for an odd length
/// or a byte that is not hex.
pub(crate) fn decode(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().chunks(2) {
        let high = char::from(pair[0]).to_digit(16)?;
        let low = char::from(pair[1]).to_digit(16)?;
        out.push(u8::try_from(high * 16 + low).ok()?);
    }
    Some(out)
}

/// The SHA-256 digest of `bytes`.
pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_round_trip_keeps_each_byte() {
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(decode(&encode(&all)).unwrap(), all);
    }

    #[test]
    fn a_bad_text_gives_none() {
        assert!(decode("abc").is_none());
        assert!(decode("zz").is_none());
        assert!(decode("0").is_none());
    }

    #[test]
    fn the_digest_is_the_known_one() {
        assert_eq!(encode(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
