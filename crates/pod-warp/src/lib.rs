//! Cloudflare WARP, spoken directly: anonymous registration, MASQUE
//! CONNECT-IP over HTTP/2 on TCP 443, and a userspace IP stack over it,
//! given to the caller as a network ([`pod_core::Dial`] and
//! [`pod_core::Datagrams`]).
//!
//! * [`register`], [`api`], [`device`], [`hints`]: the two calls of a
//!   registration and the account calls, through the HTTP client of the
//!   caller, at each base of the API in turn.
//! * [`identity`], [`key`], [`der`]: the P-256 key of the device and its
//!   self-signed certificate, in pure Rust; podnet's PEM file form.
//! * [`store`], [`files`]: the identity file of the caller: the lock, the
//!   record of a created device, the check of the owner and of the mode,
//!   and the consent to read a file of another user.
//! * [`door`]: the order of the front doors of the record, or one door
//!   that the caller pins.
//! * [`tls`], [`session`], [`capsule`]: TLS with the key of the front door
//!   pinned, and the CONNECT-IP session over HTTP/2 through the caller's
//!   dial.
//! * [`h3`]: the session over HTTP/3 on `pod-quic`; [`wg`]: the WireGuard
//!   tunnel on `pod-wg`; [`udp`]: the UDP of the caller for both
//!   ([`Way::udp`]); a carried device ([`Settings::carry`],
//!   [`CARRY_MTU`]).
//! * [`Warp`]: a registered device; [`Warp::open_session`] and
//!   [`Warp::connect`].
//! * [`leg`]: the relay leg of the caller, the race of the direct path and
//!   the relay, and the text of the caller's leg file.
//! * [`net`], [`dns`]: [`WarpNet`], the stack of `pod-netstack` over a
//!   session as a dial (names resolved inside the tunnel) and as datagrams;
//!   [`NetSettings`]: its congestion controller, its TCP burst, its counters
//!   of the link and the budget of a relay.
//! * [`check`]: the check that a packet crosses a new session.
//! * [`rules`]: the rules of a WARP proxy, with the lint of the caller.
//! * [`service`]: the facts of WARP of `pod-services`.
//!
//! The crate reads no environment variable and no file that the caller
//! does not name, prints nothing and keeps no global state. A failure is a
//! [`pod_core::Failure`] with the stage `Warp` (or the stage of the dial);
//! a line for the user is a [`pod_core::Event`] of the caller's
//! [`pod_core::Report`].
//!
//! **Build setting.** The stack is `pod-netstack`'s: each binary sets
//! `SMOLTCP_IFACE_MAX_ADDR_COUNT = "8"` in the `[env]` table of its
//! `.cargo/config.toml` (the README of `pod-netstack`).

pub mod api;
pub mod capsule;
mod carry;
pub mod check;
pub mod der;
pub mod device;
pub mod dns;
pub mod door;
pub mod files;
pub mod h3;
pub mod hints;
pub mod identity;
pub mod key;
pub mod leg;
pub mod net;
mod open;
pub mod register;
pub mod rules;
pub mod service;
pub mod session;
pub mod store;
pub mod tls;
pub mod udp;
mod warp;
pub mod wg;

pub use api::Api;
pub use check::DataPathCheck;
pub use device::Device;
pub use door::{Door, Routes};
pub use files::Foreign;
pub use hints::DeviceHints;
pub use identity::Identity;
pub use leg::{Leg, Relay};
pub use net::{NetSettings, WarpNet};
pub use session::{MasqueRx, MasqueSession, MasqueTx, Meter, Way};
pub use store::{IdentityFile, Stored};
pub use udp::{SystemUdp, UdpWay};
pub use warp::{CARRY_MTU, MTU, Settings, Warp};

/// A scratch directory of a test, new and empty.
#[cfg(test)]
pub(crate) fn test_dir(tag: &str) -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("pod-warp-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
