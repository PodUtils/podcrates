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

fn watch_text<'a>(w: &'a pod_services::Watch, name: &str) -> Option<&'a str> {
    match w.arg(name) {
        Some(Value::Text(t)) => Some(t),
        _ => None,
    }
}

/// The map of Tailscale has 95 servers on the day it was read, and the watch
/// of the map compares the host of each node.
#[test]
fn the_tailscale_fleet_holds_the_four_servers_and_a_watch_of_the_hosts() {
    let main = pod_services::get("tailscale-derp").unwrap();
    assert_eq!(main.endpoints().len(), 95);
    for host in ["derp1j", "derp1k", "derp4k", "derp4l"] {
        let host = format!("{host}.tailscale.com");
        assert!(main.endpoints().iter().any(|e| e.host() == host), "{host}");
    }
    let watch = main.watches().iter().find(|w| watch_text(w, "pointer") == Some("/Regions")).unwrap();
    assert_eq!(watch_text(watch, "member"), Some("HostName"));
}

/// The hosts of the relay and its pool are watched.
#[test]
fn the_relay_watches_its_hosts_and_its_pool() {
    let relay = pod_services::get("ajam-relay").unwrap();
    let names = |kind: &str| relay.watches().iter().filter(|w| w.kind() == kind).count();
    assert_eq!(names("tls-name"), 2);
    assert_eq!(names("http-status"), 1);
    let pool = relay.watches().iter().find(|w| watch_text(w, "pointer") == Some("/pool")).unwrap();
    assert_eq!((watch_text(pool, "member"), watch_text(pool, "role")), (Some("host"), Some("seed")));
}

/// The file host of PyPI is a service of its own, with its watches.
#[test]
fn the_file_host_of_pypi_is_a_service() {
    let files = pod_services::get("pypi-files").expect("pypi-files");
    assert_eq!(files.kind(), "package-files");
    assert_eq!(files.endpoint("files").unwrap().host(), "files.pythonhosted.org");
    assert!(files.text("probe-path").is_some_and(|path| path.starts_with("/packages/")));
    let kinds: Vec<&str> = files.watches().iter().map(|w| w.kind()).collect();
    assert_eq!(kinds, ["tls-name", "http-status"]);
    assert!(pod_services::of_kind("package-files").any(|s| s.id() == "pypi-files"));
}

/// Pinggy has SSH inside TLS, its host key, and a watch of the key.
#[test]
fn pinggy_has_ssh_inside_tls_and_its_host_key() {
    let pinggy = pod_services::get("pinggy").expect("pinggy");
    assert_eq!(pinggy.endpoint("ssh").unwrap().port(), 443);
    let skips: Vec<&str> = pinggy.texts("url-skip").unwrap().iter().map(|t| t.as_ref()).collect();
    assert_eq!(skips, ["dashboard.pinggy.io"]);
    for (name, port) in [("ssh-tls", 443), ("ssh-tls-alt", 7878)] {
        let e = pinggy.endpoint(name).unwrap();
        assert_eq!(
            (e.scheme(), e.host(), e.port(), e.sni()),
            ("ssh+tls", "free.pinggy.io", port, Some("free.pinggy.io"))
        );
    }
    assert_eq!(pinggy.text("host-key-sha256"), Some("SHA256:nFd5rfJMGuZXvfeRzJ/BtT3TfksAxTWMajcrHRcI7AM"));
    assert_eq!(pinggy.text("tunnel-zone"), Some("a.pinggy.link"));
    assert_eq!(pinggy.int("free-tunnel-minutes"), Some(60));
    assert_eq!(pinggy.fact("free-tunnel-minutes").unwrap().unit(), Some("min"));
    let watch = pinggy.watches().iter().find(|w| w.kind() == "ssh-host-key").expect("the watch of the key");
    assert_eq!(watch.arg("endpoint").and_then(|v| v.as_text()), Some("ssh"));
    assert_eq!(watch.arg("fact").and_then(|v| v.as_text()), Some("host-key-sha256"));
}

/// The three last fallbacks of podssh of T-171 (PodUtils/podssh#186): each
/// endpoint, each fact, and a watch that runs with no tunnel.
#[test]
fn the_tunnels_of_localhost_run_localtunnel_and_bore() {
    let lhr = pod_services::get("localhost-run").expect("localhost-run");
    let ssh = lhr.endpoint("ssh").unwrap();
    assert_eq!((lhr.kind(), ssh.scheme(), ssh.host(), ssh.port()), ("ssh-tunnel", "ssh", "localhost.run", 22));
    assert_eq!(lhr.text("user"), Some("nokey"));
    assert_eq!(lhr.text("host-key-sha256"), Some("SHA256:pG6qrBxubYfWa1Zadu/V0NUgjEDiBds/7e2xzte/QNM"));
    let lt = pod_services::get("localtunnel").expect("localtunnel");
    let api = lt.endpoint("api").unwrap();
    assert_eq!((lt.kind(), api.scheme(), api.host(), api.port()), ("localtunnel", "https", "localtunnel.me", 443));
    assert_eq!(lt.text("tunnel-zone"), Some("loca.lt"));
    let bore = pod_services::get("bore").expect("bore");
    let control = bore.endpoint("control").unwrap();
    assert_eq!(
        (bore.kind(), control.scheme(), control.host(), control.port()),
        ("tcp-tunnel", "tcp", "bore.pub", 7835)
    );
    assert_eq!(bore.int("control-port"), Some(i64::from(control.port())));
    assert_eq!((bore.int("tunnel-hours"), bore.fact("tunnel-hours").unwrap().unit()), (Some(24), Some("h")));
    for service in [&lhr, &bore] {
        assert!(service.watches().iter().any(|w| w.kind() == "rdap"), "{}", service.id());
    }
    // .me has no RDAP server: the watch holds the name of the domain.
    assert!(lt.watches().iter().any(|w| w.kind() == "tls-name"));
}

/// The last fallbacks of podssh of T-552 (PodUtils/podssh#236): SSH on 22,
/// the facts of their sources, the key that tunnl.gg publishes, and the
/// site, whose name a watch checks: neither domain has an RDAP server.
#[test]
fn the_tunnels_of_srv_us_and_tunnl_gg() {
    for (id, host, site) in [("srv-us", "srv.us", "srv.us"), ("tunnl-gg", "proxy.tunnl.gg", "tunnl.gg")] {
        let service = pod_services::get(id).expect(id);
        let ssh = service.endpoint("ssh").unwrap();
        assert_eq!((service.kind(), ssh.scheme(), ssh.host(), ssh.port()), ("ssh-tunnel", "ssh", host, 22), "{id}");
        let https = service.endpoint("site").unwrap();
        assert_eq!((https.scheme(), https.host(), https.port()), ("https", site, 443), "{id}");
        let kinds: Vec<&str> = service.watches().iter().map(|w| w.kind()).collect();
        assert!(kinds.contains(&"tls-name") && !kinds.contains(&"rdap"), "{id}: {kinds:?}");
        assert_eq!(service.text("tunnel-zone"), Some(site), "{id}");
    }
    let srv_us = pod_services::get("srv-us").unwrap();
    assert_eq!((srv_us.text("user"), srv_us.int("remote-port")), (Some("nomatch"), Some(1)));
    assert_eq!(srv_us.bool("session-first"), Some(true));
    // srv.us publishes no host key: podssh's live run reads it first.
    assert!(srv_us.fact("host-key-sha256").is_none());
    let tunnl = pod_services::get("tunnl-gg").unwrap();
    assert_eq!(tunnl.text("host-key-sha256"), Some("SHA256:5uQ6VobmUpY4wyMmm5jnAwFtt3aX06WYDDM4ER+2WUw"));
    assert_eq!(tunnl.text("command"), Some("output=json"));
    assert!(tunnl.fact("user").is_none(), "tunnl.gg takes any user but stable and pro");
    let watched = tunnl.watches().iter().any(|w| w.kind() == "ssh-host-key");
    assert!(watched, "no watch of the key of tunnl.gg");
}

/// The demo of Sandhole (T-186, PodUtils/podssh@T-743): SSH on 22 and plain
/// SSH on 443, one host key watched on each port, and the tunnel zone.
#[test]
fn the_demo_of_sandhole_has_two_ssh_ports_and_one_key() {
    let sandhole = pod_services::get("sandhole").expect("sandhole");
    assert_eq!(sandhole.kind(), "ssh-tunnel");
    for (name, port) in [("ssh", 22), ("ssh-443", 443)] {
        let e = sandhole.endpoint(name).unwrap();
        assert_eq!((e.scheme(), e.host(), e.port()), ("ssh", "demo.sandhole.com.br", port), "{name}");
        let watched = sandhole
            .watches()
            .iter()
            .any(|w| w.kind() == "ssh-host-key" && w.arg("endpoint").and_then(|v| v.as_text()) == Some(name));
        assert!(watched, "no watch of the key on {name}");
    }
    assert_eq!(sandhole.text("host-key-sha256"), Some("SHA256:EV7UJ46EDUHvG0yx4I4dw+9r3bypGpBt9+vQy2w9V/o"));
    assert_eq!(sandhole.text("tunnel-zone"), Some("demo.sandhole.com.br"));
}

/// The public node of OpenBare (T-187, PodUtils/podssh@T-740): a Bare v3
/// endpoint, the facts that the JSON of its root holds, and their watches.
#[test]
fn the_bare_node_of_openbare_is_a_bare_v3_endpoint() {
    let node = pod_services::get("openbare-xyz").expect("openbare-xyz");
    assert_eq!(node.kind(), "bare-node");
    let bare = node.endpoint("bare").unwrap();
    assert_eq!((bare.scheme(), bare.host(), bare.port(), bare.path()), ("https", "openbare.xyz", 443, Some("/bare/")));
    assert_eq!((node.text("service"), node.text("protocol")), (Some("openbare-edge"), Some("bare-v3")));
    // The edge claims WebSocket, but each upgrade ends in 500 (T-198).
    assert_eq!(node.text("websocket-upgrade"), Some("500"));
    for fact in ["service", "protocol"] {
        let watched = node
            .watches()
            .iter()
            .any(|w| w.kind() == "http-json" && w.arg("fact").and_then(|v| v.as_text()) == Some(fact));
        assert!(watched, "no watch of {fact}");
    }
}

#[test]
fn the_tcp_relay_of_zerotier_is_its_fallback_address() {
    let relay = pod_services::get("zerotier-tcp-relay").expect("zerotier-tcp-relay");
    let tcp = relay.endpoint("tcp").expect("the endpoint tcp");
    assert_eq!((tcp.scheme(), tcp.host(), tcp.port()), ("tcp", "204.80.128.1", 443));
    // The fact keeps the form of the source, ADDRESS/PORT, and agrees with the endpoint.
    assert_eq!(relay.text("relay"), Some(format!("{}/{}", tcp.host(), tcp.port()).as_str()));
}

/// The last fallbacks of podssh of PodUtils/podssh#213: smee.io and hostc.
#[test]
fn the_servers_of_smee_io_and_hostc() {
    let smee = pod_services::get("smee-io").expect("smee-io");
    let server = smee.endpoint("server").unwrap();
    assert_eq!((smee.kind(), server.scheme(), server.host(), server.port()), ("smee", "https", "smee.io", 443));
    assert_eq!(pod_services::of_kind("smee").map(|s| s.id()).collect::<Vec<_>>(), ["smee-io"]);
    let hostc = pod_services::get("hostc").expect("hostc");
    let api = hostc.endpoint("api").unwrap();
    assert_eq!((hostc.kind(), api.scheme(), api.host(), api.port()), ("hostc", "https", "hostc.dev", 443));
    assert_eq!(hostc.text("tunnel-zone"), Some("hostc.app"));
    assert_eq!(hostc.int("protocol-version"), Some(5));
}
