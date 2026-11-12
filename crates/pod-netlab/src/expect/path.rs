//! The path of each probe through the router's rules, in the order of the
//! forward chain (`plan::ruleset::forward_rules`).

use std::net::IpAddr;

use super::{Bound, Expect, Seen, Verdict};
use crate::knobs::{Family, Icmp, Knobs, Proto};
use crate::layout::{CGN_ROUTER_V4, Layout};
use crate::plan::ruleset::{Counter, rejects};
use crate::probe::{BULK_BYTES, BURST, DNS_PORT, HTTP_PORT, PROXY_PORT, Probe, TLS_PORT, TRIES, dns64_answer};

/// The traffic of a probe of the guest.
#[derive(Clone, Copy)]
enum L4 {
    Port(Proto, u16),
    Echo,
}

/// A probe that the router stops, with the counters that moved.
struct Stop {
    verdict: Verdict,
    moves: Vec<Counter>,
    why: String,
}

fn stop(verdict: Verdict, moves: Vec<Counter>, why: impl Into<String>) -> Stop {
    Stop { verdict, moves, why: why.into() }
}

/// A new flow of the guest of `family` through the forward chain: the
/// counters of the rules that pass it, or where it stops.
fn reach(knobs: &Knobs, family: Family, l4: L4) -> Result<Vec<Counter>, Stop> {
    let v = family.digit();
    if !knobs.stack.has(family) {
        return Err(stop(Verdict::Deny, vec![], format!("the guest has no ipv{v}")));
    }
    let mut moves = vec![Counter::Forwarded];
    if let Some(reject) = rejects(knobs.broken, family) {
        moves.push(Counter::Broken);
        let verdict = if reject { Verdict::Refused } else { Verdict::Dropped };
        return Err(stop(verdict, moves, format!("broken ipv{v}")));
    }
    let (proto, port) = match l4 {
        L4::Echo => {
            moves.push(Counter::Ping);
            if knobs.ping_passes() {
                return Ok(moves);
            }
            return Err(stop(Verdict::Dropped, moves, "the ping rule drops an echo"));
        }
        L4::Port(proto, port) => (proto, port),
    };
    if !knobs.dns && port == DNS_PORT {
        moves.push(Counter::Dropped);
        return Err(stop(Verdict::Dropped, moves, "no-dns drops port 53"));
    }
    if !knobs.policy.allows(proto, port) {
        moves.push(Counter::Dropped);
        return Err(stop(Verdict::Dropped, moves, format!("the policy drops {proto:?} {port}")));
    }
    moves.push(match (knobs.policy.lists(), proto) {
        (None, _) => Counter::Open,
        (Some(_), Proto::Tcp) => Counter::Tcp,
        (Some(_), Proto::Udp) => Counter::Udp,
    });
    Ok(moves)
}

/// A flow of the world to the guest's side, after the carrier: the
/// counters, or where it stops.
fn arrive(knobs: &Knobs, family: Family) -> Result<Vec<Counter>, Stop> {
    if knobs.cgnat && family == Family::V4 {
        return Err(stop(Verdict::Dropped, vec![], "the carrier drops a new flow of the world"));
    }
    if let Some(reject) = rejects(knobs.broken, family) {
        let verdict = if reject { Verdict::Refused } else { Verdict::Dropped };
        return Err(stop(verdict, vec![Counter::Broken], format!("broken ipv{}", family.digit())));
    }
    Ok(vec![Counter::Inbound])
}

/// The probability that a datagram and its answer both pass the netem
/// queues of both directions.
fn round_trip(knobs: &Knobs) -> f64 {
    let p = f64::from(knobs.loss_bp.unwrap_or(0)) / 10_000.0;
    (1.0 - p) * (1.0 - p)
}

/// Whether a probe of `tries` datagrams can miss each answer by chance:
/// more than one run in 10,000.
fn can_miss(knobs: &Knobs, tries: u32) -> bool {
    (1.0 - round_trip(knobs)).powi(tries as i32) > 1e-4
}

/// The source address that the world sees from the guest's `family`.
fn world_sees(knobs: &Knobs, layout: &Layout, family: Family) -> IpAddr {
    if knobs.cgnat && family == Family::V4 {
        Layout::world_gateway(family)
    } else if knobs.nat.has(family) {
        layout.router_out(family)
    } else {
        Layout::guest(family)
    }
}

/// The expectation of an outbound probe; `tries` > 0 for a probe of
/// single datagrams, whose answers loss can take.
fn outbound(knobs: &Knobs, probe: Probe, reached: Result<Vec<Counter>, Stop>, tries: u32, fail: Verdict) -> Expect {
    match reached {
        Err(s) => Expect {
            probe,
            verdicts: vec![s.verdict],
            seen: None,
            bound: None,
            moves: s.moves,
            may_move: vec![],
            why: s.why,
        },
        Ok(mut moves) => {
            if tries > 0 && can_miss(knobs, tries) {
                let why = "passes; loss can take each try".to_string();
                return Expect {
                    probe,
                    verdicts: vec![Verdict::Allow, fail],
                    seen: None,
                    bound: None,
                    moves,
                    may_move: vec![Counter::Return],
                    why,
                };
            }
            moves.push(Counter::Return);
            Expect {
                probe,
                verdicts: vec![Verdict::Allow],
                seen: None,
                bound: None,
                moves,
                may_move: vec![],
                why: "passes".into(),
            }
        }
    }
}

fn tcp(knobs: &Knobs, family: Family, port: u16) -> Result<Vec<Counter>, Stop> {
    reach(knobs, family, L4::Port(Proto::Tcp, port))
}

/// The resolver: each nameserver of the guest's stack, IPv4 first, until
/// one passes.
fn resolver(knobs: &Knobs) -> Result<Vec<Counter>, Stop> {
    let mut moves = Vec::new();
    let mut last = stop(Verdict::Unresolved, vec![], "no nameserver");
    for family in Family::BOTH.into_iter().filter(|f| knobs.stack.has(*f)) {
        match reach(knobs, family, L4::Port(Proto::Udp, DNS_PORT)) {
            Ok(m) => {
                moves.extend(m);
                return Ok(moves);
            }
            Err(s) => {
                moves.extend(s.moves.iter().copied());
                last = s;
            }
        }
    }
    Err(stop(Verdict::Unresolved, moves, format!("no nameserver answers: {}", last.why)))
}

/// The resolver's tries: two attempts of each nameserver.
const RESOLVER_TRIES: u32 = 2;

pub(super) fn expect(knobs: &Knobs, layout: &Layout, probe: Probe) -> Expect {
    let mut e = match probe {
        Probe::Tcp(family, port) => outbound(knobs, probe, tcp(knobs, family, port), 0, Verdict::Dropped),
        Probe::Proxy(family) => outbound(knobs, probe, tcp(knobs, family, PROXY_PORT), 0, Verdict::Dropped),
        Probe::Udp(family) | Probe::UdpBurst(family) => {
            let reached = reach(knobs, family, L4::Port(Proto::Udp, crate::probe::ECHO_PORT));
            let tries = if matches!(probe, Probe::Udp(_)) { TRIES } else { 0 };
            outbound(knobs, probe, reached, tries, Verdict::Dropped)
        }
        Probe::DnsUdp(family) => {
            outbound(knobs, probe, reach(knobs, family, L4::Port(Proto::Udp, DNS_PORT)), TRIES, Verdict::Dropped)
        }
        Probe::Icmp(family) => outbound(knobs, probe, reach(knobs, family, L4::Echo), TRIES, Verdict::Dropped),
        Probe::Whoami(family) | Probe::Srcport(family) | Probe::Bulk(family) => {
            outbound(knobs, probe, tcp(knobs, family, HTTP_PORT), 0, Verdict::Dropped)
        }
        Probe::Rtt(family) => outbound(knobs, probe, tcp(knobs, family, TLS_PORT), 0, Verdict::Dropped),
        Probe::Resolve | Probe::Dns64 => outbound(knobs, probe, resolver(knobs), RESOLVER_TRIES, Verdict::Unresolved),
        Probe::Nat64Tcp(port) => nat64(knobs, probe, port),
        Probe::Nat64Whoami => nat64(knobs, probe, HTTP_PORT),
        Probe::IspWhoami => outbound(knobs, probe, tcp(knobs, Family::V4, HTTP_PORT), 0, Verdict::Dropped),
        Probe::Hairpin(family) => hairpin(knobs, probe, family),
        Probe::Inbound(family) => inbound(knobs, probe, family),
        Probe::InboundDnat(family) => inbound_dnat(knobs, probe, family),
    };
    if probe == Probe::Dns64 && !knobs.dns64 && e.verdicts.contains(&Verdict::Allow) {
        // The query passes; the answer has no address.
        e.verdicts = vec![Verdict::Unresolved];
        e.why = "the world's DNS has no AAAA of v4only and no dns64".into();
    }
    if e.verdicts == [Verdict::Allow] {
        finish(knobs, layout, &mut e);
    }
    e
}

/// What a probe that surely passes must see, and its bounds.
fn finish(knobs: &Knobs, layout: &Layout, e: &mut Expect) {
    let delay = u64::from(knobs.delay_ms.unwrap_or(0));
    match e.probe {
        Probe::Whoami(family) => e.seen = Some(Seen::Addr(world_sees(knobs, layout, family))),
        Probe::Srcport(family) => {
            let moved = knobs.nat.has(family) && knobs.nat_flags.is_some_and(|f| f.moves_ports());
            e.seen = Some(if moved { Seen::PortMoved } else { Seen::PortKept });
        }
        Probe::Nat64Whoami => {
            e.seen = Some(Seen::Addr(if knobs.cgnat {
                Layout::world_gateway(Family::V4)
            } else {
                layout.router_out(Family::V4)
            }))
        }
        Probe::IspWhoami => e.seen = Some(Seen::Addr(IpAddr::V4(CGN_ROUTER_V4))),
        Probe::Hairpin(family) => e.seen = Some(Seen::Addr(Layout::router_in(family))),
        Probe::Inbound(family) | Probe::InboundDnat(family) => e.seen = Some(Seen::Addr(Layout::world(family))),
        Probe::Dns64 => e.seen = Some(Seen::Addr(dns64_answer())),
        Probe::Rtt(_) if delay > 0 => e.bound = Some(Bound::AtLeastMs(2 * delay)),
        Probe::Bulk(_) => {
            if knobs.mtu.is_some() && knobs.icmp == Icmp::Drop {
                // The router withholds frag-needed and packet-too-big.
                e.verdicts = vec![Verdict::Timeout];
                e.moves.push(Counter::Dropped);
                e.why = "a PMTU black hole: mtu with icmp drop".into();
            } else if let Some(kbit) = knobs.rate_kbit {
                // bits / (kbit/s) = ms; 10 % below for the clock of netem.
                let ms = BULK_BYTES * 8 / u64::from(kbit) * 9 / 10;
                e.bound = Some(Bound::AtLeastMs(ms + 2 * delay));
            } else if delay > 0 {
                e.bound = Some(Bound::AtLeastMs(2 * delay));
            }
        }
        Probe::UdpBurst(_) => {
            let q = round_trip(knobs);
            let n = f64::from(BURST);
            let (mean, sd) = (n * q, (n * q * (1.0 - q)).sqrt());
            let lo = (mean - 4.0 * sd).floor().max(0.0) as u32;
            let hi = (mean + 4.0 * sd).ceil().min(n) as u32;
            e.bound = Some(Bound::Between(lo, hi));
            if lo == 0 {
                e.verdicts.push(Verdict::Dropped);
            }
        }
        _ => {}
    }
}

/// A flow through tayga: the IPv6 side passes the guest's rules, then the
/// IPv4 side passes the broken rule of IPv4.
fn nat64(knobs: &Knobs, probe: Probe, port: u16) -> Expect {
    let reached = tcp(knobs, Family::V6, port).and_then(|mut moves| match rejects(knobs.broken, Family::V4) {
        None => Ok(moves),
        Some(reject) => {
            moves.push(Counter::Broken);
            let verdict = if reject { Verdict::Refused } else { Verdict::Dropped };
            Err(stop(verdict, moves, "broken ipv4 behind tayga"))
        }
    });
    outbound(knobs, probe, reached, 0, Verdict::Dropped)
}

fn hairpin(knobs: &Knobs, probe: Probe, family: Family) -> Expect {
    let reached = if !knobs.stack.has(family) {
        Err(stop(Verdict::Deny, vec![], format!("the guest has no ipv{}", family.digit())))
    } else if !knobs.hairpin {
        Err(stop(Verdict::Refused, vec![], "no hairpin: the router's own port 2222 is closed"))
    } else {
        match rejects(knobs.broken, family) {
            Some(reject) => {
                let verdict = if reject { Verdict::Refused } else { Verdict::Dropped };
                Err(stop(verdict, vec![Counter::Forwarded, Counter::Broken], "broken family"))
            }
            None => Ok(vec![Counter::Forwarded, Counter::Inbound]),
        }
    };
    outbound(knobs, probe, reached, 0, Verdict::Dropped)
}

fn inbound(knobs: &Knobs, probe: Probe, family: Family) -> Expect {
    let reached = arrive(knobs, family).and_then(|moves| {
        if knobs.nat_on(family) {
            Err(stop(Verdict::Dropped, moves, "NAT on the family drops a new inbound flow"))
        } else {
            Ok(moves)
        }
    });
    outbound(knobs, probe, reached, 0, Verdict::Dropped)
}

fn inbound_dnat(knobs: &Knobs, probe: Probe, family: Family) -> Expect {
    let reached = if knobs.cgnat && family == Family::V4 {
        Err(stop(Verdict::Dropped, vec![], "the carrier drops a new flow to itself"))
    } else if !knobs.dnat {
        Err(stop(Verdict::Refused, vec![], "no dnat: the router's own port 2222 is closed"))
    } else {
        arrive(knobs, family)
    };
    outbound(knobs, probe, reached, 0, Verdict::Dropped)
}
