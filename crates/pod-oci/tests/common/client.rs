//! A client of the crate for the integration tests: the HTTP client of
//! `pod-proxy` over the proxy settings of the test, the compiled-in roots,
//! and the endpoints of the test named insecure.

use std::sync::Arc;

use pod_oci::Client;
use pod_oci::transport::Policy;
use pod_proxy::{Environment, ProxyChoice, Settings};

/// The proxy settings of `pairs`, as the environment of a process would
/// give them.
pub fn environment(pairs: &[(&str, &str)]) -> Environment {
    let owned: Vec<(String, String)> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    Environment::read(&move |name: &str| owned.iter().find(|(k, _)| k == name).map(|(_, v)| v.into()))
}

/// A client over `env`, with `insecure` named insecure. The trust of an
/// insecure registry is the default trust: the fixture speaks plain HTTP,
/// so the TLS try fails and the client goes to HTTP.
pub fn client(env: Environment, insecure: &[&str]) -> Client {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = Arc::new(pod_tls::Context::new(provider, pod_tls::Settings::new(pod_tls::Words::new("pod-oci"))));
    let dialer = pod_proxy::blocking::Dialer::new(
        Settings::new("pod-oci", ProxyChoice::FromEnvironment(env)),
        Arc::new(pod_proxy::resolve::System::new()),
    )
    .expect("the runtime of the dialer starts");
    let http = pod_proxy::blocking::Client::new(context, &dialer).with_user_agent("pod-oci-test");
    Client::new(http, pod_tls::Trust::Default)
        .with_policy(Policy::with_insecure(insecure))
        .with_insecure(pod_tls::Trust::Default)
}
