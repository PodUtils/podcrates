//! A binary that links pod-core, pod-tls, pod-wg, pod-confine, the STUN
//! codec of pod-turn, pod-udf, pod-beneath, pod-control and pod-unit with no
//! default feature, so that no tokio is in it.
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
    let delta = pod_natprobe::classify::ring_delta(65532, 1026);
    println!("pod-natprobe: ring delta {delta}");
    let udf = pod_udf::Udf::open(&[0u8; 4096][..]).map_or_else(|e| e.to_string(), |u| u.block_size().to_string());
    println!("pod-udf: {udf}");
    let methods = pod_control::Catalogue::core().entries().count();
    let schema = pod_control::schema().to_string().len();
    println!("pod-control: {methods} methods, a schema of {schema} bytes");
    let unit = pod_unit::Unit::new("static-probe", std::path::Path::new("/usr/local/bin/probe"), "static-probe-lean")
        .system(None);
    let text =
        pod_unit::systemd::render(&unit).map_or_else(|e| e.to_string(), |t| format!("{} lines", t.lines().count()));
    println!("pod-unit: a unit of the system of {text}");
    confine();
}

/// pod-confine is Linux only: on another target the crate is empty.
#[cfg(target_os = "linux")]
fn confine() {
    let abi = pod_confine::probe().map_or_else(|| "none".to_string(), |abi| abi.to_string());
    let rules = pod_confine::Deny::defaults().len();
    println!("pod-confine: Landlock ABI {abi}, {rules} calls in the deny list");
    let mut buf = [0u8; 64];
    let tmp = pod_beneath::open_under_base(&pod_beneath::Libc, b"/", b"/tmp", pod_beneath::O_PATH, &mut buf);
    println!("pod-beneath: /tmp under / gives {}", tmp.map_or_else(|r| r.to_string(), |_| "a descriptor".to_string()));
}

#[cfg(not(target_os = "linux"))]
fn confine() {}
