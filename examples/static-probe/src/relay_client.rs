//! pod-relay: a forward session of a context that is offline, so that the
//! opener, the failover, the token and the roads link in and nothing
//! connects; the blocking facade's runtime; the pool, the code, the grants
//! and the TLS of the QUIC road.

use std::sync::Arc;

use pod_relay::cache::Cache;
use pod_relay::{Context, Settings};

pub fn relay() -> String {
    let provider = Arc::new(pod_tls::crypto::provider());
    let tls = pod_tls::Context::new(provider.clone(), pod_tls::Settings::new(pod_tls::Words::new("static-probe")));
    let settings = pod_proxy::Settings::new("static-probe", pod_proxy::ProxyChoice::Direct);
    let dialer = pod_proxy::Dialer::new(settings, Arc::new(pod_proxy::resolve::System::new()));
    let way = pod_ws::Way::new(Arc::new(tls), Arc::new(dialer), "static-probe");
    let ctx = Context::new(way, Cache::none()).with_settings(Settings { offline: true, ..Settings::default() });
    let relays = pod_relay::relay::select_relays(None, None, &[]).map_or(0, |list| list.hosts.len());
    let path = pod_relay::relay::forward_path("probe.example", 22).unwrap_or_default();
    let opened = match pod_relay::blocking::Client::new(pod_relay::blocking::Config::new(ctx)) {
        Ok(client) => match pod_relay::relay::select_relays(None, None, &[]) {
            Ok(list) => {
                client.open_forward(&list, "probe.example", 22).map_or_else(|e| e.to_string(), |_| "opened".into())
            }
            Err(e) => e,
        },
        Err(e) => e.to_string(),
    };
    let pool =
        pod_relay::pool::parse_pool(br#"{"pool":[]}"#, pod_relay::default_relay_host()).err().unwrap_or_default();
    let grant = pod_relay::grant::parse_public("ssh-ed25519 AAAA probe").is_some();
    let quic = pod_relay::quic_road::dial::tls(provider, &pod_tls::roots_from_compiled_set())
        .map_or_else(|e| e, |_| "a QUIC TLS".into());
    format!(
        "pod-relay: {relays} hosts, {path}, offline: {opened}, a pool: {pool}, a grant key: {grant}, {quic}, a code of {} s",
        pod_relay::code::LIFETIME.as_secs()
    )
}
