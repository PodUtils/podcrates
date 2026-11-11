//! The relays and the services of other parties that the PodUtils binaries
//! use: DNS over HTTPS, STUN, DERP and iroh relays, mailboxes, the edge of
//! Cloudflare's tunnels, WARP, and the rest. Each service is one data file
//! (`services/ID.toml`), and each value of a file has its source and the
//! date when it was read. `build.rs` checks each file against the schema
//! and compiles the set in: nothing is parsed at run time.
//!
//! The crate knows no provider in its code. A consumer reads a service by
//! its id ([`get`]) or the services of a kind ([`of_kind`]), and can lay
//! its own files of the same schema over the built-in set ([`Catalog`],
//! feature `overlay`). The watches that each file names run in the
//! workflows of podcrates. [`forward`] reads a service as an SSH forward
//! relay, and reads the URL of its tunnel in its banner.

mod catalog;
pub mod forward;
mod service;

#[cfg(any(test, feature = "overlay"))]
#[allow(dead_code)]
mod schema;

pub use catalog::Catalog;
#[cfg(feature = "overlay")]
pub use catalog::OverlayError;
pub use service::{Arg, Date, Endpoint, Fact, List, Origin, Rank, Service, Source, Text, Value, Watch};

use std::borrow::Cow;

mod builtin {
    use super::*;
    include!(concat!(env!("OUT_DIR"), "/services.rs"));
}

/// Each built-in service, by rank and then by id.
pub fn all() -> &'static [Service] {
    &builtin::BUILTIN
}

/// The built-in service of `id`.
pub fn get(id: &str) -> Option<&'static Service> {
    builtin::BUILTIN.iter().find(|s| s.id() == id)
}

/// The built-in services of `kind`, by rank and then by id.
pub fn of_kind(kind: &str) -> impl Iterator<Item = &'static Service> + '_ {
    builtin::BUILTIN.iter().filter(move |s| s.kind() == kind)
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod schema_tests;
