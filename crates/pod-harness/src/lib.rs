//! pod-harness: the verification harness of podnet#501.
//!
//! The harness measures how visible a road is to a middlebox. Three
//! classifiers read one capture, in increasing cost:
//!
//! - [`field::FieldMatcher`]: the SNI, the ALPN, the JA3 and JA4 hashes and
//!   the TLS record sizes, against a rule set from JSON;
//! - [`flow::FlowStats`]: the size and timing distributions, against a
//!   nearest-centroid model from JSON;
//! - [`prefix::Prefix`]: the first N bytes, against a byte n-gram table from
//!   JSON.
//!
//! Each returns a score in 0..1 and a named reason. [`middlebox::Middlebox`]
//! applies a local fault model to a capture and records its decisions.
//! [`matrix::Table`] holds one row for each road and profile. It renders the
//! rows, including a skipped row with its reason, and it never prints a pass
//! count. [`archive::Archive`] builds the relocatable zip.
//!
//! [`claims::check_claims`] fails on a bare comparative and on a claim with
//! no conditions. [`exit`] re-exports the one table of `pod_core::Class`.
//!
//! The budget of a CI run is [`BUDGET_SECS`]. The crate reads no environment
//! variable for its settings and prints nothing from the library.

pub const BUDGET_SECS: u64 = 600;

pub mod archive;
pub mod capture;
pub mod claims;
pub mod date;
pub mod error;
pub mod exit;
pub mod field;
pub mod fixture;
pub mod flow;
pub mod matrix;
pub mod middlebox;
pub mod prefix;
pub mod tls;
pub mod zip;

pub use archive::Archive;
pub use capture::{Capture, Direction, Packet};
pub use claims::{Finding, check_claims};
pub use error::{Error, Result};
pub use field::FieldMatcher;
pub use fixture::{Case, Fixtures, Run};
pub use flow::FlowStats;
pub use matrix::{Benign, Matrix, Outcome, Profile, Row, Score, Table};
pub use middlebox::{Decision, Fault, Middlebox};
pub use prefix::Prefix;
