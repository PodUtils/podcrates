//! Turning a host name into addresses without trusting that this host's
//! resolver works. A [`Resolver`] asks, in order ([`Source::ORDER`]):
//!
//! 1. an IP literal is its own address;
//! 2. the addresses pinned for the name ([`Settings::pins`]: podssh's
//!    `--relay-addr`, podnet's `--resolve`), for hosts with no DNS at all;
//! 3. the system resolver;
//! 4. the addresses that carried a session to the name within [`FRESH`]
//!    ([`Memory::set_remembered`]: the consumer keeps them between runs);
//! 5. DNS over HTTPS to the resolvers of the settings ([`Doh`]: data; by
//!    default the services of the kind `doh` of `pod-services`), through
//!    the caller's dial ([`Resolver::with_doh`]), whose certificates are
//!    verified against the address or the name asked like any other: each
//!    resolver's JSON form where it serves one, and RFC 8484's wire form
//!    ([`wire`]) where it serves none or its answer is not of that form;
//! 6. the older addresses that carried a session, last.
//!
//! A dial through a proxy that resolves names gives the name to the proxy,
//! and none of this runs for the target. TLS to the target always checks
//! the target's name, so a wrong pinned, resolved or remembered address
//! fails the handshake instead of connecting somewhere else.
//!
//! The same resolvers, through the same way or another ([`Way`]), give the
//! SRV records of a name ([`Way::srv`]), its HTTPS record for Encrypted
//! Client Hello ([`https`], [`Way::https`]) and an answer of the JSON form
//! ([`Way::query`]).
//!
//! **No process state, no output.** The settings, the pins, the cache of
//! addresses ([`Memory`]) and the way out are values of the [`Resolver`]
//! that the consumer owns. The crate reads no variable and no file unless
//! the caller asks ([`route::has_default_route`]), prints nothing, writes
//! its diagnostics to `tracing` and counts its answers with the caller's
//! [`pod_core::Observer`].

mod ask;
pub mod doh;
mod get;
pub mod h2;
pub mod https;
pub mod json;
mod kept;
mod lookup;
mod memory;
mod miss;
mod order;
mod question;
mod resolver;
pub mod route;
mod settings;
mod way;
pub mod wire;

pub use doh::{Doh, Form, Names};
pub use json::Srv;
pub use kept::{IDLE, Kept};
pub use memory::{FRESH, Memory, Remembered, split};
pub use order::{Found, Source};
pub use pod_core::SOURCE_WAIT;
pub use resolver::Resolver;
pub use settings::{CONNECT, EACH, Nxdomain, Settings, parse_pins};
pub use way::Way;

/// The largest answer read: a DNS message is at most 64 KiB.
pub const MAX_ANSWER: usize = 64 * 1024;

/// A resolver's answer that the name does not exist (DNS status 3). It holds
/// for every record type and every resolver of its operator.
pub const NXDOMAIN: &str = "no such name (NXDOMAIN)";

/// Letters, digits, `.`, `-` and `_` only: safe in a query string and in DNS.
pub(crate) fn dns_safe(host: &str) -> bool {
    !host.is_empty() && host.len() <= 253 && host.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
}

#[cfg(test)]
mod tests;
