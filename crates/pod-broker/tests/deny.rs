//! The denies: each fixed deny in IPv4 and IPv6, the addresses inside an
//! IPv6 address, the names, the port, the addresses of the host and the
//! deny file.

use std::net::IpAddr;

use pod_broker::DenyList;
use pod_broker::deny::{FIXED_NETS, check_port};
use pod_core::Class;

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

#[test]
fn each_fixed_deny_holds_its_addresses() {
    let fixed = DenyList::fixed();
    let cases = [
        ("169.254.169.254", "fixed:metadata"),
        ("168.63.129.16", "fixed:metadata"),
        ("100.100.100.200", "fixed:metadata"),
        ("169.254.170.2", "fixed:metadata"),
        ("fd00:ec2::254", "fixed:metadata"),
        ("0.0.0.0", "fixed:unspecified"),
        ("10.0.0.1", "fixed:private"),
        ("172.16.0.1", "fixed:private"),
        ("172.31.255.255", "fixed:private"),
        ("192.168.1.1", "fixed:private"),
        ("100.64.0.1", "fixed:cgnat"),
        ("100.127.255.254", "fixed:cgnat"),
        ("127.0.0.1", "fixed:loopback"),
        ("127.255.0.9", "fixed:loopback"),
        ("169.254.1.1", "fixed:link-local"),
        ("192.0.0.170", "fixed:special"),
        ("224.0.0.251", "fixed:multicast"),
        ("239.255.255.250", "fixed:multicast"),
        ("255.255.255.255", "fixed:reserved"),
        ("::", "fixed:unspecified"),
        ("::1", "fixed:loopback"),
        ("fc00::1", "fixed:private"),
        ("fd12:3456::1", "fixed:private"),
        ("fe80::1", "fixed:link-local"),
        ("fec0::1", "fixed:site-local"),
        ("ff02::1", "fixed:multicast"),
    ];
    for (address, rule) in cases {
        assert_eq!(fixed.check_ip(ip(address)), Some(rule), "{address}");
    }
    for public in ["8.8.8.8", "93.184.215.14", "172.32.0.1", "100.128.0.1", "2606:4700::1111", "192.0.2.10"] {
        assert_eq!(fixed.check_ip(ip(public)), None, "{public}");
    }
    assert_eq!(fixed.len(), FIXED_NETS.len() + pod_broker::deny::FIXED_NAMES.len());
}

/// An IPv4 address inside an IPv6 address is denied as the IPv4 address.
#[test]
fn a_mapped_address_is_denied_as_its_ipv4_address() {
    let fixed = DenyList::fixed();
    for (address, rule) in [
        ("::ffff:169.254.169.254", "fixed:metadata"),
        ("::ffff:a9fe:a9fe", "fixed:metadata"),
        ("::ffff:10.0.0.1", "fixed:private"),
        ("::ffff:127.0.0.1", "fixed:loopback"),
        ("::127.0.0.1", "fixed:loopback"),
        ("64:ff9b::7f00:1", "fixed:loopback"),
        ("2002:0a00:0001::1", "fixed:private"),
    ] {
        assert_eq!(fixed.check_ip(ip(address)), Some(rule), "{address}");
    }
}

#[test]
fn the_names_of_metadata_and_of_loopback_are_denied() {
    let fixed = DenyList::fixed();
    assert_eq!(fixed.check_name("metadata.google.internal"), Some("fixed:metadata"));
    assert_eq!(fixed.check_name("metadata.goog"), Some("fixed:metadata"));
    assert_eq!(fixed.check_name("localhost"), Some("fixed:loopback"));
    assert_eq!(fixed.check_name("a.b.localhost"), Some("fixed:loopback"));
    assert_eq!(fixed.check_name("notlocalhost"), None);
    assert_eq!(fixed.check_name("metadata.google.internal.example"), None);
}

#[test]
fn port_25_is_denied() {
    assert_eq!(check_port(25), Some("fixed:smtp".into()));
    assert_eq!(check_port(587), None);
    assert_eq!(check_port(443), None);
}

#[test]
fn the_addresses_of_the_host_are_denied() {
    let host = DenyList::host_addresses(&[ip("203.0.113.7"), ip("::ffff:198.51.100.3"), ip("2001:db8::7")]);
    assert_eq!(host.check_ip(ip("203.0.113.7")), Some("host:address"));
    assert_eq!(host.check_ip(ip("198.51.100.3")), Some("host:address"));
    assert_eq!(host.check_ip(ip("::ffff:203.0.113.7")), Some("host:address"));
    assert_eq!(host.check_ip(ip("2001:db8::7")), Some("host:address"));
    assert_eq!(host.check_ip(ip("203.0.113.8")), None);
}

#[test]
fn a_deny_file_has_nets_addresses_and_names_with_their_lines() {
    let text = "# the deny list of the host\n\n203.0.113.0/24\n2001:db8::/32  # a net of v6\n198.51.100.5\n\
                evil.example\n*.tracker.example\n";
    let list = DenyList::parse(text, "file").unwrap();
    assert_eq!(list.len(), 5);
    assert_eq!(list.check_ip(ip("203.0.113.200")), Some("file:3"));
    assert_eq!(list.check_ip(ip("2001:db8:1::1")), Some("file:4"));
    assert_eq!(list.check_ip(ip("198.51.100.5")), Some("file:5"));
    assert_eq!(list.check_ip(ip("198.51.100.6")), None);
    assert_eq!(list.check_name("evil.example"), Some("file:6"));
    assert_eq!(list.check_name("a.tracker.example"), Some("file:7"));
    assert_eq!(list.check_name("tracker.example"), None);
    let bad = DenyList::parse("10.0.0.0/8\n10.0.0.0/99\n", "file").unwrap_err();
    assert_eq!(bad.class, Class::Usage);
    assert!(bad.text().contains("line 2"), "{bad}");
    assert!(DenyList::parse("a b\n", "file").is_err());
}

#[test]
fn a_deny_file_reads_from_its_path() {
    let dir = std::env::temp_dir().join(format!("pod-broker-deny-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("deny.txt");
    std::fs::write(&path, "203.0.113.0/24\n").unwrap();
    assert_eq!(DenyList::read(&path).unwrap().check_ip(ip("203.0.113.1")), Some("file:1"));
    assert!(DenyList::read(&dir.join("none.txt")).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}
