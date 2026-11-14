//! An OCI registry client for Linux: the parse of a reference by docker's
//! rules, digests checked while the bytes are written, the four OCI
//! documents, the platform of a pull, the logins of docker and podman, the
//! distribution API over `pod-proxy` and `pod-tls`, the policy of insecure
//! registries, the resolve of a reference and the pool that fetches the
//! blobs of a manifest into the store of the caller.
//!
//! Four rules:
//!
//! 1. HTTPS only, with no plain-HTTP fallback of its own ([`registry`]):
//!    plain HTTP only for a registry that the caller named insecure
//!    ([`transport::Policy`]), and the client says so each time.
//! 2. Each blob is checked while it is written, and gets to the store of
//!    the caller only when its digest and its size agree
//!    ([`digest::Verifier`], [`pull::Blobs`]).
//! 3. The crate builds no TLS configuration that does not check a
//!    certificate: the caller gives one for an insecure registry.
//! 4. No secret in a log or an error: [`credentials::Credential`] has no
//!    debug view of its password, and each URL in a message loses its
//!    query ([`registry::redact`]).
//!
//! The caller gives each setting: the HTTP client of `pod-proxy` (its proxy,
//! its TLS context and its user agent), the trusts, the policy, the logins
//! ([`credentials::Files`] at the paths of the caller), the sink of events
//! and the file-size ceiling. The crate reads no variable, prints nothing
//! and keeps no process state. Each failure is a [`pod_core::Failure`]
//! ([`error`]).
//!
//! The crate is Linux only: on another target it is empty.
#![cfg(target_os = "linux")]
#![forbid(unsafe_code)]

pub mod credentials;
pub mod digest;
pub mod error;
mod host;
pub mod layout;
mod net;
pub mod platform;
pub mod pull;
pub mod reference;
pub mod registry;
pub mod spec;
pub mod transport;
pub mod trust;

pub use digest::Digest;
pub use error::{Refusal, Result, refusal};
pub use net::REDIRECTS;
pub use platform::Platform;
pub use reference::Reference;
pub use registry::Client;
