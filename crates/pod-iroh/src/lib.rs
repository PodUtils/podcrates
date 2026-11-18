//! The iroh road between two ends (PodUtils/podssh@T-162): QUIC between Ed25519
//! keys, through an iroh relay (HTTPS on port 443, upgraded to a WebSocket)
//! when UDP is refused, and directly when a probe shows that UDP works. It
//! keeps a connection across a lost relay connection and a new address of
//! either end.
//!
//! The endpoint ([`endpoint`]) has no address lookup, the relays given, the
//! caller's proxy, the caller's resolver ([`resolve`]), the caller's TLS
//! provider and trust store, and UDP only after a probe ([`probe`]). A session
//! is one QUIC stream ([`stream`]) with the ALPN [`ALPN`], which carries the
//! records of the resumable layer (`pod-session`, PodUtils/podssh@T-151), so
//! that it can go on over another road (PodUtils/podssh@T-153). [`far`] serves
//! the sessions of a far end, to the client keys of its allowlist ([`allow`]);
//! [`client`] dials a node by its ticket ([`ticket`]). Each end has its key in
//! a private file ([`keys`]), and its home relay is the first of its list that
//! answers ([`relays`]); [`home_relay`] says when its connection to that
//! relay ends and is dialled again. A node keeps a record of its home relay at a server of
//! pkarr, and a client whose ticket's relay does not reach the node asks it
//! ([`pkarr`], PodUtils/podssh@T-350); the record names the node's other roads
//! too, for a client with the node's key alone ([`pinggy`]: the form of a
//! Pinggy address). QUIC's settings are iroh's own, or an arm of an A/B run
//! ([`tuning`], PodUtils/podssh@T-665). The sessions of one process to one node
//! share one connection, each on a stream of its own, a copy's below a
//! terminal's ([`share`], PodUtils/podssh@T-666).
//!
//! **The way out is the caller's.** The endpoint takes the caller's
//! [`pod_proxy::ProxyChoice`] for the relay's dial (iroh dials the relay
//! itself and takes a proxy's URL), and each request of the crate (pkarr,
//! `/ping`, the question of the version) goes through the caller's
//! [`pod_ws::Way`]. The crate reads no variable of the environment: a
//! consumer reads its variables (their names are the constants
//! [`endpoint::UDP_ENV`], [`tuning::QUIC_ENV`], [`pkarr::ENV`],
//! [`relays::ENV`]) and fills the settings.

pub mod allow;
pub mod client;
pub mod endpoint;
pub mod far;
pub mod home_relay;
pub mod keys;
pub mod pinggy;
pub mod pkarr;
pub mod probe;
pub mod relays;
pub mod resolve;
// One connection to a node for the sessions of a process.
pub mod share;
pub mod stream;
// A server of pkarr on the loopback, beside the test relay.
#[cfg(feature = "test-relay")]
pub mod test_pkarr;
#[cfg(feature = "test-relay")]
pub mod test_relay;
pub mod ticket;
// QUIC's settings, and the arms of the A/B run.
pub mod tuning;
// Whether a relay takes this build's iroh.
pub mod version;

pub use allow::Allowlist;
pub use client::{Dialer, Failure, carry};
pub use endpoint::{BindError, Options, Udp, bind};
pub use pkarr::Finder;
pub use share::Lease;
pub use stream::{Lane, Stream, accept_session, open_lane, open_session};

/// The ALPN of the sessions over iroh: a connection with another one is
/// refused in its handshake. A name of podssh's wire, which its nodes in
/// the field speak.
pub const ALPN: &[u8] = b"podssh/1";

/// The ALPN of the same sessions where the far end also reads a copy's
/// first byte ([`stream::SESSION_BULK`]): each end offers it before
/// [`ALPN`], and an end of before takes [`ALPN`].
pub const ALPN_LANES: &[u8] = b"podssh/2";

/// The features of the resumable layer that each end offers over iroh:
/// those of the relay road but `move.v1`, whose move to a new link keeps a
/// session within the limits of the WebSocket relay's sessions, which a
/// QUIC connection does not have.
pub const FEATURES: &[&str] = &["replay.v1", "heartbeat.v1"];
