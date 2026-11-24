//! The release of a product of PodUtils: its index, the check of its
//! signature and of its files, and the self-update of a running binary.
//!
//! * [`sums`]: a `SHA256SUMS` file (the lines of `sha256sum`), with one
//!   valid line for each name.
//! * [`sigstore`]: the keyless cosign bundle of `SHA256SUMS`
//!   (`SHA256SUMS.sigstore.json`), checked with no network: the digest, the
//!   signature, the chain of the leaf to Fulcio's root at the time of Rekor,
//!   the identity of the leaf ([`sigstore::Identity`], a setting of the
//!   caller), and Rekor's promise of the entry.
//! * [`verify`]: a release in one step: the bundle, then each file against
//!   `SHA256SUMS`, then the index against `SHA256SUMS`.
//! * [`index`]: the release index `release-index.json` (the schema
//!   `schemas/release-index.v1.schema.json`); `scripts/release-index.py`
//!   writes it in CI.
//! * [`version`]: the versions of the tags, in the order of semver, and the
//!   tags of the list of releases of GitHub's API and of its Atom feed.
//! * [`trial`], [`replace`], [`keep`]: the new binary runs once with
//!   `--version` beside the old one, then takes its place with a rename;
//!   the old one stays as `NAME.prev`, and a rollback puts it back.
//! * `fetch` and `update` (feature `fetch`): the files of a release over
//!   HTTPS through the caller's client, and the steps of an update.
//! * `cli` (feature `cli`): the exit codes and the manual of the binary
//!   `pod-release`, the verifier of a release directory for a CI and an
//!   installer.
//!
//! The crate prints nothing and reads no environment variable: the
//! repository, the identity, the roots, the client and the program name
//! are the caller's.

pub mod index;
pub mod keep;
pub mod replace;
pub mod sigstore;
pub mod sums;
pub mod trial;
pub mod verify;
pub mod version;

#[cfg(feature = "cli")]
pub mod cli;
#[cfg(feature = "fetch")]
pub mod fetch;
#[cfg(feature = "fetch")]
pub mod update;

pub use sigstore::{Identity, Logged, Roots};
pub use sums::Sums;
pub use verify::{Verified, verify_release};
pub use version::Version;

use pod_core::Class;

/// A time of the Unix epoch for a person, in RFC 3339 UTC to the second
/// (`pod_core::clock::rfc3339`); a time before 1970 gives 1970.
pub(crate) fn utc(secs: i64) -> String {
    pod_core::clock::rfc3339(std::time::UNIX_EPOCH + std::time::Duration::from_secs(u64::try_from(secs).unwrap_or(0)))
}

/// Why a step of a release stopped. The text names no query of a URL: a
/// signed URL of an asset carries a credential of the moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The release, a list of releases or a file did not come.
    Unavailable(String),
    /// The server answered 404: no such release or file.
    NotFound(String),
    /// A file came and failed its check: the signature, the identity, a
    /// SHA-256, the trial run of a binary.
    Refused(String),
    /// The binary could not be replaced; it is as it was.
    CantWrite(String),
    /// The settings of the caller cannot be used.
    Config(String),
}

impl Error {
    /// The text of the failure.
    pub fn why(&self) -> &str {
        match self {
            Error::Unavailable(why)
            | Error::NotFound(why)
            | Error::Refused(why)
            | Error::CantWrite(why)
            | Error::Config(why) => why,
        }
    }

    /// The class of the failure. A consumer maps it to its exit code;
    /// [`Error::CantWrite`] is `CantCreate` (73, `EX_CANTCREAT`).
    pub fn class(&self) -> Class {
        match self {
            Error::Unavailable(_) => Class::Unavailable,
            Error::CantWrite(_) => Class::CantCreate,
            Error::NotFound(_) => Class::NotFound,
            Error::Refused(_) => Class::Refused,
            Error::Config(_) => Class::Config,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.why())
    }
}

impl std::error::Error for Error {}

/// The lowercase hex of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The SHA-256 of `bytes`.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).into()
}
