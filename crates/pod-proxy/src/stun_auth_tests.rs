//! The vectors of RFC 5769: 2.1 (a request with the short-term key and a
//! FINGERPRINT), 2.2 (an IPv4 response) and 2.4 (a request with the
//! long-term key).

use super::*;

fn hex(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text.bytes().filter(u8::is_ascii_hexdigit).collect();
    digits.chunks(2).map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap()).collect()
}

/// RFC 5769, 2.1. USERNAME is padded with spaces, as the RFC gives it.
fn request() -> Vec<u8> {
    hex("00 01 00 58 21 12 a4 42 b7 e7 a7 01 bc 34 d6 86 fa 87 df ae
         80 22 00 10 53 54 55 4e 20 74 65 73 74 20 63 6c 69 65 6e 74
         00 24 00 04 6e 00 01 ff 80 29 00 08 93 2f f9 b1 51 26 3b 36
         00 06 00 09 65 76 74 6a 3a 68 36 76 59 20 20 20
         00 08 00 14 9a ea a7 0c bf d8 cb 56 78 1e f2 b5 b2 d3 f2 49 c1 b5 71 a2
         80 28 00 04 e5 7a 3b cf")
}

/// RFC 5769, 2.2.
fn response() -> Vec<u8> {
    hex("01 01 00 3c 21 12 a4 42 b7 e7 a7 01 bc 34 d6 86 fa 87 df ae
         80 22 00 0b 74 65 73 74 20 76 65 63 74 6f 72 20
         00 20 00 08 00 01 a1 47 e1 12 a6 43
         00 08 00 14 2b 91 f5 99 fd 9e 90 c3 8c 74 89 f9 2a f9 ba 53 f0 6b e7 d7
         80 28 00 04 c0 7d 4c 96")
}

/// RFC 5769, 2.4.
fn long_term_request() -> Vec<u8> {
    hex("00 01 00 60 21 12 a4 42 78 ad 34 33 c6 ad 72 c0 29 da 41 2e
         00 06 00 12 e3 83 9e e3 83 88 e3 83 aa e3 83 83 e3 82 af e3 82 b9 00 00
         00 15 00 1c 66 2f 2f 34 39 39 6b 39 35 34 64 36 4f 4c 33 34 6f 4c 39 46 53 54 76 79 36 34 73 41
         00 14 00 0b 65 78 61 6d 70 6c 65 2e 6f 72 67 00
         00 08 00 14 f6 70 24 65 6d d6 4a 3e 02 b8 e0 71 2e 85 c9 a2 8c a8 96 66")
}

const SHORT_TERM: &[u8] = b"VOkJxbRl1RmTxUk/WvJxBt";

/// The bytes of `message` before the attribute at `at`, as a writer had
/// them before it signed: the length ends there.
fn before(message: &[u8], at: usize) -> Vec<u8> {
    let mut out = message[..at].to_vec();
    out[2..4].copy_from_slice(&((at - HEADER) as u16).to_be_bytes());
    out
}

#[test]
fn the_vectors_of_rfc_5769_check() {
    assert_eq!(integrity(&request(), SHORT_TERM), Ok(()));
    assert!(fingerprint_ok(&request()));
    assert_eq!(integrity(&response(), SHORT_TERM), Ok(()));
    assert!(fingerprint_ok(&response()));
    let key = long_term_key("\u{30de}\u{30c8}\u{30ea}\u{30c3}\u{30af}\u{30b9}", "example.org", "TheMatrIX");
    assert_eq!(integrity(&long_term_request(), &key), Ok(()));
    assert!(!fingerprint_ok(&long_term_request()), "2.4 has no FINGERPRINT");
}

#[test]
fn signing_writes_the_vectors_of_rfc_5769_again() {
    let request = request();
    let at = request.len() - 8 - 24;
    assert_eq!(fingerprinted(&signed(&before(&request, at), SHORT_TERM)), request);
    let response = response();
    let at = response.len() - 8 - 24;
    assert_eq!(fingerprinted(&signed(&before(&response, at), SHORT_TERM)), response);
    let long = long_term_request();
    let key = long_term_key("\u{30de}\u{30c8}\u{30ea}\u{30c3}\u{30af}\u{30b9}", "example.org", "TheMatrIX");
    assert_eq!(signed(&before(&long, long.len() - 24), &key), long);
}

#[test]
fn a_changed_byte_or_a_wrong_key_fails() {
    let mut changed = request();
    changed[30] ^= 1;
    assert_eq!(integrity(&changed, SHORT_TERM), Err(IntegrityError::Mismatch));
    assert!(!fingerprint_ok(&changed));
    assert_eq!(integrity(&request(), b"another key"), Err(IntegrityError::Mismatch));
    let plain = super::super::binding_request(&[7; 12]);
    assert!(matches!(integrity(&plain, SHORT_TERM), Err(IntegrityError::Missing(_))));
    assert!(!fingerprint_ok(&plain));
}

#[test]
fn the_types_of_turn_put_the_class_between_the_bits_of_the_method() {
    assert_eq!(message_type(ALLOCATE, Class::Request), 0x0003);
    assert_eq!(message_type(ALLOCATE, Class::Success), 0x0103);
    assert_eq!(message_type(ALLOCATE, Class::Error), 0x0113);
    assert_eq!(message_type(SEND, Class::Indication), 0x0016);
    assert_eq!(message_type(DATA_METHOD, Class::Indication), 0x0017);
    assert_eq!(message_type(CONNECTION_ATTEMPT, Class::Indication), 0x001c);
    assert_eq!(message_type(super::super::BINDING_REQUEST, Class::Success), super::super::BINDING_SUCCESS);
}
