//! A padding profile as data: its bucket distribution, its rule, its
//! first-flight target, its pace, its entropy band and its source.
//!
//! The four profiles of issue #493 are files under `profiles/`. The crate
//! includes them in the binary, so a profile needs no directory at run time.

use serde::Deserialize;

use crate::Error;

/// The bucket sizes of the record layer.
pub const BUCKETS: [usize; 5] = [0, 128, 512, 1024, 2048];

/// The names of the profiles, in the order of the crate.
pub const NAMES: [&str; 4] = ["page-load", "video-call", "terminal", "file-transfer"];

const FILES: [(&str, &str); 4] = [
    ("page-load", include_str!("../profiles/page-load.toml")),
    ("video-call", include_str!("../profiles/video-call.toml")),
    ("terminal", include_str!("../profiles/terminal.toml")),
    ("file-transfer", include_str!("../profiles/file-transfer.toml")),
];

/// One body size of the distribution, with its relative weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bucket {
    pub size: usize,
    pub weight: u32,
}

/// When a profile pads a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// Each record.
    Always,
    /// The first `N` records of the session only.
    First(u64),
    /// Only a payload shorter than `N` bytes.
    Below(usize),
}

/// The pace of the records of a profile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pace {
    /// The declared rate, in bits per second.
    pub rate_bps: u64,
    /// The keepalive jitter, in percent.
    pub jitter_percent: u8,
    /// The mean interval between keepalives, in seconds.
    pub keepalive_secs: f64,
}

/// The entropy band of a payload.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    /// The lowest entropy of a window, in bits per byte.
    pub min_bits: f64,
    /// The highest entropy of a window, in bits per byte.
    pub max_bits: f64,
    /// The bytes of the printable run that a 64 KiB block carries once. The
    /// run is at least one byte and at most one 64 KiB block.
    pub printable_run: usize,
}

/// Where a profile comes from: its date and the version of the traffic that
/// it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The date that the profile was read, as `YYYY-MM-DD`.
    pub date: String,
    /// The version of the traffic that the profile covers.
    pub cover_version: String,
}

/// The shape that a session declares for one kind of traffic.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub name: String,
    pub protocol: String,
    /// The distribution over the body sizes. The weights are relative.
    pub buckets: Vec<Bucket>,
    pub rule: Rule,
    /// The opening write, in bytes, when the measured rate allows it.
    pub first_flight: Option<usize>,
    pub pace: Option<Pace>,
    pub entropy: Option<Band>,
    pub source: Source,
}

impl Profile {
    /// The profile called `name` from the files of the crate.
    pub fn load(name: &str) -> Result<Profile, Error> {
        let (_, text) = FILES
            .iter()
            .find(|(file, _)| *file == name)
            .ok_or_else(|| Error::UnknownProfile { name: name.to_string() })?;
        Profile::parse(name, text)
    }

    /// Each profile of the crate, in the order of [`NAMES`].
    pub fn all() -> Result<Vec<Profile>, Error> {
        NAMES.iter().map(|name| Profile::load(name)).collect()
    }

    /// The profile in `text`, checked. The name is the name of the file and
    /// it goes into each error.
    pub fn parse(name: &str, text: &str) -> Result<Profile, Error> {
        let raw: RawProfile =
            toml::from_str(text).map_err(|e| Error::Parse { name: name.to_string(), why: e.to_string() })?;
        raw.check(name)
    }

    /// The target of the opening write at `measured_rate_bps`: the bytes of
    /// two seconds, bounded by the target of the profile.
    pub fn first_flight(&self, measured_rate_bps: u64) -> usize {
        let target = self.first_flight.unwrap_or(0);
        let fit = (measured_rate_bps / 8).saturating_mul(2);
        target.min(usize::try_from(fit).unwrap_or(usize::MAX))
    }

    /// The body size that holds `payload`, given a chosen bucket: the bucket
    /// itself when it holds the payload, else the next declared bucket, else
    /// the next multiple of the largest bucket.
    pub fn body_for(&self, bucket: usize, payload: usize) -> usize {
        if bucket >= payload && (bucket > 0 || payload == 0) {
            return bucket;
        }
        if let Some(size) = self.buckets.iter().map(|b| b.size).filter(|&s| s >= payload).min() {
            return size;
        }
        payload.div_ceil(BUCKETS[BUCKETS.len() - 1]) * BUCKETS[BUCKETS.len() - 1]
    }
}

#[derive(Deserialize)]
struct RawProfile {
    name: String,
    protocol: String,
    buckets: Vec<RawBucket>,
    rule: String,
    #[serde(default)]
    first_flight: Option<usize>,
    #[serde(default)]
    pace: Option<RawPace>,
    #[serde(default)]
    entropy: Option<RawBand>,
    source: RawSource,
}

#[derive(Deserialize)]
struct RawBucket {
    size: usize,
    weight: u32,
}

#[derive(Deserialize)]
struct RawPace {
    rate: u64,
    #[serde(default = "default_jitter")]
    jitter: u8,
    #[serde(default = "default_keepalive")]
    keepalive: f64,
}

fn default_jitter() -> u8 {
    20
}

fn default_keepalive() -> f64 {
    30.0
}

#[derive(Deserialize)]
struct RawBand {
    min: f64,
    max: f64,
    printable_run: usize,
}

#[derive(Deserialize)]
struct RawSource {
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    cover_version: Option<String>,
}

impl RawProfile {
    fn check(self, name: &str) -> Result<Profile, Error> {
        let source = match (self.source.date, self.source.cover_version) {
            (None, _) => return Err(Error::MissingDate { name: name.to_string() }),
            (Some(_), None) => return Err(Error::MissingCoverVersion { name: name.to_string() }),
            (Some(date), Some(cover_version)) => Source { date, cover_version },
        };
        if !valid_date(&source.date) {
            return Err(Error::BadDate { name: name.to_string(), date: source.date.clone() });
        }
        if self.buckets.is_empty() {
            return Err(Error::NoBuckets { name: name.to_string() });
        }
        let mut buckets = Vec::with_capacity(self.buckets.len());
        for bucket in self.buckets {
            if !BUCKETS.contains(&bucket.size) {
                return Err(Error::BadBucket { name: name.to_string(), size: bucket.size });
            }
            if bucket.weight == 0 {
                return Err(Error::BadWeight { name: name.to_string(), size: bucket.size });
            }
            if buckets.iter().any(|b: &Bucket| b.size == bucket.size) {
                return Err(Error::BadBucket { name: name.to_string(), size: bucket.size });
            }
            buckets.push(Bucket { size: bucket.size, weight: bucket.weight });
        }
        let rule = parse_rule(name, &self.rule)?;
        if self.first_flight == Some(0) {
            return Err(Error::BadTarget { name: name.to_string() });
        }
        let pace = match self.pace {
            None => None,
            Some(pace) => {
                let keepalive = pace.keepalive;
                if pace.rate == 0 || pace.jitter > 100 || !keepalive.is_finite() || keepalive <= 0.0 {
                    return Err(Error::BadPace { name: name.to_string() });
                }
                Some(Pace { rate_bps: pace.rate, jitter_percent: pace.jitter, keepalive_secs: keepalive })
            }
        };
        let entropy = match self.entropy {
            None => None,
            Some(band) => {
                let finite = band.min.is_finite() && band.max.is_finite();
                if !finite
                    || band.min <= 0.0
                    || band.max < band.min
                    || band.max > 8.0
                    || band.printable_run == 0
                    || band.printable_run > 64 * 1024
                {
                    return Err(Error::BadBand { name: name.to_string() });
                }
                Some(Band { min_bits: band.min, max_bits: band.max, printable_run: band.printable_run })
            }
        };
        Ok(Profile {
            name: self.name,
            protocol: self.protocol,
            buckets,
            rule,
            first_flight: self.first_flight,
            pace,
            entropy,
            source,
        })
    }
}

fn parse_rule(name: &str, text: &str) -> Result<Rule, Error> {
    let bad = || Error::BadRule { name: name.to_string(), rule: text.to_string() };
    let (word, value) = match text.split_once(':') {
        Some((word, value)) => (word, Some(value)),
        None => (text, None),
    };
    match (word, value) {
        ("always", None) => Ok(Rule::Always),
        ("first", Some(n)) => n.parse::<u64>().map(Rule::First).map_err(|_| bad()),
        ("below", Some(n)) => n.parse::<usize>().map(Rule::Below).map_err(|_| bad()),
        _ => Err(bad()),
    }
}

fn valid_date(date: &str) -> bool {
    let bytes = date.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    bytes.iter().enumerate().all(|(at, b)| at == 4 || at == 7 || b.is_ascii_digit())
}
