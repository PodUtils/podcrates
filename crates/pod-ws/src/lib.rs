//! WebSockets for the crates of podcrates and their binaries (RFC 6455):
//!
//! * [`frame`]: the frames, byte-exact, each side masked as RFC 6455 asks;
//!   [`handshake`]: the opening handshake of a client;
//! * [`session`]: an open session ([`RelaySession`]), usable from two
//!   tasks at once, with its [`Limits`]; [`close`]: what a Close carries,
//!   its codes and a hint for each;
//! * [`client`]: the client of the relay (TCP, TLS and the upgrade, with
//!   the token in a header); [`h2_road`]: the same over HTTP/2's extended
//!   CONNECT (RFC 8441), with a [`h2_road::Pool`] that the caller owns;
//!   [`cleartext`]: the same with no TLS, and its probe;
//! * [`url`]: a WebSocket that the user names (`ws://`, `wss://`), with a
//!   subprotocol or an `Authorization` when asked; [`server`]: the server's
//!   side of the handshake; [`bridge`](mod@bridge): a session and a raw stream, both
//!   ways;
//! * [`wake`]: a check for a host that slept;
//! * [`plain`] (feature `plain-ws`): a session over plain TCP to the
//!   loopback, for tests.
//!
//! **The way out is the caller's.** Each connection goes through a [`Way`]:
//! the TLS context of the binary (`pod-tls`), its dialer
//! (`pod_core::Dial`), the reader of a name's HTTPS record for ECH (feature
//! `ech`), and its user agent. The crate reads no variable of the
//! environment, keeps no process state (the pool of HTTP/2 connections is a
//! value of the caller), and prints nothing: its diagnostics go to
//! `tracing`.
//!
//! **Typed failures.** [`ConnectError`], [`SessionError`] and [`WsError`]
//! keep their variants, which a caller matches, and each is also a
//! `pod_core::Failure` with its stage and its class.
//!
//! **No token in a log.** The relay's token goes in its header
//! ([`handshake::TOKEN_HEADER`]) and nowhere else; no error formats it.

pub mod bridge;
pub mod cleartext;
pub mod client;
pub mod close;
pub mod error;
pub mod frame;
pub mod h2_road;
pub mod handshake;
pub mod limits;
#[cfg(feature = "plain-ws")]
pub mod plain;
pub mod server;
pub mod session;
pub mod url;
pub mod wake;
pub mod way;
mod wire;

#[cfg(test)]
mod testing;

pub use bridge::{NoTally, Tally, bridge, connect_bridge};
pub use client::{
    ConnectError, DEFAULT_IDLE_TIMEOUT, DEFAULT_TIMEOUT, Endpoint, LIVENESS_ALLOWED, LIVENESS_EVERY, WsClientConfig,
    connect, doctor,
};
pub use close::close_hint;
pub use error::{SessionError, Verdict, WsError};
pub use frame::Frame;
pub use limits::Limits;
pub use pod_proxy::{Cut, DialError, Missed, TlsFailure};
pub use pod_tls::Trust;
pub use session::RelaySession;
pub use way::Way;
