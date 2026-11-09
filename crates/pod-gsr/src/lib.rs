//! The GSRN relay of gsocket (hackerschoice): two peers that share a secret
//! meet at the address that the secret gives, then speak TLS with SRP.
//!
//! `open` (feature `async`) dials the shard of the relay through the
//! `pod_core::Dial` of the caller, sends `LISTEN` or `CONNECT`, waits for the
//! `START` of the relay and pings the relay while it waits, sends `ACCEPT`,
//! and runs the TLS handshake. The result is a `Stream` whose bytes are the
//! caller's own.
//!
//! The facts of the relay are the data file `hackerschoice-gsr` of
//! `pod-services`: the domain and the port, the two keys of the derivations,
//! the SRP user, group and cipher, the shape of the password, the version of
//! the protocol, the ping interval and the idle limit. [`Settings::of`] reads
//! them one time.
//!
//! rustls has no SRP cipher suite, so the crate links OpenSSL. The workspace
//! builds OpenSSL from source, so a binary links it static (call C34).
//!
//! With no feature, the crate is the core with no tokio: [`Identity`],
//! [`Settings`], [`Role`], [`Version`] and [`check_tls`].
//!
//! The crate reads no variable, keeps no process state and prints nothing.
//! Each failure is a [`pod_core::Failure`] with its stage and its class.

// zig's compiler-rt has no __atomic_is_lock_free for i686, which OpenSSL calls.
#[cfg(all(target_arch = "x86", target_os = "linux", target_env = "musl"))]
mod atomic;
mod identity;
#[cfg(feature = "rt")]
mod session;
mod settings;
mod srp;
#[cfg(feature = "rt")]
mod stream;
#[cfg(any(fuzzing, feature = "fuzzing"))]
pub mod wire;
#[cfg(not(any(fuzzing, feature = "fuzzing")))]
mod wire;

pub use identity::{ADDR_LEN, Identity};
#[cfg(feature = "async")]
pub use session::open;
pub use settings::{MIN_PING, SERVICE, Settings};
pub use srp::check_tls;
#[cfg(feature = "async")]
pub use stream::Stream;
pub use wire::Version;

/// Which end of the tunnel this peer is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Register the address and wait for a peer.
    Listen,
    /// Connect to a peer that registered the address.
    Connect,
}
