//! A name that is not DNS-safe is refused before any question leaves.

use std::sync::Arc;
use std::time::Duration;

use pod_proxy::{Dialer, ProxyChoice};
use pod_tls::{Context, Words};
use tokio::time::Instant;

use crate::{Doh, Settings, Way};

#[tokio::test]
async fn refuses_a_name_that_changes_the_question() {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = Context::new(provider, pod_tls::Settings::new(Words::new("podssh")));
    let dial = Dialer::new(
        pod_proxy::Settings::new("podssh", ProxyChoice::Direct),
        Arc::new(pod_proxy::resolve::System::new()),
    );
    let settings = Settings::default();
    let way = Way { settings: &settings, context: &context, dial: &dial, kept: None };
    // Port 9 on the loopback: a name that passed the check would fail by a
    // connection error, not by the words of the check.
    let resolver = Doh::new([127, 0, 0, 1].into(), "doh.test").json("/dns-query").port(9);
    let soon = Instant::now() + Duration::from_secs(1);
    for name in ["a b.example", "a&b.example", "x#y.example", "a%b.example"] {
        let missed = way.ask(&resolver, name, soon).await.unwrap_err();
        assert!(missed.why.contains("is not a DNS name"), "{name}: {missed}");
        let said = way.lookup_with(std::slice::from_ref(&resolver), name, soon).await.unwrap_err();
        assert!(said.contains("is not a DNS name"), "{name}: {said}");
    }
}
