//! The rules of the public carriers: the kinds `wisp-public`, `proxy-list`
//! and `proxy-node` hold only hosts that a source names, on port 443 with
//! TLS, under no refused suffix, and with no VMess.

use std::collections::HashSet;

use pod_services::{Origin, Service};

const KINDS: [&str; 3] = ["wisp-public", "proxy-list", "proxy-node"];
// A host, an SNI or a Host header under one of these is never named.
const REFUSED: [&str; 6] = [".edu", ".k12.", ".ac.", ".sch.", ".gov", ".mil"];
const REFUSED_WORDS: [&str; 6] = ["church", "school", "college", "university", "academy", "parish"];

fn services() -> Vec<&'static Service> {
    KINDS.iter().flat_map(|k| pod_services::of_kind(k)).collect()
}

fn refused(name: &str) -> bool {
    let name = format!("{}.", name.to_ascii_lowercase());
    REFUSED.iter().any(|s| name.contains(s)) || REFUSED_WORDS.iter().any(|w| name.contains(w))
}

fn param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|p| p.strip_prefix(key)?.strip_prefix('='))
}

/// The scheme, the host and the query of a share link.
fn parts(link: &str) -> (&str, &str, &str) {
    let (scheme, rest) = link.split_once("://").expect("a share link has a scheme");
    let (auth, query) = rest.split_once('?').unwrap_or((rest, ""));
    let host_port = auth.rsplit_once('@').expect("a share link has a user part").1;
    let host = host_port.rsplit_once(':').expect("a share link has a port").0;
    (scheme, host, query)
}

#[test]
fn each_value_of_a_public_carrier_has_a_source_and_a_date() {
    for service in services() {
        let sources = service
            .endpoints()
            .iter()
            .map(|e| e.source())
            .chain(service.facts().iter().map(|f| f.source()))
            .chain(service.rank().map(|r| r.source()));
        for source in sources {
            assert!(source.read().year() >= 2026, "{}", service.id());
            match source.origin() {
                Origin::Url(u) => assert!(u.starts_with("https://"), "{}: {u}", service.id()),
                Origin::Measured(m) | Origin::Ruling(m) => assert!(!m.is_empty(), "{}", service.id()),
            }
        }
        // A host of another party is named by a page, not by a ruling.
        for endpoint in service.endpoints() {
            assert!(matches!(endpoint.source().origin(), Origin::Url(_)), "{}: endpoint source", service.id());
        }
        assert!(service.rank().is_some(), "{}: no rank", service.id());
        assert!(!service.watches().is_empty(), "{}: no watch", service.id());
    }
}

#[test]
fn no_public_carrier_is_under_a_refused_suffix_and_each_is_on_443() {
    for service in services() {
        for endpoint in service.endpoints() {
            assert_eq!(endpoint.port(), 443, "{}", service.id());
            for name in [endpoint.host(), endpoint.server_name()] {
                assert!(!refused(name), "{}: {name}", service.id());
            }
            if let Some(path) = endpoint.path() {
                assert!(!path.contains('?'), "{}: a query in the path", service.id());
            }
        }
    }
}

#[test]
fn a_wisp_server_is_a_wss_forwarder_with_a_unique_word() {
    let mut words = HashSet::new();
    for service in pod_services::of_kind("forwarder").chain(pod_services::of_kind("wisp-public")) {
        let word = service.text("word").unwrap_or_else(|| panic!("{}: no word", service.id()));
        assert!(words.insert(word), "{}: the word {word} is used twice", service.id());
    }
    for service in pod_services::of_kind("wisp-public") {
        let endpoint = service.endpoint("forwarder").expect("the endpoint forwarder");
        assert_eq!(endpoint.scheme(), "wisp+wss", "{}", service.id());
        let template = service.text("template").expect("a template");
        assert!(template.starts_with(&format!("wisp+wss://{}", endpoint.host())), "{}", service.id());
        assert!(template.ends_with('/'), "{}", service.id());
        assert!(service.text("note").is_some_and(|n| !n.is_empty()), "{}", service.id());
        let kinds: Vec<&str> = service.watches().iter().map(|w| w.kind()).collect();
        assert!(kinds.contains(&"websocket-upgrade") && kinds.contains(&"forward-banner"), "{}", service.id());
    }
}

#[test]
fn a_proxy_list_is_an_https_file_that_a_watch_reads() {
    for service in pod_services::of_kind("proxy-list") {
        let endpoint = service.endpoint("list").expect("the endpoint list");
        assert_eq!(endpoint.scheme(), "https", "{}", service.id());
        assert!(endpoint.path().is_some_and(|p| p.starts_with('/')), "{}", service.id());
        assert!(service.text("note").is_some_and(|n| !n.is_empty()), "{}", service.id());
        let watch = service.watches().iter().find(|w| w.kind() == "http-status").expect("an http-status watch");
        assert_eq!(watch.arg("status").and_then(|v| v.as_int()), Some(200), "{}", service.id());
    }
}

#[test]
fn a_proxy_node_link_is_a_form_that_podssh_reads() {
    let mut seen = HashSet::new();
    for service in pod_services::of_kind("proxy-node") {
        let endpoint = service.endpoint("node").expect("the endpoint node");
        let link = service.text("link").unwrap_or_else(|| panic!("{}: no link", service.id()));
        assert!(!link.to_ascii_lowercase().starts_with("vmess:"), "{}: VMess", service.id());
        assert!(seen.insert(link), "{}: the link twice", service.id());
        let (scheme, host, query) = parts(link);
        assert_eq!(host, endpoint.host(), "{}: the link and the endpoint differ", service.id());
        let tls_name = param(query, "sni").unwrap_or(host);
        // One name for the host, the SNI and the Host, or a public address.
        assert_eq!(tls_name, endpoint.server_name(), "{}: SNI", service.id());
        if let Some(h) = param(query, "host") {
            assert_eq!(h, host, "{}: Host", service.id());
        }
        assert!(link.contains(":443"), "{}", service.id());
        assert!(!link.contains("flow="), "{}: flow", service.id());
        assert_ne!(param(query, "security"), Some("reality"), "{}: reality", service.id());
        let kind = param(query, "type").unwrap_or("tcp");
        match scheme {
            "vless" => {
                assert_eq!(param(query, "security"), Some("tls"), "{}", service.id());
                assert!(["ws", "xhttp"].contains(&kind), "{}: VLESS type {kind}", service.id());
            }
            "trojan" => {
                assert!(["tcp", "ws"].contains(&kind), "{}: Trojan type {kind}", service.id());
                assert!(param(query, "security").is_none_or(|s| s == "tls"), "{}", service.id());
            }
            other => panic!("{}: the scheme {other}", service.id()),
        }
        let watch = service.watches().iter().find(|w| w.kind() == "proxy-banner").expect("a proxy-banner watch");
        assert_eq!(watch.arg("target").and_then(|v| v.as_text()), Some("github-ssh"), "{}", service.id());
    }
}

#[test]
fn the_refusal_rule_catches_its_names() {
    for name in
        ["a.school.example", "x.edu", "mail.k12.ca.us", "uni.ac.uk", "a.sch.uk", "a.gov", "b.mil", "st-church.org"]
    {
        assert!(refused(name), "{name}");
    }
    for name in ["raw.githubusercontent.com", "proxy.example.net", "193.160.208.240"] {
        assert!(!refused(name), "{name}");
    }
}
