//! The derivation of the verdicts and of the counters from the knobs.

use std::net::IpAddr;

use pod_netlab::Family::{V4, V6};
use pod_netlab::{Bound, Counter, Expect, Expectations, Lab, Must, Probe, Seen, Verdict, scenario, scenarios};

fn exp(name: &str) -> Expectations {
    Lab::new("sn", name, scenario(name).unwrap().knobs).unwrap().expect()
}

fn of(e: &Expectations, probe: Probe) -> &Expect {
    e.get(probe).unwrap_or_else(|| panic!("no expectation of {probe}"))
}

fn verdict(e: &Expectations, probe: Probe) -> Verdict {
    let x = of(e, probe);
    assert_eq!(x.verdicts.len(), 1, "{probe}: {x:?}");
    x.verdicts[0]
}

fn must(e: &Expectations, c: Counter) -> Option<Must> {
    e.counters.iter().find(|(k, _)| *k == c).map(|(_, m)| *m)
}

fn addr(text: &str) -> Seen {
    Seen::Addr(text.parse::<IpAddr>().unwrap())
}

#[test]
fn dual_nat_passes_and_hides_the_guest() {
    let e = exp("dual-nat");
    for port in pod_netlab::probe::WORLD_TCP {
        assert_eq!(verdict(&e, Probe::Tcp(V4, port)), Verdict::Allow);
        assert_eq!(verdict(&e, Probe::Tcp(V6, port)), Verdict::Allow);
    }
    assert_eq!(of(&e, Probe::Whoami(V4)).seen, Some(addr("198.51.100.1")));
    assert_eq!(of(&e, Probe::Whoami(V6)).seen, Some(addr("2001:db8:100::1")));
    assert_eq!(of(&e, Probe::Srcport(V4)).seen, Some(Seen::PortKept));
    assert_eq!(verdict(&e, Probe::Icmp(V4)), Verdict::Allow, "ping follows the policy open");
    assert_eq!(verdict(&e, Probe::Inbound(V4)), Verdict::Dropped, "L6");
    assert_eq!(verdict(&e, Probe::InboundDnat(V6)), Verdict::Refused);
    assert_eq!(verdict(&e, Probe::Hairpin(V4)), Verdict::Refused);
    assert_eq!(verdict(&e, Probe::Dns64), Verdict::Unresolved);
    assert_eq!(of(&e, Probe::UdpBurst(V4)).bound, Some(Bound::Between(100, 100)));
    for (c, m) in [
        (Counter::Return, Must::Move),
        (Counter::Forwarded, Must::Move),
        (Counter::Inbound, Must::Move),
        (Counter::Broken, Must::Stay),
        (Counter::Ping, Must::Move),
        (Counter::Open, Must::Move),
        (Counter::Tcp, Must::Stay),
        (Counter::Udp, Must::Stay),
        (Counter::Dropped, Must::Stay),
    ] {
        assert_eq!(must(&e, c), Some(m), "{c:?}");
    }
}

#[test]
fn a_vps_has_no_nat_and_one_family() {
    let e = exp("v6-only-vps");
    assert_eq!(of(&e, Probe::Whoami(V6)).seen, Some(addr("fd00:77::2")));
    assert_eq!(verdict(&e, Probe::Tcp(V4, 443)), Verdict::Deny);
    assert_eq!(verdict(&e, Probe::Inbound(V6)), Verdict::Allow);
    assert!(e.get(Probe::Inbound(V4)).is_none(), "no inbound probe of a family the guest lacks");
    let e = exp("v4-vps");
    assert_eq!(verdict(&e, Probe::Icmp(V6)), Verdict::Deny);
    assert_eq!(of(&e, Probe::Whoami(V4)).seen, Some(addr("10.77.0.2")));
    let e = exp("home-nat44");
    assert_eq!(of(&e, Probe::Whoami(V6)).seen, Some(addr("fd00:77::2")), "IPv6 is routed");
    assert_eq!(verdict(&e, Probe::Inbound(V6)), Verdict::Allow);
}

#[test]
fn the_policies_drop_what_they_do_not_name() {
    let e = exp("web-only");
    assert_eq!(verdict(&e, Probe::Tcp(V4, 22)), Verdict::Dropped);
    assert_eq!(verdict(&e, Probe::Tcp(V6, 443)), Verdict::Allow);
    assert_eq!(verdict(&e, Probe::DnsUdp(V4)), Verdict::Allow);
    assert_eq!(verdict(&e, Probe::Udp(V4)), Verdict::Dropped);
    assert_eq!(verdict(&e, Probe::Icmp(V4)), Verdict::Dropped);
    assert_eq!(
        (must(&e, Counter::Tcp), must(&e, Counter::Udp), must(&e, Counter::Open)),
        (Some(Must::Move), Some(Must::Move), Some(Must::Stay))
    );
    let e = exp("locked-443");
    assert_eq!(verdict(&e, Probe::Tcp(V4, 80)), Verdict::Dropped);
    assert_eq!(verdict(&e, Probe::Resolve), Verdict::Unresolved);
    let e = exp("corp-proxy");
    assert_eq!(verdict(&e, Probe::Proxy(V4)), Verdict::Allow);
    assert_eq!(verdict(&e, Probe::Tcp(V4, 443)), Verdict::Dropped);
    let e = exp("blackout");
    assert_eq!(verdict(&e, Probe::Tcp(V4, 443)), Verdict::Dropped);
    assert_eq!(must(&e, Counter::Return), Some(Must::Stay), "nothing passes a blackout");
    assert_eq!(must(&e, Counter::Dropped), Some(Must::Move));
}

#[test]
fn a_broken_family_drops_or_refuses() {
    let e = exp("broken-v4-silent");
    assert_eq!(verdict(&e, Probe::Tcp(V4, 443)), Verdict::Dropped);
    assert_eq!(verdict(&e, Probe::Tcp(V6, 443)), Verdict::Allow);
    assert_eq!(verdict(&e, Probe::Resolve), Verdict::Allow, "the IPv6 nameserver answers");
    assert_eq!(must(&e, Counter::Broken), Some(Must::Move));
    let e = exp("broken-v6-reject");
    assert_eq!(verdict(&e, Probe::Tcp(V6, 22)), Verdict::Refused);
    assert_eq!(verdict(&e, Probe::Udp(V6)), Verdict::Refused, "L2: a connected socket");
    assert_eq!(verdict(&e, Probe::Inbound(V6)), Verdict::Refused);
}

#[test]
fn a_pmtu_black_hole_stalls_the_bulk() {
    let e = exp("pmtu-blackhole");
    assert_eq!(verdict(&e, Probe::Bulk(V4)), Verdict::Timeout);
    assert_eq!(verdict(&e, Probe::Whoami(V4)), Verdict::Allow);
    assert_eq!(must(&e, Counter::Dropped), Some(Must::Move));
    let e = exp("pmtu-ok");
    assert_eq!(verdict(&e, Probe::Bulk(V6)), Verdict::Allow);
    assert_eq!(must(&e, Counter::Dropped), Some(Must::Stay));
}

#[test]
fn impairment_gives_bounds() {
    let e = exp("lossy-mobile");
    assert_eq!(of(&e, Probe::Rtt(V4)).bound, Some(Bound::AtLeastMs(80)));
    // 256 KiB at 4 Mbit/s is 524 ms; 90 % of it, and the delay of a round trip.
    assert_eq!(of(&e, Probe::Bulk(V4)).bound, Some(Bound::AtLeastMs(471 + 80)));
    // q = 0.95^2: mean 90.25, sd 2.97; 4 sigma.
    assert_eq!(of(&e, Probe::UdpBurst(V4)).bound, Some(Bound::Between(78, 100)));
    // A datagram passes 5 tries but in 1 run in 100,000.
    assert_eq!(verdict(&e, Probe::Udp(V4)), Verdict::Allow);
    // The resolver tries two times: loss can take both.
    assert_eq!(of(&e, Probe::Resolve).verdicts, [Verdict::Allow, Verdict::Unresolved]);
    let e = exp("satellite");
    assert_eq!(of(&e, Probe::Rtt(V6)).bound, Some(Bound::AtLeastMs(600)));
    assert_eq!(of(&e, Probe::Bulk(V6)).bound, Some(Bound::AtLeastMs(2097 * 9 / 10 + 600)));
}

#[test]
fn symmetric_nat_moves_the_source_port() {
    assert_eq!(of(&exp("symmetric-nat"), Probe::Srcport(V4)).seen, Some(Seen::PortMoved));
    assert_eq!(of(&exp("symmetric-nat"), Probe::Srcport(V6)).seen, Some(Seen::PortMoved));
}

#[test]
fn dnat_and_hairpin_reach_the_guest() {
    let e = exp("inbound-dnat");
    assert_eq!(verdict(&e, Probe::InboundDnat(V4)), Verdict::Allow);
    assert_eq!(of(&e, Probe::InboundDnat(V4)).seen, Some(addr("198.51.100.10")));
    assert_eq!(verdict(&e, Probe::Inbound(V4)), Verdict::Dropped);
    // L5: the forward passes before the policy web drops port 2200.
    let e = exp("hairpin-web");
    assert_eq!(verdict(&e, Probe::Hairpin(V4)), Verdict::Allow);
    assert_eq!(of(&e, Probe::Hairpin(V6)).seen, Some(addr("fd00:77::1")));
}

#[test]
fn the_ping_rule_overrides_the_policy() {
    assert_eq!(verdict(&exp("ping-allowed-web"), Probe::Icmp(V6)), Verdict::Allow);
    assert_eq!(verdict(&exp("ping-blocked"), Probe::Icmp(V4)), Verdict::Dropped);
    assert_eq!(must(&exp("ping-blocked"), Counter::Ping), Some(Must::Move));
}

#[test]
fn nat64_reaches_the_ipv4_world() {
    let e = exp("nat64-dns64");
    assert_eq!(verdict(&e, Probe::Nat64Tcp(443)), Verdict::Allow);
    assert_eq!(of(&e, Probe::Nat64Whoami).seen, Some(addr("198.51.100.1")));
    assert_eq!(of(&e, Probe::Dns64).seen, Some(addr("2001:db8:64::c633:640a")));
    assert_eq!(verdict(&e, Probe::Tcp(V4, 443)), Verdict::Deny);
    assert_eq!(verdict(&e, Probe::Inbound(V6)), Verdict::Dropped, "nat66");
    let e = exp("nat64-web");
    assert_eq!(verdict(&e, Probe::Nat64Tcp(80)), Verdict::Allow);
    assert_eq!(verdict(&exp("nat64-web"), Probe::Tcp(V6, 22)), Verdict::Dropped);
}

#[test]
fn cgnat_hides_the_router_behind_the_carrier() {
    let e = exp("cgnat");
    assert_eq!(of(&e, Probe::IspWhoami).seen, Some(addr("100.64.0.2")));
    assert_eq!(of(&e, Probe::Whoami(V4)).seen, Some(addr("198.51.100.1")));
    assert_eq!(of(&e, Probe::Whoami(V6)).seen, Some(addr("2001:db8:101::2")));
    let e = exp("cgnat-dnat");
    assert_eq!(verdict(&e, Probe::InboundDnat(V4)), Verdict::Dropped, "the carrier has no forward");
    assert!(of(&e, Probe::InboundDnat(V4)).moves.is_empty());
    assert_eq!(verdict(&e, Probe::InboundDnat(V6)), Verdict::Allow, "IPv6 is routed to the router");
}

/// A flushed chain leaves each counter still: each lab needs a counter
/// that must move, so the judge can see it.
#[test]
fn each_scenario_has_a_counter_that_must_move() {
    for s in scenarios() {
        let e = Lab::new("sn", s.name, s.knobs).unwrap().expect();
        assert_eq!(must(&e, Counter::Forwarded), Some(Must::Move), "{}", s.name);
        for x in &e.probes {
            assert!(!x.verdicts.is_empty(), "{}: {}", s.name, x.probe);
            assert!(
                x.verdicts.iter().all(|v| *v != Verdict::Allow) || x.why.starts_with("passes"),
                "{}: {x:?}",
                s.name
            );
            assert_eq!(Probe::from_name(&x.probe.name()), Some(x.probe));
        }
    }
}
