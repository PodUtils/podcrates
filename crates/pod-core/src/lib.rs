//! The bottom crate of podcrates. Each crate of the workspace shares these
//! parts, and no part here depends on another pod crate:
//!
//! * [`dial`]: the [`Target`], the [`Dial`] and [`Resolve`] traits and the
//!   boxed stream ([`BoxedIo`]);
//! * [`datagrams`]: the [`Datagrams`] trait, in the shape of the UDP socket
//!   of quinn and noq, and the system socket;
//! * [`failure`]: a [`Failure`] with its [`Stage`] and its [`Class`];
//! * [`event`]: [`Event`]s for the user and an [`Observer`] of decisions, in
//!   place of output of a crate's own;
//! * [`files`]: the private files of a program and of a user, in the
//!   places of its [`files::App`];
//! * [`entropy`]: the random source, which a test can plant;
//! * [`http1`]: the head of an HTTP/1.1 message: its bounds, its parse and
//!   its write, with no I/O, for each reader of a crate;
//! * [`lock`]: an advisory lock of a file for the life of a value, on each
//!   system;
//! * [`metered`], [`quota`], [`pick`], [`backoff`], [`clock`], [`text`],
//!   [`names`]: the small helpers.
//!
//! The feature `async` (on by default) gives [`dial`], [`datagrams`] and
//! [`metered`], which are on tokio. With no default feature, the other
//! parts build with no tokio, for a synchronous consumer.

pub mod backoff;
pub mod clock;
#[cfg(feature = "async")]
pub mod datagrams;
#[cfg(feature = "async")]
pub mod dial;
pub mod entropy;
pub mod event;
pub mod failure;
pub mod files;
pub mod http1;
pub mod lock;
#[cfg(feature = "async")]
pub mod metered;
pub mod names;
pub mod pick;
pub mod quota;
pub mod text;

#[cfg(feature = "async")]
pub use datagrams::{Datagrams, RecvMeta, SystemDatagrams, Transmit};
#[cfg(feature = "async")]
pub use dial::{BoxFuture, BoxedIo, Connected, Dial, Found, Io, Resolve, SOURCE_WAIT, Source, Target};
pub use event::{Event, Events, Level, NoEvents, NoObserver, Observer, Report};
pub use failure::{Class, Failure, Stage};

/// The version of `schemas/log.v1.schema.json`: the constant of the field
/// `schema`, the first key of each JSON log line.
pub const LOG_SCHEMA: u32 = 1;

/// The values of the field `level` of a JSON log line
/// (`schemas/log.v1.schema.json`), the most urgent first.
pub const LOG_LEVELS: [&str; 5] = ["error", "warn", "info", "debug", "trace"];
#[cfg(feature = "async")]
pub use metered::Metered;
