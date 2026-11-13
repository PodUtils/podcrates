//! The exporter of a finished handshake: key material for a label, which
//! only the two ends of the channel can make.
//!
//! The handshake hash of Noise is not secret: it hashes the messages of the
//! handshake, which the relay sees. Thus the exporter's secret is the
//! HKDF-SHA-256 extract of the two keys that the handshake split, with the
//! hash as the salt: it binds the keys to this handshake, and the relay
//! cannot make it. No key of the transport leaves the crate; a label gives
//! a key that is independent of the transport's keys and of each other
//! label's key.
//!
//! The info of each expand is `pod-e2e exporter`, a zero byte, the label's
//! length in one byte, the label, the context's length in four bytes
//! (big-endian) and the context. A label that starts with [`RESERVED`] is
//! the crate's own ([`crate::record`]): a caller cannot ask for it.

use std::fmt;

use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::Error;

/// The start of each label of the crate's own keys.
pub const RESERVED: &str = "pod-e2e ";
/// The most bytes of one export: HKDF-SHA-256's limit.
pub const MAX_EXPORT: usize = 255 * 32;

/// The start of the info of each expand.
const INFO: &[u8] = b"pod-e2e exporter\0";

/// The exporter of one channel's handshake, at one end.
pub struct Exporter {
    secret: Zeroizing<[u8; 32]>,
    hash: [u8; 32],
    initiator: bool,
}

/// The secret stays out of each print.
impl fmt::Debug for Exporter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Exporter {{ initiator: {} }}", self.initiator)
    }
}

impl Exporter {
    /// The exporter of a handshake whose hash is `hash` and whose split gave
    /// `keys` (the initiator's key, then the responder's), at the end of
    /// the role `initiator`.
    pub(crate) fn new(hash: &[u8], keys: &[u8; 64], initiator: bool) -> Exporter {
        let mut salt = [0u8; 32];
        // SHA-256 gives 32 bytes; a shorter hash is set in at the start.
        let n = hash.len().min(32);
        salt[..n].copy_from_slice(&hash[..n]);
        let (prk, _) = Hkdf::<Sha256>::extract(Some(&salt), &keys[..]);
        let mut secret = Zeroizing::new([0u8; 32]);
        secret.copy_from_slice(&prk);
        Exporter { secret, hash: salt, initiator }
    }

    /// The handshake hash of Noise: the binding of the channel. It is not
    /// secret: the relay can compute it from the messages that it carried.
    pub fn handshake_hash(&self) -> [u8; 32] {
        self.hash
    }

    /// Whether this end started the handshake (an operator).
    pub fn is_initiator(&self) -> bool {
        self.initiator
    }

    /// Fill `out` with the key material of `label` and `context`. Both ends
    /// of one channel get the same bytes; another label, another context or
    /// another channel gets other bytes. A label is 1 to 255 bytes, and does
    /// not start with [`RESERVED`]; `out` has at most [`MAX_EXPORT`] bytes.
    pub fn export(&self, label: &str, context: &[u8], out: &mut [u8]) -> Result<(), Error> {
        if label.starts_with(RESERVED) {
            return Err(Error::Malformed(format!("the label {label:?} of an export is the crate's own")));
        }
        self.expand(label, context, out)
    }

    /// [`Exporter::export`] with no check of the label: the crate's own keys.
    pub(crate) fn expand(&self, label: &str, context: &[u8], out: &mut [u8]) -> Result<(), Error> {
        let label_len = u8::try_from(label.len()).ok().filter(|n| *n > 0);
        let context_len = u32::try_from(context.len()).ok();
        let (Some(label_len), Some(context_len)) = (label_len, context_len) else {
            return Err(Error::Malformed(format!(
                "an export of a label of {} bytes and a context of {} bytes",
                label.len(),
                context.len()
            )));
        };
        if out.len() > MAX_EXPORT {
            return Err(Error::Malformed(format!("an export of {} bytes, where {MAX_EXPORT} is the most", out.len())));
        }
        let mut info = Vec::with_capacity(INFO.len() + 5 + label.len() + context.len());
        info.extend_from_slice(INFO);
        info.push(label_len);
        info.extend_from_slice(label.as_bytes());
        info.extend_from_slice(&context_len.to_be_bytes());
        info.extend_from_slice(context);
        let hkdf = Hkdf::<Sha256>::from_prk(&self.secret[..]).map_err(|e| Error::Malformed(e.to_string()))?;
        hkdf.expand(&info, out).map_err(|e| Error::Malformed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (Exporter, Exporter) {
        let keys = [7u8; 64];
        (Exporter::new(&[3u8; 32], &keys, true), Exporter::new(&[3u8; 32], &keys, false))
    }

    fn export(e: &Exporter, label: &str, context: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        e.export(label, context, &mut out).unwrap();
        out
    }

    #[test]
    fn the_label_and_the_context_and_the_hash_each_give_another_key() {
        let (a, b) = pair();
        let key = export(&a, "podnet link", b"one");
        assert_eq!(key, export(&b, "podnet link", b"one"), "both ends");
        assert_ne!(key, export(&a, "podnet link2", b"one"), "another label");
        assert_ne!(key, export(&a, "podnet link", b"two"), "another context");
        // The label's length goes into the info: a label and a context that
        // join to the same bytes give two keys.
        assert_ne!(export(&a, "ab", b"c"), export(&a, "a", b"bc"));
        let other = Exporter::new(&[4u8; 32], &[7u8; 64], true);
        assert_ne!(key, export(&other, "podnet link", b"one"), "another handshake hash");
        // The relay can compute the hash: the keys of the split make the key.
        let other = Exporter::new(&[3u8; 32], &[8u8; 64], true);
        assert_ne!(key, export(&other, "podnet link", b"one"), "the same hash with other keys");
        assert_eq!(a.handshake_hash(), [3u8; 32]);
        assert!(a.is_initiator() && !b.is_initiator());
    }

    #[test]
    fn a_label_of_the_crate_and_a_size_out_of_its_limit_are_refused() {
        let (a, _) = pair();
        let mut out = [0u8; 32];
        assert!(a.export("pod-e2e record", b"", &mut out).is_err(), "the crate's own label");
        assert!(a.expand("pod-e2e record", b"", &mut out).is_ok(), "the crate makes its own");
        assert!(a.export("", b"", &mut out).is_err(), "an empty label");
        assert!(a.export(&"x".repeat(256), b"", &mut out).is_err(), "a label of 256 bytes");
        assert!(a.export("x", b"", &mut vec![0u8; MAX_EXPORT + 1]).is_err());
        assert!(a.export("x", b"", &mut vec![0u8; MAX_EXPORT]).is_ok());
        assert!(!format!("{a:?}").contains("secret"), "no secret in a print");
    }
}
