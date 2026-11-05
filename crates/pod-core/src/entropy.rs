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
}
