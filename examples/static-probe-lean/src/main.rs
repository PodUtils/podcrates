//! A binary that links pod-core, pod-tls, pod-wg, pod-confine and the STUN
//! codec of pod-turn with no default feature, so that no tokio is in it.
//! `scripts/check-static.sh` checks the binary on each target, and
//! `scripts/check-probe.py` checks the tree (docs/design.md, section 7).

fn main() {
    let provider = std::sync::Arc::new(pod_tls::crypto::provider());
    let tls = pod_tls::Context::new(provider, pod_tls::Settings::new(pod_tls::Words::new("static-probe-lean")));
    let roots = pod_tls::roots_from_compiled_set();
    let tls_config = tls.client_config(&roots).expect("a client configuration");
    println!("pod-core: {} {}", pod_core::text::one_line("probe\nline"), pod_core::clock::utc_minutes(0));
    println!("pod-tls: {} roots, {} suites", roots.count, tls_config.crypto_provider().cipher_suites.len());
    let keys = pod_wg::Keys { private: [1; 32], peer_public: [2; 32], reserved: [0; 3], preshared: None };
    let init = pod_wg::initiate(&keys).map_or_else(|e| e.to_string(), |i| format!("{} bytes", i.message.len()));
    println!("pod-wg: an initiation of {init}");
    let key = pod_turn::stun::long_term_key("probe", "probe.example", "probe");
    let signed = pod_turn::stun::signed(&pod_turn::stun::binding_request(&[7; 12]), &key);
    println!("pod-turn: a signed Binding request of {} bytes", signed.len());
    confine();
}

/// pod-confine is Linux only: on another target the crate is empty.
#[cfg(target_os = "linux")]
fn confine() {
    let abi = pod_confine::probe().map_or_else(|| "none".to_string(), |abi| abi.to_string());
    let rules = pod_confine::Deny::defaults().len();
    println!("pod-confine: Landlock ABI {abi}, {rules} calls in the deny list");
}

#[cfg(not(target_os = "linux"))]
fn confine() {}
