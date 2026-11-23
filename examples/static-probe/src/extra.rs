//! The static probes of `pod-cover`, `pod-shape`, `pod-harness`,
//! `pod-sidecar` and `pod-offline`.

/// pod-cover: the switch, the proof length and the mux link in.
pub fn cover() -> String {
    let switch = pod_cover::Switch::new([7u8; 32]).bind(&[2u8; 32]);
    let proof = switch.prove([1u8; 16], 1_700_000_000);
    format!("pod-cover: {} bytes in a switch proof", proof.len())
}

/// pod-shape: the built-in traffic profiles and the shaper link in.
pub fn shape() -> String {
    let profile = pod_shape::Profile::load("page-load").expect("page-load profile");
    let mut shaper = pod_shape::Shaper::new(profile, 7);
    let shaped = shaper.shape(5);
    format!("pod-shape: {} profiles, shaped {} bytes", pod_shape::NAMES.len(), shaped.body)
}

/// pod-harness: the claim checker and the budget link in.
pub fn harness() -> String {
    let findings = pod_harness::check_claims("No claim here.");
    format!("pod-harness: {} findings, budget {} s", findings.len(), pod_harness::BUDGET_SECS)
}

/// pod-sidecar: the simulator and the disagreement probe link in.
pub fn sidecar() -> String {
    let mut silent = pod_sidecar::probe::Silent;
    let finding = pod_sidecar::Probe::run(&mut silent);
    let raw = pod_sidecar::probe::raw_capability().map_or_else(|r| r.reason().to_string(), |()| "available".into());
    format!("pod-sidecar: enabled {}, silent probe {}, raw {raw}", pod_sidecar::ENABLED, finding.verdict)
}

/// pod-offline: the serial address parser and the KISS frame codec link in.
pub fn offline() -> String {
    let spec = pod_offline::serial::SerialSpec::parse("serial:/dev/ttyUSB0,115200").expect("a serial spec");
    let frame = pod_offline::serial::kiss::encode(pod_offline::serial::kiss::PORT_AX25, b"probe").expect("KISS");
    format!("pod-offline: {}, {} bytes in a KISS frame", spec.address(), frame.len())
}

/// pod-natprobe: the sans-IO probe machine, simulator and JSON report link in.
pub fn natprobe() -> String {
    let pp: std::net::SocketAddr = "203.0.113.1:3478".parse().expect("pp");
    let aa: std::net::SocketAddr = "203.0.113.2:3479".parse().expect("aa");
    let mut world = pod_natprobe::sim::SimWorld::new(
        std::net::Ipv4Addr::new(192, 168, 1, 10),
        vec![pod_natprobe::sim::NatSpec::home_eim_apdf(std::net::IpAddr::V4(std::net::Ipv4Addr::new(
            198, 51, 100, 10,
        )))],
        vec![pod_natprobe::sim::SimStunServer::rfc5780("s1", pp, aa)],
        1,
    );
    let mut cfg = pod_natprobe::Settings::for_profile(
        pod_natprobe::Profile::Fast,
        vec![pod_natprobe::ServerEndpoint::new("s1", pp)],
    )
    .with_addr_policy(pod_natprobe::wire::AddrPolicy::Lab);
    cfg.run_nat64 = false;
    cfg.run_portmap = false;
    cfg.run_ipv6 = false;
    let (rep, _) = pod_natprobe::sim::drive_sim(cfg, &mut world, 1).expect("sim");
    let _ = pod_natprobe::Prober::new();
    let json_len = rep.to_json().map_or(0, |j| j.len());
    format!("pod-natprobe: {}, {} bytes JSON", rep, json_len)
}
