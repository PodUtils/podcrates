//! Where random bytes come from: the system, or a planted source in a test.
//! A crate that makes a key, a nonce or a session id takes an [`Entropy`],
//! so that a test can give known bytes and see a failure of the system.

use std::fmt;

/// The system gave no random bytes. It is never answered with weaker bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoRandom;

impl fmt::Display for NoRandom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the system gave no random bytes")
    }
}

impl std::error::Error for NoRandom {}

/// A source of random bytes.
pub trait Entropy {
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), NoRandom>;
}

/// The random source of the system. Reading it can fail (no `getrandom`, a
/// seccomp filter); [`Entropy::fill`] then returns an error where a panic
/// would end each session of the process.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsEntropy;

impl Entropy for OsEntropy {
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), NoRandom> {
        use rand::TryRng;
        rand::rngs::SysRng.try_fill_bytes(buf).map_err(|_| NoRandom)
    }
}

/// Whether `given` is the secret `expected`. The time does not depend on
/// where the first difference is: a caller cannot find the secret byte by
/// byte. The length of a secret is public. An empty secret matches nothing.
pub fn same_secret(expected: &[u8], given: &[u8]) -> bool {
    if expected.is_empty() || expected.len() != given.len() {
        return false;
    }
    let diff = expected.iter().zip(given).fold(0u8, |d, (x, y)| d | (x ^ y));
    std::hint::black_box(diff) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_fills_a_buffer() {
        let (mut a, mut b) = ([0u8; 32], [0u8; 32]);
        OsEntropy.fill(&mut a).unwrap();
        OsEntropy.fill(&mut b).unwrap();
        assert_ne!(a, b, "two reads of 32 random bytes");
    }

    #[test]
    fn a_planted_source_can_fail() {
        struct Dry;
        impl Entropy for Dry {
            fn fill(&mut self, _: &mut [u8]) -> Result<(), NoRandom> {
                Err(NoRandom)
            }
        }
        assert_eq!(Dry.fill(&mut [0u8; 4]).unwrap_err().to_string(), "the system gave no random bytes");
    }

    #[test]
    fn a_secret_matches_itself_alone() {
        assert!(same_secret(b"made-up", b"made-up"));
        assert!(!same_secret(b"made-up", b"made-uq"));
        assert!(!same_secret(b"made-up", b"made-up-"));
        assert!(!same_secret(b"", b""));
    }
}
