use super::*;
use crate::sys;
use crate::verdict::Outcome;

fn findings(rows: &[(&'static str, Outcome)]) -> Findings {
    Findings {
        rows: rows.to_vec(),
        ..Findings::empty()
    }
}

fn target_shape() -> Findings {
    // The reconstruction of TOOL.md section 3: namespaces creatable by
    // clone, every mount attach denied, chroot permitted, ptrace filtered.
    // Both the borrowed rows (census, attribution) and the supervise
    // tier's own rows run, so both are in the fixture.
    findings(&[
        ("unshare(CLONE_NEWNS)", Outcome::denied(sys::EPERM)),
        ("clone(CLONE_NEWNS)", Outcome::ok()),
        ("mount(tmpfs,/mnt)", Outcome::denied(sys::EPERM)),
        (
            "mount(tmpfs,/mnt) in clone(NEWNS)",
            Outcome::denied(sys::EPERM),
        ),
        ("seccomp(NEW_LISTENER)", Outcome::ok()),
        ("open /proc/self/mem O_RDONLY", Outcome::ok()),
        ("ptrace(PTRACE_TRACEME)", Outcome::denied(sys::EPERM)),
        ("seccomp(NEW_LISTENER) [supervise]", Outcome::ok()),
        ("process_vm_readv(own pid) [supervise]", Outcome::ok()),
        (
            "ptrace(PTRACE_TRACEME) [supervise]",
            Outcome::denied(sys::EPERM),
        ),
        ("chroot(/tmp)", Outcome::ok()),
        ("pidfd_getfd(-1,-1) [control]", Outcome::denied(sys::EBADF)),
        ("kcmp(-1,-1,...) [control]", Outcome::denied(sys::ESRCH)),
    ])
}

#[test]
fn the_target_shape_selects_chroot() {
    assert_eq!(Selection::choose(&target_shape()).rung, Rung::Chroot);
}

#[test]
fn an_unconfined_host_selects_namespace() {
    let f = findings(&[
        ("unshare(CLONE_NEWNS)", Outcome::ok()),
        ("clone(CLONE_NEWNS)", Outcome::ok()),
        ("mount(tmpfs,/mnt)", Outcome::ok()),
        ("chroot(/tmp)", Outcome::ok()),
        ("pidfd_getfd(-1,-1) [control]", Outcome::denied(sys::EBADF)),
        ("kcmp(-1,-1,...) [control]", Outcome::denied(sys::ESRCH)),
    ]);
    assert_eq!(Selection::choose(&f).rung, Rung::Namespace);
}

#[test]
fn a_listener_without_ptrace_does_not_reach_supervise() {
    // TODO/supervise.md T-0606: the tier is refused when any leg is
    // missing, never entered and degraded per call. The refusal names
    // the tier's own leg, not the borrowed census row beside it.
    let s = Selection::choose(&target_shape());
    assert_eq!(s.rung, Rung::Chroot);
    let why = s
        .rejected
        .iter()
        .find(|r| r.rung == Rung::Supervise)
        .unwrap();
    assert!(
        why.evidence
            .contains("ptrace(PTRACE_TRACEME) [supervise]=EPERM"),
        "{}",
        why.evidence
    );
}

#[test]
fn three_clear_owned_legs_reach_supervise() {
    // The positive path the test above refuses: namespaces unavailable,
    // all three tier-owned legs ok, so the tier is entered.
    let s = Selection::choose(&findings(&[
        ("unshare(CLONE_NEWNS)", Outcome::denied(sys::EPERM)),
        ("clone(CLONE_NEWNS)", Outcome::denied(sys::EPERM)),
        ("seccomp(NEW_LISTENER) [supervise]", Outcome::ok()),
        ("process_vm_readv(own pid) [supervise]", Outcome::ok()),
        ("ptrace(PTRACE_TRACEME) [supervise]", Outcome::ok()),
    ]));
    assert_eq!(s.rung, Rung::Supervise);
    assert!(s.rejected.iter().all(|r| r.rung != Rung::Supervise));
}

#[test]
fn a_weaker_rung_never_satisfies_the_strict_floor() {
    let s = Selection::choose(&target_shape());
    assert!(!s.meets(Selection::STRICT_FLOOR));
    assert_eq!(s.exit_code(Strictness::Refuse), 1);
    assert_eq!(s.exit_code(Strictness::Warn), 0);
}

#[test]
fn a_control_that_cannot_answer_is_reported_apart_from_the_rung() {
    // ⭐ A kernel without CONFIG_CHECKPOINT_RESTORE: kcmp(2) answers
    // ENOSYS, which is the control saying it is not available rather than
    // the discriminator working. That is a statement about the diagnostic
    // and not about the rung, so it is reported and it does not move the
    // exit code: see `Selection::exit_code`.
    let mut rows = vec![
        ("unshare(CLONE_NEWNS)", Outcome::ok()),
        ("mount(tmpfs,/mnt)", Outcome::ok()),
        ("pidfd_getfd(-1,-1) [control]", Outcome::denied(sys::EBADF)),
        (
            "kcmp(-1,-1,...) [control]",
            Outcome::skip(Some(sys::ENOSYS), "no kcmp here"),
        ),
    ];
    let s = Selection::choose(&findings(&rows));
    assert_eq!(s.rung, Rung::Namespace);
    assert!(!s.controls_answered);
    assert_eq!(s.control_notes.len(), 1);
    assert_eq!(s.exit_code(Strictness::Refuse), 0);
    rows[3] = ("kcmp(-1,-1,...) [control]", Outcome::denied(sys::ESRCH));
    let s = Selection::choose(&findings(&rows));
    assert!(s.controls_answered);
    assert!(s.control_notes.is_empty());
}

#[test]
fn a_control_answering_the_wrong_errno_is_caught_as_well_as_a_missing_one() {
    // ⛔ The failure this exists for is subtler than absence: a control
    // that answers something else has stopped separating a filter from a
    // policy, and the rows around it are no longer attributions.
    let f = findings(&[
        ("chroot(/tmp)", Outcome::ok()),
        ("pidfd_getfd(-1,-1) [control]", Outcome::denied(sys::EPERM)),
        ("kcmp(-1,-1,...) [control]", Outcome::denied(sys::ESRCH)),
    ]);
    let s = Selection::choose(&f);
    assert!(!s.controls_answered);
    assert!(
        s.control_notes[0].contains("stopped separating"),
        "{:?}",
        s.control_notes
    );
}

#[test]
fn the_banner_reports_what_the_rung_provides_and_not_what_the_machine_permits() {
    // ⛔ The reconstruction of 2026-09-08 has no Landlock, so move_mount
    // attaches there and the machine's own mount summary reads `full`. The
    // selected rung is still `chroot`, which mounts nothing, and a banner
    // saying otherwise is the exact claim TOOL.md section 4.1 forbids.
    let f = findings(&[
        ("unshare(CLONE_NEWNS)", Outcome::denied(sys::EPERM)),
        ("clone(CLONE_NEWNS)", Outcome::ok()),
        ("mount(tmpfs,/mnt)", Outcome::denied(sys::EPERM)),
        ("chroot(/tmp)", Outcome::ok()),
        ("ptrace(PTRACE_TRACEME)", Outcome::denied(sys::EPERM)),
        ("fsopen(tmpfs)", Outcome::ok()),
        ("fsmount(tmpfs)", Outcome::ok()),
        ("move_mount(-> /tmp/mm-probe)", Outcome::ok()),
        ("openat(detached tmpfs, O_DIRECTORY)", Outcome::ok()),
    ]);
    let s = Selection::choose(&f);
    assert_eq!(s.rung, Rung::Chroot);
    assert_eq!(crate::mounts::Mounts::of(&f).summary(), "full");
    assert_eq!(Provides::of(s.rung, &f).mounts, "none");
    assert_eq!(Provides::of(s.rung, &f).network, "host-shared");
}

#[test]
fn the_chroot_rung_says_what_it_must_never_claim() {
    assert!(Rung::Chroot.must_never_claim().contains("network"));
    assert!(Rung::Chroot.must_never_claim().contains("/proc"));
}

/// TODO/enter.md T-1339: the namespace rung is mount-only. The
/// banner cells state the rung (mount-only, its tmpfs topology,
/// host-shared network and pids) and the never-claim guards the
/// rest, so a mount namespace never reads as isolation it is not.
#[test]
fn the_namespace_rung_reports_mount_only() {
    let f = Findings::empty();
    let p = Provides::of(Rung::Namespace, &f);
    assert_eq!(p.namespaces, "mount-only");
    assert_eq!(p.mounts, "private, tmpfs on /tmp");
    assert_eq!(p.network, "host-shared");
    assert_eq!(p.pids, "host-shared");
    assert!(Rung::Namespace.must_never_claim().contains("network"));
    assert!(Rung::Namespace.must_never_claim().contains("/proc"));
}
#[test]
fn the_banner_cell_names_the_errno_rather_than_the_word_denied() {
    let f = target_shape();
    assert_eq!(banner_cell(&f, "clone(CLONE_NEWNS)"), "clone(NEWNS)=ok");
    assert_eq!(banner_cell(&f, "ptrace(PTRACE_TRACEME)"), "ptrace=EPERM");
}
