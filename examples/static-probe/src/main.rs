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
    // A resolver of the default settings, whose resolvers are the data of
    // pod-services.
    let resolver = pod_dns::Resolver::new(pod_dns::Settings::default());
    let first = resolver.settings().resolvers.first().map_or_else(String::new, |r| format!("{} ({})", r.ip, r.name));
    // A broker with the fixed denies and an open policy: the policy reader,
    // the deny check and the dial of pod-proxy link in.
    let open = pod_broker::Policy::from_json(r#"{"version":1,"mode":"open"}"#).expect("a policy");
    let broker = pod_broker::Broker::new(
        pod_broker::Settings::default(),
        std::sync::Arc::new(pod_proxy::resolve::System::new()),
        pod_broker::upstream("static-probe", None, std::sync::Arc::new(pod_proxy::resolve::System::new())),
    );
    let metadata = pod_broker::DenyList::fixed().check_ip([169, 254, 169, 254].into()).unwrap_or("none").to_string();
    let mut lines = vec![
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
        format!(
            "pod-dns: {}, {} resolvers, the first {first}",
            pod_core::Resolve::label(&resolver),
            resolver.settings().resolvers.len()
        ),
    ];
    lines.push(format!(
        "pod-broker: {} jobs, mode {}, 169.254.169.254 denied by {metadata}",
        broker.list().len(),
        open.compile().map_or_else(|e| e.to_string(), |c| c.mode().word().to_string())
    ));
    // The SRP contexts of both sides: the OpenSSL of pod-gsr links in.
    let gsr = pod_gsr::Settings::of_data().and_then(|settings| {
        let host = pod_gsr::Identity::new("static-probe", &settings).host(&settings);
        pod_gsr::check_tls(&settings).map(|openssl| format!("{host}, {openssl}"))
    });
    lines.push(format!("pod-gsr: {}", gsr.unwrap_or_else(|e| e.to_string())));
    // The blocking face with a secret of no byte (with no argument): it
    // refuses before the dial, and its runtime links in.
    let secret = "x".repeat(std::env::args().len().saturating_sub(1));
    let blocking = pod_gsr::Settings::of_data().and_then(|settings| {
        let mut entropy = pod_core::entropy::OsEntropy;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        pod_gsr::blocking::open(&dialer, &settings, pod_gsr::Role::Connect, &secret, &mut entropy, deadline)
    });
    lines.push(format!("pod-gsr blocking: {}", blocking.map_or_else(|e| e.to_string(), |s| format!("{s:?}"))));
    lines.extend(hostprobe());
    lines.extend(supervise());
    lines.extend(guest());
    for line in lines {
        println!("{line}");
    }
}

/// pod-hostprobe is Linux only: on another target the crate is empty.
#[cfg(target_os = "linux")]
fn hostprobe() -> Option<String> {
    Some(format!(
        "pod-hostprobe: pid {}, ELF machine 0x{:x}, {} probes, {} free inodes at /",
        pod_hostprobe::sys::getpid(),
        pod_hostprobe::binfmt::SELF_MACHINE,
        pod_hostprobe::probes::PROBES.len(),
        pod_hostprobe::space::read("/").map_or_else(|e| e.to_string(), |h| h.free.inodes.to_string())
    ))
}

#[cfg(not(target_os = "linux"))]
fn hostprobe() -> Option<String> {
    None
}

/// pod-supervise is Linux only: on another target the crate is empty.
#[cfg(target_os = "linux")]
fn supervise() -> Option<String> {
    let s = pod_supervise::Settings::new(std::env::temp_dir().join("static-probe-supervise"));
    Some(format!(
        "pod-supervise: name {}, {} records",
        pod_supervise::generated_name(&s, &pod_supervise::new_id()),
        pod_supervise::table::read::<pod_supervise::table::Any>(&s)
            .map_or_else(|e| e.to_string(), |t| t.containers.len().to_string())
    ))
}

#[cfg(not(target_os = "linux"))]
fn supervise() -> Option<String> {
    None
}

/// pod-guest is Linux only: on another target the crate is empty.
#[cfg(target_os = "linux")]
fn guest() -> Option<String> {
    let s = pod_guest::Settings::new(std::env::temp_dir());
    let request = pod_guest::Request {
        command: "ver".into(),
        token: pod_guest::agent::nonce().unwrap_or_else(|e| e.to_string()),
        timeout: std::time::Duration::from_secs(1),
    };
    let files = pod_guest::mailbox(&s, pod_guest::agent::AGENT_CMD, &request)
        .map_or_else(|e| e.to_string(), |m| m.list().len().to_string());
    // An empty stream: the store and its digest link in.
    let dest = std::env::temp_dir().join(format!("static-probe-guest-{}.img", std::process::id()));
    let ceiling = pod_guest::Ceiling { max_bytes: 1, fsize: u64::MAX };
    let digest =
        pod_guest::fetch::store(std::io::empty(), Some(0), &dest, None, ceiling).unwrap_or_else(|e| e.to_string());
    let _ = std::fs::remove_file(&dest);
    Some(format!(
        "pod-guest: a mailbox of {files} files, {} arguments of the UEFI line, sha256 of nothing {digest}",
        pod_guest::argv(&pod_guest::Plan {
            emulator: "qemu-system-x86_64".into(),
            accel: pod_guest::accel_for(Some(pod_hostprobe::machine::Profile::Tcg)).unwrap_or(pod_guest::Accel::Tcg),
            share: "/usr/share/qemu".into(),
            firmware_code: "code.fd".into(),
            firmware_vars: "vars.fd".into(),
            root: "run.qcow2".into(),
            mailbox: "mailbox.img".into(),
            serial: "serial.log".into(),
            monitor: "qmp.sock".into(),
            memory_mib: 512,
            cpus: 1,
            emu_args: Vec::new(),
        })
        .len()
    ))
}

#[cfg(not(target_os = "linux"))]
fn guest() -> Option<String> {
    None
}
