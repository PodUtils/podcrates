//! The display form of a fingerprint (podnet's link workstream, T-278, call
//! C118): a form for a person to read aloud, compare and type. It is base32
//! (RFC 4648: `A` to `Z` and `2` to `7`) of 25 bytes in five groups of
//! eight characters, with a `-` between two groups: the first 21 bytes of
//! the SHA-256 fingerprint of the key (the digest of
//! [`PublicKey::fingerprint`]), then a checksum of 4 bytes, the head of
//! SHA-256 over a label and those 21 bytes. 25 bytes are 200 bits, so the
//! 40 characters hold no padding. A changed character or a swap of two
//! characters fails the checksum, but for one chance in 2^32.

use std::fmt;

use sha2::{Digest, Sha256};

use super::PublicKey;

/// The groups of the display form.
pub const DISPLAY_GROUPS: usize = 5;
/// The characters of a group.
pub const GROUP_CHARS: usize = 8;
/// The bytes of the SHA-256 fingerprint that the form keeps: 168 bits, so a
/// second key with the same form costs about 2^168 tries.
const DIGEST_BYTES: usize = 21;
/// The bytes of the checksum.
const CHECK_BYTES: usize = 4;
/// What the checksum hashes before the digest.
const CHECK_LABEL: &[u8] = b"pod-identity display fingerprint, version 1";
/// The alphabet of RFC 4648 base32.
const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// The head of the SHA-256 fingerprint of a key, as the display form holds
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayFingerprint([u8; DIGEST_BYTES]);

/// Why a text is no display form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayError {
    /// The count of characters, with no separator, is not 40.
    Length(usize),
    /// A character that is no base32 and no separator.
    Character(char),
    /// The checksum does not match: a typing error.
    Checksum,
}

impl fmt::Display for DisplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let all = DISPLAY_GROUPS * GROUP_CHARS;
        match self {
            DisplayError::Length(n) => write!(f, "a fingerprint has {all} characters of base32; this one has {n}"),
            DisplayError::Character(c) => write!(f, "{c:?} is no character of a fingerprint (A to Z, 2 to 7)"),
            DisplayError::Checksum => {
                f.write_str("the checksum of the fingerprint does not match: a character is wrong")
            }
        }
    }
}

impl std::error::Error for DisplayError {}

impl DisplayFingerprint {
    /// The display fingerprint of `key`.
    pub fn of(key: &PublicKey) -> DisplayFingerprint {
        let mut head = [0u8; DIGEST_BYTES];
        head.copy_from_slice(&key.digest()[..DIGEST_BYTES]);
        DisplayFingerprint(head)
    }

    /// The form that [`fmt::Display`] writes: case is free, and each `-` and
    /// space is skipped; the checksum must match.
    pub fn parse(text: &str) -> Result<DisplayFingerprint, DisplayError> {
        let mut values = Vec::with_capacity(DISPLAY_GROUPS * GROUP_CHARS);
        for c in text.trim().chars() {
            if c == '-' || c == ' ' {
                continue;
            }
            let upper = c.to_ascii_uppercase();
            let at = ALPHABET.iter().position(|a| char::from(*a) == upper).ok_or(DisplayError::Character(c))?;
            values.push(at as u8);
        }
        if values.len() != DISPLAY_GROUPS * GROUP_CHARS {
            return Err(DisplayError::Length(values.len()));
        }
        let bytes = from_base32(&values);
        let mut head = [0u8; DIGEST_BYTES];
        head.copy_from_slice(&bytes[..DIGEST_BYTES]);
        if bytes[DIGEST_BYTES..] != checksum(&head) {
            return Err(DisplayError::Checksum);
        }
        Ok(DisplayFingerprint(head))
    }

    /// True when the form is the one of `key`.
    pub fn matches(&self, key: &PublicKey) -> bool {
        *self == DisplayFingerprint::of(key)
    }
}

impl fmt::Display for DisplayFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut bytes = [0u8; DIGEST_BYTES + CHECK_BYTES];
        bytes[..DIGEST_BYTES].copy_from_slice(&self.0);
        bytes[DIGEST_BYTES..].copy_from_slice(&checksum(&self.0));
        let text = to_base32(&bytes);
        for (n, group) in text.as_bytes().chunks(GROUP_CHARS).enumerate() {
            if n > 0 {
                f.write_str("-")?;
            }
            // The alphabet is ASCII.
            f.write_str(std::str::from_utf8(group).map_err(|_| fmt::Error)?)?;
        }
        Ok(())
    }
}

impl PublicKey {
    /// The display form of the fingerprint: five groups of eight characters
    /// of base32 with a checksum ([`DisplayFingerprint`]).
    pub fn display_fingerprint(&self) -> String {
        DisplayFingerprint::of(self).to_string()
    }
}

fn checksum(head: &[u8; DIGEST_BYTES]) -> [u8; CHECK_BYTES] {
    let digest = Sha256::new().chain_update(CHECK_LABEL).chain_update(head).finalize();
    let mut out = [0u8; CHECK_BYTES];
    out.copy_from_slice(&digest[..CHECK_BYTES]);
    out
}

/// Base32 of 25 bytes: 40 characters, no padding.
fn to_base32(bytes: &[u8; DIGEST_BYTES + CHECK_BYTES]) -> String {
    let (mut acc, mut bits, mut out) = (0u32, 0u32, String::with_capacity(40));
    for b in bytes {
        acc = (acc << 8) | u32::from(*b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(ALPHABET[((acc >> bits) & 31) as usize]));
        }
    }
    out
}

/// The 25 bytes of 40 values of base32.
fn from_base32(values: &[u8]) -> [u8; DIGEST_BYTES + CHECK_BYTES] {
    let (mut acc, mut bits, mut out, mut at) = (0u32, 0u32, [0u8; DIGEST_BYTES + CHECK_BYTES], 0);
    for v in values {
        acc = (acc << 5) | u32::from(*v);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out[at] = (acc >> bits) as u8;
            at += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u8) -> PublicKey {
        crate::Identity::from_seed(&[n; 32]).public()
    }

    #[test]
    fn the_form_is_five_groups_of_eight_and_reads_back() {
        let k = key(7);
        let text = k.display_fingerprint();
        let groups: Vec<&str> = text.split('-').collect();
        assert_eq!(groups.len(), DISPLAY_GROUPS, "{text}");
        assert!(groups.iter().all(|g| g.len() == GROUP_CHARS && g.bytes().all(|b| ALPHABET.contains(&b))), "{text}");
        let back = DisplayFingerprint::parse(&text).unwrap();
        assert!(back.matches(&k) && !back.matches(&key(8)));
        // Case and separators are free.
        let loose = text.to_ascii_lowercase().replace('-', " ");
        assert_eq!(DisplayFingerprint::parse(&loose), Ok(back));
        assert_eq!(DisplayFingerprint::parse(&text.replace('-', "")), Ok(back));
    }

    /// The form holds the head of the digest of the OpenSSH fingerprint.
    #[test]
    fn the_form_holds_the_head_of_the_sha256_fingerprint() {
        let k = key(1);
        let print = k.fingerprint();
        let digest = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD_NO_PAD,
            print.strip_prefix(crate::FINGERPRINT_PREFIX).unwrap(),
        )
        .unwrap();
        assert_eq!(DisplayFingerprint::of(&k).0[..], digest[..DIGEST_BYTES]);
    }

    /// Each change of one character, and each swap of two neighbours that
    /// differ, fails the checksum.
    #[test]
    fn a_changed_or_swapped_character_fails_the_checksum() {
        let text: Vec<u8> = key(3).display_fingerprint().replace('-', "").into_bytes();
        for at in 0..text.len() {
            for c in ALPHABET.iter().filter(|c| **c != text[at]) {
                let mut wrong = text.clone();
                wrong[at] = *c;
                let wrong = String::from_utf8(wrong).unwrap();
                assert_eq!(DisplayFingerprint::parse(&wrong), Err(DisplayError::Checksum), "{wrong}");
            }
        }
        for at in 0..text.len() - 1 {
            if text[at] == text[at + 1] {
                continue;
            }
            let mut swapped = text.clone();
            swapped.swap(at, at + 1);
            let swapped = String::from_utf8(swapped).unwrap();
            assert_eq!(DisplayFingerprint::parse(&swapped), Err(DisplayError::Checksum), "{swapped}");
        }
    }

    #[test]
    fn a_text_of_another_length_or_alphabet_is_refused() {
        let text = key(4).display_fingerprint();
        assert_eq!(DisplayFingerprint::parse(&text[..text.len() - 1]), Err(DisplayError::Length(39)));
        assert_eq!(DisplayFingerprint::parse(&format!("{text}A")), Err(DisplayError::Length(41)));
        let with_one = format!("1{}", &text[1..]);
        assert_eq!(DisplayFingerprint::parse(&with_one), Err(DisplayError::Character('1')));
        assert_eq!(DisplayFingerprint::parse(""), Err(DisplayError::Length(0)));
    }

    /// A known vector: the key of the seed of 32 zero bytes, computed apart
    /// with Python's `cryptography` (Ed25519, SHA-256, RFC 4648 base32).
    #[test]
    fn the_form_of_a_known_key() {
        let k = key(0);
        assert_eq!(k.fingerprint(), "SHA256:tAXFyTXI8xtDaujAEcwJslAYc9/6FKcUkd2Lw0xDhPo");
        assert_eq!(k.display_fingerprint(), "WQC4LSJV-ZDZRWQ3K-5DABDTAJ-WJIBQ467-7KYVQCOL");
    }
}
