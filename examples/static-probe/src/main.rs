//! A binary that links each crate of podcrates and calls one function of
//! each, so that the linker keeps them. `scripts/check-static.sh` checks the
//! binary on each target (docs/design.md, section 7). Each new crate adds
//! one line here.

mod acme;
mod actions;
mod cloudflare;
mod devtunnel;
mod egress;
mod extra;
mod hold;
mod host;
mod iroh;
mod link;
mod manual;
mod netlab;
mod qga;
mod relay;
mod relay_client;
mod release;
mod tor;
mod transit;
mod turn;
mod udf;
mod warp;
mod wormhole;

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
    // The blocking face: its private runtime links in.
    let blocking_dialer = pod_proxy::blocking::Dialer::new(
        pod_proxy::Settings::new(
            "static-probe",
            pod_proxy::ProxyChoice::FromEnvironment(pod_proxy::Environment::read(&|_| None)),
        ),
        std::sync::Arc::new(pod_proxy::resolve::System::new()),
    );
    let blocking_proxy = blocking_dialer
        .and_then(|d| d.proxy_at("probe.example", 443).map_err(std::io::Error::other))
        .map_or_else(|e| e.to_string(), |p| format!("{p:?}"));
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
    let open = pod_broker::Policy::from_json(r#"{"schema":1,"mode":"open"}"#).expect("a policy");
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
        format!("pod-proxy blocking: {blocking_proxy}"),
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
    lines.extend(host::hostprobe());
    lines.extend(host::supervise());
    lines.extend(hold::hold());
    lines.extend(host::guest());
    lines.extend(host::enter());
    lines.extend(host::confine());
    lines.extend(host::beneath());
    lines.extend(lab());
    lines.extend(oci(tls));
    lines.extend(extract());
    lines.extend(rootfs());
    lines.push(ws());
    lines.push(wormhole::wormhole());
    lines.push(quic());
    lines.push(netstack());
    lines.push(mutual());
    lines.extend([relay::session(), relay::derp()]);
    lines.extend([relay::e2e(), relay::wg()]);
    lines.push(relay_client::relay());
    lines.push(job());
    lines.push(incus());
    lines.push(qga::qga());
    lines.extend([actions::actions(), actions::results()]);
    lines.push(netlab::netlab());
    lines.extend([turn::turn(), transit::transit()]);
    lines.extend([tor::tor(), iroh::iroh()]);
    lines.extend([egress::egress(), acme::acme()]);
    lines.extend([udf::udf(), manual::manual()]);
    lines.extend([extra::cover(), extra::shape(), extra::harness(), extra::sidecar(), extra::offline()]);
    let direct = pod_proxy::Dialer::new(
        pod_proxy::Settings::new("static-probe", pod_proxy::ProxyChoice::Direct),
        std::sync::Arc::new(pod_proxy::resolve::System::new()),
    );
    let direct: std::sync::Arc<dyn pod_core::Dial> = std::sync::Arc::new(direct);
    let release_tls = std::sync::Arc::new(pod_tls::Context::new(
        std::sync::Arc::new(pod_tls::crypto::provider()),
        pod_tls::Settings::new(pod_tls::Words::new("static-probe")),
    ));
    lines.push(release::release(pod_proxy::Client::new(release_tls, direct.clone())));
    lines.push(link::link(direct.clone()));
    lines.push(warp::warp(std::sync::Arc::new(pod_tls::crypto::provider()), direct.clone()));
    lines.push(cloudflare::cloudflare(direct.clone()));
    lines.push(devtunnel::devtunnel(std::sync::Arc::new(pod_tls::crypto::provider()), direct));
    for line in lines {
        println!("{line}");
    }
}

/// pod-lab is Linux only: on another target the crate holds its exit
/// codes alone. A check of a profile against an empty set: the table, the
/// check and the BPF program of a filter link in.
#[cfg(target_os = "linux")]
fn lab() -> Option<String> {
    let p = pod_lab::find(pod_lab::PROFILES, "chroot-denied")?;
    let v = pod_lab::check(p, &pod_hostprobe::Findings::empty());
    Some(format!("pod-lab: {} profiles, {} {}", pod_lab::PROFILES.len(), v.profile, v.parts[0].state.word()))
}

#[cfg(not(target_os = "linux"))]
fn lab() -> Option<String> {
    Some(format!("pod-lab: the exit code of a usage error is {}", pod_lab::codes::EXIT_USAGE))
}

/// pod-oci is Linux only: on another target the crate is empty. A client
/// with no request: the parse of a reference, the platform, the policy, the
/// roots and the HTTP client of `pod-proxy` link in.
#[cfg(target_os = "linux")]
fn oci(tls: pod_tls::Context) -> Option<String> {
    let reference = pod_oci::Reference::parse("alpine").map_or_else(|e| e.to_string(), |r| r.to_string());
    let roots = pod_oci::trust::roots(pod_oci::trust::host_bundle(&|_| None).as_ref());
    let dialer = pod_proxy::blocking::Dialer::new(
        pod_proxy::Settings::new("static-probe", pod_proxy::ProxyChoice::Direct),
        std::sync::Arc::new(pod_proxy::resolve::System::new()),
    );
    let client = dialer.map(|d| {
        let http = pod_proxy::blocking::Client::new(std::sync::Arc::new(tls), &d);
        pod_oci::Client::new(http, pod_tls::Trust::Default)
            .with_policy(pod_oci::transport::Policy::with_insecure(&["localhost:5000"]))
    });
    let policy = client.map_or_else(|e| e.to_string(), |c| c.policy.disclosure("localhost:5000").unwrap_or_default());
    Some(format!("pod-oci: {reference} for {}, {} roots, {policy}", pod_oci::Platform::host(), roots.count))
}

#[cfg(not(target_os = "linux"))]
fn oci(_tls: pod_tls::Context) -> Option<String> {
    None
}

/// pod-extract is Linux only: on another target the crate is empty.
#[cfg(target_os = "linux")]
fn extract() -> Option<String> {
    // An image with no layer: the space check, the sidecar and the marker
    // link in; the decompressors link in through the media types.
    let base = std::env::temp_dir().join(format!("static-probe-extract-{}", std::process::id()));
    let place = pod_extract::Place::under(&base);
    let done = pod_extract::extract(&pod_extract::Settings::default(), "static-probe", &[], &place)
        .map_or_else(|e| e.to_string(), |x| format!("{} layers, extracted {}", x.layers, place.is_extracted()));
    let _ = place.remove();
    let _ = std::fs::remove_dir(&base);
    let zstd = pod_extract::layer::Compression::of("application/vnd.oci.image.layer.v1.tar+zstd")
        .map_or_else(|e| e.to_string(), |c| format!("{c:?}"));
    Some(format!(
        "pod-extract: {done}, zstd layer {zstd}, openat2 {}, .wh.x is {:?}",
        pod_extract::safety::have_openat2(),
        pod_extract::whiteout::classify(".wh.x")
    ))
}

#[cfg(not(target_os = "linux"))]
fn extract() -> Option<String> {
    None
}

/// pod-rootfs is Linux only: on another target the crate is empty.
#[cfg(target_os = "linux")]
fn rootfs() -> Option<String> {
    // An empty root: each fixup links in, and the probe of a mirror answers
    // with no request.
    let base = std::env::temp_dir().join(format!("static-probe-rootfs-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&base);
    let done = pod_rootfs::complete(&base, &pod_rootfs::Settings::default(), &mut pod_rootfs::reach::Fixed(false))
        .map_or_else(
            |e| e.to_string(),
            |r| {
                format!(
                    "{} rows, {} changed, {} failed, libc {}",
                    r.fixups.len(),
                    r.changed(),
                    r.failed(),
                    r.libc.word()
                )
            },
        );
    let _ = std::fs::remove_dir_all(&base);
    Some(format!("pod-rootfs: {done}, deb.debian.org is a name {}", pod_rootfs::reach::is_name("deb.debian.org")))
}

#[cfg(not(target_os = "linux"))]
fn rootfs() -> Option<String> {
    None
}

/// pod-ws: a way through a direct dialer, with ECH and the records of
/// pod-dns; the frames, the accept value, a pool of HTTP/2 and the
/// settings of the accept loop link in.
fn ws() -> String {
    let provider = std::sync::Arc::new(pod_tls::crypto::provider());
    let tls = pod_tls::Context::new(provider, pod_tls::Settings::new(pod_tls::Words::new("static-probe")));
    let settings = pod_proxy::Settings::new("static-probe", pod_proxy::ProxyChoice::Direct);
    let dialer = pod_proxy::Dialer::new(settings, std::sync::Arc::new(pod_proxy::resolve::System::new()));
    let way = pod_ws::Way::new(std::sync::Arc::new(tls), std::sync::Arc::new(dialer), "static-probe");
    let frame = pod_ws::Frame { fin: true, opcode: pod_ws::frame::OPCODE_BINARY, payload: b"probe".to_vec() };
    let bytes = pod_ws::frame::encode(&frame, pod_ws::frame::Role::Client, [1, 2, 3, 4]);
    let pool = pod_ws::h2_road::Pool::new();
    format!(
        "pod-ws: {} bytes for a frame of 5, accept {}, way {}, {pool:?}, limits {:?}, {} bytes before ready",
        bytes.len(),
        pod_ws::handshake::accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
        way.label(),
        pod_ws::Limits::default(),
        pod_ws::serve::Serve::default().reserve()
    )
}

/// pod-tls: a server of mutual TLS with a pinned key, on the pure
/// provider: the verifier of a client and the pins link in.
fn mutual() -> String {
    let provider = std::sync::Arc::new(pod_tls::crypto::provider());
    let pin = pod_tls::mutual::Pin::of_spki(b"probe");
    let pins = pod_tls::mutual::Pins::new([pin]);
    let served = pod_tls::server::Served::new(None);
    let config = pod_tls::mutual::server_config(provider, served, pins.clone(), &[])
        .map_or_else(|e| e.to_string(), |_| "a server of mutual TLS".into());
    format!("pod-tls: {config}, pins {}", pins.list().len())
}

/// pod-quic: the probe's packet, a client configuration on the pure
/// provider, and an endpoint over the system's datagrams in a runtime of
/// one thread: noq, the socket over Datagrams and the keys link in.
fn quic() -> String {
    let provider = std::sync::Arc::new(pod_tls::crypto::provider());
    let roots = std::sync::Arc::new(pod_tls::roots_from_compiled_set().roots);
    let settings = pod_quic::Settings::default();
    let config = pod_quic::connect::tls(provider, roots, &[b"h3"])
        .and_then(|tls| pod_quic::connect::client_config(tls, &settings, false))
        .map_or_else(|e| e.to_string(), |_| "a client configuration".into());
    let endpoint = tokio::runtime::Builder::new_current_thread().enable_all().build().map_or_else(
        |e| e.to_string(),
        |runtime| {
            runtime.block_on(async {
                let datagrams = pod_core::SystemDatagrams::bind(([127, 0, 0, 1], 0).into()).await;
                let endpoint = datagrams
                    .map_err(|e| e.to_string())
                    .and_then(|d| pod_quic::over(std::sync::Arc::new(d), None).map_err(|e| e.to_string()));
                endpoint.map_or_else(|e| e, |e| format!("an endpoint at {:?}", e.local_addr().ok()))
            })
        },
    );
    format!("pod-quic: a probe of {} bytes, {config}, {endpoint}", pod_quic::probe::packet(&[0; 8]).len())
}

/// pod-netstack: one UDP datagram of 4000 bytes through a loopback stack in
/// a runtime of one thread: smoltcp with its IPv4 fragments (MTU 1280), the
/// reactor and the device link in.
fn netstack() -> String {
    let done = tokio::runtime::Builder::new_current_thread().enable_all().build().map_or_else(
        |e| e.to_string(),
        |runtime| {
            runtime.block_on(async {
                let mut net = pod_netstack::NetStack::loopback();
                let addr = std::net::SocketAddr::from(([10, 66, 0, 2], 5353));
                let mut socket = match net.bind_udp(addr).await {
                    Ok(socket) => socket,
                    Err(e) => return e.to_string(),
                };
                let mut buffer = [0u8; 4096];
                let sent = socket.send_to(&[7u8; 4000], addr).await;
                let wait = std::time::Duration::from_secs(2);
                let got = tokio::time::timeout(wait, socket.recv_from(&mut buffer)).await;
                net.shutdown().await;
                match (sent, got) {
                    (Ok(_), Ok(Ok((n, from)))) => format!("{n} bytes from {from}"),
                    (sent, got) => format!("no datagram: {sent:?} {got:?}"),
                }
            })
        },
    );
    format!("pod-netstack: {done}, at most {} addresses", pod_netstack::MAX_ADDRESSES)
}

/// pod-incus: a call to a socket where no incus answers: the client, the
/// HTTP reader of pod-proxy and the refusal by name link in.
fn incus() -> String {
    let client = pod_incus::Client::new(pod_incus::Settings::new("/nonexistent/incus/unix.socket"));
    let answer = tokio::runtime::Builder::new_current_thread().enable_all().build().map_or_else(
        |e| e.to_string(),
        |runtime| runtime.block_on(client.server()).map_or_else(|e| e.to_string(), |s| s.environment.server_version),
    );
    format!("pod-incus: {answer}")
}

/// pod-job: an offer from JSON, its check, and the public report of a
/// refused job: the readers and the writers of serde link in.
fn job() -> String {
    let offer = serde_json::from_str::<pod_job::JobOffer>(r#"{"schema":1,"tier":"trusted"}"#)
        .map_or_else(|e| e.to_string(), |o| format!("{:?}", o.check(false)));
    // From JSON: an optional field that the crate adds needs no change here.
    let report: pod_job::RunReport = serde_json::from_str(
        r#"{"schema":1,"job_id":"probe-1","source":"probe","tier":"untrusted","host":"probe.example",
            "exit_reason":"refused","wall_s":0,"refusal_code":"tier-needs-guest"}"#,
    )
    .expect("a refused report");
    let public = serde_json::to_string(&report.public()).unwrap_or_default();
    format!("pod-job: an offer with a tier: {offer}; check {:?}; {public}", report.check())
}
