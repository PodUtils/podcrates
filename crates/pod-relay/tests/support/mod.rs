//! The way out of the tests: a direct dial over the system's resolver, TLS
//! on ring, and a context with no cache or with the cache of a test.

// Each test binary uses a part of it.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;

use pod_relay::Context;
use pod_relay::cache::Cache;

/// A dial of `proxy` over the system's resolver.
fn dial_of(proxy: pod_proxy::ProxyChoice) -> Arc<pod_proxy::Dialer> {
    let settings = pod_proxy::Settings::new("pod-relay", proxy);
    Arc::new(pod_proxy::Dialer::new(settings, Arc::new(pod_proxy::resolve::System::new())))
}

/// A direct dial over the system's resolver.
pub fn dialer() -> Arc<pod_proxy::Dialer> {
    dial_of(pod_proxy::ProxyChoice::Direct)
}

/// The way of the tests through `dialer`.
fn way_through(dialer: Arc<pod_proxy::Dialer>) -> pod_ws::Way {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls = Arc::new(pod_tls::Context::new(provider, pod_tls::Settings::new(pod_tls::Words::new("pod-relay"))));
    pod_ws::Way::new(tls, dialer, "pod-relay/test")
}

/// The way of the tests.
pub fn way() -> pod_ws::Way {
    way_through(dialer())
}

/// A context through `dialer`, which names the proxy of each host.
fn through(dialer: Arc<pod_proxy::Dialer>) -> &'static Context {
    let named = dialer.clone();
    let proxy_of = Arc::new(move |host: &str| named.proxy_for(host).ok().flatten());
    let ctx = Context::new(way_through(dialer), Cache::none()).with_proxy_of(proxy_of);
    // Each live test makes its own, which lives as long as the test binary.
    Box::leak(Box::new(ctx))
}

/// The context of a live test: the proxy that the environment of the test
/// names, read and never set, and a resolver with the pins of
/// `PODSSH_LIVE_RELAY_ADDR` (`NAME=ADDR,...`).
pub fn live() -> &'static Context {
    let environment = pod_proxy::Environment::read(&|name| std::env::var_os(name));
    let pins = std::env::var("PODSSH_LIVE_RELAY_ADDR").ok().and_then(|p| pod_dns::parse_pins(&p).ok());
    let resolver = Arc::new(pod_dns::Resolver::new(pod_dns::Settings {
        pins: pins.unwrap_or_default(),
        ..pod_dns::Settings::default()
    }));
    let settings = pod_proxy::Settings::new("pod-relay", pod_proxy::ProxyChoice::FromEnvironment(environment));
    let dialer = Arc::new(pod_proxy::Dialer::new(settings, resolver.clone()));
    let ctx = through(dialer);
    let ctx = ctx.clone().with_resolver(resolver);
    Box::leak(Box::new(ctx))
}

/// The context of a live test through `proxy`.
pub fn via(proxy: pod_proxy::Proxy) -> &'static Context {
    through(dial_of(pod_proxy::ProxyChoice::Via(proxy)))
}

/// A context with no cache.
pub fn context() -> Context {
    Context::new(way(), Cache::none())
}

/// A context with its cache in `dirs`.
pub fn context_in(dirs: Vec<PathBuf>) -> Context {
    Context::new(way(), Cache::in_dirs(dirs))
}
