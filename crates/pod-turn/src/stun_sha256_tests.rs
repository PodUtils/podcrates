//! The SHA-256 credentials: a fixed vector, computed apart from this crate
//! (Python's `hashlib.sha256` for the key and `hmac` with SHA-256 for the
//! MAC, over the bytes that the test builds), with the user, the realm and
//! the password of RFC 5769, 2.4.

use super::*;
use crate::stun::{ALLOCATE, NONCE, REALM, REQUESTED_TRANSPORT, USERNAME, message};

const USER: &str = "\u{30de}\u{30c8}\u{30ea}\u{30c3}\u{30af}\u{30b9}";
const REALM_TEXT: &str = "example.org";
const PASSWORD: &str = "TheMatrIX";
const NONCE_TEXT: &[u8] = b"obMatJos2AAABf//499k954d6OL34oL9FSTvy64sA";
const ID: [u8; 12] = [0x78, 0xad, 0x34, 0x33, 0xc6, 0xad, 0x72, 0xc0, 0x29, 0xda, 0x41, 0x2e];

/// SHA-256 of `user:realm:password`, from Python's hashlib.
const KEY: &str = "dd295a613b9058c3c23d6dc7165bda072304d989c9d0af3a8c7e184b4f9bb4a1";

/// The whole Allocate request: REQUESTED-TRANSPORT (UDP), USERNAME, REALM,
/// NONCE, PASSWORD-ALGORITHMS (SHA-256, MD5), PASSWORD-ALGORITHM (SHA-256),
/// MESSAGE-INTEGRITY-SHA256; from Python's hmac.
const SIGNED: &str = "000300982112a44278ad3433c6ad72c029da412e00190004110000000006001\
                      2e3839ee38388e383aae38383e382afe382b900000014000b6578616d706c652e\
                      6f726700001500296f624d61744a6f733241414142662f2f3439396b3935346436\
                      4f4c33346f4c39465354767936347341000000800200080002000000010000001d\
                      000400020000001c0020c35c2a7bcb14a4b5fda1079f55696460714837887fb9aa\
                      38434550a857c58fa3";

fn hex(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text.bytes().filter(u8::is_ascii_hexdigit).collect();
    digits.chunks(2).map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap()).collect()
}

fn unsigned() -> Vec<u8> {
    let algorithms = write_password_algorithms(&[PasswordAlgorithm::Sha256, PasswordAlgorithm::Md5]);
    let chosen = PasswordAlgorithm::Sha256.attribute();
    message(
        ALLOCATE,
        &ID,
        &[
            (REQUESTED_TRANSPORT, [17u8, 0, 0, 0].as_slice()),
            (USERNAME, USER.as_bytes()),
            (REALM, REALM_TEXT.as_bytes()),
            (NONCE, NONCE_TEXT),
            (PASSWORD_ALGORITHMS, algorithms.as_slice()),
            (PASSWORD_ALGORITHM, chosen.as_slice()),
        ],
    )
}

#[test]
fn the_sha256_key_and_integrity_give_the_bytes_computed_apart() {
    assert_eq!(sha256_key(USER, REALM_TEXT, PASSWORD).to_vec(), hex(KEY));
    assert_eq!(PasswordAlgorithm::Sha256.key(USER, REALM_TEXT, PASSWORD), hex(KEY));
    let signed = signed_sha256(&unsigned(), &hex(KEY));
    assert_eq!(signed, hex(SIGNED));
    assert_eq!(integrity_sha256(&signed, &hex(KEY)), Ok(()));
    // A FINGERPRINT after it is out of the HMAC: the length of the header
    // that the HMAC covers ends at MESSAGE-INTEGRITY-SHA256.
    let with_fingerprint = crate::stun::fingerprinted(&signed);
    assert!(crate::stun::fingerprint_ok(&with_fingerprint));
    assert_eq!(integrity_sha256(&with_fingerprint, &hex(KEY)), Ok(()));
    let read = crate::stun::parse(&signed).unwrap();
    assert_eq!(integrity_of(&read.attributes), Some(PasswordAlgorithm::Sha256));
    let list = password_algorithms(read.attribute(PASSWORD_ALGORITHMS).unwrap()).unwrap();
    assert_eq!(list, vec![(0x0002, &b""[..]), (0x0001, &b""[..])]);
}

#[test]
fn a_changed_byte_a_wrong_key_or_the_md5_key_fails() {
    let signed = hex(SIGNED);
    let mut changed = signed.clone();
    // A byte of the user name, inside its value.
    changed[34] ^= 1;
    assert_eq!(integrity_sha256(&changed, &hex(KEY)), Err(IntegrityError::Mismatch));
    let md5 = PasswordAlgorithm::Md5.key(USER, REALM_TEXT, PASSWORD);
    assert_eq!(md5.len(), 16);
    assert_eq!(integrity_sha256(&signed, &md5), Err(IntegrityError::Mismatch));
    // The MD5 integrity is no SHA-256 one: neither check takes the other.
    let md5_signed = crate::stun::signed(&unsigned(), &md5);
    assert!(matches!(integrity_sha256(&md5_signed, &md5), Err(IntegrityError::Missing(_))));
    assert!(matches!(crate::stun::integrity(&signed, &hex(KEY)), Err(IntegrityError::Missing(_))));
}

/// A HMAC cut to 16 bytes checks (RFC 8489, 14.6); 12 bytes, or a length
/// that is not a multiple of 4, is no MESSAGE-INTEGRITY-SHA256.
#[test]
fn a_truncated_hmac_of_16_bytes_or_more_checks() {
    let signed = hex(SIGNED);
    let body = &signed[..signed.len() - 36];
    for (len, ok) in [(16usize, true), (28, true), (12, false), (18, false)] {
        let mut cut = body.to_vec();
        cut[2..4].copy_from_slice(&((body.len() - 20 + 4 + len + (4 - len % 4) % 4) as u16).to_be_bytes());
        let mut covered = cut.clone();
        covered[2..4].copy_from_slice(&((body.len() - 20 + 4 + len) as u16).to_be_bytes());
        let full = <hmac::Hmac<sha2::Sha256> as hmac::KeyInit>::new_from_slice(&hex(KEY)).unwrap();
        let mac = hmac::Mac::finalize(hmac::Mac::chain_update(full, &covered)).into_bytes();
        cut.extend_from_slice(&MESSAGE_INTEGRITY_SHA256.to_be_bytes());
        cut.extend_from_slice(&(len as u16).to_be_bytes());
        cut.extend_from_slice(&mac[..len]);
        cut.resize(cut.len() + (4 - len % 4) % 4, 0);
        assert_eq!(integrity_sha256(&cut, &hex(KEY)).is_ok(), ok, "{len} bytes");
    }
}

#[test]
fn password_algorithms_read_and_refuse_a_cut_parameter() {
    assert_eq!(PasswordAlgorithm::from_number(1), Some(PasswordAlgorithm::Md5));
    assert_eq!(PasswordAlgorithm::from_number(2), Some(PasswordAlgorithm::Sha256));
    assert_eq!(PasswordAlgorithm::from_number(3), None);
    let value = [0u8, 7, 0, 2, 0xaa, 0xbb, 0, 0, 0, 2, 0, 0];
    assert_eq!(password_algorithms(&value).unwrap(), vec![(7, &[0xaa, 0xbb][..]), (2, &b""[..])]);
    assert!(password_algorithms(&value[..6]).unwrap_err().contains("past the value"));
    assert!(password_algorithms(&value[..2]).unwrap_err().contains("cut at 2 bytes"));
}

#[test]
fn a_nonce_with_the_cookie_gives_its_feature_bits() {
    assert_eq!(nonce_features(NONCE_TEXT), Some(1));
    assert_eq!(nonce_features(b"obMatJos2gAAArest"), Some(0x80_0000));
    assert_eq!(nonce_features(b"obMatJos2///"), None, "three characters");
    assert_eq!(nonce_features(b"obMatJos2AA*Arest"), None, "no base64");
    assert_eq!(nonce_features(b"f//499k954d6OL34oL9FSTvy64sA"), None);
}
