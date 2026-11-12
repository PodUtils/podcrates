//! The service `tox-tcp-relay` (PodUtils/podssh@T-734): the public TCP
//! relays of Tox on port 443, each with the public key of its node, in the
//! order of the keys (C83). podssh picks a relay by its place in that
//! order, so both ends of a pair must read one order.

use std::collections::BTreeSet;
use std::net::IpAddr;

use pod_services::{Origin, Service};

const LIST: &str = "https://nodes.tox.chat/json";

fn relays() -> &'static Service {
    pod_services::get("tox-tcp-relay").expect("the service tox-tcp-relay")
}

/// The fact `public-key-NAME` of each endpoint, in the order of the file.
fn keys(service: &Service) -> Vec<&str> {
    let key = |name: &str| service.text(&format!("public-key-{name}"));
    service.endpoints().iter().map(|e| key(e.name()).unwrap_or_else(|| panic!("{}: no key", e.name()))).collect()
}

#[test]
fn each_tox_relay_is_an_ip_literal_on_443_with_the_key_of_its_node() {
    let service = relays();
    assert_eq!(service.kind(), "tcp-relay");
    assert!(!service.endpoints().is_empty());
    for (endpoint, key) in service.endpoints().iter().zip(keys(service)) {
        let name = endpoint.name();
        assert_eq!((endpoint.scheme(), endpoint.port()), ("tcp", 443), "{name}");
        assert!(endpoint.host().parse::<IpAddr>().is_ok(), "{name}: {} is no IP literal", endpoint.host());
        assert!(key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()), "{name}: {key}");
        // The name holds the head of the key: a new key is a new name.
        assert_eq!(name, format!("node-{}", key[..8].to_ascii_lowercase()));
        assert!(matches!(endpoint.source().origin(), Origin::Url(u) if u.as_ref() == LIST), "{name}");
    }
    // Each fact is the key of one endpoint, and each source is the list.
    assert_eq!(service.facts().len(), service.endpoints().len());
    for fact in service.facts() {
        assert!(matches!(fact.source().origin(), Origin::Url(u) if u.as_ref() == LIST), "{}", fact.name());
    }
    let hosts: BTreeSet<&str> = service.endpoints().iter().map(|e| e.host()).collect();
    assert_eq!(hosts.len(), service.endpoints().len(), "one endpoint for each node");
}

#[test]
fn the_tox_relays_come_in_the_order_of_their_keys() {
    let keys = keys(relays());
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(keys, sorted, "the endpoints are in the order of their keys, each key once");
}

#[test]
fn the_watch_of_the_tox_relays_holds_each_endpoint_to_the_list() {
    let watch = relays().watches().iter().find(|w| w.kind() == "http-json").expect("a watch http-json");
    let text = |name: &str| watch.arg(name).and_then(|v| v.as_text());
    assert_eq!(text("url"), Some(LIST));
    assert_eq!((text("key"), text("fact-prefix")), (Some("public_key"), Some("public-key-")));
    assert_eq!(text("port-member"), Some("tcp_ports"));
    assert!(watch.arg("member").is_some() && watch.arg("true-members").is_some());
}
