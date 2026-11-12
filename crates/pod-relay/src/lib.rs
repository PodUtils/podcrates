//! The client of a WebSocket-to-TCP relay of the podssh contract, for the
//! crates of podcrates and their binaries:
//!
//! - [`budget`]: the bytes and the age of one stream through the relay,
//!   after which a caller replaces it (podnet's `RelayBudget`);
//! - [`stream`]: a forward session as a byte stream;
//! - [`relay`]: which relay hosts to use, in what order (an explicit list,
//!   or the default host followed by alternates from the relay's own pool);
//! - [`pool`]: the pool, as the relay publishes it, cached between runs,
//!   with its signed lists and the relay members of a net;
//! - [`health`]: one host's `/health`;
//! - [`token`] and [`cache`]: tokens from the settings, the cache or a fresh
//!   mint, kept between runs in a private file, never printed;
//! - [`addresses`]: the addresses that carried a session to each relay
//!   host, kept in the cache for a run whose resolver fails;
//! - [`mod@open`]: a forward session, failing over from host to host with
//!   backoff, one bounded attempt per host; [`failover`] and [`failures`]:
//!   the modes of a failover and the record of each host's faults;
//! - [`gate`]: each relay origin's gate, which the sessions of a run share,
//!   so that a wave of failures costs one probe;
//! - [`rank`], [`rank_list`], [`rank_auto`]: the rank of the relay hosts as
//!   measured from this host, the order that it gives, and the rank made in
//!   a session's background;
//! - [`cleartext`]: the road with no TLS on the relay's port, last, with a
//!   token of one session, for a proxy that cuts TLS;
//! - [`h2_road`]: which forward sessions are streams of one HTTP/2
//!   connection, by RFC 8441's extended CONNECT;
//! - [`quic_road`]: QUIC on UDP to a relay that offers it, raced with the
//!   TCP roads where a probe found that UDP passes (feature `quic`);
//! - [`http_road`]: a forward session over plain requests of HTTP/1.1 where
//!   a proxy refuses the WebSocket upgrade;
//! - [`forward_layer`]: a forward session in the resumable layer of
//!   `pod-session`, whose far end is the relay role;
//! - [`keyed`]: a mint or a pair that the relay asks this client's key to
//!   sign;
//! - `pair` (feature `pair`): the pairs of the reverse road, made, asked
//!   about and stopped on the relay's control host;
//! - `reverse` (feature `pair`): the node and the operator of the reverse
//!   road, with the end-to-end channel of `pod-e2e` and the resumable layer;
//! - `grant` (feature `grant`): grants used once, OpenSSH user certificates
//!   that the node checks in the channel before it dials;
//! - `code` (feature `code`): the exchange of a pairing by a short code,
//!   over the mailbox of `pod-wormhole`;
//! - `blocking` (feature `blocking`): a synchronous facade over the pairs,
//!   the runners and the forward sessions, for a caller with no async
//!   runtime: it owns a runtime on the current thread, and refuses a call
//!   from inside a tokio runtime.
//!
//! **The caller gives the way out and the state.** Each call that reaches a
//! relay takes a [`Context`]: the `pod_ws::Way` (the TLS context, the
//! dialer, the user agent), the places of the cache, the [`Settings`] that
//! the consumer read from its environment, the sink of its events, and the
//! state that the sessions of a run share. The crate reads no variable of
//! the environment and keeps no process state.
//!
//! **Typed failures.** Each error converts to a `pod_core::Failure` with its
//! stage and class (the module [`failure`]).

pub mod addresses;
#[cfg(feature = "blocking")]
pub mod blocking;
pub mod budget;
pub mod cache;
pub mod cleartext;
#[cfg(feature = "code")]
pub mod code;
pub mod context;
pub mod failover;
pub mod failure;
pub mod failures;
pub mod forward_layer;
#[cfg(feature = "fuzzing")]
pub mod fuzzing;
pub mod gate;
#[cfg(feature = "grant")]
pub mod grant;
pub mod h2_road;
pub mod health;
pub mod http_road;
pub mod keyed;
pub mod open;
#[cfg(feature = "pair")]
pub mod pair;
pub mod pool;
pub mod quic_road;
pub mod rank;
pub mod rank_auto;
pub mod rank_list;
pub mod relay;
#[cfg(feature = "pair")]
pub mod reverse;
pub mod settings;
pub mod stream;
#[cfg(test)]
mod testing;
pub mod token;

pub use budget::RelayBudget;
pub use context::Context;
pub use open::{Failure, OpenError, Opened, Request, open};
pub use relay::{Relay, default_relay_host};
pub use settings::{Names, Settings};
