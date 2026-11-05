//! A binary that links each crate of podcrates and calls one function of
//! each, so that the linker keeps them. `scripts/check-static.sh` checks the
//! binary on each target (docs/design.md, section 7). Each new crate adds
//! one line here.

fn main() {
    let target: pod_core::Target = "probe.example:443".parse().expect("a target");
    let mut discipline = pod_term::echo::Discipline::new();
    let typed: usize = b"probe\r".iter().map(|b| discipline.key(*b).len()).sum();
    let identity = pod_identity::Identity::from_seed(&[7; 32]);
    let lines = [
        format!("pod-core: {} {}", pod_core::text::one_line(&target.to_string()), pod_core::clock::utc_minutes(0)),
        format!("pod-term: {typed} events for a typed line"),
        format!("pod-identity: {}", identity.public().fingerprint()),
    ];
    for line in lines {
        println!("{line}");
    }
}
