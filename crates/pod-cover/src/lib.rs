//! The primitives of a listener that answers as a static site until a client
//! proves that it holds the key (PodUtils/podnet#491 and PodUtils/podssh#225).
//!
//! | Module | The rule | What it holds |
//! | --- | --- | --- |
//! | [`site`] | COVER-1 to COVER-3 | a tree that is read once, name routing, an unknown name's own 404, no traversal, a `Server` value with no name of this project |
//! | [`script`] | SCRIPT-1 to SCRIPT-3 | the record histogram, the timing distribution and the split policy as TOML data, with a named refusal of a missing script, date or cover version |
//! | [`switch`] | SWITCH-1 to SWITCH-4 | the HMAC proof over a timestamp, a nonce and the TLS exporter, the window, the replay set and the fall-through to the site |
//! | [`mux`] | MUX-1 and MUX-2 | the schedule of one page load and one session over one volume budget, the cover first |
//! | [`credential`] | DELEG-1 to DELEG-3 | the application-layer credential that one name's key issues, and the check against the same chain |
//!
//! The crate is sans-IO. It opens no socket, reads no clock and keeps no
//! global state. The caller gives the bytes, the instant and the key. Its
//! one read of the file system is [`site::Site::load`] and
//! [`script::Script::load`], which a binary does once at start.
//!
//! The proof of [`switch`] is not a TLS feature. It runs after a real
//! handshake, in the first application bytes. The credential of
//! [`credential`] is not RFC 9345 (its module says what it proves).

pub mod credential;
pub mod mux;
pub mod script;
pub mod site;
pub mod switch;

pub use credential::{Credential, CredentialError};
pub use mux::{Chunk, Frame, Lane, Mux};
pub use script::{Record, Script, ScriptError, SplitPolicy};
pub use site::{Asset, Response, Site, SiteError, Sites};
pub use switch::{Outcome, Switch};
