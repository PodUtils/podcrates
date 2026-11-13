//! The ranges of the knobs and their contradictions. sand-net refused
//! three (nat64 needs v6, cgnat needs v4 and NAT44, hairpin needs dnat);
//! the others are what the issue implies (call C146).

use std::fmt;

use super::{Family, Icmp, Knobs, Nat, Policy};

/// Two knobs that cannot go together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contradiction {
    /// `--nat64` with `--stack v4`: the guest has no IPv6 to translate.
    Nat64NeedsV6,
    /// `--cgnat` with `--stack v6`.
    CgnatNeedsV4,
    /// `--cgnat` with no NAT44: a carrier NAT sits behind a home NAT.
    CgnatNeedsNat44,
    /// `--hairpin` with no `--dnat`: no forward to reach.
    HairpinNeedsDnat,
    /// `--dns64` with no `--nat64`: the synthesized addresses go nowhere.
    Dns64NeedsNat64,
    /// `--nat-flags` with no masquerade rule.
    NatFlagsNeedNat,
    /// `--nat` names a family that the guest does not have.
    NatWithoutFamily(Family),
    /// `--broken` names a family that carries no flow of the guest.
    BrokenWithoutFamily(Family),
    /// `--icmp drop` with no `--mtu`: no packet is too big.
    IcmpDropNeedsMtu,
    /// `--policy` together with `--tcp` or `--udp`.
    PolicyAndPorts,
}

impl Contradiction {
    pub fn text(self) -> String {
        match self {
            Contradiction::Nat64NeedsV6 => "nat64 needs the v6 stack".into(),
            Contradiction::CgnatNeedsV4 => "cgnat needs the v4 stack".into(),
            Contradiction::CgnatNeedsNat44 => "cgnat needs nat44".into(),
            Contradiction::HairpinNeedsDnat => "hairpin needs dnat".into(),
            Contradiction::Dns64NeedsNat64 => "dns64 needs nat64".into(),
            Contradiction::NatFlagsNeedNat => "nat-flags need a nat".into(),
            Contradiction::NatWithoutFamily(f) => {
                format!("nat names ipv{} and the stack has no ipv{}", f.digit(), f.digit())
            }
            Contradiction::BrokenWithoutFamily(f) => {
                format!("broken names ipv{} and no flow of the guest uses ipv{}", f.digit(), f.digit())
            }
            Contradiction::IcmpDropNeedsMtu => "icmp drop needs an mtu".into(),
            Contradiction::PolicyAndPorts => "policy and tcp or udp lists exclude each other".into(),
        }
    }
}

/// A knob set that `Knobs::check` refuses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KnobError {
    Contradiction(Contradiction),
    /// A value out of the range of its knob.
    OutOfRange {
        knob: &'static str,
        value: u64,
        min: u64,
        max: u64,
    },
    /// A port that a list of `--tcp` or `--udp` names two times, or port 0.
    BadPort {
        knob: &'static str,
        port: u16,
    },
    /// A list of `--tcp` or `--udp` with more than `MAX_PORTS` ports.
    TooManyPorts {
        knob: &'static str,
        count: usize,
    },
    /// A flag that `Knobs::parse_flags` does not know, or a flag with no
    /// value or a bad value.
    BadFlag {
        flag: String,
        why: String,
    },
}

impl fmt::Display for KnobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KnobError::Contradiction(c) => write!(f, "contradictory knobs: {}", c.text()),
            KnobError::OutOfRange { knob, value, min, max } => {
                write!(f, "{knob} {value} is out of its range {min} to {max}")
            }
            KnobError::BadPort { knob, port } => write!(f, "{knob}: port {port} is 0 or named two times"),
            KnobError::TooManyPorts { knob, count } => {
                write!(f, "{knob}: {count} ports, more than {MAX_PORTS}")
            }
            KnobError::BadFlag { flag, why } => write!(f, "flag {flag}: {why}"),
        }
    }
}

impl std::error::Error for KnobError {}

impl From<KnobError> for pod_core::Failure {
    fn from(e: KnobError) -> pod_core::Failure {
        pod_core::Failure::caused(pod_core::Stage::Host, pod_core::Class::Usage, e.to_string(), e)
    }
}

/// The most ports of one list of `--tcp` or `--udp`.
pub const MAX_PORTS: usize = 64;
/// The range of `--mtu`. IPv6 needs 1280; 1500 is the MTU of each link.
pub const MTU: (u16, u16) = (1280, 1499);
/// The range of `--delay` in milliseconds.
pub const DELAY_MS: (u32, u32) = (1, 10_000);
/// The range of `--loss` in hundredths of a percent: past 20 % a TCP
/// probe does not end in its time limit (call C146).
pub const LOSS_BP: (u16, u16) = (1, 2_000);
/// The range of `--rate` in kbit/s.
pub const RATE_KBIT: (u32, u32) = (64, 10_000_000);

fn range(knob: &'static str, value: Option<u64>, (min, max): (u64, u64)) -> Result<(), KnobError> {
    match value {
        Some(v) if v < min || v > max => Err(KnobError::OutOfRange { knob, value: v, min, max }),
        _ => Ok(()),
    }
}

fn ports(knob: &'static str, list: &[u16]) -> Result<(), KnobError> {
    if list.len() > MAX_PORTS {
        return Err(KnobError::TooManyPorts { knob, count: list.len() });
    }
    for (i, &port) in list.iter().enumerate() {
        if port == 0 || list[..i].contains(&port) {
            return Err(KnobError::BadPort { knob, port });
        }
    }
    Ok(())
}

impl Knobs {
    /// Refuse a value out of its range and each contradiction.
    pub fn check(&self) -> Result<(), KnobError> {
        range("mtu", self.mtu.map(u64::from), (MTU.0.into(), MTU.1.into()))?;
        range("delay", self.delay_ms.map(u64::from), (DELAY_MS.0.into(), DELAY_MS.1.into()))?;
        range("loss", self.loss_bp.map(u64::from), (LOSS_BP.0.into(), LOSS_BP.1.into()))?;
        range("rate", self.rate_kbit.map(u64::from), (RATE_KBIT.0.into(), RATE_KBIT.1.into()))?;
        if let Policy::Ports { tcp, udp } = &self.policy {
            ports("tcp", tcp)?;
            ports("udp", udp)?;
        }
        let refuse = |c| Err(KnobError::Contradiction(c));
        if self.nat64 && !self.stack.has(Family::V6) {
            return refuse(Contradiction::Nat64NeedsV6);
        }
        if self.cgnat && !self.stack.has(Family::V4) {
            return refuse(Contradiction::CgnatNeedsV4);
        }
        if self.cgnat && !self.nat.has(Family::V4) {
            return refuse(Contradiction::CgnatNeedsNat44);
        }
        if self.hairpin && !self.dnat {
            return refuse(Contradiction::HairpinNeedsDnat);
        }
        if self.dns64 && !self.nat64 {
            return refuse(Contradiction::Dns64NeedsNat64);
        }
        if self.nat_flags.is_some() && self.nat == Nat::None && !self.nat64 {
            return refuse(Contradiction::NatFlagsNeedNat);
        }
        for family in Family::BOTH {
            if self.nat.has(family) && !self.stack.has(family) {
                return refuse(Contradiction::NatWithoutFamily(family));
            }
        }
        if let Some(broken) = self.broken {
            let family = broken.family();
            // NAT64 carries IPv6 flows of the guest over IPv4.
            let used = self.stack.has(family) || (family == Family::V4 && self.nat64);
            if !used {
                return refuse(Contradiction::BrokenWithoutFamily(family));
            }
        }
        if self.icmp == Icmp::Drop && self.mtu.is_none() {
            return refuse(Contradiction::IcmpDropNeedsMtu);
        }
        Ok(())
    }
}
