//! The helpers of the tests: the bytes of a text of hex digits, and a
//! transport that plays a script of responses.

#![allow(dead_code)]

pub mod fake;

use pod_core::{Class, Failure, Stage};

/// The bytes of a text of hex digits; spaces and line ends do not count.
pub fn unhex(text: &str) -> Vec<u8> {
    let text: String = text.split_whitespace().collect();
    assert!(text.len().is_multiple_of(2), "an odd count of hex digits: {text}");
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex")).collect()
}

/// A transport that gives `responses` in order and keeps each command that
/// came; with no response left, it fails as a TPM that is not there.
pub fn scripted(
    responses: Vec<Vec<u8>>,
    seen: &mut Vec<Vec<u8>>,
) -> impl FnMut(&[u8]) -> Result<Vec<u8>, Failure> + '_ {
    let mut responses = responses.into_iter();
    move |command: &[u8]| {
        seen.push(command.to_vec());
        responses.next().ok_or_else(|| Failure::new(Stage::Host, Class::Unavailable, "no TPM"))
    }
}

/// One DER element, written in the tests by hand and with no check, so that
/// a test can make a key file that the writer of the crate refuses.
pub fn tlv(tag: u8, contents: &[u8]) -> Vec<u8> {
    let n = contents.len();
    let mut out = vec![tag];
    if n < 0x80 {
        out.push(n as u8);
    } else if n <= 0xFF {
        out.extend_from_slice(&[0x81, n as u8]);
    } else {
        out.extend_from_slice(&[0x82, (n >> 8) as u8, n as u8]);
    }
    out.extend_from_slice(contents);
    out
}

/// The contents of a DER INTEGER of `n`, in its fewest octets.
pub fn der_int(n: u32) -> Vec<u8> {
    let bytes = n.to_be_bytes();
    let skip = bytes.iter().take_while(|b| **b == 0).count().min(3);
    let mut out = if bytes[skip] & 0x80 != 0 { vec![0] } else { vec![] };
    out.extend_from_slice(&bytes[skip..]);
    tlv(0x02, &out)
}

/// A `TPMPolicy` of `code` and `body`.
pub fn der_statement(code: u32, body: &[u8]) -> Vec<u8> {
    tlv(0x30, &[tlv(0xA0, &der_int(code)), tlv(0xA1, &tlv(0x04, body))].concat())
}

/// The OID of a loadable key (2.23.133.10.1.3).
pub const LOADABLE: &[u8] = &[0x06, 0x06, 0x67, 0x81, 0x05, 0x0A, 0x01, 0x03];

/// A `TPMKey` of a loadable key of `parent`, with the optional fields
/// `fields` (each whole), the public part `0000` and the private part
/// `0000`.
pub fn der_key(fields: &[Vec<u8>], parent: u32) -> Vec<u8> {
    let mut body = LOADABLE.to_vec();
    for f in fields {
        body.extend_from_slice(f);
    }
    body.extend(der_int(parent));
    body.extend(tlv(0x04, &[0, 0]));
    body.extend(tlv(0x04, &[0, 0]));
    tlv(0x30, &body)
}
