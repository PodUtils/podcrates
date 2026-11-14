//! pod-wg: the WireGuard initiator of WARP, sans I/O.
//!
//! The crate makes and reads the bytes of each message of WireGuard (the
//! WireGuard paper and wireguard.com/protocol), on this side as the
//! initiator: the handshake (Noise IKpsk2, with a preshared key or none), the
//! cookie of a responder under load, the transport messages with the window
//! of their counters, and the timers of a tunnel (the rekey, its resend, a
//! dead path, the keepalive). It owns no socket, no task and no clock: the
//! caller sends and receives the bytes, and gives the time `now` of its
//! monotonic clock to each call that reads a timer.
//!
//! - [`Keys`]: the keys of a device and of its peer, their preshared key,
//!   and WARP's reserved bytes (bytes 1 to 3 of each message).
//! - [`initiate`], [`initiate_from`], [`initiate_with`]: an
//!   [`Initiation`]; [`Initiation::finish`] reads the response into a
//!   [`Session`].
//! - [`Cookie`]: the cookie of a reply; [`Initiation::add_mac2`] puts its
//!   mac2 in an initiation.
//! - [`Session`]: [`Session::seal`] and [`Session::open`] of the transport
//!   messages.
//! - [`Tunnel`]: the current and the previous session, the rekey and the
//!   keepalive; [`Stamps`]: the timestamps of the initiations, which never go
//!   back.
//! - `standin` (feature `standin`): a responder for the tests of a
//!   consumer, in memory and on the loopback.
//!
//! The crate reads no environment variable, prints nothing and keeps no
//! process state. The random bytes come from the system, or from the
//! caller's [`pod_core::entropy::Entropy`]. Each [`Error`] converts to a
//! [`pod_core::Failure`] of the stage `Warp`. ChaCha20-Poly1305 is ring's,
//! X25519 is curve25519-dalek's, BLAKE2s and HChaCha20 are the crate's.

mod blake2s;
mod cookie;
mod crypto;
mod error;
#[cfg(feature = "fuzzing")]
pub mod fuzzing;
mod handshake;
mod session;
#[cfg(any(test, feature = "standin"))]
pub mod standin;
mod tunnel;

pub use cookie::{COOKIE_AGE, COOKIE_REPLY_LEN, Cookie};
pub use crypto::Stamps;
pub use error::Error;
pub use handshake::{
    INITIATION_LEN, Initiation, Keys, MSG_COOKIE, MSG_DATA, MSG_INITIATION, MSG_RESPONSE, RESPONSE_LEN, initiate,
    initiate_from, initiate_with,
};
pub use session::Session;
pub use tunnel::{HANDSHAKE_DEAD, KEEPALIVE, Outgoing, REKEY_AFTER, REKEY_TIMEOUT, Received, Tunnel};
