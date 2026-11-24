//! The helpers of the tests: the bytes of a text of hex digits, and a
//! transport that plays a script of responses.

#![allow(dead_code)]

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
