//! A binary that links pod-core and pod-tls with no default feature, so that
//! no tokio is in it. `scripts/check-static.sh` checks the binary on each
//! target, and `scripts/check-probe.py` checks the tree (docs/design.md,
//! section 7).

fn main() {
    let provider = std::sync::Arc::new(pod_tls::crypto::provider());
    let tls = pod_tls::Context::new(provider, pod_tls::Settings::new(pod_tls::Words::new("static-probe-lean")));
    let roots = pod_tls::roots_from_compiled_set();
    let tls_config = tls.client_config(&roots).expect("a client configuration");
    println!("pod-core: {} {}", pod_core::text::one_line("probe\nline"), pod_core::clock::utc_minutes(0));
    println!("pod-tls: {} roots, {} suites", roots.count, tls_config.crypto_provider().cipher_suites.len());
}
