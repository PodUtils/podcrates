//! OpenSSH's public line of an Ed25519 key, `ssh-ed25519 BASE64`, then an
//! optional comment, as `ssh-keygen -y` and `podssh keygen -y` print it. The
//! BASE64 is the key's SSH encoding: the type, then the 32 bytes, each after
//! its length in four bytes.

use base64::Engine;

use super::{OPENSSH_TYPE, PublicKey};

impl PublicKey {
    /// The key as OpenSSH's public line, with no comment.
    pub fn openssh(&self) -> String {
        format!("{OPENSSH_TYPE} {}", base64::engine::general_purpose::STANDARD.encode(blob(&self.0)))
    }

    /// The key of OpenSSH's public line: its first two words, the type and
    /// the BASE64; the words after them are the comment. `None` for a line
    /// of another type, or a BASE64 that does not hold one Ed25519 key.
    pub fn from_openssh(line: &str) -> Option<PublicKey> {
        let mut words = line.split_whitespace();
        from_words(words.next()?, words.next()?)
    }
}

/// The SSH encoding of an Ed25519 key.
pub(crate) fn blob(key: &[u8; 32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(51);
    for part in [OPENSSH_TYPE.as_bytes(), &key[..]] {
        out.extend_from_slice(&(part.len() as u32).to_be_bytes());
        out.extend_from_slice(part);
    }
    out
}

/// The key of the two first words of OpenSSH's public line.
pub(crate) fn from_words(kind: &str, base64: &str) -> Option<PublicKey> {
    if kind != OPENSSH_TYPE {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(base64).ok()?;
    let (named, rest) = field(&bytes)?;
    let (key, rest) = field(rest)?;
    if named != OPENSSH_TYPE.as_bytes() || !rest.is_empty() {
        return None;
    }
    Some(PublicKey(key.try_into().ok()?))
}

/// One field of the wire: four bytes of length, then the bytes; and the
/// bytes after it.
fn field(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let len = u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    let end = 4usize.checked_add(len)?;
    Some((bytes.get(4..end)?, bytes.get(end..)?))
}
