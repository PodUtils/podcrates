//! QUIC for the crates of podcrates and their binaries, on noq (call C43):
//!
//! * [`Settings`]: the transport and the handshake's wait; the default is
//!   cloudflared's.
//! * [`keys`]: the reset key and the token key of an endpoint,
//!   HMAC-SHA-256 in pure Rust, so that noq needs no `ring` and no `aws-lc`.
//! * [`endpoint`]: an endpoint on a UDP socket of the system, or [`over`]
//!   any [`pod_core::Datagrams`] (WARP, UDP in a stream); a server
//!   configuration with a token key.
//! * [`connect`]: TLS 1.3 for QUIC on the caller's provider, the client
//!   configuration, a connection within the handshake's wait, and a race of
//!   handshakes.
//! * [`probe`]: one QUIC packet of a reserved version, whose Version
//!   Negotiation tells that UDP reaches a server of QUIC.
//! * [`queued`]: [`pod_core::Datagrams`] over a [`queued::Link`] that has
//!   only async send and receive (podnet's userspace stack under WARP).
//! * [`socket`]: the socket of noq over a [`pod_core::Datagrams`], with a
//!   waker for each sender.
//!
//! **The provider is the caller's.** [`connect::tls`] takes an
//! `Arc<rustls::crypto::CryptoProvider>`, [`connect::client_config`] and
//! [`endpoint::server_config`] take the TLS configuration of the caller,
//! and the crate installs no process default. The tests run on ring.
//!
//! **Failures are typed.** Each function returns a [`pod_core::Failure`]
//! with its stage (QUIC, TLS, or the UDP connection) and its class.
//!
//! The crate re-exports [`noq`], so that a consumer names one version of
//! the stack.

pub mod connect;
pub mod endpoint;
pub mod keys;
pub mod probe;
pub mod queued;
mod settings;
pub mod socket;

pub use endpoint::over;
pub use noq;
pub use settings::{
    DATAGRAMS, FIRST_PACKET_V4, FIRST_PACKET_V6, HANDSHAKE_WAIT, KEEPALIVE, MAX_STREAMS, SILENCE, Settings,
};
