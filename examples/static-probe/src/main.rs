//! A binary that links each crate of podcrates and calls one function of
//! each, so that the linker keeps them. `scripts/check-static.sh` checks the
//! binary on each target (docs/design.md, section 7).

fn main() {
    let target: pod_core::Target = "probe.example:443".parse().expect("a target");
    let lines = [
        format!("pod-core: {} {}", pod_core::text::one_line(&target.to_string()), pod_core::clock::utc_minutes(0)),
    ];
    for line in lines {
        println!("{line}");
    }
}
