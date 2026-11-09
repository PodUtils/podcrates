//! DNS over HTTPS against the public resolvers of the default settings.
//! Ignored by default: it needs the network.
//!
//! ```sh
//! python3 -I scripts/names-proxy.py --listen 127.0.0.1:3129 &
//! POD_DNS_NAMES_PROXY=http://127.0.0.1:3129 \
//!     cargo test -p pod-dns --test live_doh -- --ignored --nocapture
//! ```

use std::sync::Arc;
use std::time::Duration;

use pod_dns::{Doh, Form, NXDOMAIN, Settings, Way};
use pod_proxy::{Dialer, Proxy, ProxyChoice};
use pod_tls::{Context, Words};
use tokio::time::Instant;

/// The TLS context of the tests, on ring.
fn context() -> Context {
    Context::new(Arc::new(rustls::crypto::ring::default_provider()), pod_tls::Settings::new(Words::new("podssh")))
}

/// A dial of `proxy`, over the system resolver.
fn dialer(proxy: ProxyChoice) -> Dialer {
    Dialer::new(pod_proxy::Settings::new("podssh", proxy), Arc::new(pod_proxy::resolve::System::new()))
}

/// Each resolver's certificate must verify against its IP address with the
/// default trust, and each must answer.
#[tokio::test]
#[ignore = "network: public DNS-over-HTTPS resolvers"]
async fn every_fallback_resolver_answers_through_verified_tls() {
    let (settings, context, dial) = (Settings::default(), context(), dialer(ProxyChoice::Direct));
    let way = Way { settings: &settings, context: &context, dial: &dial, kept: None };
    let found = way.lookup("github.com", Instant::now() + Duration::from_secs(20)).await;
    eprintln!("github.com via the resolver list: {found:?}");
    assert!(found.as_ref().is_ok_and(|ips| !ips.is_empty()), "{found:?}");
    // Each one alone, so a fallback that can never work is noticed; twice
    // at most, as alone it has no next resolver to cover a lost SYN.
    for resolver in &settings.resolvers {
        let mut one = Err(String::new());
        for attempt in 1..=TRIES {
            let deadline = Instant::now() + Duration::from_secs(10);
            one = way.lookup_with(std::slice::from_ref(resolver), "github.com", deadline).await;
            eprintln!("resolver {} alone, attempt {attempt}: {one:?}", resolver.ip);
            if one.as_ref().is_ok_and(|ips| !ips.is_empty()) {
                break;
            }
        }
        assert!(one.as_ref().is_ok_and(|ips| !ips.is_empty()), "{}: {one:?}", resolver.ip);
    }
}

/// The asks of one resolver alone. A connection lost once (a SYN with no
/// answer for 3 s) is the network's; a resolver that fails each time is the
/// fallback that never works.
const TRIES: u32 = 2;

/// Each resolver answers in RFC 8484's wire form too, by its address, which
/// its certificate names: the fallback of the JSON form, and Quad9's only
/// form.
#[tokio::test]
#[ignore = "network: public DNS-over-HTTPS resolvers"]
async fn every_resolver_answers_in_the_wire_form() {
    let (settings, context, dial) = (Settings::default(), context(), dialer(ProxyChoice::Direct));
    let way = Way { settings: &settings, context: &context, dial: &dial, kept: None };
    for resolver in &settings.resolvers {
        let wire_only = Doh { json: None, ..resolver.clone() };
        let mut answered = Err(pod_proxy::Missed::words(""));
        for attempt in 1..=TRIES {
            answered = way.ask(&wire_only, "github.com", Instant::now() + Duration::from_secs(10)).await;
            eprintln!("resolver {} in the wire form, attempt {attempt}: {answered:?}", resolver.ip);
            if answered.is_ok() {
                break;
            }
        }
        let (ips, form) = answered.unwrap_or_else(|e| panic!("{}: {e}", resolver.ip));
        assert!(!ips.is_empty() && form == Form::Wire, "{}: {ips:?} {form:?}", resolver.ip);
    }
}

/// A name that cannot exist ends the lookup at the first resolver's answer,
/// instead of asking every resolver for every record type.
#[tokio::test]
#[ignore = "network: public DNS-over-HTTPS resolvers"]
async fn a_name_that_does_not_exist_stops_at_the_first_answer() {
    let (settings, context, dial) = (Settings::default(), context(), dialer(ProxyChoice::Direct));
    let way = Way { settings: &settings, context: &context, dial: &dial, kept: None };
    let missing = way.lookup("podssh-no-such-name.invalid", Instant::now() + Duration::from_secs(20)).await;
    eprintln!("a reserved, nonexistent name: {missing:?}");
    let why = missing.expect_err("an .invalid name never resolves");
    assert_eq!(why, format!("{}: {NXDOMAIN}", settings.resolvers[0].ip));
}

/// The edge's SRV record through a proxy that takes names alone:
/// `POD_DNS_NAMES_PROXY` names one, as `live.yml` starts
/// `scripts/names-proxy.py`. It refuses each resolver's address,
/// so the records come by a resolver's name.
#[tokio::test]
#[ignore = "network: public DNS-over-HTTPS resolvers, through a local proxy of names alone"]
async fn the_edge_s_record_comes_through_a_proxy_of_names_alone() {
    let url = std::env::var("POD_DNS_NAMES_PROXY").expect("POD_DNS_NAMES_PROXY names the proxy");
    let proxy = ProxyChoice::Via(Proxy::parse(&url).expect("a proxy URL"));
    let (settings, context, dial) = (Settings::default(), context(), dialer(proxy));
    let way = Way { settings: &settings, context: &context, dial: &dial, kept: None };
    let deadline = Instant::now() + Duration::from_secs(30);
    let found = way.srv("_v2-origintunneld._tcp.argotunnel.com", deadline).await;
    eprintln!("the edge's SRV record through {url}: {found:?}");
    let records = found.expect("the records, by a resolver's name");
    assert!(!records.is_empty() && records.iter().all(|r| r.port == 7844), "{records:?}");
}
