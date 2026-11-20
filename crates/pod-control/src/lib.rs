//! One local control API for each long-running program of PodUtils: JSON
//! lines on a Unix socket or a Windows named pipe, version [`PROTOCOL`] in
//! each line ([`wire`]).
//!
//! * The catalogue ([`Method`], [`Spec`]): the wire name, the route (local
//!   or delegated), the tier and the summary of each method. The dispatch,
//!   `methods` and the JSON Schema ([`schema()`]) are made from it. A host
//!   adds its own methods as [`Extra`] rows.
//! * The tiers ([`Tier`]): read, operate and master. A method above the
//!   tier of the caller gets the answer of a name that is not there
//!   (`unknown-method`), word for word. A method whose effect outlives its
//!   token has the master tier; `auth` gives the master token.
//! * The host ([`Host`]): `status`, `metrics`, `reload`, `drain` and `stop`,
//!   answered in the thread of the host from [`Incoming`]; `version`,
//!   `methods` and `auth` are local. A request can call the client back on
//!   its connection ([`Context::call`]), within a budget for each request and
//!   for each connection.
//! * The frame: one cap of length, a check of UTF-8, a deadline for each
//!   whole line, and the check of the peer user at the accept.
//! * The client ([`Client`]) and the conformance check that a program runs
//!   against its own server ([`conformance`]).
//!
//! The crate starts no runtime: the server is std threads, so a synchronous
//! program takes it as it is. It prints nothing and does not exit.

mod catalogue;
mod client;
pub mod conformance;
mod dispatch;
mod frame;
mod host;
#[cfg(windows)]
pub mod pipe;
mod schema;
mod server;
#[cfg(unix)]
pub mod unix;
pub mod wire;

pub use catalogue::{Catalogue, CatalogueFault, Entry, Extra, Method, Route, Spec, Tier, is_method_name};
pub use client::{Back, Client, ClientSettings, Error, MethodInfo, Version};
pub use frame::Stream;
pub use host::{CallFault, Context, Drain, Host, Metrics, Outcome, Peer};
pub use schema::{SCHEMA_ID, schema};
pub use server::{
    CALLBACKS_PER_CONNECTION, CALLBACKS_PER_REQUEST, Call, Incoming, LINE_MAX, Settings, Snapshot, Token,
};
pub use wire::{Code, Fault, PROTOCOL};

#[cfg(test)]
mod tests;
