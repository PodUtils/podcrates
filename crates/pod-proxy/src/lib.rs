//! The way out of a TCP connection for the crates of podcrates and their
//! binaries:
//!
//! * [`Dialer`] ([`dial`]): directly, with the race of the addresses that
//!   the caller's resolver gives (RFC 8305) and Multipath TCP on request
//!   ([`mptcp`]); or through an HTTP CONNECT proxy, with Basic or Digest
//!   ([`digest`]) and the words of a 407, or a SOCKS5 proxy (`socks5://`
//!   resolves the name here, `socks5h://` sends it). It implements
//!   [`pod_core::Dial`]; a proxy on a Unix socket or at a command gives a
//!   [`Stream`] of its own kind.
//! * [`Proxy`] ([`proxy`]): the proxy that a URL names, on TCP, on a Unix
//!   socket or at a command; [`Environment`] ([`env`](mod@env)): the proxy
//!   and the `NO_PROXY` list of the variables of an environment, which the
//!   caller reads through its own reader, by its [`Rules`]; [`detect`]: the
//!   proxy of a tool's configuration.
//! * [`DialError`], [`Cut`], [`TlsFailure`], [`Missed`]: why a connection
//!   failed and how it ended, each also a [`pod_core::Failure`].
//! * [`http`]: one HTTP/1.1 request and its response over an open stream,
//!   and a server's reading of a request; [`Client`]: a GET over HTTPS, and
//!   a request of any method to an `http://` or `https://` URL.
//! * [`Credentials`]: the user and the password that a client of a served
//!   proxy gives.
//! * [`lowat`], [`write_limit`]: the unsent bytes of a socket, and a limit
//!   of a write that makes no progress; [`stun`]: the codec of STUN.
//! * [`resolve::System`]: the system resolver with the caller's pins.
//!
//! **No process state, no output.** The settings are a value that the
//! consumer fills once ([`Settings`]); the crate reads no variable of the
//! environment and no file unless the caller asks ([`Environment::read`],
//! [`Environment::or_detected`]), prints nothing, writes its diagnostics to
//! `tracing` and counts its decisions with the caller's
//! [`pod_core::Observer`]. The names of the consumer (the program in the
//! words, the user agent) are settings.

pub mod client;
mod connect;
pub mod credentials;
pub mod cut;
pub mod detect;
pub mod dial;
pub mod digest;
pub mod env;
mod error;
pub mod hint;
pub mod host;
pub mod http;
pub mod lowat;
pub mod mptcp;
pub mod proxy;
mod proxy_auth;
mod race;
pub mod resolve;
mod socks5;
pub mod stream;
pub mod stun;
pub mod write_limit;

pub use client::Client;
pub use connect::MAX_PROXY_RESPONSE;
pub use credentials::Credentials;
pub use cut::{Cut, Missed, TlsFailure};
pub use dial::{ATTEMPT_DELAY, Dialer, ProxyChoice, Settings};
pub use env::{
    Environment, Given, NO_PROXY_VARS, NO_PROXY_VARS_UPPER_FIRST, NoProxy, Origin, PROXY_VARS, PROXY_VARS_UPPER_FIRST,
    Rules,
};
pub use error::DialError;
pub use host::authority;
pub use mptcp::Multipath;
pub use proxy::{Carrier, HttpProxy, Proxy, ProxyKind};
pub use race::{dial_addrs, group_errors, interleave};
#[cfg(fuzzing)]
pub use socks5::fuzz_exchange;
pub use stream::{ChildIo, Stream};
