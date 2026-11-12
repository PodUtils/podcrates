//! The layout and the commands of a lab, with the lessons of sand-net.

use std::net::IpAddr;

use pod_netlab::layout::{NAT64_PREFIX, nat64_address};
use pod_netlab::plan::ruleset::{FORWARD_CHAIN, TABLE, counters_json, forward_rules, router_rules};
use pod_netlab::{Counter, Knobs, Lab, LabError, Layout, Role, Settings, Step, scenario, scenarios};

fn settings() -> Settings {
    Settings { state_dir: "/run/netlab".into(), ping_gid: 1000 }
}

fn lab(name: &str) -> Lab {
    Lab::new("sn", name, scenario(name).unwrap().knobs).unwrap()
}

fn argvs(steps: &[Step]) -> Vec<Vec<String>> {
    steps.iter().filter_map(|s| if let Step::Run { argv, .. } = s { Some(argv.clone()) } else { None }).collect()
}

fn joined(steps: &[Step]) -> Vec<String> {
    argvs(steps).iter().map(|a| a.join(" ")).collect()
}

#[test]
fn the_names_follow_sand_net() {
    let lab = lab("dual-nat");
    assert_eq!(lab.namespace(Role::Guest), "sn-dual-nat-g");
    assert_eq!(lab.namespace(Role::Router), "sn-dual-nat-r");
    assert_eq!(lab.namespace(Role::World), "sn-dual-nat-w");
    for bad in ["", "Dual", "a b", "-a", "a-", "x".repeat(25).as_str(), "a;b"] {
        assert!(
            matches!(Lab::new("sn", bad, Knobs::default()), Err(LabError::BadName { what: "name", .. })),
            "{bad:?}"
        );
    }
    assert!(matches!(Lab::new("toolongpre", "a", Knobs::default()), Err(LabError::BadName { what: "prefix", .. })));
    let bad = Settings { state_dir: "run/netlab".into(), ping_gid: 0 };
    assert!(matches!(lab.up(&bad), Err(LabError::BadStateDir(_))));
}

#[test]
fn the_layout_has_the_addresses_of_issue_178() {
    let lab = lab("dual-nat");
    let has = |role, dev, addr: &str| {
        lab.layout().addresses.iter().any(|a| a.role == role && a.device == dev && a.addr.to_string() == addr)
    };
    assert!(has(Role::Guest, "gr", "10.77.0.2") && has(Role::Guest, "gr", "fd00:77::2"));
    assert!(has(Role::Router, "rg", "10.77.0.1") && has(Role::Router, "rg", "fd00:77::1"));
    assert!(has(Role::Router, "rw", "198.51.100.1") && has(Role::Router, "rw", "2001:db8:100::1"));
    assert!(has(Role::World, "wr", "198.51.100.10") && has(Role::World, "wr", "2001:db8:100::10"));
    let cgnat = self::lab("cgnat");
    assert_eq!(cgnat.layout().roles, [Role::Guest, Role::Router, Role::World, Role::Carrier]);
    assert_eq!(cgnat.namespace(Role::Carrier), "sn-cgnat-c");
    assert_eq!(cgnat.layout().router_out(pod_netlab::Family::V4).to_string(), "100.64.0.2");
    assert_eq!(cgnat.layout().outside, "rc");
}

/// Each address is private (the guest, the pool of NAT64), shared (RFC
/// 6598, cgnat) or for documentation (RFC 5737, RFC 3849).
#[test]
fn each_address_is_in_a_range_for_labs() {
    let allowed = |addr: IpAddr| match addr {
        IpAddr::V4(a) => {
            let o = a.octets();
            (o[0] == 10 && o[1] == 77) || (o[0] == 198 && o[1] == 51 && o[2] == 100) || (o[0] == 100 && o[1] == 64)
        }
        IpAddr::V6(a) => {
            let s = a.segments();
            (s[0] == 0xfd00 && s[1] == 0x77) || (s[0] == 0x2001 && s[1] == 0xdb8)
        }
    };
    for s in scenarios() {
        let lab = Lab::new("sn", s.name, s.knobs).unwrap();
        for a in &lab.layout().addresses {
            assert!(allowed(a.addr), "{}: {}", s.name, a.addr);
        }
    }
    // The prefix of tayga is not the well-known 64:ff9b::/96 (F18).
    assert_eq!(NAT64_PREFIX.segments()[..2], [0x2001, 0xdb8]);
    assert_eq!(nat64_address("198.51.100.10".parse().unwrap()).to_string(), "2001:db8:64::c633:640a");
}

#[test]
fn each_step_is_an_argument_list_of_a_known_program() {
    for s in scenarios() {
        let lab = Lab::new("sn", s.name, s.knobs).unwrap();
        for steps in [lab.up(&settings()).unwrap(), lab.down(&settings()).unwrap()] {
            for argv in argvs(&steps) {
                assert!(matches!(argv[0].as_str(), "ip" | "tc"), "{}: {argv:?}", s.name);
                let inner = if argv[1] == "netns" && argv[2] == "exec" { argv[4].as_str() } else { "ip" };
                assert!(matches!(inner, "ip" | "sysctl" | "nft" | "ping" | "tayga"), "{}: {argv:?}", s.name);
                for arg in &argv {
                    assert!(!arg.is_empty() && !arg.contains(['\n', ';', '|', '&', '`', '$']), "{}: {argv:?}", s.name);
                }
            }
        }
    }
}

#[test]
fn up_sets_the_sysctls_of_the_lessons() {
    let up = joined(&lab("broken-v4-reject").up(&settings()).unwrap());
    // L1: the router's ICMP errors are not rate-limited.
    assert!(up.contains(&"ip netns exec sn-broken-v4-reject-r sysctl -w net.ipv4.icmp_ratelimit=0".to_string()));
    assert!(up.contains(&"ip netns exec sn-broken-v4-reject-r sysctl -w net.ipv6.icmp.ratelimit=0".to_string()));
    // F20: one contiguous range, the probing gid.
    assert!(
        up.contains(&"ip netns exec sn-broken-v4-reject-g sysctl -w net.ipv4.ping_group_range=1000 1000".to_string())
    );
    let v6 = joined(&lab("v6-only-vps").up(&settings()).unwrap());
    assert!(v6.contains(&"ip -n sn-v6-only-vps-g addr del 127.0.0.1/8 dev lo".to_string()));
    let v4 = joined(&lab("v4-vps").up(&settings()).unwrap());
    let disable = v4.iter().position(|l| l.ends_with("net.ipv6.conf.default.disable_ipv6=1")).unwrap();
    let veth = v4.iter().position(|l| l.starts_with("ip link add gr")).unwrap();
    assert!(disable < veth, "the guest's device takes the default");
    assert!(!v4.iter().any(|l| l.contains("fd00:77::2/64")));
}

#[test]
fn one_packet_goes_along_each_hop_before_the_netem_queues() {
    let up = joined(&lab("lossy-mobile").up(&settings()).unwrap());
    let first_netem = up.iter().position(|l| l.starts_with("tc ")).unwrap();
    let hops: Vec<usize> = up.iter().enumerate().filter(|(_, l)| l.contains(" ping -c 1 ")).map(|(i, _)| i).collect();
    // The guest and the world on each family, the router to the world.
    assert_eq!(hops.len(), 6);
    assert!(hops.iter().all(|i| *i < first_netem), "L3");
    assert!(
        up.contains(
            &"tc -n sn-lossy-mobile-r qdisc add dev rg root netem delay 40ms loss 5% rate 4000kbit".to_string()
        )
    );
    assert!(
        up.contains(
            &"tc -n sn-lossy-mobile-r qdisc add dev rw root netem delay 40ms loss 5% rate 4000kbit".to_string()
        )
    );
}

#[test]
fn the_mtu_is_on_each_route_of_the_router() {
    let up = joined(&lab("pmtu-blackhole").up(&settings()).unwrap());
    let r = "ip -n sn-pmtu-blackhole-r";
    for line in [
        format!("{r} -4 route add default via 198.51.100.10 mtu 1400"),
        format!("{r} -6 route add default via 2001:db8:100::10 mtu 1400"),
        format!("{r} -4 route replace 10.77.0.0/24 dev rg proto kernel scope link src 10.77.0.1 mtu 1400"),
        format!("{r} -6 route replace fd00:77::/64 dev rg proto kernel metric 256 mtu 1400"),
        format!("{r} -4 route replace 198.51.100.0/24 dev rw proto kernel scope link src 198.51.100.1 mtu 1400"),
    ] {
        assert!(up.contains(&line), "{line}");
    }
}

#[test]
fn the_table_has_the_nine_named_counters() {
    let json = counters_json();
    for c in Counter::ALL {
        assert!(json.contains(&format!(r#""name":"{}""#, c.name())), "{json}");
    }
    let names: Vec<&str> = Counter::ALL.iter().map(|c| c.name()).collect();
    assert_eq!(names, ["return", "forwarded", "inbound", "broken", "ping", "open", "tcp", "udp", "dropped"]);
    let up = lab("dual-nat").up(&settings()).unwrap();
    let nft: Vec<&Step> =
        up.iter().filter(|s| matches!(s, Step::Run { argv, .. } if argv.contains(&"nft".to_string()))).collect();
    assert!(
        matches!(nft[0], Step::Run { argv, stdin: Some(s), .. } if argv.ends_with(&["-j".into(), "-f".into(), "-".into()]) && *s == json)
    );
    assert!(matches!(nft[1], Step::Run { stdin: Some(s), .. } if s.starts_with(&format!("table inet {TABLE} {{"))));
}

#[test]
fn no_chain_is_named_fwd() {
    for s in scenarios() {
        let text = router_rules(&Lab::new("sn", s.name, s.knobs).unwrap());
        for line in text.lines().map(str::trim).filter(|l| l.starts_with("chain ")) {
            assert_ne!(line.split_whitespace().nth(1), Some("fwd"), "F9: {}", s.name);
        }
        assert!(text.contains(&format!("chain {FORWARD_CHAIN} {{")));
    }
}

/// L5: a forwarded DNAT flow is new; `ct status dnat` passes before each
/// rule that drops a new flow. L6: with NAT on a family, a new inbound flow
/// of it is dropped.
#[test]
fn dnat_passes_first_and_nat_drops_new_inbound_flows() {
    for s in scenarios() {
        let lab = Lab::new("sn", s.name, s.knobs.clone()).unwrap();
        let rules = forward_rules(&lab);
        let dnat = rules.iter().position(|r| r.starts_with("ct status dnat")).unwrap();
        let first_drop = rules.iter().position(|r| r.contains("ct state new") && r.ends_with(" drop"));
        assert!(first_drop.is_none_or(|d| dnat < d), "{}: {rules:#?}", s.name);
        let out = lab.layout().outside;
        for family in pod_netlab::Family::BOTH {
            let verdict = if s.knobs.nat_on(family) { "drop" } else { "accept" };
            let rule = format!(
                "iifname \"{out}\" meta nfproto {} ct state new counter name \"inbound\" {verdict}",
                family.nfproto()
            );
            assert!(rules.contains(&rule), "{}: {rule}", s.name);
        }
        assert_eq!(rules.last().unwrap(), "counter name \"dropped\" drop");
    }
}

#[test]
fn the_rules_of_each_knob_are_there() {
    let text = |name| router_rules(&lab(name));
    assert!(text("symmetric-nat").contains("oifname \"rw\" meta nfproto ipv4 masquerade fully-random"));
    assert!(text("web-only").contains("iifname \"rg\" tcp dport { 80, 443 } counter name \"tcp\" accept"));
    assert!(text("web-only").contains("iifname \"rg\" udp dport { 53 } counter name \"udp\" accept"));
    assert!(
        text("broken-v6-reject")
            .contains("meta nfproto ipv6 counter name \"broken\" reject with icmpx type port-unreachable")
    );
    assert!(text("pmtu-blackhole").contains("icmpv6 type packet-too-big counter name \"dropped\" drop"));
    assert!(!text("pmtu-ok").contains("packet-too-big"));
    assert!(
        text("inbound-dnat").contains("iifname \"rw\" ip daddr 198.51.100.1 tcp dport 2222 dnat ip to 10.77.0.2:2200")
    );
    let hairpin = text("hairpin-nat");
    assert!(hairpin.contains("iifname \"rg\" ip6 daddr 2001:db8:100::1 tcp dport 2222 dnat ip6 to [fd00:77::2]:2200"));
    assert!(hairpin.contains("oifname \"rg\" ip saddr 10.77.0.0/24 ct status dnat masquerade"));
    let nat64 = text("nat64-dns64");
    assert!(
        nat64.contains("oifname \"rw\" meta nfproto ipv4 masquerade")
            || nat64.contains("ip saddr 192.168.255.0/24 masquerade")
    );
    assert!(nat64.contains("iifname \"nat64\" accept"));
}

#[test]
fn nat64_runs_tayga_in_the_router_with_its_own_files() {
    let up = lab("nat64-web").up(&settings()).unwrap();
    let conf = up.iter().find_map(|s| {
        if let Step::Write { path, contents } = s {
            path.ends_with("-tayga.conf").then_some(contents.clone())
        } else {
            None
        }
    });
    assert_eq!(
        conf.unwrap(),
        "tun-device nat64\nipv4-addr 192.168.255.1\nprefix 2001:db8:64::/96\ndynamic-pool 192.168.255.0/24\n"
    );
    let lines = joined(&up);
    assert!(lines.contains(&"ip netns exec sn-nat64-web-r tayga --config /run/netlab/sn-nat64-web-tayga.conf --pidfile /run/netlab/sn-nat64-web-tayga.pid".to_string()));
    let down = lab("nat64-web").down(&settings()).unwrap();
    assert_eq!(down[0], Step::Stop { pidfile: "/run/netlab/sn-nat64-web-tayga.pid".into() });
}

#[test]
fn the_guest_resolves_through_the_world() {
    let resolv = |name| {
        lab(name).up(&settings()).unwrap().into_iter().find_map(|s| match s {
            Step::Write { path, contents } if path == format!("/etc/netns/sn-{name}-g/resolv.conf") => Some(contents),
            _ => None,
        })
    };
    assert_eq!(
        resolv("dual-nat").unwrap(),
        "nameserver 198.51.100.10\nnameserver 2001:db8:100::10\noptions timeout:2 attempts:2\n"
    );
    assert_eq!(resolv("v6-only-vps").unwrap(), "nameserver 2001:db8:100::10\noptions timeout:2 attempts:2\n");
}

#[test]
fn down_removes_each_namespace_and_may_fail() {
    let down = lab("cgnat").down(&settings()).unwrap();
    let dels: Vec<String> = down
        .iter()
        .filter_map(|s| match s {
            Step::Run { argv, must: false, .. } => Some(argv.join(" ")),
            Step::Run { must: true, .. } => panic!("a step of down must not stop it: {s:?}"),
            _ => None,
        })
        .collect();
    assert_eq!(
        dels,
        ["ip netns del sn-cgnat-c", "ip netns del sn-cgnat-w", "ip netns del sn-cgnat-r", "ip netns del sn-cgnat-g"]
    );
    assert!(down.contains(&Step::Remove { path: "/etc/netns/sn-cgnat-g".into() }));
}

#[test]
fn the_carrier_has_its_own_table_and_routes() {
    let up = lab("cgnat-dnat").up(&settings()).unwrap();
    let carrier = up.iter().find_map(|s| match s {
        Step::Run { argv, stdin: Some(text), .. } if argv[3] == "sn-cgnat-dnat-c" => Some(text.clone()),
        _ => None,
    });
    let carrier = carrier.unwrap();
    assert!(carrier.contains("oifname \"cw\" meta nfproto ipv4 masquerade"));
    assert!(carrier.contains("iifname \"cw\" meta nfproto ipv4 ct state new drop"));
    let lines = joined(&up);
    assert!(lines.contains(&"ip -n sn-cgnat-dnat-c -4 route add 10.77.0.0/24 via 100.64.0.2".to_string()));
    assert!(lines.contains(&"ip -n sn-cgnat-dnat-r -4 route add default via 100.64.0.1".to_string()));
    assert_eq!(Layout::world_gateway(pod_netlab::Family::V4).to_string(), "198.51.100.1");
}
