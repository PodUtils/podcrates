//! The service `syncthing-relay` (PodUtils/podssh@T-571): the public
//! relays of Syncthing on port 443, each with its device ID, in the order
//! of the device IDs (C104). podssh picks a relay by its place in that
//! order and pins the TLS of the relay to its device ID.

use std::collections::BTreeSet;
use std::net::IpAddr;

use pod_services::{Origin, Service};

const POOL: &str = "https://relays.syncthing.net/endpoint/full";

fn relays() -> &'static Service {
    pod_services::get("syncthing-relay").expect("the service syncthing-relay")
}

/// The fact `device-id-NAME` of each endpoint, in the order of the file.
fn ids(service: &Service) -> Vec<&str> {
    let id = |name: &str| service.text(&format!("device-id-{name}"));
    service.endpoints().iter().map(|e| id(e.name()).unwrap_or_else(|| panic!("{}: no device ID", e.name()))).collect()
}

/// A device ID as Syncthing writes it: eight groups of seven characters of
/// base32 (A to Z, 2 to 7), with a `-` between two groups.
fn is_device_id(id: &str) -> bool {
    let groups: Vec<&str> = id.split('-').collect();
    groups.len() == 8
        && groups
            .iter()
            .all(|g| g.len() == 7 && g.bytes().all(|b| b.is_ascii_uppercase() || (b'2'..=b'7').contains(&b)))
}

#[test]
fn each_syncthing_relay_is_an_ip_literal_on_443_with_its_device_id() {
    let service = relays();
    assert_eq!(service.kind(), "syncthing-relay");
    assert!(!service.endpoints().is_empty());
    for (endpoint, id) in service.endpoints().iter().zip(ids(service)) {
        let name = endpoint.name();
        assert_eq!((endpoint.scheme(), endpoint.port()), ("tls", 443), "{name}");
        assert!(endpoint.host().parse::<IpAddr>().is_ok(), "{name}: {} is no IP literal", endpoint.host());
        assert!(is_device_id(id), "{name}: {id}");
        // The name holds the head of the device ID: a new device is a new name.
        assert_eq!(name, format!("relay-{}", id[..7].to_ascii_lowercase()));
        assert!(matches!(endpoint.source().origin(), Origin::Url(u) if u.as_ref() == POOL), "{name}");
    }
    assert_eq!(service.facts().len(), service.endpoints().len());
    for fact in service.facts() {
        assert!(matches!(fact.source().origin(), Origin::Url(u) if u.as_ref() == POOL), "{}", fact.name());
    }
    let hosts: BTreeSet<&str> = service.endpoints().iter().map(|e| e.host()).collect();
    assert_eq!(hosts.len(), service.endpoints().len(), "one endpoint for each relay");
}

#[test]
fn the_syncthing_relays_come_in_the_order_of_their_device_ids() {
    let ids = ids(relays());
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(ids, sorted, "the endpoints are in the order of their device IDs, each ID once");
}

#[test]
fn the_watch_of_the_syncthing_relays_holds_each_endpoint_to_the_pool() {
    let watch = relays().watches().iter().find(|w| w.kind() == "http-json").expect("a watch http-json");
    let text = |name: &str| watch.arg(name).and_then(|v| v.as_text());
    assert_eq!(text("url"), Some(POOL));
    assert_eq!((text("url-member"), text("key"), text("fact-prefix")), (Some("url"), Some("id"), Some("device-id-")));
    assert_eq!((text("member"), text("port-member")), (Some("host"), Some("port")));
}
