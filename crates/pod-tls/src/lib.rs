//! TLS for the crates of podcrates and their binaries: where the trust
//! anchors come from ([`Trust`]), the rustls configuration of a client over
//! them, with the provider that the caller gives ([`Context`]), the words
//! of a failed handshake ([`Words`]), a TLS server with a P-256 key
//! ([`server`]), the split of a ClientHello into records ([`fragment`]),
//! and, behind features, the pure-Rust crypto provider
//! ([`crypto`], feature `pure`) and Encrypted Client Hello ([`ech`],
//! feature `ech`).
//!
//! **The provider is the caller's.** Each function that makes a client or
//! a server configuration takes an `Arc<rustls::crypto::CryptoProvider>`,
//! and the crate installs no process default: `install_default` is global
//! and succeeds once in a process, so a library that called it would take
//! the choice from the binary that embeds it.
//!
//! **There is no `--insecure`, and there is no parameter that would become
//! one.** The only verifier that the crate builds is rustls's
//! `WebPkiServerVerifier`, whose `verify_server_cert` calls
//! `verify_server_name` each time. A caller that wants another verifier
//! brings its whole configuration ([`Trust::caller`]), and it is used as it
//! is.
//!
//! **No process state.** The configurations kept for each trust, the
//! groups that servers took and the state of ECH are fields of a
//! [`Context`], which the binary makes once and shares.

#[cfg(feature = "pure")]
pub mod crypto;
#[cfg(feature = "ech")]
pub mod ech;

pub mod bundle;
mod context;
mod error;
pub mod fragment;
pub mod groups;
mod hint;
mod kept;
pub mod server;
mod store;
pub mod stores;
mod trust;
mod wrap;

pub use context::{Context, Settings, client_config};
pub use error::Error;
pub use hint::{Words, clock_refused, handshake_error, handshake_failure, handshake_hint};
pub use trust::{
    CallerConfig, ROOTS_PUBLISHED, ROOTS_VERSION, SYSTEM_BUNDLES, TlsRoots, Trust, roots_from_bundle,
    roots_from_compiled_set,
};
pub use wrap::tls_over;
