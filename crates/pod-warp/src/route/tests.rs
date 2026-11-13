use super::*;

/// PodUtils/podnet issue 278: a failed second route removes the first; no
/// half stays.
#[test]
fn a_failed_route_removes_the_routes_before_it() {
    let mut log = Vec::new();
    let err = install_all(&[1, 2, 3], |item, add| {
        log.push((*item, add));
        if *item == 2 && add {
            return Err("no route");
        }
        Ok(())
    })
    .unwrap_err();
    assert_eq!(err, "no route");
    assert_eq!(log, [(1, true), (2, true), (1, false)]);
}

#[test]
fn the_resolv_conf_text() {
    let text = resolv_text("# marker", &["192.0.2.53".parse().unwrap(), "2001:db8::53".parse().unwrap()]);
    assert_eq!(text, "# marker\nnameserver 192.0.2.53\nnameserver 2001:db8::53\n");
}

/// The nameservers: the UDP resolvers on port 53 (3 at most), else the
/// plain DNS of `cloudflare-dns`.
#[test]
fn the_nameservers_of_the_host() {
    let ip = |s: &str| s.parse::<IpAddr>().unwrap();
    assert_eq!(nameservers(&[]), [ip("1.1.1.1"), ip("1.0.0.1")]);
    let list = crate::resolver::parse_resolvers(&[
        "tls://9.9.9.9#dns.quad9.net".into(),
        "192.0.2.1:5353".into(),
        "192.0.2.2,192.0.2.3,192.0.2.4,192.0.2.5".into(),
    ])
    .unwrap();
    assert_eq!(nameservers(&list), [ip("192.0.2.2"), ip("192.0.2.3"), ip("192.0.2.4")]);
    let tls_only = crate::resolver::parse_resolvers(&["tls://9.9.9.9#dns.quad9.net".into()]).unwrap();
    assert_eq!(nameservers(&tls_only), nameservers_of_service());
}

/// A /proc/net/route table: 192.0.2.0/24 on eth0, and optionally the
/// default route via 192.0.2.1 and a /1 of the TUN.
fn table(default: bool) -> Vec<Entry> {
    let mut text = String::from(
        "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\n\
         eth0\t000200C0\t00000000\t0001\t0\t0\t0\t00FFFFFF\n\
         podnet0\t00000000\t00000000\t0001\t0\t0\t0\t00000080\n",
    );
    if default {
        text.push_str("eth0\t00000000\t010200C0\t0003\t0\t0\t100\t00000000\n");
    }
    parse_entries(&text)
}

/// A /proc/net/ipv6_route table: fd00:77::/64 on eth0, ::1 on lo, a /1 of
/// the TUN, and optionally the default route via fd00:77::1.
fn table6(default: bool) -> Vec<Entry6> {
    let mut text = String::from(
        "fd000077000000000000000000000000 40 00000000000000000000000000000000 00 00000000000000000000000000000000 00000100 00000001 00000000 00000001     eth0\n\
         00000000000000000000000000000001 80 00000000000000000000000000000000 00 00000000000000000000000000000000 00000000 00000002 00000000 80200001       lo\n\
         00000000000000000000000000000000 01 00000000000000000000000000000000 00 00000000000000000000000000000000 00000001 00000000 00000000 00000001    podnet0\n",
    );
    if default {
        text.push_str("00000000000000000000000000000000 00 00000000000000000000000000000000 00 fd000077000000000000000000000001 00000400 00000001 00000000 00000003     eth0\n");
    }
    parse_entries6(&text)
}

/// podnet's IPv6-only kit, 0001: an IPv6 front door gets a host route
/// through the IPv6 default route; an address on a connected prefix needs
/// none; with no IPv6 route it is unreachable.
#[test]
fn the_plan_of_each_ipv6_masque_address() {
    let ip = |s: &str| s.parse::<Ipv6Addr>().unwrap();
    let with = table6(true);
    assert_eq!(with.len(), 4);
    assert_eq!(
        plan_exempt6(ip("2606:4700:103::2"), &with, "podnet0"),
        Exempt6::Via { dev: "eth0".into(), gateway: Some(ip("fd00:77::1")) }
    );
    assert_eq!(plan_exempt6(ip("fd00:77::9"), &with, "podnet0"), Exempt6::No);
    assert_eq!(plan_exempt6(ip("::1"), &with, "podnet0"), Exempt6::No);
    let none = table6(false);
    assert_eq!(plan_exempt6(ip("2606:4700:103::2"), &none, "podnet0"), Exempt6::Unreachable);
}

/// PodUtils/podnet issue 105: with no default route, a proxy on the
/// loopback or on the connected subnet needs no host route; an address with
/// no route is unreachable; with a default route, a far address gets a host
/// route through it.
#[test]
fn the_plan_of_each_masque_address() {
    let ip = |s: &str| s.parse::<Ipv4Addr>().unwrap();
    let none = table(false);
    assert_eq!(parse_route_table("Iface\n"), None);
    assert_eq!(plan_exempt(ip("127.0.0.1"), &none, "podnet0"), Exempt::No);
    assert_eq!(plan_exempt(ip("192.0.2.10"), &none, "podnet0"), Exempt::No);
    assert_eq!(plan_exempt(ip("198.51.100.7"), &none, "podnet0"), Exempt::Unreachable);
    let with = table(true);
    assert_eq!(
        plan_exempt(ip("198.51.100.7"), &with, "podnet0"),
        Exempt::Via { dev: "eth0".into(), gateway: Some(ip("192.0.2.1")) }
    );
    assert_eq!(plan_exempt(ip("192.0.2.10"), &with, "podnet0"), Exempt::No);
}

#[test]
fn the_default_gateway_of_the_route_table() {
    let table = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
eth0\t00000000\t010200C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\n\
eth1\t00000000\t0101A8C0\t0003\t0\t0\t50\t00000000\t0\t0\t0\n\
eth0\t000200C0\t00000000\t0001\t0\t0\t0\t00FFFFFF\t0\t0\t0\n";
    assert_eq!(
        parse_route_table(table),
        Some(Gateway { dev: "eth1".into(), gateway: Ipv4Addr::new(192, 168, 1, 1) }),
        "the lowest metric"
    );
    assert_eq!(parse_route_table("Iface\tDestination\n"), None);
}

#[test]
fn masks() {
    assert_eq!(mask(1), Ipv4Addr::new(128, 0, 0, 0));
    assert_eq!(mask(32), Ipv4Addr::BROADCAST);
    assert_eq!(mask(0), Ipv4Addr::UNSPECIFIED);
    assert_eq!(mask(40), Ipv4Addr::BROADCAST, "a prefix past 32 is 32");
}

/// A required address with no route is refused before a route changes.
#[cfg(target_os = "linux")]
#[test]
fn a_required_address_with_no_route_is_refused() {
    let tables = Tables { v4: table(false), v6: Vec::new() };
    let required = vec!["198.51.100.7".parse().unwrap()];
    let err = Routes::setup("podnet0", required, Vec::new(), false, &tables).unwrap_err();
    assert_eq!(err.class, pod_core::Class::Unavailable);
    assert!(err.text().contains("no route to 198.51.100.7"), "{}", err.chain());
    // An optional address with no route is skipped, and a loopback or
    // on-link address needs no route: no ioctl runs.
    let optional = vec!["198.51.100.7".parse().unwrap(), "2001:db8::7".parse().unwrap()];
    let near = vec!["127.0.0.1".parse().unwrap(), "192.0.2.10".parse().unwrap()];
    let routes = Routes::setup("podnet0", near, optional, false, &tables).unwrap();
    assert!(routes.exempted().is_empty());
}
