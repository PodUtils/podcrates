//! The SSH forward relays (T-224, PodUtils/podnet#455): each service of the
//! kind `ssh-tunnel` that publishes a URL has the facts that podnet reads,
//! each with its source, and the rule of each one picks the URL of the
//! banner that its source shows.

use std::time::Duration;

use pod_services::forward::{Banner, Relay, UrlRule};
use pod_services::{Origin, Service, Value};

/// The forward relays.
const FORWARD: [&str; 6] = ["localhost-run", "pinggy", "sandhole", "serveo", "srv-us", "tunnl-gg"];
/// The services of the kind `ssh-tunnel` that are no forward relay of the
/// data: `ssh-j` publishes no URL (a jump and a forward by name); `tuns-sh`
/// publishes one, but no banner of it was read, and a session needs an
/// account (T-375).
const NOT_FORWARD: [&str; 2] = ["ssh-j", "tuns-sh"];
/// Each fact that `Relay` reads, and its type.
const FACTS: [(&str, &str); 7] = [
    ("url-match", "texts"),
    ("url-skip", "texts"),
    ("user", "text"),
    ("free-tunnel-minutes", "int"),
    ("keepalive-seconds", "int"),
    ("command", "text"),
    ("session-first", "bool"),
];

/// The banner of Pinggy's page "Getting Started".
const PINGGY_DOCS: &str = "// You can access local server via following URL(s):\n\
    http://uljtt-30-47-152-61.run.pinggy-free.link\nhttps://uljtt-30-47-152-61.run.pinggy-free.link\n";
/// The banner of `free.pinggy.io` that podnet read on 2026-10-09 (podnet's
/// test `picks_the_tunnel_url_and_not_the_dashboard`).
const PINGGY_PODNET: &str = "  You are not authenticated.\n  Your tunnel will expire in 60 minutes. \
    Upgrade to Pinggy Pro to get unrestricted tunnels. https://dashboard.pinggy.io\n  \
    https://blirg-2400-1a00-5b2f-821c-869e-56ff-fe03-2b71.free.pinggy.net\n  \
    https://jcydy-2400-1a00-5b2f-821c-869e-56ff-fe03-2b71.run.pinggy-free.link\n";
/// The sample of the landing page of localhost.run, with the ends of line
/// of an SSH session.
const LOCALHOST_RUN: &str = "\r\n===============================================\r\n\
    Welcome to localhost.run! \r\n===============================================\r\n\r\n\
    ** your connection id is 7f3a-..., please mention it\r\n   if you send me any bug reports **\r\n\r\n\
    a1b2c3d4e5f6.lhr.life tunneled with tls termination, \r\nhttps://a1b2c3d4e5f6.lhr.life\r\n\r\n\
    GET  /           200  12ms\r\n";
/// The lines that `src/ssh/forwarding.rs` of Sandhole writes, with the
/// escapes of their colors; the time and the name are made up.
const SANDHOLE: &str = "\x1b[2m2026-11-11T12:00:00Z\x1b[0m \x1b[1;32m Starting HTTP\x1b[0m on \
    \x1b[4mhttp://abcdef.demo.sandhole.com.br\x1b[0m for localhost\r\n\
    \x1b[2m2026-11-11T12:00:00Z\x1b[0m \x1b[1;32mStarting HTTPS\x1b[0m on \
    \x1b[4mhttps://abcdef.demo.sandhole.com.br\x1b[0m for localhost\r\n";
/// The line of a forward of port 1 that `backend/main.go` of srv.us writes
/// to the user `nomatch`, with the name of its README.
const SRV_US: &str = "1: https://qp556ma755ktlag5b2xyt334ae.srv.us/\r\n";
/// The first line of tunnl.gg's command `output=json`, as its README shows
/// it: no space ends the URL.
const TUNNL_GG_JSON: &str = "{\"event\":\"tunnel\",\"url\":\"https://brave-fox-7c41e09b.tunnl.gg\",\
    \"subdomain\":\"brave-fox-7c41e09b\",\"stable\":true,\"idle_timeout_seconds\":7200,\
    \"max_lifetime_seconds\":86400}\n";
/// The banner of a shell that `sessionBanner` of tunnl.gg writes, with the
/// escapes of its colors.
const TUNNL_GG_SHELL: &str = "\r\n\x1b[1;32m  \u{25cf} Tunnel is live\x1b[0m\x1b[38;5;245m on tunnl.gg\x1b[0m\r\n\r\n\
    \x1b[38;5;245m  URL      \x1b[0m\x1b[38;5;141mhttps://brave-fox-7c41e09b.tunnl.gg\x1b[0m\r\n";

fn relay(id: &str) -> Relay<'static> {
    let service = pod_services::get(id).unwrap_or_else(|| panic!("no service {id}"));
    Relay::of(service).unwrap_or_else(|| panic!("{id} is no forward relay"))
}

fn authorities(relay: &Relay) -> Vec<String> {
    relay.endpoints().map(|e| e.authority()).collect()
}

fn rule(matches: &[&str], skips: &[&str]) -> UrlRule {
    let texts = |list: &[&str]| list.iter().map(|s| s.to_string()).collect();
    UrlRule { matches: texts(matches), skips: texts(skips) }
}

#[test]
fn each_ssh_tunnel_is_a_forward_relay_or_named_as_not_one() {
    let ids: Vec<&str> = pod_services::of_kind("ssh-tunnel").map(Service::id).collect();
    let mut named: Vec<&str> = FORWARD.iter().chain(&NOT_FORWARD).copied().collect();
    named.sort();
    assert_eq!(ids, named, "a new service of the kind ssh-tunnel goes into one of the two lists");
    for id in NOT_FORWARD {
        let service = pod_services::get(id).unwrap();
        assert!(Relay::of(service).is_none(), "{id}");
        assert!(FACTS.iter().all(|(name, _)| service.fact(name).is_none()), "{id} has a fact of a forward relay");
    }
}

#[test]
fn each_forward_relay_has_the_facts_that_its_reader_needs() {
    for id in FORWARD {
        let relay = relay(id);
        let ports: Vec<u16> = relay.endpoints().map(|e| e.port()).collect();
        assert!(!ports.is_empty(), "{id}: no endpoint of plain SSH");
        // The order of the file is the order to try: 443 first.
        assert!(ports.iter().skip_while(|&&p| p == 443).all(|&p| p != 443), "{id}: {ports:?}");
        let rule = relay.url_rule();
        assert!(!rule.matches.is_empty(), "{id}: no text that the URL holds");
        assert!(rule.matches.iter().chain(&rule.skips).all(|t| !t.trim().is_empty()), "{id}: an empty text");
        for (name, kind) in FACTS {
            let Some(fact) = relay.service().fact(name) else { continue };
            let typed = match kind {
                "texts" => fact.value().as_texts().is_some(),
                "text" => fact.value().as_text().is_some(),
                "bool" => fact.value().as_bool().is_some(),
                _ => fact.value().as_int().is_some_and(|n| n > 0),
            };
            assert!(typed, "{id}: {name} is not of the type {kind}");
            let origin = fact.source().origin();
            assert!(
                matches!(origin, Origin::Url(_) | Origin::Measured(_)),
                "{id}: {name} has no document or measurement"
            );
        }
        let unit = |name| relay.service().fact(name).map(|f| f.unit());
        assert!(unit("keepalive-seconds").is_none_or(|u| u == Some("s")), "{id}");
        assert!(unit("free-tunnel-minutes").is_none_or(|u| u == Some("min")), "{id}");
    }
}

/// The relays whose document names the zone of `url-match` (T-225).
const ZONE_DOCUMENTS: [&str; 3] = ["localhost-run", "pinggy", "serveo"];

/// One watch `forward-url` for each relay, on its first endpoint to try: a
/// run of each watch opens one session with each relay. A watch `http-text`
/// holds the document of the zone of the rule.
#[test]
fn each_forward_relay_has_a_watch_of_its_banner_and_of_the_document_of_its_zone() {
    for id in FORWARD {
        let relay = relay(id);
        let watches = relay.service().watches();
        let banners: Vec<_> = watches.iter().filter(|w| w.kind() == "forward-url").collect();
        assert_eq!(banners.len(), 1, "{id}: one session for each run");
        let first = relay.endpoints().next().map(|e| e.name());
        assert_eq!(banners[0].arg("endpoint").and_then(Value::as_text), first, "{id}");
        let documents: Vec<&str> = watches
            .iter()
            .filter(|w| w.kind() == "http-text")
            .filter_map(|w| w.arg("contains").and_then(Value::as_text))
            .collect();
        if !ZONE_DOCUMENTS.contains(&id) {
            assert!(documents.is_empty(), "{id}: a document of its zone goes into ZONE_DOCUMENTS");
            continue;
        }
        let rule = relay.url_rule();
        let zones: Vec<&str> =
            rule.matches.iter().filter(|m| !m.starts_with("http")).map(|m| m.trim_start_matches('.')).collect();
        assert_eq!(documents.len(), 1, "{id}: one document of its zone");
        assert!(zones.iter().any(|z| documents[0].contains(z)), "{id}: {:?} names none of {zones:?}", documents[0]);
    }
}

#[test]
fn the_facts_of_each_relay_are_the_ones_of_its_sources() {
    let minutes = |n: u64| Some(Duration::from_secs(n * 60));
    let seconds = |n: u64| Some(Duration::from_secs(n));
    let pinggy = relay("pinggy");
    assert_eq!(authorities(&pinggy), ["free.pinggy.io:443"]);
    assert_eq!(pinggy.url_rule(), rule(&["https://", ".run.pinggy-free.link"], &["dashboard.pinggy.io"]));
    assert_eq!((pinggy.user(), pinggy.session_limit(), pinggy.keepalive()), (Some("http"), minutes(60), seconds(30)));
    let serveo = relay("serveo");
    assert_eq!(authorities(&serveo), ["serveo.net:443", "serveo.net:22"]);
    assert_eq!(serveo.url_rule(), rule(&["https://", ".serveousercontent.com"], &["console.serveo.net"]));
    assert_eq!((serveo.user(), serveo.session_limit(), serveo.keepalive()), (None, None, seconds(30)));
    let lhr = relay("localhost-run");
    assert_eq!(authorities(&lhr), ["localhost.run:22"]);
    assert_eq!(lhr.url_rule(), rule(&["https://", ".lhr.life"], &[]));
    assert_eq!((lhr.user(), lhr.session_limit(), lhr.keepalive()), (Some("nokey"), None, seconds(60)));
    let sandhole = relay("sandhole");
    assert_eq!(authorities(&sandhole), ["demo.sandhole.com.br:443", "demo.sandhole.com.br:22"]);
    assert_eq!(sandhole.url_rule(), rule(&["https://", ".demo.sandhole.com.br"], &[]));
    assert_eq!((sandhole.user(), sandhole.session_limit(), sandhole.keepalive()), (None, None, None));
    // The URL of a tunnel of Sandhole is a name under its zone.
    let zone = sandhole.service().text("tunnel-zone").unwrap();
    assert_eq!(sandhole.url_rule().matches[1], format!(".{zone}"));
    let srv_us = relay("srv-us");
    assert_eq!(authorities(&srv_us), ["srv.us:22"]);
    assert_eq!(srv_us.url_rule(), rule(&["https://", ".srv.us/"], &[]));
    assert_eq!((srv_us.user(), srv_us.session_limit(), srv_us.keepalive()), (Some("nomatch"), None, None));
    assert_eq!((srv_us.command(), srv_us.session_first()), (None, true));
    let tunnl = relay("tunnl-gg");
    assert_eq!(authorities(&tunnl), ["proxy.tunnl.gg:22"]);
    assert_eq!(tunnl.url_rule(), rule(&["https://", ".tunnl.gg"], &[]));
    assert_eq!((tunnl.user(), tunnl.session_limit(), tunnl.keepalive()), (None, minutes(1440), seconds(30)));
    assert_eq!((tunnl.command(), tunnl.session_first()), (Some("output=json"), false));
    for relay in [srv_us, tunnl] {
        let zone = relay.service().text("tunnel-zone").unwrap();
        assert!(relay.url_rule().matches[1].starts_with(&format!(".{zone}")), "{}", relay.service().id());
    }
    // The other relays write the URL to a shell, and answer the forward
    // before a session.
    for id in ["pinggy", "serveo", "localhost-run", "sandhole"] {
        assert_eq!((relay(id).command(), relay(id).session_first()), (None, false), "{id}");
    }
}

#[test]
fn the_rule_of_each_relay_picks_the_url_of_the_banner_of_its_source() {
    let samples = [
        ("pinggy", PINGGY_DOCS, "https://uljtt-30-47-152-61.run.pinggy-free.link"),
        ("pinggy", PINGGY_PODNET, "https://jcydy-2400-1a00-5b2f-821c-869e-56ff-fe03-2b71.run.pinggy-free.link"),
        ("localhost-run", LOCALHOST_RUN, "https://a1b2c3d4e5f6.lhr.life"),
        ("sandhole", SANDHOLE, "https://abcdef.demo.sandhole.com.br"),
        ("srv-us", SRV_US, "https://qp556ma755ktlag5b2xyt334ae.srv.us/"),
        // A login that GitHub knows: the first URL is the key's.
        (
            "srv-us",
            "1: https://qp556ma755ktlag5b2xyt334ae.srv.us/, https://pcarrier.gh.srv.us/\r\n",
            "https://qp556ma755ktlag5b2xyt334ae.srv.us/",
        ),
        ("tunnl-gg", TUNNL_GG_JSON, "https://brave-fox-7c41e09b.tunnl.gg"),
        ("tunnl-gg", TUNNL_GG_SHELL, "https://brave-fox-7c41e09b.tunnl.gg"),
        // The URL that the home page of Serveo shows.
        // The banner of an anonymous session (GitHub #98): the tunnel under
        // serveousercontent.com, and the console, which the rule skips.
        (
            "serveo",
            "Forwarding HTTP traffic from https://c629a466dce8453e-38-25-151-89.serveousercontent.com\r\nhttps://console.serveo.net/settings\r\n",
            "https://c629a466dce8453e-38-25-151-89.serveousercontent.com",
        ),
        (
            "serveo",
            "https://console.serveo.net/settings\r\nhttps://abc.serveousercontent.com\r\n",
            "https://abc.serveousercontent.com",
        ),
    ];
    for (id, banner, url) in samples {
        let rule = relay(id).url_rule();
        assert_eq!(rule.pick(banner).as_deref(), Some(url), "{id}");
        for size in [1, 2, 3, 7, 64] {
            let mut reader = Banner::new(rule.clone());
            let pushed = banner.as_bytes().chunks(size).find_map(|chunk| reader.push(chunk).map(str::to_string));
            assert_eq!(pushed.as_deref(), Some(url), "{id} in chunks of {size}");
        }
    }
}

#[test]
fn the_rule_of_each_relay_refuses_the_other_urls_of_the_service() {
    for (id, url) in [
        ("pinggy", "https://dashboard.pinggy.io"),
        ("pinggy", "http://uljtt-30-47-152-61.run.pinggy-free.link"),
        ("localhost-run", "https://admin.localhost.run/"),
        ("localhost-run", "https://twitter.com/localhost_run"),
        ("serveo", "https://serveo.net/docs/"),
        ("serveo", "https://console.serveo.net/settings"),
        ("sandhole", "http://abcdef.demo.sandhole.com.br"),
        ("sandhole", "https://demo.sandhole.com.br/"),
        // srv.us's pastebin, and the warning page of tunnl.gg.
        ("srv-us", "https://srv.us/e3jyi7vowlwqyzaewdfv444laadkys4a"),
        ("tunnl-gg", "https://tunnl.gg/#/warning?redirect=x&subdomain=brave-fox-7c41e09b"),
    ] {
        assert!(!relay(id).url_rule().accepts(url), "{id} takes {url}");
    }
}

/// The port of the remote forward: Sandhole's demo refuses port 0 and
/// names 80 (T-373, GitHub #97); srv.us hashes the port into the name, and
/// podssh's ends agree on 1 (T-552); a relay with no fact gives none.
#[test]
fn the_remote_port_is_the_relay_s_fact() {
    assert_eq!(relay("sandhole").remote_port(), Some(80));
    assert_eq!(relay("srv-us").remote_port(), Some(1));
    // Each forward-url watch of srv.us asks for that port.
    let watch = relay("srv-us").service().watches().iter().find(|w| w.kind() == "forward-url").unwrap();
    assert_eq!(watch.arg("port").and_then(Value::as_int), Some(1));
    for id in ["pinggy", "serveo", "localhost-run", "tunnl-gg"] {
        assert_eq!(relay(id).remote_port(), None, "{id}");
    }
}
