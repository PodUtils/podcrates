//! The knobs: their ranges, each contradiction by its type, and the flags
//! of sand-net.

use pod_netlab::knobs::loss_text;
use pod_netlab::{Broken, Contradiction, Family, Icmp, KnobError, Knobs, Nat, NatFlags, Policy, Stack, scenarios};

fn refused(knobs: Knobs) -> Contradiction {
    match knobs.check() {
        Err(KnobError::Contradiction(c)) => c,
        other => panic!("expected a contradiction, got {other:?}"),
    }
}

#[test]
fn the_three_contradictions_of_sand_net_are_refused() {
    let k = Knobs { stack: Stack::V4, nat64: true, ..Knobs::default() };
    assert_eq!(refused(k), Contradiction::Nat64NeedsV6);
    let k = Knobs { stack: Stack::V6, cgnat: true, nat: Nat::Nat66, ..Knobs::default() };
    assert_eq!(refused(k), Contradiction::CgnatNeedsV4);
    let k = Knobs { cgnat: true, nat: Nat::Nat66, ..Knobs::default() };
    assert_eq!(refused(k), Contradiction::CgnatNeedsNat44);
    let k = Knobs { hairpin: true, ..Knobs::default() };
    assert_eq!(refused(k), Contradiction::HairpinNeedsDnat);
}

#[test]
fn the_implied_contradictions_are_refused() {
    let k = Knobs { dns64: true, ..Knobs::default() };
    assert_eq!(refused(k), Contradiction::Dns64NeedsNat64);
    let k = Knobs { nat_flags: Some(NatFlags::Random), ..Knobs::default() };
    assert_eq!(refused(k), Contradiction::NatFlagsNeedNat);
    let k = Knobs { stack: Stack::V4, nat: Nat::Both, ..Knobs::default() };
    assert_eq!(refused(k), Contradiction::NatWithoutFamily(Family::V6));
    let k = Knobs { stack: Stack::V6, nat: Nat::Nat44, ..Knobs::default() };
    assert_eq!(refused(k), Contradiction::NatWithoutFamily(Family::V4));
    let k = Knobs { stack: Stack::V4, broken: Some(Broken::V6Drop), ..Knobs::default() };
    assert_eq!(refused(k), Contradiction::BrokenWithoutFamily(Family::V6));
    let k = Knobs { icmp: Icmp::Drop, ..Knobs::default() };
    assert_eq!(refused(k), Contradiction::IcmpDropNeedsMtu);
    let flags = ["--policy", "web", "--tcp", "22"];
    assert_eq!(Knobs::parse_flags(&flags), Err(KnobError::Contradiction(Contradiction::PolicyAndPorts)));
}

#[test]
fn nat64_carries_a_broken_ipv4_on_a_v6_stack() {
    let k = Knobs { stack: Stack::V6, nat64: true, broken: Some(Broken::V4Reject), ..Knobs::default() };
    assert_eq!(k.check(), Ok(()));
    let k = Knobs { stack: Stack::V6, nat64: true, nat_flags: Some(NatFlags::FullyRandom), ..Knobs::default() };
    assert_eq!(k.check(), Ok(()), "the masquerade of the pool takes the flag");
}

#[test]
fn each_range_is_held_at_both_ends() {
    let ok = |k: Knobs| k.check().is_ok();
    let out = |k: Knobs, knob| matches!(k.check(), Err(KnobError::OutOfRange { knob: n, .. }) if n == knob);
    assert!(out(Knobs { mtu: Some(1279), ..Knobs::default() }, "mtu"));
    assert!(out(Knobs { mtu: Some(1500), ..Knobs::default() }, "mtu"));
    assert!(ok(Knobs { mtu: Some(1280), ..Knobs::default() }));
    assert!(ok(Knobs { mtu: Some(1499), ..Knobs::default() }));
    assert!(out(Knobs { delay_ms: Some(0), ..Knobs::default() }, "delay"));
    assert!(out(Knobs { delay_ms: Some(10_001), ..Knobs::default() }, "delay"));
    assert!(ok(Knobs { delay_ms: Some(10_000), ..Knobs::default() }));
    assert!(out(Knobs { loss_bp: Some(0), ..Knobs::default() }, "loss"));
    assert!(out(Knobs { loss_bp: Some(2_001), ..Knobs::default() }, "loss"));
    assert!(ok(Knobs { loss_bp: Some(2_000), ..Knobs::default() }));
    assert!(out(Knobs { rate_kbit: Some(63), ..Knobs::default() }, "rate"));
    assert!(out(Knobs { rate_kbit: Some(10_000_001), ..Knobs::default() }, "rate"));
    assert!(ok(Knobs { rate_kbit: Some(64), ..Knobs::default() }));
}

#[test]
fn a_port_list_has_no_zero_no_double_and_a_limit() {
    let ports = |tcp: Vec<u16>| Knobs { policy: Policy::Ports { tcp, udp: vec![] }, ..Knobs::default() }.check();
    assert_eq!(ports(vec![0]), Err(KnobError::BadPort { knob: "tcp", port: 0 }));
    assert_eq!(ports(vec![22, 443, 22]), Err(KnobError::BadPort { knob: "tcp", port: 22 }));
    assert_eq!(ports((1..=65).collect()), Err(KnobError::TooManyPorts { knob: "tcp", count: 65 }));
    assert_eq!(ports((1..=64).collect()), Ok(()));
}

#[test]
fn the_flags_of_sand_net_read() {
    let k = Knobs::parse_flags(&[
        "--stack",
        "dual",
        "--nat",
        "both",
        "--nat-flags",
        "fully-random",
        "--tcp",
        "22,443",
        "--udp",
        "",
        "--broken",
        "v6-reject",
        "--mtu",
        "1400",
        "--icmp",
        "drop",
        "--delay",
        "40ms",
        "--loss",
        "5%",
        "--rate",
        "4000",
        "--dnat",
        "--hairpin",
        "--ping",
        "allow",
        "--no-dns",
    ])
    .unwrap();
    assert_eq!(k.nat_flags, Some(NatFlags::FullyRandom));
    assert_eq!(k.policy, Policy::Ports { tcp: vec![22, 443], udp: vec![] });
    assert_eq!(
        (k.mtu, k.icmp, k.delay_ms, k.loss_bp, k.rate_kbit),
        (Some(1400), Icmp::Drop, Some(40), Some(500), Some(4000))
    );
    assert!(k.dnat && k.hairpin && !k.dns);
    assert_eq!(Knobs::parse_flags(&["--loss", "0.25"]).unwrap().loss_bp, Some(25));
    assert_eq!(Knobs::parse_flags(&["--loss", "0.5"]).unwrap().loss_bp, Some(50));
    for bad in [&["--stack"][..], &["--stack", "v5"], &["--mtu", "big"], &["--loss", "1.234"], &["--fwd", "x"], &["x"]]
    {
        assert!(matches!(Knobs::parse_flags(bad), Err(KnobError::BadFlag { .. })), "{bad:?}");
    }
    assert!(matches!(Knobs::parse_flags(&["--mtu", "1000"]), Err(KnobError::OutOfRange { .. })));
}

/// A refused value says why (PodUtils/podvm#347): a number out of its
/// range names the value as written, with its unit, and the range; a
/// negative number has its own text; a bad word names the value and lists
/// the words of the flag.
#[test]
fn a_refused_value_names_the_range_or_the_words() {
    let text = |args: &[&str]| Knobs::parse_flags(args).unwrap_err().to_string();
    assert_eq!(text(&["--mtu", "99999"]), "mtu 99999 bytes is out of its range 1280 bytes to 1499 bytes");
    assert_eq!(
        text(&["--mtu", "99999999999999999999999"]),
        "mtu 99999999999999999999999 bytes is out of its range 1280 bytes to 1499 bytes"
    );
    assert_eq!(text(&["--mtu", "1000"]), "mtu 1000 bytes is out of its range 1280 bytes to 1499 bytes");
    assert_eq!(text(&["--loss", "101"]), "loss 101% is out of its range 0.01% to 20%");
    assert_eq!(text(&["--loss", "20.5%"]), "loss 20.5% is out of its range 0.01% to 20%");
    assert_eq!(text(&["--loss", "0"]), "loss 0% is out of its range 0.01% to 20%");
    assert_eq!(text(&["--delay", "20000ms"]), "delay 20000 ms is out of its range 1 ms to 10000 ms");
    assert_eq!(text(&["--rate", "10"]), "rate 10 kbit/s is out of its range 64 kbit/s to 10000000 kbit/s");
    assert_eq!(text(&["--tcp", "22,70000"]), "tcp port 70000 is out of its range 1 to 65535");
    assert_eq!(text(&["--udp", "0"]), "udp port 0 is out of its range 1 to 65535");
    assert_eq!(text(&["--mtu", "big"]), "flag --mtu: the value \"big\" is not a whole number");
    assert_eq!(text(&["--mtu", "-1"]), "flag --mtu: the value -1 is negative; give 1280 bytes to 1499 bytes");
    assert_eq!(text(&["--delay", "-5"]), "flag --delay: the value -5 is negative; give 1 ms to 10000 ms");
    assert_eq!(text(&["--loss", "-1"]), "flag --loss: the value -1 is negative; give 0.01% to 20%");
    assert_eq!(text(&["--loss", "-0.5%"]), "flag --loss: the value -0.5 is negative; give 0.01% to 20%");
    assert_eq!(text(&["--nat", "bogus"]), "flag --nat: the value \"bogus\" is not one of: none, nat44, nat66, both");
    assert_eq!(
        text(&["--policy", "x"]),
        "flag --policy: the value \"x\" is not one of: open, web, web-nodns, only443, blackout"
    );
    assert_eq!(text(&["--nat", "both", "extra"]), "flag extra: the word is not a flag: each value follows its flag");
    // A knob set made in code names the value that it holds.
    let k = Knobs { loss_bp: Some(2_001), ..Knobs::default() };
    assert_eq!(k.check().unwrap_err().to_string(), "loss 20.01% is out of its range 0.01% to 20%");
    assert_eq!(
        Knobs::parse_flags(&["--udp", "none", "--tcp", "22"]).unwrap().policy,
        Policy::Ports { tcp: vec![22], udp: vec![] }
    );
}

/// The text of `--cgnat` names each NAT that the scenario `cgnat` takes.
#[test]
fn the_text_of_cgnat_agrees_with_its_scenario() {
    let e = Knobs::parse_flags(&["--nat", "nat66", "--cgnat"]).unwrap_err().to_string();
    assert_eq!(e, "contradictory knobs: cgnat needs a NAT of ipv4 (--nat nat44 or --nat both)");
    let cgnat = pod_netlab::scenario("cgnat").unwrap().knobs;
    assert!(e.contains(&format!("--nat {}", cgnat.nat.word())), "{e}");
    for nat in ["nat44", "both"] {
        assert!(Knobs::parse_flags(&["--nat", nat, "--cgnat"]).is_ok(), "{nat}");
    }
}

/// `allow` is sand-net's word for `--icmp pass`.
#[test]
fn icmp_takes_allow_as_pass() {
    assert_eq!(Knobs::parse_flags(&["--icmp", "allow"]).unwrap().icmp, Icmp::Pass);
    assert_eq!(Knobs::parse_flags(&["--icmp", "pass"]).unwrap().icmp, Icmp::Pass);
    let e = Knobs::parse_flags(&["--icmp", "open"]).unwrap_err().to_string();
    assert_eq!(e, "flag --icmp: the value \"open\" is not one of: pass, allow, drop");
}

#[test]
fn each_scenario_writes_flags_that_read_back() {
    for s in scenarios() {
        let flags = s.knobs.flags();
        assert_eq!(Knobs::parse_flags(&flags).as_ref(), Ok(&s.knobs), "{}: {flags:?}", s.name);
        // The printed line of a scenario reads back after a shell splits it.
        let line = flags.join(" ");
        let split: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(Knobs::parse_flags(&split).as_ref(), Ok(&s.knobs), "{}: {line:?}", s.name);
    }
    for (tcp, udp) in [(vec![], vec![]), (vec![22], vec![]), (vec![], vec![53]), (vec![22], vec![53])] {
        let k = Knobs { policy: Policy::Ports { tcp, udp }, ..Knobs::default() };
        let line = k.flags().join(" ");
        let split: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(Knobs::parse_flags(&split).as_ref(), Ok(&k), "{line:?}");
    }
    assert!(Knobs::default().flags().is_empty());
}

#[test]
fn a_loss_reads_as_netem_writes_it() {
    assert_eq!(loss_text(500), "5%");
    assert_eq!(loss_text(25), "0.25%");
    assert_eq!(loss_text(1), "0.01%");
    assert_eq!(loss_text(1250), "12.50%");
}

#[test]
fn a_failure_of_the_knobs_is_a_usage_failure_of_the_host() {
    let failure: pod_core::Failure = Knobs { hairpin: true, ..Knobs::default() }.check().unwrap_err().into();
    assert_eq!((failure.stage, failure.class), (pod_core::Stage::Host, pod_core::Class::Usage));
    assert!(failure.text().contains("hairpin needs dnat"), "{}", failure.text());
}
