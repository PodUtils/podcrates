//! The carriers and the stores of a host with no network (PodUtils/podnet#499).
//!
//! A host with no path has a serial port, a radio and a disk. This crate
//! gives the library parts that a session layer puts above them:
//!
//! * [`serial`]: the address `serial:PATH[,BAUD]`, the raw 8N1 open of a
//!   POSIX terminal that restores its settings, the AX.25 frames of a
//!   point-to-point line in KISS framing, and the window of 1 to 7 frames
//!   with a timeout derived from the baud rate.
//! * [`hf`]: an ARDOP or VARA modem behind the same serial line, with an
//!   ASCII command set, a busy signal, a turnaround delay and a half-duplex
//!   rule. The session ARQ defers to the ARQ of the modem ([`hf::DeferArq`]).
//! * [`spool`]: a directory of files that stores a message for a peer that
//!   comes later. A file carries its checksum, its sequence and the sender.
//! * [`discovery`]: a signed announcement in a DNS-SD TXT record, one radio
//!   interface, and the rule that a bad signature is ignored.
//! * [`bundle`]: a self-describing bundle with a signed manifest, and a
//!   delta from a named base.
//! * [`capture`]: one record format for the bytes of a link, merged by the
//!   timestamp, with redaction when the record is written.
//! * [`pairing`]: an eight-word code, single-use, bound to an Ed25519 key
//!   of `pod-identity`.
//! * [`replication`]: an append-only log with last-writer-wins for each key,
//!   a vector clock and a stated tie break. The README states the model.
//!
//! **No process state.** The crate reads no variable of the environment and
//! prints nothing. A path, a key and a clock are the caller's.
//!
//! **Typed failures.** [`Error`] gives its kind as a variant and as one
//! name, for a test and a log line.
//!
//! The feature `serial` is on by default. It adds the open of a serial
//! line, which uses `libc` on Unix. The frame layers build without it.

mod error;
pub(crate) mod hex;

pub mod bundle;
pub mod capture;
pub mod discovery;
pub mod hf;
pub mod pairing;
pub mod replication;
pub mod serial;
pub mod spool;

pub use error::Error;
