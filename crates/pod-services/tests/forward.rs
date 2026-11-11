//! The SSH forward relays (T-224, PodUtils/podnet#455): each service of the
//! kind `ssh-tunnel` that publishes a URL has the facts that podnet reads,
//! each with its source, and the rule of each one picks the URL of the
//! banner that its source shows.

use std::time::Duration;

use pod_services::forward::{Banner, Relay, UrlRule};
use pod_services::{Origin, Service};

/// The forward relays.
const FORWARD: [&str; 4] = ["localhost-run", "pinggy", "sandhole", "serveo"];
/// The services of the kind `ssh-tunnel` that publish no URL: `ssh-j` is a
/// jump and a forward by name.
const NOT_FORWARD: [&str; 1] = ["ssh-j"];
/// Each fact that `Relay` reads, and its type.
const FACTS: [(&str, &str); 5] = [
    ("url-match", "texts"),
    ("url-skip", "texts"),
    ("user", "text"),
    ("free-tunnel-minutes", "int"),
    ("keepalive-seconds", "int"),
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
    assert_eq!(serveo.url_rule(), rule(&["https://", ".serveo.net"], &[]));
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
}

#[test]
fn the_rule_of_each_relay_picks_the_url_of_the_banner_of_its_source() {
    let samples = [
        ("pinggy", PINGGY_DOCS, "https://uljtt-30-47-152-61.run.pinggy-free.link"),
        ("pinggy", PINGGY_PODNET, "https://jcydy-2400-1a00-5b2f-821c-869e-56ff-fe03-2b71.run.pinggy-free.link"),
        ("localhost-run", LOCALHOST_RUN, "https://a1b2c3d4e5f6.lhr.life"),
        ("sandhole", SANDHOLE, "https://abcdef.demo.sandhole.com.br"),
        // The URL that the home page of Serveo shows.
        ("serveo", "https://abc.serveo.net/welcome\r\n", "https://abc.serveo.net/welcome"),
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
        ("sandhole", "http://abcdef.demo.sandhole.com.br"),
        ("sandhole", "https://demo.sandhole.com.br/"),
    ] {
        assert!(!relay(id).url_rule().accepts(url), "{id} takes {url}");
    }
}
