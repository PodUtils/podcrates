//! The way of the tests: TLS on ring, as each test of podcrates runs it,
//! and a direct dial, a dial through a proxy, or a dial with pins.

#![allow(dead_code)]

use std::net::IpAddr;
use std::sync::Arc;

use pod_proxy::{Dialer, Proxy, ProxyChoice, Settings};
use pod_tls::{Context, Words};
use pod_ws::Way;

/// The user agent of the tests.
pub const USER_AGENT: &str = "pod-ws/test";

fn tls() -> Arc<Context> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Arc::new(Context::new(provider, pod_tls::Settings::new(Words::new("pod-ws"))))
}

fn through(proxy: ProxyChoice, pins: Vec<(String, IpAddr)>) -> Way {
    let resolve = Arc::new(pod_proxy::resolve::System::with_pins(pins));
    Way::new(tls(), Arc::new(Dialer::new(Settings::new("pod-ws", proxy), resolve)), USER_AGENT)
}

/// A direct way.
pub fn way() -> Way {
    through(ProxyChoice::Direct, Vec::new())
}

/// A direct way, with each name of `pins` at its address.
pub fn way_with_pins(pins: &[(&str, IpAddr)]) -> Way {
    through(ProxyChoice::Direct, pins.iter().map(|(n, ip)| (n.to_string(), *ip)).collect())
}

/// A way through the proxy of `url`.
pub fn way_via(url: &str) -> Way {
    through(ProxyChoice::Via(Proxy::parse(url).unwrap()), Vec::new())
}
