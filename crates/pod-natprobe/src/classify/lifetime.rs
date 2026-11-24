//! Mapping lifetime (RFC 5780 section 4.6) rung schedules and bracket classifier.

use std::time::Duration;

use crate::report::{LifetimeBound, NatLifetime, Unknown, UnknownReason};

/// Default coarse rung idle durations in seconds (`5..=240 s`).
pub const COARSE_RUNGS_SECS: [u64; 7] = [5, 10, 20, 40, 80, 160, 240];

/// Result of probing one idle rung socket via the control socket's `RESPONSE-PORT` request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RungOutcome {
    /// Reply arrived on the idle rung socket `X_k`.
    Alive,
    /// Reply arrived on the control socket `Y` (the old binding expired and was recreated).
    ExpiredRecreated,
    /// ICMP unreachable reported on the rung socket.
    ExpiredUnreachable,
    /// No reply arrived after retransmissions.
    ExpiredSilent,
    /// Pooling confound: `Y`'s mapped IP disagreed with `X_k`'s mapped IP.
    InvalidPooling,
}

impl RungOutcome {
    pub fn is_expired(self) -> bool {
        matches!(self, Self::ExpiredRecreated | Self::ExpiredUnreachable | Self::ExpiredSilent)
    }
}

/// Compute three geometrically spaced fine rungs inside `(low, high)`.
pub fn fine_rungs(low: Duration, high: Duration) -> [Duration; 3] {
    let lo = low.as_secs_f64().max(0.1);
    let hi = high.as_secs_f64().max(lo);
    let ratio = hi / lo;
    [
        Duration::from_secs_f64(lo * ratio.powf(0.25)),
        Duration::from_secs_f64(lo * ratio.powf(0.50)),
        Duration::from_secs_f64(lo * ratio.powf(0.75)),
    ]
}

/// Classify the observed rung outcomes `(idle_duration, outcome)` after a passing positive
/// control (`control_passed == true`).
pub fn classify_lifetime(
    control_outcome: RungOutcome,
    rungs: &[(Duration, RungOutcome)],
    repetitions: u8,
) -> Result<NatLifetime, Unknown> {
    match control_outcome {
        RungOutcome::Alive => {}
        RungOutcome::ExpiredRecreated => {
            return Err(Unknown {
                reason: UnknownReason::ServerIgnoresResponsePort,
                constraint: crate::report::Constraint::None,
            });
        }
        RungOutcome::InvalidPooling => {
            return Err(Unknown {
                reason: UnknownReason::PoolingConfounds,
                constraint: crate::report::Constraint::None,
            });
        }
        RungOutcome::ExpiredSilent | RungOutcome::ExpiredUnreachable => {
            return Err(Unknown {
                reason: UnknownReason::ResponsePortUnsupported,
                constraint: crate::report::Constraint::None,
            });
        }
    }

    if rungs.iter().any(|&(_, o)| o == RungOutcome::InvalidPooling) {
        return Err(Unknown { reason: UnknownReason::PoolingConfounds, constraint: crate::report::Constraint::None });
    }

    let mut ordered = rungs.to_vec();
    ordered.sort_by_key(|(d, _)| *d);

    let mut low: Option<Duration> = None;
    let mut high: Option<Duration> = None;
    for (d, outcome) in ordered {
        match outcome {
            RungOutcome::Alive => {
                if let Some(h) = high
                    && d >= h
                {
                    return Err(Unknown {
                        reason: UnknownReason::NonMonotone,
                        constraint: crate::report::Constraint::None,
                    });
                }
                low = Some(low.map_or(d, |cur| cur.max(d)));
            }
            RungOutcome::ExpiredRecreated | RungOutcome::ExpiredUnreachable | RungOutcome::ExpiredSilent => {
                high = Some(high.map_or(d, |cur| cur.min(d)));
            }
            // The check above returns first.
            RungOutcome::InvalidPooling => {}
        }
    }

    let (bound, estimate, meets_req5) = match (low, high) {
        (Some(lo), Some(hi)) if lo < hi => {
            let est = Duration::from_secs_f64((lo.as_secs_f64() * hi.as_secs_f64()).sqrt());
            let req5 = if hi < Duration::from_secs(120) {
                Some(false)
            } else if lo >= Duration::from_secs(120) {
                Some(true)
            } else {
                None
            };
            (LifetimeBound::Exact { low: lo, high: hi }, est, req5)
        }
        (Some(lo), None) => {
            let req5 = (lo >= Duration::from_secs(120)).then_some(true);
            (LifetimeBound::AtLeast(lo), lo, req5)
        }
        (None, Some(hi)) => {
            let req5 = (hi < Duration::from_secs(120)).then_some(false);
            (LifetimeBound::Below(hi), hi / 2, req5)
        }
        _ => {
            return Err(Unknown { reason: UnknownReason::TooFewSamples, constraint: crate::report::Constraint::None });
        }
    };

    let keepalive_hint = (estimate / 3).clamp(Duration::from_secs(5), Duration::from_secs(25));
    Ok(NatLifetime { bound, estimate, keepalive_hint, meets_req5, repetitions: repetitions.max(1) })
}
