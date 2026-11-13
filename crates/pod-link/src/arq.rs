//! The selective-repeat ARQ: one ordered byte stream over a datagram
//! carrier that loses, reorders and duplicates its datagrams (row E2-2 of
//! PodUtils/podnet#468, PodUtils/podnet@T-277). A [`crate::Link`] runs over
//! an [`ArqStream`] as it runs over a reliable carrier; the ARQ knows no
//! stream of the link (call C262).
//!
//! The rules of podnet:
//!
//! - a frame has a 32-bit sequence, a 16-bit fragment index and a fragment
//!   count ([`wire`]); a write is one record, cut into fragments of at most
//!   1 KiB and the carrier's unit;
//! - an `ACK` has a bitmap of 32 bytes (256 frames) after the next frame in
//!   order, and at most 256 frames are in flight; each lost fragment goes
//!   again by itself;
//! - an RTO of 200 ms to 8 s; 8 retries of one frame, then the ARQ ends
//!   with [`ArqError::Timeout`] (`arq-timeout`), and the link above it ends;
//! - the sequence never wraps to 0 under the same keys: the ARQ asks the
//!   caller for a rekey ([`ArqHandle::rekey_wanted`]) before the wrap, and
//!   new frames wait at the wrap until [`ArqHandle::rekeyed`] (call C264).
//!
//! [`Arq`] is the state machine with no I/O, for a caller with its own
//! loop; [`ArqStream`] drives it in a task over a
//! [`crate::datagram::Datagrams`].

mod error;
mod machine;
mod recv;
mod rto;
mod send;
mod settings;
mod stats;
mod stream;
pub mod wire;

pub use error::ArqError;
pub use machine::Arq;
pub use settings::{ArqSettings, ArqSettingsError, MAX_RETRIES, MIN_RECEIVE_BUFFER, RETRIES, RTO_CEILING, RTO_FLOOR};
pub use stats::Stats;
pub use stream::{ArqHandle, ArqStream};
