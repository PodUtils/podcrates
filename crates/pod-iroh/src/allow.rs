//! Who may reach a node over the iroh road (PodUtils/podssh@T-163): the client
//! keys of the node's allowlist, the same file and the same reading on each
//! road (`pod_identity::allow`, PodUtils/podssh@T-087). This type asks that
//! one parser with iroh's keys; it holds no copy of it. A line holds a key in
//! one of the forms of `pod_identity::KEY_FORMS`: OpenSSH's public line, the
//! fingerprint, the key in iroh's hex or base32, or the display fingerprint.

use std::path::Path;

use iroh::PublicKey;

/// The QUIC close code of a client whose key the node refuses.
pub const REFUSED: u32 = 403;
/// The reason that goes with [`REFUSED`]. It names no file of the node.
pub const REFUSED_REASON: &str = "this client's key is not in the node's allowlist";

/// The keys of an allowlist, asked with iroh's keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Allowlist(pod_identity::allow::Allowlist);

impl Allowlist {
    /// The keys of `text`.
    pub fn parse(text: &str) -> Allowlist {
        Allowlist(pod_identity::allow::Allowlist::parse(text))
    }

    /// The allowlist in the file at `path`; a missing file is an error, so the
    /// node says that nobody can get in.
    pub fn read(path: &Path) -> Result<Allowlist, String> {
        pod_identity::allow::Allowlist::read(path).map(Allowlist)
    }

    /// Whether `key` may get in.
    pub fn admits(&self, key: &PublicKey) -> bool {
        self.0.admits(&crate::keys::identity_key(key))
    }

    /// Whether `key` may use `route` of the node.
    pub fn grants(&self, key: &PublicKey, route: &str) -> bool {
        self.0.grants(&crate::keys::identity_key(key), route)
    }

    /// Whether `key` may reach the node's target that has no route: a key
    /// whose lines all say `routes=A,B` reaches those routes alone.
    pub fn reaches_target(&self, key: &PublicKey) -> bool {
        self.0.reaches_target(&crate::keys::identity_key(key))
    }

    /// How many keys the list holds.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The numbers of the lines that are not a key, from 1.
    pub fn bad_lines(&self) -> &[usize] {
        self.0.bad_lines()
    }

    /// The words of a refusal of the bad lines, with the forms of a line;
    /// `None` when each line is a key or a comment.
    pub fn bad_lines_said(&self) -> Option<String> {
        self.0.bad_lines_said()
    }
}
