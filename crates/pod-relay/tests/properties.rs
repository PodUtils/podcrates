//! Properties of relay lists and paths (podssh T-202), each checked on the inputs
//! that proptest makes, 256 by default, and shrunk to the smallest that
//! fails: a list keeps its order and drops its duplicates however each host
//! is spelled, each host that it gives passes `check_host` or is an IPv6
//! address in its canonical form (T-234), and the forward path of each is
//! `/connect/HOST/PORT`; for any text, what parses is such a list, and a
//! forward path that is made has three segments and nothing that could add
//! a query, a fragment or another request.
//!
//! A property that fails writes its input to
//! `tests/properties.proptest-regressions`, committed with the repair:
//! proptest tries those inputs first at each run.

use std::net::Ipv6Addr;

use pod_relay::relay::{Relay, check_host, forward_path, parse_relay_list, url_host};
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::Index;
use proptest::test_runner::FileFailurePersistence;

/// proptest's settings, 256 inputs unless `PROPTEST_CASES` says otherwise,
/// and a failing input kept beside this file.
fn config() -> ProptestConfig {
    ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::WithSource("proptest-regressions"))),
        ..ProptestConfig::default()
    }
}

/// A host as `check_host` takes it, in either case: letters, digits, `.`,
/// `-` and `_`, not starting with `-` or `.`.
fn host() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_][a-zA-Z0-9._-]{0,30}"
}

/// A host as a list spells it, and as the relay keeps it: a name in either
/// case, kept in lower case; or an IPv6 address in brackets, in capitals or
/// not, compressed or with its eight groups, kept with no brackets in its
/// canonical form (T-234).
fn spelled_host() -> impl Strategy<Value = (String, String)> {
    let address = (any::<Ipv6Addr>(), any::<bool>(), any::<bool>()).prop_map(|(a, upper, groups)| {
        let text = match groups {
            true => a.segments().iter().map(|s| format!("{s:x}")).collect::<Vec<_>>().join(":"),
            false => a.to_string(),
        };
        let text = if upper { text.to_ascii_uppercase() } else { text };
        (format!("[{text}]"), a.to_string())
    });
    prop_oneof![host().prop_map(|h| (h.clone(), h.to_ascii_lowercase())), address]
}

/// One item of a list, as a user or the relay's documents spell it, with
/// the relay that podssh must read from it: a scheme or none, a port or
/// none, a trailing `/` or none, and spaces around it.
fn item() -> impl Strategy<Value = (String, Relay)> {
    let scheme = prop::sample::select(vec!["", "https://", "wss://"]);
    let pad = prop::sample::select(vec!["", " ", "  "]);
    (spelled_host(), prop::option::of(1u16..), scheme, any::<bool>(), pad).prop_map(
        |((text_host, host), port, scheme, slash, pad)| {
            let mut text = format!("{pad}{scheme}{text_host}");
            if let Some(port) = port {
                text.push_str(&format!(":{port}"));
            }
            if slash {
                text.push('/');
            }
            text.push_str(pad);
            (text, Relay { host, port: port.unwrap_or(443) })
        },
    )
}

/// The same relay spelled another way: in capitals, with a scheme, its port
/// and a trailing `/`.
fn respelled(relay: &Relay) -> String {
    format!("wss://{}:{}/", url_host(&relay.host).to_ascii_uppercase(), relay.port)
}

/// Whether `host` is as a relay keeps it: a name that passes the check, or
/// an IPv6 address with no brackets in its canonical form.
fn kept(host: &str) -> bool {
    check_host(host).is_ok() || host.parse::<Ipv6Addr>().is_ok_and(|a| a.to_string() == host)
}

proptest! {
    #![proptest_config(config())]

    /// A list keeps its order, drops each later spelling of a relay that it
    /// already holds, and gives hosts that pass the check, each with the
    /// forward path `/connect/HOST/PORT`.
    #[test]
    fn a_list_keeps_its_order_and_drops_its_duplicates(
        items in vec(item(), 1..8),
        again in vec((any::<Index>(), any::<Index>(), any::<bool>()), 0..4),
    ) {
        let mut items = items;
        for (which, to, other_spelling) in &again {
            let (text, relay) = items[which.index(items.len())].clone();
            let text = if *other_spelling { respelled(&relay) } else { text };
            items.insert(to.index(items.len() + 1), (text, relay));
        }
        let list = items.iter().map(|(text, _)| text.as_str()).collect::<Vec<_>>().join(",");
        let hosts = parse_relay_list(&list).map_err(TestCaseError::fail)?;
        let mut want: Vec<Relay> = Vec::new();
        for (_, relay) in &items {
            if !want.contains(relay) {
                want.push(relay.clone());
            }
        }
        prop_assert_eq!(&hosts, &want, "{}", list);
        for relay in &hosts {
            prop_assert!(kept(&relay.host), "{:?}", relay);
            let path = forward_path(&relay.host, relay.port).map_err(TestCaseError::fail)?;
            prop_assert_eq!(path, format!("/connect/{}/{}", relay.host, relay.port));
        }
    }

    /// For any text, a list that parses is not empty, and each of its hosts
    /// is in lower case, passes the check or is an IPv6 address in its
    /// canonical form, and has a port that is not 0.
    #[test]
    fn any_text_parses_only_to_hosts_that_pass_the_check(
        text in prop_oneof![any::<String>(), "[a-zA-Z0-9.:/_@?#%\\[\\] ,-]{0,40}"],
    ) {
        if let Ok(hosts) = parse_relay_list(&text) {
            prop_assert!(!hosts.is_empty(), "{:?}", text);
            for relay in &hosts {
                prop_assert!(kept(&relay.host), "{:?} gave {:?}", text, relay);
                prop_assert_eq!(relay.host.to_ascii_lowercase(), relay.host.clone());
                prop_assert!(relay.port != 0, "{:?} gave {:?}", text, relay);
            }
        }
    }

    /// For any host and port, a forward path that is made is
    /// `/connect/HOST/PORT`: three segments after the first `/`, and no `?`,
    /// `#`, `%`, space or control character, so nothing that a user types
    /// adds a query, a fragment, an escape or a line to the request.
    #[test]
    fn a_forward_path_is_three_segments_and_nothing_more(
        host in prop_oneof![
            any::<String>(),
            "[a-zA-Z0-9.:/_?#% -]{0,20}",
            host(),
            any::<std::net::Ipv6Addr>().prop_map(|a| a.to_string()),
        ],
        port in any::<u16>(),
    ) {
        if let Ok(path) = forward_path(&host, port) {
            prop_assert_eq!(&path, &format!("/connect/{host}/{port}"));
            prop_assert_eq!(path.split('/').count(), 4, "{}", path);
            prop_assert!(!path.contains(['?', '#', '%', ' ']) && !path.chars().any(char::is_control), "{}", path);
            prop_assert!(port != 0);
        }
    }

    /// Each host that the check takes, and each IPv6 address with no
    /// brackets, is a target of a forward path.
    #[test]
    fn each_host_name_and_ipv6_address_is_a_target(
        name in host(),
        address in any::<std::net::Ipv6Addr>(),
        port in 1u16..,
    ) {
        prop_assert!(check_host(&name).is_ok(), "{}", name);
        prop_assert!(forward_path(&name, port).is_ok(), "{}", name);
        prop_assert!(forward_path(&address.to_string(), port).is_ok(), "{}", address);
    }
}
