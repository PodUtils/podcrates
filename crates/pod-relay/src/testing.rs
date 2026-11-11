//! The context of the unit tests: a direct dial, TLS on ring, no cache.

use std::sync::Arc;

use crate::cache::Cache;
use crate::context::Context;

/// The way of the tests.
pub fn way() -> pod_ws::Way {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls = Arc::new(pod_tls::Context::new(provider, pod_tls::Settings::new(pod_tls::Words::new("pod-relay"))));
    let settings = pod_proxy::Settings::new("pod-relay", pod_proxy::ProxyChoice::Direct);
    let dial = pod_proxy::Dialer::new(settings, Arc::new(pod_proxy::resolve::System::new()));
    pod_ws::Way::new(tls, Arc::new(dial), "pod-relay/test")
}

/// A context with no cache.
pub fn context() -> Context {
    Context::new(way(), Cache::none())
}

/// A context with its cache in `dirs`.
pub fn context_in(dirs: Vec<std::path::PathBuf>) -> Context {
    Context::new(way(), Cache::in_dirs(dirs))
}
