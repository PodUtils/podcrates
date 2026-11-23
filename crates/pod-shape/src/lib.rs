//! The shape of the record layer as data.
//!
//! A [`Profile`] names the traffic that a session imitates: a page load, a
//! video call, a terminal or a file transfer. It holds a distribution over
//! the record buckets, the rule that says when to pad, the first-flight
//! target, the pace and the entropy band. The profile files live under
//! `profiles/` and are part of the binary.
//!
//! * [`Shaper`] chooses a bucket for each record from a seed, pads the body,
//!   and counts the padded bytes and the body bytes. It never repeats a
//!   short cycle.
//! * [`Profile::first_flight`] gives the opening write that fits two seconds
//!   at the measured rate, bounded by the target of the profile.
//! * [`Pacer`] gives the delay of a record from the measured rate, and a
//!   keepalive jittered by about 20 percent.
//! * [`Entropy`] fills a payload inside the band of the profile, inserts one
//!   printable run per 64 KiB, and refuses a nonce outside its four-minute
//!   window.
//! * [`ShapeReport`] puts the declared shape beside the observed one, with
//!   the states [`State::Holds`], [`State::Fails`] and [`State::Unseen`]. A
//!   divergence is a warning in words, not an error.
//!
//! The crate prints nothing, reads no environment variable and keeps no
//! process state. The caller gives the profile and the seed.

mod entropy;
mod error;
mod pacer;
mod profile;
mod report;
mod rng;
mod shaper;

pub use entropy::{Entropy, NONCE_STEP_SECS, NONCE_WINDOW_SECS, Nonce, printable_runs};
pub use error::Error;
pub use pacer::Pacer;
pub use profile::{BUCKETS, Band, Bucket, NAMES, Pace, Profile, Rule, Source};
pub use report::{PaddedFraction, Shape, ShapeReport, State};
pub use shaper::{Shaped, Shaper};
