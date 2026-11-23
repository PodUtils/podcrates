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
