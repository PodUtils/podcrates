//! The tests of the relay hosts, apart from `relay.rs` for its length: the
//! budgets, the order of the hosts, their spellings, the paths, and the
//! words of an IPv6 target.

use super::*;

#[test]
fn each_budget_is_under_its_cap() {
    // The byte budget stays under the cap, with room for what the relay
    // can hold queued.
    assert!(SESSION_BYTE_BUDGET < session_byte_cap() && session_byte_cap() - SESSION_BYTE_BUDGET >= 2 << 20);
    assert!(SESSION_TIME_BUDGET < session_time_cap());
    // The idle cut is far shorter than either: keepalives handle it.
    assert!(std::time::Duration::from_secs(relay_idle_secs()) < SESSION_TIME_BUDGET);
}

#[test]
fn the_budget_variable_lowers_the_budget_and_never_raises_it() {
    let ctx = crate::testing::context();
    let byte_budget = |env| byte_budget(&ctx, env);
    assert_eq!(byte_budget(None), SESSION_BYTE_BUDGET);
    assert_eq!(byte_budget(Some("3000000")), 3_000_000);
    assert_eq!(byte_budget(Some(" 4194304 ")), 4 << 20);
    assert_eq!(byte_budget(Some("999999999999")), SESSION_BYTE_BUDGET, "never above the budget");
    for ignored in ["", "abc", "-5", "1000", "3e6"] {
        assert_eq!(byte_budget(Some(ignored)), SESSION_BYTE_BUDGET, "{ignored:?}");
    }
}

#[test]
fn the_default_relay_comes_first_then_pool_alternates() {
    let pool: Vec<String> = ["a.example", "b.example", "tcp.ssh.relay.ajam.dev", "c.example", "d.example"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let list = select_relays(None, None, &pool).unwrap();
    assert!(!list.explicit);
    let hosts: Vec<&str> = list.hosts.iter().map(|r| r.host.as_str()).collect();
    assert_eq!(hosts, vec![default_relay_host(), "a.example", "b.example", "c.example"]);
    assert_eq!(list.primary().host, default_relay_host());
    let alone = select_relays(None, Some("  ".into()), &[]).unwrap();
    assert_eq!(alone.hosts, vec![Relay { host: default_relay_host().into(), port: 443 }]);
}

/// podssh T-346: the hosts of other deployments follow the default list, each
/// once; a list that the user named takes none.
#[test]
fn the_pool_then_other_deployments_follow_the_default_host() {
    let other = |h: &str| Relay { host: h.into(), port: 443 };
    let default = select_relays(None, None, &["a.ssh.relay.ajam.dev".into()]).unwrap();
    let others = vec![other("relay.example.org"), other("a.ssh.relay.ajam.dev"), other("relay.example.org")];
    let list = with_others(default, others);
    let hosts: Vec<&str> = list.hosts.iter().map(|r| r.host.as_str()).collect();
    assert_eq!(hosts, vec![default_relay_host(), "a.ssh.relay.ajam.dev", "relay.example.org"]);
    let named = with_others(select_relays(Some("mine.example"), None, &[]).unwrap(), vec![other("b.example")]);
    assert_eq!(named.hosts, vec![other("mine.example")], "a list that the user named is the relay alone");
}

#[test]
fn the_flag_beats_the_environment_and_lists_are_kept_as_given() {
    let list = select_relays(Some("flag.example"), Some("env.example".into()), &["pool.example".into()]).unwrap();
    assert!(list.explicit);
    assert_eq!(list.hosts, vec![Relay { host: "flag.example".into(), port: 443 }]);
    let list =
        select_relays(None, Some("one.example:8443, two.example ,three.example,one.example:8443".into()), &[]).unwrap();
    let hosts: Vec<(&str, u16)> = list.hosts.iter().map(|r| (r.host.as_str(), r.port)).collect();
    assert_eq!(hosts, vec![("one.example", 8443), ("two.example", 443), ("three.example", 443)]);
    assert!(select_relays(Some(", ,"), None, &[]).is_err());
    assert!(select_relays(Some("good.example,bad host!"), None, &[]).is_err());
    assert!(select_relays(Some("not a host"), None, &[]).is_err(), "spaces never separate hosts");
}

#[test]
fn relay_spellings_from_the_relays_documents_are_accepted() {
    for v in ["wss://tcp-1.ssh.relay.example/", "https://tcp-1.ssh.relay.example", "TCP-1.ssh.relay.example"] {
        assert_eq!(parse_relay(v).unwrap().host, "tcp-1.ssh.relay.example", "{v}");
    }
    assert!(parse_relay("relay.example:0").is_err());
    assert!(parse_relay("relay.example:http").is_err());
    assert!(parse_relay("relay example").is_err());
}

#[test]
fn forward_paths_cannot_be_bent_by_the_target() {
    assert_eq!(forward_path("github.com", 22).unwrap(), "/connect/github.com/22");
    assert_eq!(forward_path("10.1.2.3", 2222).unwrap(), "/connect/10.1.2.3/2222");
    for bad in ["", "a/b", "a?b", "a#b", "a b", "a\r\nX: y", "-oProxy", ".hidden", "[::1]", "a@b"] {
        assert!(forward_path(bad, 22).is_err(), "{bad:?}");
    }
    assert!(forward_path("github.com", 0).is_err());
}

/// The paths of the contract's table of addresses.
#[test]
fn the_paths_are_the_published_ones() {
    assert_eq!(forward_path("github.com", 22).unwrap(), "/connect/github.com/22");
    assert_eq!(node_path("podssh").unwrap(), "/v1/node/podssh");
    assert_eq!(operator_path("podssh").unwrap(), "/v1/connect/podssh");
}

/// No name or host bends a path: each that could is refused before a path
/// exists, and never encoded, as an encoded `/` hides another path.
#[test]
fn names_cannot_bend_the_path() {
    let long_name = "n".repeat(129);
    let long_host = "h".repeat(254);
    for bad in ["a/b", "a?b", "a#b", "..", ".", "-x", "a b", "a\r\nX-Injected: 1", ""] {
        assert!(forward_path(bad, 22).is_err(), "{bad:?}");
        assert!(node_path(bad).is_err(), "{bad:?}");
        assert!(operator_path(bad).is_err(), "{bad:?}");
    }
    assert!(forward_path(&long_host, 22).is_err());
    assert!(node_path(&long_name).is_err() && operator_path(&long_name).is_err());
    assert_eq!(node_path("a.b_c-1").unwrap(), "/v1/node/a.b_c-1");
    assert_eq!(operator_path("a.b_c-1").unwrap(), "/v1/connect/a.b_c-1");
    assert_eq!(forward_path("2001:db8::1", 22).unwrap(), "/connect/2001:db8::1/22");
    assert!(forward_path("github.com", 0).is_err());
}

/// A pair name goes into `/v1/node/<name>` and `/v1/connect/<name>`:
/// refused, never encoded, when it could change the path.
#[test]
fn node_names_cannot_bend_the_path() {
    for good in ["podssh", "a.b_c-1", "A9", &"n".repeat(128)] {
        assert!(check_node_name(good).is_ok(), "{good:?}");
    }
    let long = "n".repeat(129);
    for bad in ["", "a/b", "a?b", "a#b", "..", ".", "-x", ".x", "a b", "a\r\nX: y", "a%2fb", "a:b", "é", &long] {
        assert!(check_node_name(bad).is_err(), "{bad:?}");
    }
}

/// The relay takes the bare literal (as measured); brackets are
/// for the command line, never for the path.
#[test]
fn ipv6_literals_are_targets_and_zones_are_not() {
    assert_eq!(forward_path("2001:db8::1", 22).unwrap(), "/connect/2001:db8::1/22");
    assert_eq!(forward_path("::1", 8079).unwrap(), "/connect/::1/8079");
    // Eight groups: one address, not an address and a port.
    assert_eq!(forward_path("2001:db8::1:22", 22).unwrap(), "/connect/2001:db8::1:22/22");
    assert_eq!(forward_path("::ffff:192.0.2.1", 22).unwrap(), "/connect/::ffff:192.0.2.1/22");
    for bad in
        ["[2001:db8::1]", "fe80::1%eth0", "fe80::1%25eth0", "2001:db8::1/64", "2001:db8:::1", "a:b", "host:22", ":::"]
    {
        assert!(forward_path(bad, 22).is_err(), "{bad:?}");
        assert!(check_target(bad).is_err(), "{bad:?}");
    }
    assert!(is_ipv6_literal("2001:db8::1") && !is_ipv6_literal("[2001:db8::1]") && !is_ipv6_literal("192.0.2.1"));
    // A relay host is an IPv6 address in brackets only (T-234): a bare one
    // has no port that can be told from its last group; a zone stays out.
    assert_eq!(parse_relay("[2001:db8::1]"), Ok(Relay { host: "2001:db8::1".into(), port: 443 }));
    for bad in ["2001:db8::1", "::1", "2001:db8::1:22", "[fe80::1%eth0]", "[fe80::1%25eth0]:443", "[192.0.2.1]"] {
        assert!(parse_relay(bad).is_err(), "{bad:?}");
    }
    let bare = parse_relay("https://2001:db8::1:8443").unwrap_err();
    assert!(bare.contains("in brackets"), "{bare}");
}

/// podnet's `relay_urls`: a relay URL and the relay that it names; each
/// spelling that names no relay is refused. podnet refuses a host with no
/// scheme, which podssh's spellings take
/// (`relay_spellings_from_the_relays_documents_are_accepted`); a plain URL
/// names a stand-in relay on the loopback with the feature `plain-ws` only.
#[test]
fn relay_urls() {
    let relay = parse_relay("https://tcp.ssh.relay.ajam.dev/").unwrap();
    assert_eq!(relay.url(), "https://tcp.ssh.relay.ajam.dev");
    assert_eq!(relay.port, 443);
    let relay = parse_relay("wss://relay.example:8443").unwrap();
    assert_eq!(relay.url(), "https://relay.example:8443");
    let plain = parse_relay("http://127.0.0.1:9");
    if cfg!(feature = "plain-ws") {
        assert_eq!(plain, Ok(Relay { host: "127.0.0.1".into(), port: 9 }));
        assert_eq!(parse_relay("ws://localhost"), Ok(Relay { host: "localhost".into(), port: 80 }));
    } else {
        assert!(plain.unwrap_err().contains("plain-ws"));
    }
    for bad in [
        "http://relay.example",
        "ws://192.0.2.1:80",
        "https://relay.example/connect",
        "https://user@relay.example",
        "https://",
        "https://relay.example:x",
        "https://::1",
        "https://[::1",
        "https://[::1]x",
        "http://[2001:db8::1]",
    ] {
        assert!(parse_relay(bad).is_err(), "{bad}");
    }
}

/// podnet's issue 273: an IPv6 relay host in brackets keeps its port.
#[test]
fn an_ipv6_relay_host_keeps_its_port() {
    let relay = parse_relay("https://[::1]:8443").unwrap();
    assert_eq!((relay.host.as_str(), relay.port), ("::1", 8443));
    assert_eq!(relay.url(), "https://[::1]:8443");
    if cfg!(feature = "plain-ws") {
        let relay = parse_relay("http://[::1]:8080").unwrap();
        assert_eq!((relay.host.as_str(), relay.port), ("::1", 8080));
    }
    let relay = parse_relay("https://[2001:db8::1]").unwrap();
    assert_eq!((relay.host.as_str(), relay.port), ("2001:db8::1", 443));
    assert_eq!(relay.url(), "https://[2001:db8::1]");
}

/// Each spelling of one IPv6 relay is one relay with one name, in
/// brackets: the name of its token, of its `Host`, of its cache files, and
/// a name that parses back to it, as a stored pair and a cached token's
/// minting relay are read back.
#[test]
fn an_ipv6_relay_has_one_name_in_brackets() {
    for spelling in ["[2001:DB8::1]:8443", "https://[2001:db8:0:0::1]:8443/", "wss://[2001:0db8::0001]:8443"] {
        let relay = parse_relay(spelling).unwrap();
        assert_eq!(relay, Relay { host: "2001:db8::1".into(), port: 8443 }, "{spelling}");
        assert_eq!(crate::token::relay_name(&relay), "[2001:db8::1]:8443");
        assert_eq!(crate::pool::host_header(&relay), "[2001:db8::1]:8443");
        assert_eq!(crate::token::token_key(&relay), "[2001:db8::1]:8443");
        assert_eq!(
            crate::cache::file_name(&crate::token::token_key(&relay)),
            "relay-token-_5b2001_3adb8_3a_3a1_5d_3a8443.json"
        );
        assert_eq!(crate::pool::file_name(&relay.host), "relay-pool-2001_3adb8_3a_3a1.json");
        assert_eq!(parse_relay(&relay.name()), Ok(relay.clone()), "the name parses back");
        let key = crate::token::token_key(&relay);
        assert!(crate::token::usable(&key, Some(&relay.name())), "the cached token of the relay is sent to it");
    }
    let list = parse_relay_list("[::1]:8443, https://[0::1]:8443, [::1]").unwrap();
    assert_eq!(list, vec![Relay { host: "::1".into(), port: 8443 }, Relay { host: "::1".into(), port: 443 }]);
    assert_eq!((list[1].name(), list[1].url()), ("[::1]".to_string(), "https://[::1]".to_string()));
    assert_eq!((url_host("::1"), url_host("relay.example")), ("[::1]".to_string(), "relay.example".to_string()));
}

#[test]
fn ipv6_note_only_for_an_ipv6_target_that_the_relay_closed_at_once() {
    assert!(ipv6_note(true, "target closed before sending anything").is_some());
    assert!(ipv6_note(false, "target closed before sending anything").is_none());
    assert!(ipv6_note(true, "idle timeout").is_none());
    // The role's words (podssh T-253), and the roads that the note names.
    let note = ipv6_note(true, &format!("connect failed: {NO_IPV6_ROUTE}")).unwrap();
    assert!(note.contains("podssh relay serve") && note.contains("node://NAME") && note.contains("warp://"));
}

/// The Close of a relay that reached no IPv6 host (podssh T-253): the default
/// relay's, and the role's after a lazy dial; no other.
#[test]
fn a_close_that_says_the_relay_reached_no_ipv6_host() {
    assert!(unreached_ipv6(Some(1011), "target closed before sending anything"));
    assert!(unreached_ipv6(Some(1011), &format!("connect failed: {NO_IPV6_ROUTE} ([2001:db8::7]:22: x)")));
    for (code, reason) in [
        (Some(1000), "target closed before sending anything"),
        (Some(1011), "connect failed: refused"),
        (Some(1011), "write failed: reset"),
        (None, "target closed before sending anything"),
    ] {
        assert!(!unreached_ipv6(code, reason), "{code:?} {reason}");
    }
}
