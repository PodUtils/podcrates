//! The binding of one channel for the check of a grant (PodUtils/podssh#355):
//! a value that both ends know before the operator sends its proof, and
//! that no other channel has.
//!
//! The grant rides in the operator's proof, the third message of Noise XX,
//! so the final handshake hash and the exporter do not exist yet when the
//! operator makes its grant. The binding is the SHA-256 of [`LABEL`] and
//! the handshake hash after the node's message: the operator has that hash
//! after it reads the node's message, the node after it writes it. The hash
//! covers the prologue, both ephemeral keys and the node's encrypted static
//! key and proof, so two channels never share it.
//!
//! The binding is not secret: a relay computes the hash from the messages
//! that it carries. A grant that names it must carry a signature of a key
//! that the node trusts; the binding makes such a grant worthless on each
//! other channel.

use std::fmt;

use sha2::{Digest, Sha256};
use zeroize::Zeroize;

/// What the binding hashes before the handshake hash: it separates the
/// binding from the final hash that [`crate::Channel::handshake_hash`]
/// gives.
pub const LABEL: &[u8] = b"pod-e2e grant binding\0";

/// The binding of one channel, the same at its two ends.
#[derive(Clone)]
pub struct Binding([u8; 32]);

impl Binding {
    /// The binding of a handshake whose hash after the node's message is
    /// `hash`: what each end of a channel computes, public for the test
    /// vectors of another implementation.
    pub fn of_hash(hash: &[u8]) -> Binding {
        let mut sha = Sha256::new();
        sha.update(LABEL);
        sha.update(hash);
        Binding(sha.finalize().into())
    }

    /// A binding of known bytes: for a test vector, or a test of a check of
    /// a grant. A channel gives its own with `Offered::binding` and
    /// `Asked::binding`.
    pub fn from_bytes(bytes: [u8; 32]) -> Binding {
        Binding(bytes)
    }

    /// The 32 bytes, for the key of a MAC.
    pub fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Equal in constant time: a check of a grant compares a binding.
impl PartialEq for Binding {
    fn eq(&self, other: &Binding) -> bool {
        pod_core::entropy::same_secret(&self.0, &other.0)
    }
}

impl Eq for Binding {}

/// No byte of the binding in a log.
impl fmt::Debug for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Binding(..)")
    }
}

impl Drop for Binding {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_binding_hashes_the_label_and_the_hash() {
        let hash = [7u8; 32];
        let mut sha = Sha256::new();
        sha.update(b"pod-e2e grant binding\0");
        sha.update(hash);
        let want: [u8; 32] = sha.finalize().into();
        assert_eq!(Binding::of_hash(&hash).bytes(), &want);
        assert_ne!(Binding::of_hash(&hash).bytes(), &hash, "the binding is not the hash");
        assert_ne!(Binding::of_hash(&hash), Binding::of_hash(&[8u8; 32]));
        assert_eq!(Binding::from_bytes(want), Binding::of_hash(&hash));
    }

    #[test]
    fn the_debug_output_holds_no_byte_of_the_binding() {
        let binding = Binding::from_bytes([0xab; 32]);
        let shown = format!("{binding:?}");
        assert_eq!(shown, "Binding(..)");
        assert!(!shown.contains("ab") && !shown.contains("171"), "{shown}");
    }
}
