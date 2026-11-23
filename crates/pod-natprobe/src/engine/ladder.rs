//! Adaptive retransmission ladder, Karn-safe SRTT tracker and token-bucket pacer.

use std::collections::HashMap;
use std::net::IpAddr;

use crate::report::Tick;
use crate::settings::{BucketConfig, LadderProfile};

/// Precomputed retransmission schedule in microseconds from send #0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LadderSchedule {
    pub offsets_us: Vec<u64>,
    pub end_us: u64,
    pub reuse_tid: bool,
}

impl LadderSchedule {
    /// Compute the schedule for `profile`, optional server SRTT (`srtt_us`), and deterministic
    /// jitter seed (`0` disables jitter).
    pub fn build(profile: LadderProfile, srtt_us: Option<u64>, jitter_seed: u64, probe_id: u32) -> Self {
        match profile {
            LadderProfile::Rfc8489 => {
                // RFC 8489 section 6.2.1: RTO = 500 ms, Rc = 7, Rm = 16 -> 39.5 s total.
                let offsets_ms = [0u64, 500, 1_500, 3_500, 7_500, 15_500, 31_500];
                let offsets_us = offsets_ms.iter().map(|&m| m * 1_000).collect();
                Self { offsets_us, end_us: 39_500_000, reuse_tid: true }
            }
            LadderProfile::Interactive => {
                let r_ms = match srtt_us {
                    Some(s) => ((2 * s) / 1_000).clamp(100, 500),
                    None => 250,
                };
                let end_ms = (20 * r_ms).min(5_000);
                let mut offsets_us = Vec::with_capacity(5);
                for k in 0u32..5 {
                    let base_ms = ((1u64 << k) - 1) * r_ms;
                    if base_ms > end_ms.saturating_sub(r_ms) {
                        break;
                    }
                    let mut off = base_ms * 1_000;
                    if k > 0 && jitter_seed != 0 {
                        off = apply_jitter(off, jitter_seed, probe_id, k);
                    }
                    offsets_us.push(off);
                }
                Self { offsets_us, end_us: end_ms * 1_000, reuse_tid: false }
            }
        }
    }
}

fn apply_jitter(base_us: u64, seed: u64, probe_id: u32, k: u32) -> u64 {
    let mut x = seed ^ ((u64::from(probe_id)) << 32) ^ u64::from(k);
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    // Uniform integer factor in [90, 110] percent (+/-10%).
    let pct = 90 + (x % 21);
    (base_us * pct) / 100
}

/// Per-server-IP smoothed RTT estimator (RFC 6298 alpha = 1/8).
#[derive(Clone, Debug, Default)]
pub struct SrttMap {
    by_ip: HashMap<IpAddr, u64>,
}

impl SrttMap {
    /// Record an unambiguous RTT sample in microseconds.
    pub fn record(&mut self, ip: IpAddr, sample_us: u64) {
        let sample = sample_us.max(1_000);
        self.by_ip.entry(ip).and_modify(|cur| *cur = (*cur * 7 + sample) / 8).or_insert(sample);
    }

    /// Current smoothed RTT for `ip`, if observed.
    pub fn get(&self, ip: IpAddr) -> Option<u64> {
        self.by_ip.get(&ip).copied()
    }
}

/// Token bucket limiting new STUN transactions per second plus per-server inflight cap.
#[derive(Clone, Debug)]
pub struct TokenBucket {
    burst: u64,
    refill_per_sec: u64,
    max_inflight: usize,
    tokens_scaled: u64,
    last_refill: Tick,
    inflight_per_ip: HashMap<IpAddr, usize>,
}

const SCALE: u64 = 1_000_000;

impl TokenBucket {
    /// Initialize a full bucket at tick 0.
    pub fn new(cfg: BucketConfig) -> Self {
        let burst = u64::from(cfg.burst.max(1));
        Self {
            burst,
            refill_per_sec: u64::from(cfg.refill_per_sec.max(1)),
            max_inflight: cfg.max_inflight_per_server.max(1),
            tokens_scaled: burst * SCALE,
            last_refill: 0,
            inflight_per_ip: HashMap::new(),
        }
    }

    fn refill(&mut self, now: Tick) {
        if now <= self.last_refill {
            return;
        }
        let dt_us = now - self.last_refill;
        self.last_refill = now;
        let added = dt_us.saturating_mul(self.refill_per_sec);
        let cap = self.burst.saturating_mul(SCALE);
        self.tokens_scaled = self.tokens_scaled.saturating_add(added).min(cap);
    }

    /// Try to acquire one new-transaction token for `server_ip` at `now`.
    pub fn try_acquire(&mut self, now: Tick, server_ip: IpAddr) -> bool {
        self.refill(now);
        let current = self.inflight_per_ip.get(&server_ip).copied().unwrap_or(0);
        if current >= self.max_inflight || self.tokens_scaled < SCALE {
            return false;
        }
        self.tokens_scaled -= SCALE;
        *self.inflight_per_ip.entry(server_ip).or_insert(0) += 1;
        true
    }

    /// True when `server_ip` has reached the per-server inflight transaction cap.
    pub fn is_inflight_full(&self, server_ip: IpAddr) -> bool {
        self.inflight_per_ip.get(&server_ip).copied().unwrap_or(0) >= self.max_inflight
    }

    /// Next tick at which a token will be available (or `now` if one is ready now).
    pub fn next_token_tick(&mut self, now: Tick) -> Tick {
        self.refill(now);
        if self.tokens_scaled >= SCALE {
            return now;
        }
        let missing = SCALE - self.tokens_scaled;
        let wait_us = missing.div_ceil(self.refill_per_sec);
        now.saturating_add(wait_us)
    }

    /// Release one completed transaction slot for `server_ip`.
    pub fn release(&mut self, server_ip: IpAddr) {
        if let Some(cnt) = self.inflight_per_ip.get_mut(&server_ip) {
            *cnt = cnt.saturating_sub(1);
        }
    }
}
