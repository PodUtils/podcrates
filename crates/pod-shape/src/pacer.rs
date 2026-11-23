//! The pacer: the delay of a record from the measured rate, and a keepalive
//! that is jittered by the profile's percent.

use std::time::Duration;

use crate::Profile;
use crate::rng::Rng;

/// The rate of a session and its keepalive jitter.
pub struct Pacer {
    rate_bps: u64,
    jitter_percent: u8,
    keepalive: Duration,
    rng: Rng,
}

impl Pacer {
    /// A pacer at `rate_bps`, with a keepalive of 30 seconds and a jitter of
    /// 20 percent.
    pub fn new(rate_bps: u64, jitter_percent: u8, seed: u64) -> Pacer {
        Pacer {
            rate_bps: rate_bps.max(1),
            jitter_percent: jitter_percent.min(100),
            keepalive: Duration::from_secs(30),
            rng: Rng::new(seed),
        }
    }

    /// A pacer of `profile` at `measured_rate_bps`. A measured rate above
    /// zero replaces the declared rate. With no pace in the profile, the
    /// declared values are a rate of `measured_rate_bps`, a jitter of 20
    /// percent and a keepalive of 30 seconds.
    pub fn from_profile(profile: &Profile, measured_rate_bps: u64) -> Pacer {
        let pace = profile.pace;
        let rate = match (measured_rate_bps, pace) {
            (0, Some(pace)) => pace.rate_bps,
            (0, None) => 1,
            (rate, _) => rate,
        };
        let (jitter, keepalive) = match pace {
            Some(pace) => (pace.jitter_percent, pace.keepalive_secs),
            None => (20, 30.0),
        };
        Pacer {
            rate_bps: rate.max(1),
            jitter_percent: jitter.min(100),
            keepalive: Duration::from_secs_f64(keepalive),
            rng: Rng::new(seed_of(&profile.name)),
        }
    }

    /// The delay of a record of `bytes` bytes at the rate.
    pub fn delay(&self, bytes: usize) -> Duration {
        let bits = (bytes as f64) * 8.0;
        Duration::from_secs_f64(bits / self.rate_bps as f64)
    }

    /// The interval before the next keepalive: the mean interval, moved by
    /// the jitter of the profile.
    pub fn keepalive(&mut self) -> Duration {
        let spread = f64::from(self.jitter_percent) / 100.0;
        let factor = 1.0 + spread * (2.0 * self.rng.unit() - 1.0);
        self.keepalive.mul_f64(factor)
    }

    /// The mean interval of the keepalives.
    pub fn keepalive_mean(&self) -> Duration {
        self.keepalive
    }
}

fn seed_of(name: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in name.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
