//! A binary that links pod-core, pod-tls and pod-wg with no default feature,
//! so that no tokio is in it. `scripts/check-static.sh` checks the binary on each
//! target, and `scripts/check-probe.py` checks the tree (docs/design.md,
//! section 7).

fn main() {
    let provider = std::sync::Arc::new(pod_tls::crypto::provider());
    let tls = pod_tls::Context::new(provider, pod_tls::Settings::new(pod_tls::Words::new("static-probe-lean")));
    let roots = pod_tls::roots_from_compiled_set();
    let tls_config = tls.client_config(&roots).expect("a client configuration");
    println!("pod-core: {} {}", pod_core::text::one_line("probe\nline"), pod_core::clock::utc_minutes(0));
    println!("pod-tls: {} roots, {} suites", roots.count, tls_config.crypto_provider().cipher_suites.len());
    let keys = pod_wg::Keys { private: [1; 32], peer_public: [2; 32], reserved: [0; 3] };
    let init = pod_wg::initiate(&keys).map_or_else(|e| e.to_string(), |i| format!("{} bytes", i.message.len()));
    println!("pod-wg: an initiation of {init}");
}
