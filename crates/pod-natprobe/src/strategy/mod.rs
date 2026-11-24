//! Peer-A x Peer-B NAT traversal strategy predictor, two-sided birthday probability maths
//! and ring-wrapped port spray planning.

use crate::classify::{RING_SIZE, ring_wrap};
use crate::report::{Certainty, FilteringBehavior, HairpinBehavior, MappingBehavior, PortAllocation};

/// Reduced per-peer NAT profile fed into [`predict`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerClass {
    pub udp_ok: bool,
    pub mapping: Option<MappingBehavior>,
    pub filtering: Option<FilteringBehavior>,
    pub predictable: bool,
}

/// Which peer initiates or sprays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum Side {
    A,
    B,
    Both,
}

/// Why a direct UDP path requires a relay fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum RelayReason {
    UdpBlocked,
    NoReturnTraffic,
    BothHard,
    NoHairpin,
}

/// Recommended traversal strategy between two peers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum TraversalStrategy {
    /// One peer sends directly to the other's advertised endpoints.
    Direct { dialer: Side },
    /// Both peers exchange endpoints and send simultaneously.
    SimultaneousOpen,
    /// Classes unknown: run simultaneous open with a relay leg started in parallel.
    SimultaneousOpenWithRelay,
    /// Spray focused ports predicted around the hard peer's mapped port.
    PortPrediction { side: Side },
    /// Two-sided birthday spray against a random-port dependent NAT.
    BirthdaySpray { hard: Side },
    /// Direct traversal is infeasible; use a relay.
    RelayRequired(RelayReason),
}

fn dep(p: &PeerClass) -> bool {
    p.mapping != Some(MappingBehavior::EndpointIndependent)
}

/// Predict traversal feasibility for two peers behind distinct NATs.
/// Verified against all 81 `(mapping, filtering)^2` cells.
pub fn predict(a: &PeerClass, b: &PeerClass) -> TraversalStrategy {
    use FilteringBehavior::{AddressAndPortDependent, EndpointIndependent, NoReturnTraffic};
    if !a.udp_ok || !b.udp_ok {
        return TraversalStrategy::RelayRequired(RelayReason::UdpBlocked);
    }
    if a.filtering == Some(NoReturnTraffic) || b.filtering == Some(NoReturnTraffic) {
        return TraversalStrategy::RelayRequired(RelayReason::NoReturnTraffic);
    }
    if a.mapping.is_none() || b.mapping.is_none() || a.filtering.is_none() || b.filtering.is_none() {
        return TraversalStrategy::SimultaneousOpenWithRelay;
    }
    let dial_ok = |d: &PeerClass, t: &PeerClass| {
        t.filtering == Some(EndpointIndependent) && (d.filtering != Some(AddressAndPortDependent) || !dep(t))
    };
    if dial_ok(a, b) {
        return TraversalStrategy::Direct { dialer: Side::A };
    }
    if dial_ok(b, a) {
        return TraversalStrategy::Direct { dialer: Side::B };
    }
    // Each filtering is known here (checked above); the most restrictive class stands in.
    let f = |p: &PeerClass| p.filtering.unwrap_or(AddressAndPortDependent);
    let (da, db) = (dep(a), dep(b));
    let two_sided = (!da && !db)
        || (da != db && f(if da { b } else { a }) != AddressAndPortDependent)
        || (da
            && db
            && ((f(a) == EndpointIndependent && f(b) != AddressAndPortDependent)
                || (f(b) == EndpointIndependent && f(a) != AddressAndPortDependent)));
    if two_sided {
        return TraversalStrategy::SimultaneousOpen;
    }
    if da && db {
        return if a.predictable && b.predictable {
            TraversalStrategy::PortPrediction { side: Side::Both }
        } else {
            TraversalStrategy::RelayRequired(RelayReason::BothHard)
        };
    }
    let (hard, side) = if da { (a, Side::A) } else { (b, Side::B) };
    if hard.predictable {
        TraversalStrategy::PortPrediction { side }
    } else {
        TraversalStrategy::BirthdaySpray { hard: side }
    }
}

/// Predict traversal strategy when both peers share the same public IP address.
/// Returns `(strategy, certainty)`.
pub fn predict_same_nat(lan_reachable: bool, hairpin: Option<HairpinBehavior>) -> (TraversalStrategy, Certainty) {
    if lan_reachable {
        return (TraversalStrategy::Direct { dialer: Side::Both }, Certainty::Exact);
    }
    match hairpin {
        Some(HairpinBehavior::ExternalSource) => (TraversalStrategy::Direct { dialer: Side::Both }, Certainty::Exact),
        Some(HairpinBehavior::InternalSource | HairpinBehavior::ExternalIpOnly) => {
            // Receiver must accept packets from an unexpected source endpoint.
            (TraversalStrategy::Direct { dialer: Side::Both }, Certainty::Suspected)
        }
        Some(HairpinBehavior::None) => (TraversalStrategy::RelayRequired(RelayReason::NoHairpin), Certainty::Exact),
        _ => (TraversalStrategy::SimultaneousOpenWithRelay, Certainty::Inferred),
    }
}

/// Exact two-sided birthday hit probability `1 - C(n - a, b) / C(n, b)`:
/// probability that at least one of `b` distinct sprayed destination ports matches one of `a`
/// sockets opened on the hard NAT side inside a port pool of size `n` (default `64512`).
pub fn birthday_exact(a: u32, b: u32, n: u32) -> f64 {
    if a == 0 || b == 0 || n == 0 {
        return 0.0;
    }
    let mut miss = 1.0f64;
    for i in 0..b {
        let num = f64::from(n) - f64::from(a) - f64::from(i);
        if num <= 0.0 {
            return 1.0;
        }
        miss *= num / (f64::from(n) - f64::from(i));
    }
    1.0 - miss
}

/// Plan a bounded list of candidate destination ports (at most 2001) to spray around `anchor`
/// after `elapsed_s` seconds and `own_open` newly opened sockets on the hard side.
pub fn plan_spray(
    alloc: PortAllocation,
    anchor: u16,
    elapsed_s: f64,
    own_open: u32,
    random_count: usize,
    seed: u64,
) -> Vec<u16> {
    let cap = 2001usize;
    match alloc {
        PortAllocation::Preserving => vec![anchor],
        PortAllocation::Sequential { delta, .. } => {
            let d = i64::from(delta).clamp(-256, 256);
            let mut out = Vec::with_capacity(61);
            for k in -10i64..=50 {
                let p = ring_wrap(i64::from(anchor) + k * d);
                if !out.contains(&p) {
                    out.push(p);
                }
            }
            out
        }
        PortAllocation::GlobalCounter { drift_per_s, .. } => {
            let drift = f64::from(drift_per_s);
            let predicted = i64::from(anchor) + (drift * elapsed_s).round() as i64;
            let fwd = (0.5 * drift.abs() * elapsed_s + 3.0 * f64::from(own_open)).round().clamp(20.0, 1800.0) as i64;
            let back = (0.15 * drift.abs() * elapsed_s).round().clamp(10.0, 200.0) as i64;
            let mut out = Vec::with_capacity(((fwd + back + 1) as usize).min(cap));
            for k in -back..=fwd {
                if out.len() >= cap {
                    break;
                }
                let p = ring_wrap(predicted.saturating_add(k));
                if !out.contains(&p) {
                    out.push(p);
                }
            }
            out
        }
        PortAllocation::Random { pool } => {
            let (lo, span) = match pool {
                Some(b) if b.hi > b.lo => (u32::from(b.lo), u32::from(b.hi - b.lo) + 1),
                _ => (1024, RING_SIZE),
            };
            let want = random_count.min(cap).min(span as usize);
            let mut state = seed ^ 0x9e37_79b9_7f4a_7c15;
            let mut out = Vec::with_capacity(want);
            while out.len() < want {
                state = splitmix64(state);
                let p = (lo + (state as u32 % span)) as u16;
                if !out.contains(&p) {
                    out.push(p);
                }
            }
            out
        }
    }
}

pub(crate) fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}
