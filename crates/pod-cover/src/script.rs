//! A cover script: the record sizes, the inter-record delays and the split
//! policy of a cover implementation, as data.
//!
//! The script holds a date and the version of the cover that it measured
//! (SCRIPT-3). A script with no date, no cover version or no split policy is
//! refused at load (SCRIPT-2). [`Script::replay`] turns the histogram and the
//! timing into a deterministic sequence of records for a payload of a given
//! length, so a test can compare a run with the measurement.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

/// One bucket of the record-size histogram.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct SizeBucket {
    /// The fewest bytes of a record of this bucket.
    pub min: usize,
    /// The most bytes of a record of this bucket.
    pub max: usize,
    /// The count of records of this bucket in the measurement.
    pub count: u64,
}

/// One bucket of the inter-record timing distribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct TimingBucket {
    /// The delay of this bucket, in microseconds.
    pub micros: u64,
    /// The most microseconds that a replay adds or takes from `micros`.
    #[serde(default)]
    pub jitter_micros: u64,
    /// The count of records of this bucket in the measurement.
    pub count: u64,
}

/// How the cover splits a TLS record: the smallest and the largest record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct SplitPolicy {
    pub min: usize,
    pub max: usize,
}

/// The smallest record of a split policy (TLS permits 16 bytes).
pub const MIN_RECORD: usize = 16;

/// The largest record of a split policy: a TLS record holds 16 KiB.
pub const MAX_RECORD: usize = 16 * 1024;

/// One record of a replayed script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    /// The bytes of the record.
    pub size: usize,
    /// The delay after the record.
    pub delay: Duration,
}

/// A cover script. The fields are the data of the measurement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Script {
    /// The date of the measurement, `YYYY-MM-DD`.
    pub date: String,
    /// The version of the cover that the measurement read.
    pub cover_version: String,
    /// The seed of the replay. Two replays of one script are equal.
    pub seed: u64,
    /// The record-size histogram.
    pub record_size: Vec<SizeBucket>,
    /// The inter-record timing distribution.
    pub timing: Vec<TimingBucket>,
    /// The TLS record split policy.
    pub split: SplitPolicy,
}

/// Why a script was not read.
#[derive(Debug)]
pub enum ScriptError {
    /// The file is not there (SCRIPT-2: a missing script is a failure at
    /// load, never a generic stream).
    MissingFile(PathBuf),
    /// The file could not be read.
    Io { path: PathBuf, error: std::io::Error },
    /// The TOML is not a script.
    Parse(String),
    /// The script has no date (SCRIPT-3).
    MissingDate,
    /// The script names no cover version (SCRIPT-3).
    MissingCoverVersion,
    /// The script has no TLS record split policy.
    MissingSplit,
    /// A value of the script is not a measurement.
    Invalid(String),
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScriptError::MissingFile(path) => {
                write!(f, "the cover script {} is not there; a cover site needs its measurement", path.display())
            }
            ScriptError::Io { path, error } => write!(f, "the cover script {}: {error}", path.display()),
            ScriptError::Parse(why) => write!(f, "the cover script is not TOML of a script: {why}"),
            ScriptError::MissingDate => write!(f, "the cover script has no date; a script with no date is deleted"),
            ScriptError::MissingCoverVersion => {
                write!(f, "the cover script has no cover_version; a measurement names the cover it read")
            }
            ScriptError::MissingSplit => write!(f, "the cover script has no split policy"),
            ScriptError::Invalid(why) => write!(f, "the cover script is not valid: {why}"),
        }
    }
}

impl std::error::Error for ScriptError {}

/// The TOML document, before its checks.
#[derive(Deserialize)]
struct Raw {
    date: Option<String>,
    cover_version: Option<String>,
    #[serde(default)]
    seed: u64,
    #[serde(default)]
    record_size: Vec<SizeBucket>,
    #[serde(default)]
    timing: Vec<TimingBucket>,
    split: Option<SplitPolicy>,
}

impl Script {
    /// Read the script at `path`.
    pub fn load(path: impl AsRef<Path>) -> Result<Script, ScriptError> {
        let path = path.as_ref();
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(ScriptError::MissingFile(path.to_path_buf()));
            }
            Err(error) => return Err(ScriptError::Io { path: path.to_path_buf(), error }),
        };
        Script::parse(&text)
    }

    /// Read a script from its TOML.
    pub fn parse(text: &str) -> Result<Script, ScriptError> {
        let raw: Raw = toml::from_str(text).map_err(|e| ScriptError::Parse(e.to_string()))?;
        let date = raw.date.filter(|d| !d.trim().is_empty()).ok_or(ScriptError::MissingDate)?;
        let cover_version =
            raw.cover_version.filter(|v| !v.trim().is_empty()).ok_or(ScriptError::MissingCoverVersion)?;
        let split = raw.split.ok_or(ScriptError::MissingSplit)?;
        check_date(&date)?;
        check_buckets(&raw.record_size, &raw.timing)?;
        check_split(split)?;
        Ok(Script { date, cover_version, seed: raw.seed, record_size: raw.record_size, timing: raw.timing, split })
    }

    /// A determined sequence of records for a payload of `len` bytes. The
    /// sizes add up to `len`. The last record may be shorter than its
    /// bucket. The same script and length give the same sequence.
    pub fn replay(&self, len: usize) -> Vec<Record> {
        let mut rng = SplitMix64::new(self.seed ^ 0x636f_7665_7221_0001);
        let sizes = self.record_size.iter().fold(0u64, |sum, b| sum.saturating_add(b.count));
        let delays = self.timing.iter().fold(0u64, |sum, b| sum.saturating_add(b.count));
        let mut records = Vec::new();
        let mut left = len;
        while left > 0 {
            let bucket = pick_size(&self.record_size, sizes, rng.next());
            let span = (bucket.max - bucket.min + 1) as u64;
            let want = bucket.min + (rng.next() % span) as usize;
            let size = want.min(left);
            let timing = pick_timing(&self.timing, delays, rng.next());
            let delay = delay_of(timing, rng.next());
            records.push(Record { size, delay });
            left -= size;
        }
        records
    }

    /// Split `payload` by the split policy. The parts add up to `payload`.
    /// The same payload and script give the same parts.
    pub fn split_payload<'a>(&self, payload: &'a [u8]) -> Vec<&'a [u8]> {
        let mut rng = SplitMix64::new(self.seed ^ 0x7370_6c69_7421_0002);
        let span = (self.split.max - self.split.min + 1) as u64;
        let mut parts = Vec::new();
        let mut at = 0;
        while at < payload.len() {
            let want = self.split.min + (rng.next() % span) as usize;
            let end = (at + want).min(payload.len());
            parts.push(&payload[at..end]);
            at = end;
        }
        parts
    }
}

/// A size bucket by its weight, from a value below the sum of the counts.
fn pick_size(buckets: &[SizeBucket], total: u64, value: u64) -> SizeBucket {
    let mut at = value % total.max(1);
    for bucket in buckets {
        if at < bucket.count {
            return *bucket;
        }
        at -= bucket.count;
    }
    *buckets.last().expect("a checked histogram")
}

/// A timing bucket by its weight, from a value below the sum of the counts.
fn pick_timing(buckets: &[TimingBucket], total: u64, value: u64) -> TimingBucket {
    let mut at = value % total.max(1);
    for bucket in buckets {
        if at < bucket.count {
            return *bucket;
        }
        at -= bucket.count;
    }
    *buckets.last().expect("a checked timing")
}

/// The delay of a timing bucket, with its jitter.
fn delay_of(bucket: TimingBucket, value: u64) -> Duration {
    let jitter = bucket.jitter_micros;
    let micros = if jitter == 0 {
        bucket.micros
    } else {
        let span = jitter.saturating_mul(2).saturating_add(1);
        bucket.micros.saturating_sub(jitter).saturating_add(value % span)
    };
    Duration::from_micros(micros)
}

/// The date of a measurement, `YYYY-MM-DD`.
fn check_date(date: &str) -> Result<(), ScriptError> {
    let bytes = date.as_bytes();
    let shape = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes.iter().enumerate().all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
    if !shape {
        return Err(ScriptError::Invalid(format!("the date {date:?} is not YYYY-MM-DD")));
    }
    let number = |from: usize, to: usize| date[from..to].parse::<u32>().ok();
    let (year, month, day) = (number(0, 4), number(5, 7), number(8, 10));
    let okay = matches!(year, Some(2000..=2100)) && matches!(month, Some(1..=12)) && matches!(day, Some(1..=31));
    if !okay {
        return Err(ScriptError::Invalid(format!("the date {date:?} is not a day")));
    }
    Ok(())
}

/// Each bucket holds at least one record, and each range is a range.
fn check_buckets(sizes: &[SizeBucket], timing: &[TimingBucket]) -> Result<(), ScriptError> {
    if sizes.is_empty() {
        return Err(ScriptError::Invalid("the record-size histogram is empty".into()));
    }
    let mut total = 0u64;
    for bucket in sizes {
        if bucket.min == 0 || bucket.min > bucket.max || bucket.max > MAX_RECORD || bucket.count == 0 {
            return Err(ScriptError::Invalid(format!(
                "the record-size bucket {}..={} count {} is not a measurement of a TLS record",
                bucket.min, bucket.max, bucket.count
            )));
        }
        total = total.checked_add(bucket.count).ok_or_else(|| ScriptError::Invalid("the counts overflow".into()))?;
    }
    if timing.is_empty() {
        return Err(ScriptError::Invalid("the timing distribution is empty".into()));
    }
    let mut total = 0u64;
    for bucket in timing {
        if bucket.count == 0 || bucket.jitter_micros > 1_000_000 {
            return Err(ScriptError::Invalid(format!(
                "the timing bucket {} us jitter {} us count {} is not a measurement",
                bucket.micros, bucket.jitter_micros, bucket.count
            )));
        }
        total = total.checked_add(bucket.count).ok_or_else(|| ScriptError::Invalid("the counts overflow".into()))?;
    }
    Ok(())
}

fn check_split(split: SplitPolicy) -> Result<(), ScriptError> {
    if split.min < MIN_RECORD || split.max > MAX_RECORD || split.min > split.max {
        return Err(ScriptError::Invalid(format!(
            "the split policy {}..={} is not within {MIN_RECORD}..={MAX_RECORD}",
            split.min, split.max
        )));
    }
    Ok(())
}

/// A small determined generator built on `pod_core::pick::siphash24`. Each
/// replay and each split has its own seed.
struct SplitMix64 {
    seed: u64,
    step: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> SplitMix64 {
        SplitMix64 { seed, step: 0 }
    }

    fn next(&mut self) -> u64 {
        self.step = self.step.wrapping_add(1);
        pod_core::pick::siphash24((self.seed, 0x9e37_79b9_7f4a_7c15), &self.step.to_le_bytes())
    }
}
