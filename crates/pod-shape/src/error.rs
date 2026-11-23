//! Why a profile, a shaper or an entropy source refused a value.

use std::error::Error as StdError;
use std::fmt;

/// The failure of a profile or of a value that a profile gives.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// No profile file has this name.
    UnknownProfile { name: String },
    /// The TOML text is not a profile.
    Parse { name: String, why: String },
    /// The profile has no source date.
    MissingDate { name: String },
    /// The profile has no source version.
    MissingCoverVersion { name: String },
    /// The source date is not `YYYY-MM-DD`.
    BadDate { name: String, date: String },
    /// The profile has no bucket.
    NoBuckets { name: String },
    /// A bucket size is not one of [`crate::BUCKETS`].
    BadBucket { name: String, size: usize },
    /// A bucket has no weight.
    BadWeight { name: String, size: usize },
    /// The padding rule is not `always`, `first:N` or `below:N`.
    BadRule { name: String, rule: String },
    /// The first-flight target is zero.
    BadTarget { name: String },
    /// The pace has a rate of zero, a jitter above 100 or a keepalive of zero.
    BadPace { name: String },
    /// The entropy band is reversed, outside 0 to 8 bits per byte, or too
    /// narrow for the alphabet.
    BadBand { name: String },
    /// The entropy source cannot pick a symbol count inside the band.
    BandTooNarrow { min: f64, max: f64 },
    /// The printable run is longer than one 64 KiB block.
    PrintableRunTooLong { run: usize, max: usize },
    /// A measured window is outside the entropy band.
    EntropyBand { at: usize, bits: f64 },
    /// A 64 KiB block has the wrong count of printable runs.
    PrintableRuns { block: usize, count: usize, want: usize },
    /// A nonce is older than the acceptance window.
    NonceExpired { age_secs: u64 },
    /// A nonce carries a time after the current one.
    NonceFuture,
    /// The nonce was accepted before.
    Replay,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnknownProfile { name } => write!(f, "no profile is named {name}"),
            Error::Parse { name, why } => write!(f, "the profile {name} is not TOML for a profile: {why}"),
            Error::MissingDate { name } => write!(f, "the profile {name} has no source date"),
            Error::MissingCoverVersion { name } => write!(f, "the profile {name} has no source cover version"),
            Error::BadDate { name, date } => write!(f, "the profile {name} has the date {date}, not YYYY-MM-DD"),
            Error::NoBuckets { name } => write!(f, "the profile {name} has no bucket"),
            Error::BadBucket { name, size } => {
                write!(f, "the profile {name} has the bucket {size}, not one of 0, 128, 512, 1024 and 2048")
            }
            Error::BadWeight { name, size } => write!(f, "the profile {name} has the bucket {size} with no weight"),
            Error::BadRule { name, rule } => {
                write!(f, "the profile {name} has the rule {rule}, not always, first:N or below:N")
            }
            Error::BadTarget { name } => write!(f, "the profile {name} has a first-flight target of zero"),
            Error::BadPace { name } => write!(f, "the profile {name} has a pace that cannot run"),
            Error::BadBand { name } => write!(f, "the profile {name} has an entropy band that cannot hold"),
            Error::BandTooNarrow { min, max } => {
                write!(f, "no symbol count gives an entropy between {min} and {max} bits per byte")
            }
            Error::PrintableRunTooLong { run, max } => {
                write!(f, "a printable run of {run} bytes is above the block of {max} bytes")
            }
            Error::EntropyBand { at, bits } => {
                write!(f, "the window at {at} has {bits:.3} bits per byte, outside the band")
            }
            Error::PrintableRuns { block, count, want } => {
                write!(f, "the block at {block} has {count} printable runs, where it wants {want}")
            }
            Error::NonceExpired { age_secs } => write!(f, "a nonce is {age_secs} seconds old, outside the window"),
            Error::NonceFuture => f.write_str("a nonce carries a time after the current one"),
            Error::Replay => f.write_str("a nonce was accepted before"),
        }
    }
}

impl StdError for Error {}
