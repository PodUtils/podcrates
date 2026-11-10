//! The way of the unit tests: TLS on ring, as each test of podcrates runs
//! it, and a direct dial with the system resolver.

use std::sync::Arc;

use pod_proxy::{Dialer, ProxyChoice, Settings};
use pod_tls::{Context, Words};

use crate::way::Way;

/// A direct way, with the TLS of a context of its own.
pub(crate) fn way() -> Way {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls = Arc::new(Context::new(provider, pod_tls::Settings::new(Words::new("pod-ws"))));
    let dial = Dialer::new(Settings::new("pod-ws", ProxyChoice::Direct), Arc::new(pod_proxy::resolve::System::new()));
    Way::new(tls, Arc::new(dial), "pod-ws/test")
}
