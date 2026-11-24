//! External port allocation and delta/drift classifier using 64512-port ring arithmetic.

use std::net::SocketAddr;

use super::verdict::{RING_LO, RING_SIZE, ring_delta};
use crate::report::{Certainty, Flag, PoolBand, PortAllocation, Tick, Unknown, UnknownReason};

/// One fresh-socket sample to a single pinned destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocSample {
    pub local_port: u16,
    pub mapped: SocketAddr,
    pub at: Tick,
}

/// Output of [`classify_allocation`].
#[derive(Clone, Debug, PartialEq)]
pub enum AllocOutcome {
    Known { value: PortAllocation, certainty: Certainty, confidence: f32, flags: Vec<Flag> },
    Unknown(Unknown),
}

/// Classify port allocation across fresh sockets sent to one pinned destination.
/// Requires at least 3 samples (`n < 3` yields `TooFewSamples`).
/// When `allow_random` is false (Fast profile), returns `NotPreserving` instead of `Random`.
pub fn classify_allocation(samples: &[AllocSample], allow_random: bool) -> AllocOutcome {
    let n = samples.len();
    if n < 3 {
        return AllocOutcome::Unknown(Unknown {
            reason: UnknownReason::TooFewSamples,
            constraint: crate::report::Constraint::None,
        });
    }
    let mut flags = Vec::new();
    let low_ports = samples.iter().any(|s| u32::from(s.mapped.port()) < RING_LO);
    if low_ports {
        flags.push(Flag::LowPorts);
    }

    // Strict majority kept local port (2 of 4 is NOT preserving).
    let kept = samples.iter().filter(|s| s.local_port == s.mapped.port()).count();
    if 2 * kept > n {
        if kept < n {
            flags.push(Flag::PreserveCollisions);
        }
        let base = if n >= 5 { 0.95 } else { 0.80 };
        let conf = if kept < n { base * 0.5 } else { base };
        return AllocOutcome::Known {
            value: PortAllocation::Preserving,
            certainty: Certainty::Exact,
            confidence: conf,
            flags,
        };
    }

    let deltas: Vec<i32> = samples.windows(2).map(|w| ring_delta(w[0].mapped.port(), w[1].mapped.port())).collect();
    let delta = lower_median(&deltas);
    let jitter = deltas.iter().map(|&d| d.abs_diff(delta)).max().unwrap_or(0).min(u32::from(u16::MAX)) as u16;

    if delta != 0 && delta.abs() <= 256 && jitter <= 64 {
        let tight = delta.abs() <= 10 && jitter <= 3;
        if allow_random || tight {
            let conf = if tight {
                if n >= 6 {
                    0.90
                } else if n >= 5 {
                    0.85
                } else {
                    0.60
                }
            } else {
                0.75
            };
            return AllocOutcome::Known {
                value: PortAllocation::Sequential { delta: delta as i16, jitter, tight },
                certainty: Certainty::Exact,
                confidence: conf,
                flags,
            };
        }
    }

    // Global counter test (n >= 6, elapsed > 0, >=80% one sign, max|d| <= 2000, sum|d| <= RING/4,
    // drift > 10 ports/s). The max|d| and sum|d| bounds eliminate uniform-random false positives.
    let elapsed_us = match (samples.first(), samples.last()) {
        (Some(first), Some(last)) => last.at.saturating_sub(first.at),
        _ => 0,
    };
    if n >= 6 && elapsed_us > 0 {
        let pos = deltas.iter().filter(|&&d| d > 0).count();
        let neg = deltas.iter().filter(|&&d| d < 0).count();
        let consistent = pos.max(neg) >= deltas.len() - (deltas.len() / 5);
        let max_abs = deltas.iter().map(|d| d.abs()).max().unwrap_or(0);
        let sum_abs: i64 = deltas.iter().map(|&d| i64::from(d.abs())).sum();
        let sum_signed: i64 = deltas.iter().map(|&d| i64::from(d)).sum();
        let elapsed_s = (elapsed_us as f64) / 1_000_000.0;
        let drift = (sum_signed as f64) / elapsed_s;
        if consistent && max_abs <= 2000 && sum_abs <= i64::from(RING_SIZE / 4) && drift.abs() > 10.0 {
            let drift_i16 = drift.round().clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16;
            return AllocOutcome::Known {
                value: PortAllocation::GlobalCounter { drift_per_s: drift_i16, jitter },
                certainty: Certainty::Suspected,
                confidence: 0.45,
                flags,
            };
        }
    }

    if !allow_random || n < 5 {
        return AllocOutcome::Unknown(Unknown {
            reason: if !allow_random { UnknownReason::NotPreserving } else { UnknownReason::TooFewSamples },
            constraint: crate::report::Constraint::None,
        });
    }

    let pool = estimate_pool_band(samples);
    if pool.is_some() {
        flags.push(Flag::NarrowPool);
    }
    let conf = if n >= 6 { 0.80 } else { 0.70 };
    AllocOutcome::Known { value: PortAllocation::Random { pool }, certainty: Certainty::Exact, confidence: conf, flags }
}

/// Deep idle-gap test separating a time-driven shared counter from a per-mapping sequencer.
/// `advance` is `ring_delta(last_mapped_port, post_gap_mapped_port)`.
pub fn is_time_correlated_counter(delta: i16, jitter: u16, advance: i32) -> bool {
    let step = i32::from(delta).abs().max(1);
    advance.abs() >= 3 * step + 3 * i32::from(jitter)
}

/// Estimate a restricted allocation pool band when the observed span is under one quarter of
/// the 64512-port ring.
pub fn estimate_pool_band(samples: &[AllocSample]) -> Option<PoolBand> {
    let n = samples.len() as u32;
    if n < 5 {
        return None;
    }
    let lo = samples.iter().map(|s| s.mapped.port()).min()?;
    let hi = samples.iter().map(|s| s.mapped.port()).max()?;
    let span = u32::from(hi.saturating_sub(lo));
    if span < RING_SIZE / 4 {
        let width_est = (span.saturating_mul(n + 1) / (n - 1)).max(1);
        Some(PoolBand { lo, hi, width_est })
    } else {
        None
    }
}

fn lower_median(vals: &[i32]) -> i32 {
    let mut sorted = vals.to_vec();
    sorted.sort_unstable();
    let idx = (sorted.len() - 1) / 2;
    sorted[idx]
}
