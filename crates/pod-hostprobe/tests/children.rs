//! The probe children of the whole set. libtest does not know the child
//! flag, so this test binary has no libtest (`harness = false`): it reads the
//! flag first, as the binary of a consumer does, and then runs the set.
//!
//! Each row must come back with a verdict that its child established: no
//! row is a skip because the child said nothing, spoke a line that the
//! parent cannot read, or disagreed with its own exit status.
//!
//! Before the run, the test process takes a private mount namespace where it
//! can, so that a probe that mounts as root changes no mount of the host.
//! Where it cannot, the probes cannot mount either.

#[cfg(target_os = "linux")]
fn main() {
    use pod_hostprobe::verdict::Verdict;
    use pod_hostprobe::{Settings, probes, run, run_child, sys, writable};

    let settings = Settings::default();
    let args: Vec<String> = std::env::args().collect();
    if args.get(1) == Some(&settings.child_flag) {
        let name = args.get(2).map_or("", String::as_str);
        std::process::exit(run_child(name, &mut std::io::stdout()));
    }

    // One thread here: no libtest. The arguments of `cargo test` (a filter)
    // do not apply to this binary.
    let private = sys::unshare(sys::CLONE_NEWNS).is_ok() && {
        let (none, slash) = (sys::cempty(), sys::CBuf::new("/").expect("no NUL"));
        sys::mount(&none, &slash, &none, sys::MS_REC | sys::MS_PRIVATE).is_ok()
    };

    let f = run(&settings, &writable::candidates(&[]));
    assert_eq!(f.rows.len(), probes::PROBES.len());
    let mut counts = [0usize; 3];
    let unread = ["said nothing this parent can read", "spoke a line this parent cannot read", "disagree"];
    for (name, o) in &f.rows {
        match o.verdict {
            Verdict::Ok => counts[0] += 1,
            Verdict::Denied => {
                assert!(o.errno.is_some(), "{name} denied with no errno");
                counts[1] += 1;
            }
            Verdict::Skip => {
                assert!(
                    !unread.iter().any(|u| o.reason.contains(u)),
                    "{name}: the child gave no verdict: {}",
                    o.reason
                );
                counts[2] += 1;
            }
        }
    }
    println!(
        "children: {} rows, {} ok, {} denied, {} skipped; private mount namespace: {}; children from {}",
        f.rows.len(),
        counts[0],
        counts[1],
        counts[2],
        if private { "yes" } else { "no" },
        f.self_exe
    );
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("children: skipping, pod-hostprobe is Linux only");
}
