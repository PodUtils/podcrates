//! A binary that links each crate of podcrates and calls one function of
//! each, so that the linker keeps them. `scripts/check-static.sh` checks the
//! binary on each target (docs/design.md, section 7). Each new crate adds
//! one line here.

fn main() {
    let target: pod_core::Target = "probe.example:443".parse().expect("a target");
    let mut discipline = pod_term::echo::Discipline::new();
    let typed: usize = b"probe\r".iter().map(|b| discipline.key(*b).len()).sum();
    let identity = pod_identity::Identity::from_seed(&[7; 32]);
    let provider = std::sync::Arc::new(pod_tls::crypto::provider());
    let tls = pod_tls::Context::new(provider, pod_tls::Settings::new(pod_tls::Words::new("static-probe")));
    let roots = pod_tls::roots_from_compiled_set();
    let tls_config = tls.client_config(&roots).expect("a client configuration");
    let environment = pod_proxy::Environment::read(&|_| None);
    let settings = pod_proxy::Settings::new("static-probe", pod_proxy::ProxyChoice::FromEnvironment(environment));
    let dialer = pod_proxy::Dialer::new(settings, std::sync::Arc::new(pod_proxy::resolve::System::new()));
    let mut services = pod_services::Catalog::new();
    // An empty file of the overlay: the TOML reader runs, and refuses it.
    let empty = services.lay("probe.toml", "").map_or_else(|e| e.field, |s| s.id().to_string());
    let doh = services.of_kind("doh").next().and_then(|s| s.endpoints().first()).map_or_else(String::new, |e| e.url());
    let lines = [
        format!("pod-core: {} {}", pod_core::text::one_line(&target.to_string()), pod_core::clock::utc_minutes(0)),
        format!("pod-term: {typed} events for a typed line"),
        format!("pod-identity: {}", identity.public().fingerprint()),
        format!(
            "pod-tls: {} roots, {} suites, ECH {}",
            roots.count,
            tls_config.crypto_provider().cipher_suites.len(),
            tls.ech().mode().word()
        ),
        format!(
            "pod-proxy: {}, {} names inside, MPTCP: {}",
            pod_core::Dial::label(&dialer),
            if pod_core::Dial::names_inside(&dialer) { "with" } else { "no" },
            pod_proxy::mptcp::probe().map_or_else(|why| why, |()| "offered".into())
        ),
        format!(
            "pod-services: {} services, the first DoH resolver {doh}, an empty overlay refused at {empty}",
            services.iter().count()
        ),
    ];
    for line in lines {
        println!("{line}");
    }
}
