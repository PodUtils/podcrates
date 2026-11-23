//! The roads out that are not the relay: a proxy, a forwarder or a server
//! of another product carries the bytes to a target, or to a bridge of
//! Tor, or to a tunnel of Cloudflare. Each road is behind its own feature:
//!
//! * `via` ([`via`]): a second forward proxy over TLS, by HTTP/1.1's or
//!   HTTP/2's CONNECT;
//! * `forwarder` ([`forwarder`]): a WebSocket forwarder of another product
//!   that takes its target from the client (in its URL, in a first frame,
//!   or by Wisp); it turns on `wisp`;
//! * `wisp` ([`wisp`]): many TCP streams over one WebSocket, by the Wisp
//!   protocol of Mercury Workshop;
//! * `wstunnel` ([`wstunnel`]): a server of erebe/wstunnel;
//! * `meek` ([`meek`]) and `webtunnel` ([`webtunnel`]): Tor's two
//!   pluggable transports meek_lite and WebTunnel, with what they share
//!   ([`pt`]);
//! * `cloudflare` ([`cloudflare`]): the road of `cloudflared access tcp`,
//!   with the credentials of Cloudflare Access;
//! * `vless` ([`vless`]), `vmess` ([`vmess`]) and `trojan` ([`trojan`]): a
//!   server of Xray-core's protocols that the user runs, over a WebSocket,
//!   XHTTP or TLS;
//! * `shadowsocks` ([`shadowsocks`]): a Shadowsocks server, in the AEAD
//!   form or Shadowsocks 2022, over TCP or a WebSocket.
//! * `eice` ([`eice`]): AWS's EC2 Instance Connect Endpoint, its URL
//!   presigned with SigV4;
//! * `iap` ([`iap`]): Google Cloud's Identity-Aware Proxy for TCP, a session
//!   that a new link takes up.
//!
//! **Shared parts.** [`carry`] makes a WebSocket session a byte stream,
//! [`end`] says what the end of this side's input does, [`Note`] takes a
//! peer's words, [`Duplex`] is the stream of a tunnel. They have no
//! feature: each road, and each road of a consumer that is not here yet,
//! uses them.
//!
//! **The way out is the caller's.** Each road dials through a
//! [`pod_ws::Way`]: the dialer, the TLS context of the binary and its user
//! agent. The crate reads no variable of the environment, keeps no process
//! state and prints nothing: a peer's words go to the caller's events
//! ([`pod_core::Events`]), and its diagnostics go to `tracing`.
//!
//! **Each road that takes its target from the client is a
//! [`pod_core::Dial`]**: [`via::Client`], [`forwarder::Forwarder`],
//! [`wisp::Road`], [`wstunnel::Tunnel`], and `vless::Road`, `vmess::Road`,
//! `trojan::Road` and `shadowsocks::Road` (each with its feature). Its failure is a
//! [`pod_core::Failure`] with its stage and its class. meek_lite, WebTunnel,
//! the Access client, EICE and IAP reach one far end that the bridge, the
//! tunnel or the cloud names, and are not dialers.

use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncWrite};

pub mod carry;
pub mod coalesce;
pub mod end;
pub mod replay;
mod road;

#[cfg(feature = "cloudflare")]
pub mod cloudflare;
#[cfg(feature = "eice")]
pub mod eice;
#[cfg(feature = "forwarder")]
pub mod forwarder;
#[cfg(feature = "fuzzing")]
pub mod fuzzing;
#[cfg(feature = "iap")]
pub mod iap;
#[cfg(feature = "meek")]
pub mod meek;
#[cfg(any(feature = "meek", feature = "webtunnel"))]
pub mod pt;
#[cfg(feature = "shadowsocks")]
pub mod shadowsocks;
#[cfg(feature = "trojan")]
pub mod trojan;
#[cfg(feature = "via")]
pub mod via;
#[cfg(feature = "vless")]
pub mod vless;
#[cfg(feature = "vmess")]
pub mod vmess;
#[cfg(feature = "webtunnel")]
pub mod webtunnel;
#[cfg(feature = "wisp")]
pub mod wisp;
#[cfg(feature = "wstunnel")]
pub mod wstunnel;

pub use carry::{Carried, Ended, Form, Status};
pub use end::AtEnd;

/// Where a peer's own words go (a forwarder's text frame, a Wisp server's
/// message of the day): the caller's events, each one line made safe.
pub type Note = Arc<dyn pod_core::Events>;

/// A stream of a tunnel: TLS, a stream of HTTP/2, a pipe in memory.
pub trait Duplex: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> Duplex for T {}
