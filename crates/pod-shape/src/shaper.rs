//! The shaper: a bucket for each record, from a seed, with the rule of the
//! profile, and the count of the padded bytes and the body bytes.

use crate::report::PaddedFraction;
use crate::rng::Rng;
use crate::{Error, Profile, Rule};

/// The decision for one record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shaped {
    /// The bytes of the body: the payload and its padding.
    pub body: usize,
    /// True when the body is longer than the payload.
    pub padded: bool,
}

/// The bucket chooser and the counters of one session.
pub struct Shaper {
    profile: Profile,
    rng: Rng,
    records: u64,
    padded: u64,
    body: u64,
    last: Option<usize>,
    run: u32,
}

impl Shaper {
    /// A shaper of `profile` with `seed`.
    pub fn new(profile: Profile, seed: u64) -> Shaper {
        Shaper { profile, rng: Rng::new(seed), records: 0, padded: 0, body: 0, last: None, run: 0 }
    }

    /// A shaper of the profile called `name`, with `seed`.
    pub fn from_name(name: &str, seed: u64) -> Result<Shaper, Error> {
        Ok(Shaper::new(Profile::load(name)?, seed))
    }

    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// One bucket of the distribution. The weights are relative.
    pub fn select_bucket(&mut self) -> usize {
        let mut picked = self.weighted();
        if self.profile.buckets.len() > 1 {
            let mut tries = 0;
            while Some(picked) == self.last && self.run >= 8 && tries < 4 {
                picked = self.weighted();
                tries += 1;
            }
            if Some(picked) == self.last {
                self.run = self.run.saturating_add(1);
            } else {
                self.last = Some(picked);
                self.run = 1;
            }
        }
        picked
    }

    fn weighted(&mut self) -> usize {
        let total: u64 = self.profile.buckets.iter().map(|b| u64::from(b.weight)).sum();
        let mut point = self.rng.below(total.max(1));
        for bucket in &self.profile.buckets {
            let weight = u64::from(bucket.weight);
            if point < weight {
                return bucket.size;
            }
            point -= weight;
        }
        self.profile.buckets.last().map_or(0, |b| b.size)
    }

    /// The body of a record whose payload is `payload_len` bytes. The rule of
    /// the profile decides whether the record pads at all.
    pub fn shape(&mut self, payload_len: usize) -> Shaped {
        let pad = match self.profile.rule {
            Rule::Always => true,
            Rule::First(count) => self.records < count,
            Rule::Below(limit) => payload_len < limit,
        };
        let body = if pad {
            let bucket = self.select_bucket();
            self.profile.body_for(bucket, payload_len)
        } else {
            payload_len
        };
        self.records = self.records.saturating_add(1);
        self.padded = self.padded.saturating_add((body - payload_len) as u64);
        self.body = self.body.saturating_add(body as u64);
        Shaped { body, padded: body > payload_len }
    }

    /// The records that the shaper shaped.
    pub fn records(&self) -> u64 {
        self.records
    }

    /// The padding bytes of the bodies.
    pub fn padded_out(&self) -> u64 {
        self.padded
    }

    /// The bytes of the bodies.
    pub fn body_out(&self) -> u64 {
        self.body
    }

    /// The padded bytes over the body bytes.
    pub fn padded_fraction(&self) -> PaddedFraction {
        PaddedFraction::of(self.padded, self.body)
    }
}
