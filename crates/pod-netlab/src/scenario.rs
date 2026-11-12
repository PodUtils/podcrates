//! The offline scenarios of `sand net scenarios` (issue 178), each a knob
//! set. The issue gives the numbers of lossy-mobile and satellite; each
//! other knob set comes from the scenario's name (call C145). The VPN and
//! the live scenarios are T-348.

use crate::knobs::{Broken, Icmp, Knobs, Nat, NatFlags, Ping, Policy, Stack};

/// One scenario.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scenario {
    pub name: &'static str,
    pub knobs: Knobs,
    /// Whether the issue gives the knob set (`false`: derived from the name).
    pub given: bool,
}

/// The base of most scenarios: a home router with a dual stack and NAT on
/// both families.
fn dual_nat() -> Knobs {
    Knobs { stack: Stack::Dual, nat: Nat::Both, ..Knobs::default() }
}

fn with(f: impl FnOnce(&mut Knobs)) -> Knobs {
    let mut k = dual_nat();
    f(&mut k);
    k
}

/// The names of the offline scenarios, in the order of the issue.
pub const NAMES: [&str; 27] = [
    "dual-nat",
    "home-nat44",
    "v6-only-nat66",
    "v6-only-vps",
    "v4-vps",
    "web-only",
    "locked-443",
    "corp-proxy",
    "blackout",
    "broken-v4-silent",
    "broken-v4-reject",
    "broken-v6-silent",
    "broken-v6-reject",
    "pmtu-ok",
    "pmtu-blackhole",
    "lossy-mobile",
    "satellite",
    "symmetric-nat",
    "inbound-dnat",
    "ping-allowed-web",
    "ping-blocked",
    "nat64-dns64",
    "nat64-web",
    "cgnat",
    "cgnat-dnat",
    "hairpin-nat",
    "hairpin-web",
];

/// The knob set of a scenario, and whether the issue gives it.
fn knobs_of(name: &str) -> Option<(Knobs, bool)> {
    let nat64 = |policy: Policy| Knobs {
        stack: Stack::V6,
        nat: Nat::Nat66,
        nat64: true,
        dns64: true,
        policy,
        ..Knobs::default()
    };
    Some(match name {
        "dual-nat" => (dual_nat(), false),
        "home-nat44" => (Knobs { nat: Nat::Nat44, ..Knobs::default() }, false),
        "v6-only-nat66" => (Knobs { stack: Stack::V6, nat: Nat::Nat66, ..Knobs::default() }, false),
        "v6-only-vps" => (Knobs { stack: Stack::V6, ..Knobs::default() }, false),
        "v4-vps" => (Knobs { stack: Stack::V4, ..Knobs::default() }, false),
        "web-only" => (with(|k| k.policy = Policy::Web), false),
        "locked-443" => (with(|k| k.policy = Policy::Only443), false),
        "corp-proxy" => (with(|k| k.policy = Policy::Ports { tcp: vec![3128], udp: vec![] }), false),
        "blackout" => (with(|k| k.policy = Policy::Blackout), false),
        "broken-v4-silent" => (with(|k| k.broken = Some(Broken::V4Drop)), false),
        "broken-v4-reject" => (with(|k| k.broken = Some(Broken::V4Reject)), false),
        "broken-v6-silent" => (with(|k| k.broken = Some(Broken::V6Drop)), false),
        "broken-v6-reject" => (with(|k| k.broken = Some(Broken::V6Reject)), false),
        "pmtu-ok" => (with(|k| k.mtu = Some(1400)), false),
        "pmtu-blackhole" => (
            with(|k| {
                k.mtu = Some(1400);
                k.icmp = Icmp::Drop;
            }),
            false,
        ),
        "lossy-mobile" => (
            with(|k| {
                k.delay_ms = Some(40);
                k.loss_bp = Some(500);
                k.rate_kbit = Some(4_000);
            }),
            true,
        ),
        "satellite" => (
            with(|k| {
                k.delay_ms = Some(300);
                k.rate_kbit = Some(1_000);
            }),
            true,
        ),
        "symmetric-nat" => (with(|k| k.nat_flags = Some(NatFlags::FullyRandom)), false),
        "inbound-dnat" => (with(|k| k.dnat = true), false),
        "ping-allowed-web" => (
            with(|k| {
                k.policy = Policy::Web;
                k.ping = Ping::Allow;
            }),
            false,
        ),
        "ping-blocked" => (with(|k| k.ping = Ping::Drop), false),
        "nat64-dns64" => (nat64(Policy::Open), false),
        "nat64-web" => (nat64(Policy::Web), false),
        "cgnat" => (with(|k| k.cgnat = true), false),
        "cgnat-dnat" => (
            with(|k| {
                k.cgnat = true;
                k.dnat = true;
            }),
            false,
        ),
        "hairpin-nat" => (
            with(|k| {
                k.dnat = true;
                k.hairpin = true;
            }),
            false,
        ),
        "hairpin-web" => (
            with(|k| {
                k.dnat = true;
                k.hairpin = true;
                k.policy = Policy::Web;
            }),
            false,
        ),
        _ => return None,
    })
}

/// The scenario of `name`, or `None` for a name that is not an offline
/// scenario.
pub fn scenario(name: &str) -> Option<Scenario> {
    let name = NAMES.into_iter().find(|n| *n == name)?;
    knobs_of(name).map(|(knobs, given)| Scenario { name, knobs, given })
}

/// Each offline scenario, in the order of `NAMES`.
pub fn scenarios() -> Vec<Scenario> {
    NAMES.into_iter().filter_map(scenario).collect()
}
