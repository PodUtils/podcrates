//! `experiments/365-namespace.sh` as integration tests: the denied-host half.
//!
//! The lane refuses unshare and mounts, so the probe selects below the
//! namespace rung and every run enters as before. Three clauses:
//! ns-lane-1 selects below namespace, ns-lane-2 enters chroot with a
//! banner reading mode=chroot and no namespace topology, ns-lane-3 reads
//! EnteredRung chroot. Direct assertions on the probe library: the lane
//! shape is built with the real mount row names, so the mount legs decide
//! by the names the banner quotes. No live engine, no mounts.

use podvm_probe::select::{Provides, Rung, Selection, Strictness};
use podvm_probe::verdict::Outcome;
use podvm_probe::{sys, Findings};

/// The lane shape from 365: unshare refused, both mount spellings refused,
/// chroot permitted, both controls answering. Real row names throughout.
fn lane() -> Findings {
    Findings {
        rows: vec![
            ("unshare(CLONE_NEWNS)", Outcome::denied(sys::EPERM)),
            ("clone(CLONE_NEWNS)", Outcome::denied(sys::EPERM)),
            ("mount(tmpfs,/mnt)", Outcome::denied(sys::EPERM)),
            (
                "mount(tmpfs,/mnt) in clone(NEWNS)",
                Outcome::denied(sys::EPERM),
            ),
            ("chroot(/tmp)", Outcome::ok()),
            ("pidfd_getfd(-1,-1) [control]", Outcome::denied(sys::EBADF)),
            ("kcmp(-1,-1,...) [control]", Outcome::denied(sys::ESRCH)),
        ],
        ..Findings::empty()
    }
}

/// ns-lane-1. The lane selects below namespace: creation refused and both
/// mount spellings refused, so the namespace rung is rejected with the
/// real mount row names in its evidence, and chroot is selected.
#[test]
fn lane_shape_selects_below_namespace() {
    let s = Selection::choose(&lane());
    assert!(
        s.rung != Rung::Namespace,
        "lane selected {:?}, want below namespace",
        s.rung
    );
    assert_eq!(s.rung, Rung::Chroot, "lane rung is not chroot");
    let why = s
        .rejected
        .iter()
        .find(|r| r.rung == Rung::Namespace)
        .expect("no namespace rejection on the lane shape");
    assert!(
        why.evidence.contains("mount(tmpfs,/mnt)="),
        "the rejection hides the mount leg: {}",
        why.evidence
    );
    assert!(
        why.evidence.contains("mount(tmpfs,/mnt) in clone(NEWNS)="),
        "the rejection hides the clone mount leg: {}",
        why.evidence
    );
    assert!(s.controls_answered, "the lane controls stopped answering");
}

/// ns-lane-2. The entered banner reads mode=chroot and names no namespace
/// fallback: the chroot rung mounts nothing and its namespaces cell never
/// reads mount-only, and the never-claim guards what it must not imply.
#[test]
fn lane_banner_names_chroot_and_no_namespace_topology() {
    let f = lane();
    let s = Selection::choose(&f);
    assert_eq!(s.rung.word(), "chroot", "lane word is not chroot");
    let p = Provides::of(s.rung, &f);
    assert_eq!(p.mounts, "none", "chroot mounts read {}", p.mounts);
    assert!(
        p.namespaces != "mount-only",
        "chroot namespaces read mount-only"
    );
    assert!(
        Rung::Chroot.must_never_claim().contains("/proc"),
        "the chroot never-claim names no /proc guard"
    );
}

/// ns-lane-3. EnteredRung reads chroot, and strict refuses the lane rung
/// while a plain probe run still exits 0.
#[test]
fn lane_entered_word_reads_chroot() {
    let s = Selection::choose(&lane());
    assert_eq!(s.rung.word(), "chroot", "the entered word is not chroot");
    assert!(
        !s.meets(Selection::STRICT_FLOOR),
        "a lane rung meets the namespace floor"
    );
    assert_eq!(
        s.exit_code(Strictness::Refuse),
        1,
        "strict accepts a lane rung"
    );
    assert_eq!(
        s.exit_code(Strictness::Warn),
        0,
        "a plain probe run must exit 0"
    );
}
