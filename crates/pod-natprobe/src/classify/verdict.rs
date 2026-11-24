//! Pure RFC 5780 mapping and filtering verdict functions plus 64512-port ring arithmetic.

use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};

use crate::report::{
    Certainty, Constraint, FilteringBehavior, Flag, IpPooling, MappingBehavior, Unknown, UnknownReason,
};

/// Minimum transmissions before a timeout may feed a restrictive filtering verdict.
pub const MIN_SENDS_FOR_TIMEOUT: u8 = 4;

/// Lower bound of the dynamic/registered UDP port ring (`1024..=65535`).
pub const RING_LO: u32 = 1024;
/// Number of ports in `1024..=65535`.
pub const RING_SIZE: u32 = 64_512;

/// Signed distance `a -> b` on the `1024..=65535` ring, in `-32256..=32256`.
pub fn ring_delta(a: u16, b: u16) -> i32 {
    if u32::from(a) < RING_LO || u32::from(b) < RING_LO {
        return i32::from(b.wrapping_sub(a) as i16);
    }
    let (a, b) = (u32::from(a), u32::from(b));
    let d = (b + RING_SIZE - a) % RING_SIZE;
    if d <= RING_SIZE / 2 { d as i32 } else { d as i32 - RING_SIZE as i32 }
}

/// Wrap an arbitrary signed port offset back onto the `1024..=65535` ring.
pub fn ring_wrap(port: i64) -> u16 {
    let offset = (port - i64::from(RING_LO)).rem_euclid(i64::from(RING_SIZE));
    (offset + i64::from(RING_LO)) as u16
}

/// Raw outcome of one RFC 5780 filtering probe step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe<T> {
    Ok(T),
    Timeout {
        sends: u8,
        elapsed_ms: u32,
    },
    /// Response arrived from a source other than the expected one.
    WrongSource,
    /// Server answered 420 or 400 to a request carrying CHANGE-REQUEST.
    RefusedChangeRequest,
}

/// Outcome of a positive filtering control probe.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Control {
    Ok,
    Failed,
    /// Premise failed (dependent mapping put the pinhole on a different external mapping).
    Invalid,
    #[default]
    NotRun,
}

/// Pure verdict output before wrapping into [`crate::report::Obs`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict<T> {
    Known { value: T, certainty: Certainty, flags: Vec<Flag> },
    Unknown(Unknown),
}

fn unk<T>(reason: UnknownReason, constraint: Constraint) -> Verdict<T> {
    Verdict::Unknown(Unknown { reason, constraint })
}

fn known<T>(value: T, certainty: Certainty, flags: Vec<Flag>) -> Verdict<T> {
    Verdict::Known { value, certainty, flags }
}

fn enough(p: &Probe<()>) -> bool {
    match p {
        Probe::Timeout { sends, .. } => *sends >= MIN_SENDS_FOR_TIMEOUT,
        _ => true,
    }
}

/// RFC 5780 section 4.4 filtering verdict with mandatory positive controls.
/// `t2` = `CHANGE-REQUEST 0x06`, `t3` = `CHANGE-REQUEST 0x02`.
pub fn filtering_verdict(t2: Probe<()>, t3: Probe<()>, c06: Control, c02: Control) -> Verdict<FilteringBehavior> {
    use FilteringBehavior::{AddressAndPortDependent, AddressDependent, EndpointIndependent};
    if matches!(t2, Probe::Ok(())) {
        return known(EndpointIndependent, Certainty::Exact, Vec::new());
    }
    if matches!(t2, Probe::RefusedChangeRequest) || matches!(t3, Probe::RefusedChangeRequest) {
        return unk(UnknownReason::ServerLacksChangeRequest, Constraint::None);
    }
    if matches!(t2, Probe::WrongSource) {
        return if matches!(t3, Probe::Ok(())) {
            unk(UnknownReason::ChangeIpUnverified, Constraint::FilteringNotAddressAndPortDependent)
        } else {
            unk(UnknownReason::ServerIgnoresChangeRequest, Constraint::None)
        };
    }
    if !enough(&t2) {
        let (sends, elapsed_ms) = timeout_info(&t2);
        return unk(UnknownReason::Timeout { sends, elapsed_ms }, Constraint::None);
    }
    match t3 {
        Probe::Ok(()) => match c06 {
            Control::Ok => known(AddressDependent, Certainty::Exact, Vec::new()),
            _ => unk(UnknownReason::ChangeIpUnverified, Constraint::FilteringNotAddressAndPortDependent),
        },
        Probe::Timeout { sends, elapsed_ms } if sends < MIN_SENDS_FOR_TIMEOUT => {
            unk(UnknownReason::Timeout { sends, elapsed_ms }, Constraint::None)
        }
        // The control of 0x02 alone proves that the server sends from the other port and that
        // the filter drops it on this mapping: an address-dependent filter lets that port in.
        // An address-dependent mapping keeps the control valid, because PA has the IP of PP.
        Probe::Timeout { .. } => {
            if c02 == Control::Ok {
                known(AddressAndPortDependent, Certainty::Exact, Vec::new())
            } else {
                unk(UnknownReason::ServerSilentOrControlFailed, Constraint::None)
            }
        }
        Probe::WrongSource | Probe::RefusedChangeRequest => {
            unk(UnknownReason::ServerIgnoresChangeRequest, Constraint::None)
        }
    }
}

fn timeout_info(p: &Probe<()>) -> (u8, u32) {
    match p {
        Probe::Timeout { sends, elapsed_ms } => (*sends, *elapsed_ms),
        _ => (0, 0),
    }
}

/// Mapped endpoint as `(ip, port)`.
pub type Mapped = (IpAddr, u16);

/// RFC 5780 section 4.3 single-server mapping verdict (`m1` = PP, `m2` = AP, `m3` = AA, `m1b` = PP repeat).
pub fn mapping_verdict(
    m1: Option<Mapped>,
    m2: Option<Mapped>,
    m3: Option<Mapped>,
    m1b: Option<Mapped>,
    local: Mapped,
) -> Verdict<MappingBehavior> {
    use MappingBehavior::{AddressAndPortDependent, AddressDependent, EndpointIndependent};
    let Some(m1) = m1 else {
        return unk(UnknownReason::NoT1, Constraint::None);
    };
    if m1 == local {
        return known(EndpointIndependent, Certainty::Exact, Vec::new());
    }
    let Some(m2) = m2 else {
        return unk(UnknownReason::NoT2, Constraint::None);
    };
    if m1b.is_some_and(|b| b != m1) {
        return unk(UnknownReason::UnstableMapping, Constraint::None);
    }
    if m2 == m1 {
        return known(EndpointIndependent, Certainty::Exact, Vec::new());
    }
    let Some(m3) = m3 else {
        return unk(UnknownReason::NoT3, Constraint::MappingNotEndpointIndependent);
    };
    if m3 == m2 {
        known(AddressDependent, Certainty::Exact, Vec::new())
    } else {
        known(AddressAndPortDependent, Certainty::Exact, Vec::new())
    }
}

/// Multi-server fallback mapping verdict when no STUN server returned a usable `OTHER-ADDRESS`.
/// Each entry in `obs` is `(destination_addr, mapped_addr)` observed on the single socket `M`.
/// `primary_mapped` is the mapped address observed on the primary destination that `control_repeat`
/// re-probed (falling back to `obs[0].1` when `None`).
pub fn mapping_verdict_multi(
    obs: &[(SocketAddr, SocketAddr)],
    primary_mapped: Option<SocketAddr>,
    control_repeat: Option<SocketAddr>,
    local: SocketAddr,
    pooling: Option<IpPooling>,
) -> Verdict<MappingBehavior> {
    use MappingBehavior::{AddressAndPortDependent, AddressDependent, EndpointIndependent};
    if obs.is_empty() {
        return unk(UnknownReason::NoT1, Constraint::None);
    }
    let distinct_dest_ips: BTreeSet<IpAddr> = obs.iter().map(|(d, _)| d.ip()).collect();
    let baseline = primary_mapped.unwrap_or(obs[0].1);
    if baseline == local {
        return known(EndpointIndependent, Certainty::Exact, Vec::new());
    }
    if distinct_dest_ips.len() < 2 {
        return unk(UnknownReason::NeedTwoDistinctServerIps, Constraint::None);
    }
    if control_repeat.is_some_and(|r| r != baseline) {
        return unk(UnknownReason::UnstableMapping, Constraint::None);
    }
    let first = obs[0].1;
    if obs.iter().all(|(_, m)| *m == first) {
        return known(EndpointIndependent, Certainty::Exact, Vec::new());
    }
    let all_ports_equal = obs.iter().all(|(_, m)| m.port() == first.port());
    if all_ports_equal {
        return match pooling {
            Some(IpPooling::Arbitrary) => known(EndpointIndependent, Certainty::Exact, vec![Flag::ArbitraryIp]),
            _ => unk(UnknownReason::PoolingConfounds, Constraint::None),
        };
    }
    // Ports differ across destinations. AD vs APD requires a pair of observations sharing a
    // destination IP on distinct destination ports.
    for i in 0..obs.len() {
        for j in (i + 1)..obs.len() {
            if obs[i].0.ip() == obs[j].0.ip() && obs[i].0.port() != obs[j].0.port() {
                return if obs[i].1 == obs[j].1 {
                    known(AddressDependent, Certainty::Exact, Vec::new())
                } else {
                    known(AddressAndPortDependent, Certainty::Exact, Vec::new())
                };
            }
        }
    }
    unk(UnknownReason::NoT3, Constraint::MappingNotEndpointIndependent)
}
