//! The API over the built-in set: by id, by kind, the typed accessors, and
//! the rules that the build guarantees for each service.

use std::net::{IpAddr, SocketAddr};

use pod_services::{Catalog, Origin, Value};

#[test]
fn a_service_is_read_by_its_id() {
    let relay = pod_services::get("ajam-relay").expect("the relay of the owner");
    assert_eq!(relay.id(), "ajam-relay");
    assert_eq!(relay.kind(), "relay");
    assert_eq!(relay.endpoint("default").unwrap().host(), "tcp.ssh.relay.ajam.dev");
    assert!(pod_services::get("no-such-service").is_none());
    assert!(relay.endpoint("no-such-endpoint").is_none());
    assert!(relay.fact("no-such-fact").is_none());
}

#[test]
fn the_services_of_a_kind_come_by_rank_then_by_id() {
    let doh: Vec<&str> = pod_services::of_kind("doh").map(|s| s.id()).collect();
    assert_eq!(doh, ["cloudflare-doh", "google-doh", "quad9-doh"]);
    let forwarders: Vec<&str> = pod_services::of_kind("forwarder").map(|s| s.id()).collect();
    assert_eq!(forwarders, ["mudlet-forwarder", "ssheasy-forwarder", "mudix-forwarder", "wisp-forwarder"]);
    let mailboxes: Vec<&str> = pod_services::of_kind("mailbox").map(|s| s.id()).collect();
    assert_eq!(mailboxes, ["magic-wormhole-mailbox", "leastauthority-mailbox"]);
    assert_eq!(pod_services::of_kind("no-such-kind").count(), 0);
    // The whole set: ranked services first, each kind's order kept.
    let all = pod_services::all();
    let keys: Vec<(u32, &str)> = all.iter().map(|s| (s.rank().map_or(u32::MAX, |r| r.value()), s.id())).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

#[test]
fn the_endpoints_of_a_role_come_in_the_order_of_the_file() {
    let relay = pod_services::get("ajam-relay").unwrap();
    let seed: Vec<&str> = relay.endpoints_of("seed").map(|e| e.host()).collect();
    assert_eq!(
        seed,
        [
            "tcp-us-east-1.ssh.relay.ajam.dev",
            "tcp-eu-west-3.ssh.relay.ajam.dev",
            "tcp-ap-southeast-1.ssh.relay.ajam.dev",
            "tcp-azure-westeurope.ssh.relay.ajam.dev",
            "tcp-us-central1.ssh.relay.ajam.dev",
            "tcp-ap-south-1.ssh.relay.ajam.dev",
        ]
    );
    let v6: Vec<IpAddr> =
        pod_services::get("cloudflare-doh").unwrap().endpoints_of("v6").flat_map(|e| e.ip_addrs()).collect();
    assert_eq!(v6, ["2606:4700:4700::1111".parse::<IpAddr>().unwrap(), "2606:4700:4700::1001".parse().unwrap()]);
}

#[test]
fn an_endpoint_gives_its_url_its_authority_and_its_addresses() {
    let relay = pod_services::get("ajam-relay").unwrap().endpoint("default").unwrap();
    assert_eq!(relay.url(), "https://tcp.ssh.relay.ajam.dev");
    assert_eq!(relay.authority(), "tcp.ssh.relay.ajam.dev:443");
    assert!(relay.socket_addrs().is_empty(), "a name with no address");
    let mailbox = pod_services::get("magic-wormhole-mailbox").unwrap().endpoint("mailbox").unwrap();
    assert_eq!(mailbox.url(), "wss://relay.magic-wormhole.io/v1");
    let edge = pod_services::get("cloudflare-tunnel-edge").unwrap().endpoint("h2-region1").unwrap();
    assert_eq!(edge.url(), "tls://region1.v2.argotunnel.com:7844");
    assert_eq!(edge.server_name(), "h2.cftunnel.com");
    assert!(edge.alpn().is_empty(), "HTTP/2 to the edge offers no ALPN");
    let udp = pod_services::get("cloudflare-dns").unwrap().endpoint("udp-1").unwrap();
    assert_eq!(udp.socket_addrs(), ["1.1.1.1:53".parse::<SocketAddr>().unwrap()], "the host is the address");
    let trace = pod_services::get("cloudflare-dns").unwrap().endpoint("trace").unwrap();
    assert_eq!(trace.server_name(), "one.one.one.one");
    assert_eq!(
        trace.socket_addrs(),
        ["1.1.1.1:443".parse::<SocketAddr>().unwrap(), "[2606:4700:4700::1111]:443".parse().unwrap()]
    );
    let doh = pod_services::get("cloudflare-doh").unwrap().endpoint("v6-1").unwrap();
    assert_eq!(doh.url(), "https://cloudflare-dns.com");
}

#[test]
fn a_fact_is_read_by_its_type() {
    let relay = pod_services::get("ajam-relay").unwrap();
    assert_eq!(relay.int("idle-timeout-ms"), Some(180_000));
    assert_eq!(relay.fact("idle-timeout-ms").unwrap().unit(), Some("ms"));
    assert_eq!(relay.text("idle-timeout-ms"), None, "an integer is not a text");
    assert_eq!(relay.texts("signed-list-keys"), Some(&[][..]), "an empty list of keys");
    let edge = pod_services::get("cloudflare-tunnel-edge").unwrap();
    assert_eq!(edge.date("roots-first-end").unwrap().to_string(), "2029-08-15");
    assert_eq!(edge.date("cloudflared-released").map(|d| (d.year(), d.month(), d.day())), Some((2026, 10, 30)));
    assert!(edge.text("roots").unwrap().contains("-----BEGIN CERTIFICATE-----"), "a file's text");
    assert_eq!(pod_services::get("quad9-doh").unwrap().bool("http2-only"), Some(true));
    assert_eq!(pod_services::get("quad9-doh").unwrap().text("json-path"), None, "Quad9 serves no JSON");
    let keys = pod_services::get("github-ssh").unwrap().texts("host-key-sha256").unwrap();
    assert_eq!(keys.len(), 3);
    assert_eq!(Value::Ints(vec![1, 2].into()).to_string(), "1, 2");
}

#[test]
fn each_value_has_its_source_and_its_date() {
    for service in pod_services::all() {
        let sources = service
            .endpoints()
            .iter()
            .map(|e| e.source())
            .chain(service.facts().iter().map(|f| f.source()))
            .chain(service.rank().map(|r| r.source()));
        for source in sources {
            match source.origin() {
                Origin::Url(u) => {
                    assert!(u.starts_with("https://") || u.starts_with("http://"), "{}: {u}", service.id())
                }
                Origin::Measured(m) | Origin::Ruling(m) => assert!(!m.is_empty(), "{}", service.id()),
            }
            let read = source.read();
            assert!(read.year() >= 2026 && pod_services::Date::new(read.year(), read.month(), read.day()).is_some());
        }
        assert!(!service.watches().is_empty(), "{} has no watch", service.id());
        assert!(service.watches().iter().all(|w| !w.kind().is_empty()));
    }
}

#[test]
fn a_watch_names_its_kind_and_its_arguments() {
    let relay = pod_services::get("ajam-relay").unwrap();
    let watch = &relay.watches()[0];
    assert_eq!(watch.kind(), "http-json");
    assert_eq!(watch.arg("pointer").and_then(Value::as_text), Some("/limits"));
    assert_eq!(watch.arg("facts").and_then(Value::as_texts).map(<[_]>::len), Some(5));
    assert!(watch.arg("no-such-argument").is_none());
}

#[test]
fn a_catalog_with_no_overlay_is_the_built_in_set() {
    let catalog = Catalog::new();
    assert_eq!(catalog.iter().count(), pod_services::all().len());
    assert_eq!(catalog.get("google-stun"), pod_services::get("google-stun"));
    let stun: Vec<&str> = catalog.of_kind("stun").map(|s| s.id()).collect();
    assert_eq!(stun, ["cloudflare-stun", "google-stun"]);
    assert!(catalog.overlay().is_empty());
}

#[test]
fn a_date_is_a_day_of_the_calendar() {
    assert!(pod_services::Date::new(2028, 2, 29).is_some());
    assert!(pod_services::Date::new(2026, 2, 29).is_none());
    assert!(pod_services::Date::new(2100, 2, 29).is_none());
    assert!(pod_services::Date::new(2026, 13, 1).is_none());
    assert_eq!(pod_services::Date::new(2026, 1, 2).unwrap().to_string(), "2026-01-02");
}

/// The public Piping Servers in their order: ppng.io first, then the one on
/// Render, which holds a request body until its end (PodUtils/podssh#158).
#[test]
fn the_piping_servers_come_in_their_order() {
    let piping: Vec<&str> = pod_services::of_kind("piping").map(|s| s.id()).collect();
    assert_eq!(piping, ["ppng-piping", "onrender-piping"]);
    let render = pod_services::get("onrender-piping").expect("onrender-piping");
    assert_eq!(render.endpoint("server").map(|e| e.host()), Some("piping.onrender.com"));
}

/// DERP: tailcat's servers first, then Tailscale's main fleet in the order
/// of its regions, the first server of each region first
/// (PodUtils/podssh#159).
#[test]
fn the_derp_fleets_come_in_their_order() {
    let derp: Vec<&str> = pod_services::of_kind("derp").map(|s| s.id()).collect();
    assert_eq!(derp, ["tailcat-derp", "tailscale-derp"]);
    let main = pod_services::get("tailscale-derp").expect("tailscale-derp");
    let first = main.endpoints().first().expect("a server");
    assert_eq!((first.role(), first.host()), (Some("nyc"), "derp1f.tailscale.com"));
    let regions: std::collections::BTreeSet<_> = main.endpoints().iter().filter_map(|e| e.role()).collect();
    assert_eq!(regions.len(), 28, "{regions:?}");
}
