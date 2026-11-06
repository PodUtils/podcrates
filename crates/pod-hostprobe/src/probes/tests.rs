use super::*;

#[test]
fn every_row_name_is_unique() {
    let mut names: Vec<&str> = PROBES.iter().map(|p| p.name).collect();
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    assert_eq!(before, names.len(), "two probes share a row name");
}

#[test]
fn the_set_is_at_least_the_size_its_acceptance_names() {
    // TODO/probe.md T-0101's Prove asserts `.probes | length >= 24`.
    assert!(PROBES.len() >= 24, "{} probes", PROBES.len());
}

#[test]
fn the_attribution_block_is_the_reference_order() {
    // TODO/milestones.md T-1101 compares this block against
    // experiments/results/attribute.txt row for row, so its names and its
    // order are the reference's `attrOrder`.
    let got: Vec<&str> = PROBES
        .iter()
        .filter(|p| p.group == Group::Attribution)
        .map(|p| p.name)
        .collect();
    assert_eq!(
        got,
        vec![
            "mount(2) bogus target",
            "umount2(2) bogus target",
            "pivot_root(2) bogus paths",
            "process_vm_readv(bogus pid)",
            "pidfd_getfd(-1,-1) [control]",
            "kcmp(-1,-1,...) [control]",
            "fsopen(tmpfs)",
            "fsmount(tmpfs)",
            "open_tree(/tmp, CLONE)",
            "move_mount(-> bogus dest)",
            "move_mount(-> /tmp/mm-probe)",
            "openat(detached tmpfs, O_DIRECTORY)",
            "seccomp(NEW_LISTENER)",
            "open /proc/self/mem O_RDONLY",
            "open /proc/self/mem O_RDWR",
            "landlock_create_ruleset(VERSION)",
        ]
    );
}

#[test]
fn the_machine_block_is_six_legs_in_order() {
    // TODO/machine.md T-1301: `crate::machine` and the report read the
    // verdicts through `MACHINE_LEGS`, so the block and the constant
    // answer together or not at all.
    let got: Vec<&str> = PROBES
        .iter()
        .filter(|p| p.group == Group::Machine)
        .map(|p| p.name)
        .collect();
    assert_eq!(got.as_slice(), MACHINE_LEGS);
}

#[test]
fn the_supervise_block_is_mediation_then_supervision_in_order() {
    // TODO/supervise.md T-0606: `crate::supervise`, the report and the
    // selection read the verdicts through `SUPERVISE_LEGS` and
    // `SUPERVISION_LEGS`, so the block, the constants and the readers
    // answer together or not at all.
    let got: Vec<&str> = PROBES
        .iter()
        .filter(|p| p.group == Group::Supervise)
        .map(|p| p.name)
        .collect();
    let mut want: Vec<&str> = SUPERVISE_LEGS.to_vec();
    want.extend(SUPERVISION_LEGS.iter());
    assert_eq!(got.as_slice(), want.as_slice());
}

#[test]
fn the_propagation_change_is_only_ever_probed_inside_a_mount_namespace() {
    // The one probe with an unbounded effect on the caller's machine.
    for p in PROBES {
        if p.name.contains("MS_SLAVE") {
            match p.kind {
                Kind::Child { ns_flags, .. } => {
                    assert_eq!(ns_flags & sys::CLONE_NEWNS, sys::CLONE_NEWNS, "{}", p.name)
                }
                Kind::Clone(_) => panic!("{} must run a body", p.name),
            }
        }
    }
}

/// T-0503: only the OPEN row with `Ok` promises a pty. Existence is not
/// function, a denial is the refusal itself, and a skip never ran.
#[test]
fn only_an_ok_open_promises_a_pty() {
    fn findings(rows: Vec<(&'static str, Outcome)>) -> crate::Findings {
        crate::Findings {
            rows,
            identity: crate::identity::Identity::default(),
            writable: Vec::new(),
            self_exe: String::new(),
        }
    }
    let open = "open(/dev/ptmx, O_RDWR)";
    let stat = "stat(/dev/ptmx)";
    assert!(ptmx_usable(&findings(vec![(open, Outcome::ok())])));
    assert!(!ptmx_usable(&findings(vec![(
        open,
        Outcome::denied(crate::sys::Errno(2))
    )])));
    assert!(!ptmx_usable(&findings(vec![(
        open,
        Outcome::skip(None, "nope")
    )])));
    assert!(!ptmx_usable(&findings(vec![])));
    // Stat Ok beside an open denial: the node is there and will not open.
    assert!(!ptmx_usable(&findings(vec![
        (stat, Outcome::ok()),
        (open, Outcome::denied(crate::sys::Errno(13))),
    ])));
}

/// T-1003: only an `Ok` open promises a FUSE mount. Existence is not
/// function, a denial is the refusal itself, and a skip never ran.
#[test]
fn only_an_ok_open_promises_a_fuse_mount() {
    fn findings(rows: Vec<(&'static str, Outcome)>) -> crate::Findings {
        crate::Findings {
            rows,
            identity: crate::identity::Identity::default(),
            writable: Vec::new(),
            self_exe: String::new(),
        }
    }
    let open = "open(/dev/fuse, O_RDWR)";
    assert!(fuse_usable(&findings(vec![(open, Outcome::ok())])));
    assert!(!fuse_usable(&findings(vec![(
        open,
        Outcome::denied(crate::sys::Errno(2))
    )])));
    assert!(!fuse_usable(&findings(vec![(
        open,
        Outcome::skip(None, "nope")
    )])));
    assert!(!fuse_usable(&findings(vec![])));
}

/// T-1317: only an `Ok` chroot row promises entry. A denial is the
/// up-front refusal itself, and a skip never ran.
#[test]
fn only_an_ok_chroot_promises_entry() {
    fn findings(rows: Vec<(&'static str, Outcome)>) -> crate::Findings {
        crate::Findings {
            rows,
            identity: crate::identity::Identity::default(),
            writable: Vec::new(),
            self_exe: String::new(),
        }
    }
    let chroot = "chroot(/tmp)";
    assert!(chroot_usable(&findings(vec![(chroot, Outcome::ok())])));
    assert!(!chroot_usable(&findings(vec![(
        chroot,
        Outcome::denied(crate::sys::Errno(1))
    )])));
    assert!(!chroot_usable(&findings(vec![(
        chroot,
        Outcome::skip(None, "nope")
    )])));
    assert!(!chroot_usable(&findings(vec![])));
}

/// T-1003: the FUSE row is a Census leg in the outer environment before
/// any chroot, beside the ptmx pair it copies the rule from.
#[test]
fn the_fuse_row_opens_the_node_outside_any_namespace() {
    let p = PROBES
        .iter()
        .find(|p| p.name == "open(/dev/fuse, O_RDWR)")
        .expect("the FUSE row is in PROBES");
    assert!(matches!(p.group, Group::Census), "{}", p.name);
    match p.kind {
        Kind::Child { ns_flags, .. } => {
            assert_eq!(ns_flags, 0, "{} runs outside any namespace", p.name)
        }
        Kind::Clone(_) => panic!("{} is not a child probe", p.name),
    }
}
#[test]
fn the_root_listing_block_is_three_outer_legs_in_order() {
    let want = [
        "readdir(/)",
        "open(/bin, O_RDONLY) by name",
        "creat(/, O_CREAT|O_EXCL)",
    ];
    let mut at = 0;
    for p in PROBES {
        if at < want.len() && p.name == want[at] {
            assert!(
                matches!(p.group, Group::Census),
                "{} is not a Census leg",
                p.name
            );
            match p.kind {
                Kind::Child { ns_flags, .. } => {
                    assert_eq!(ns_flags, 0, "{} runs outside any namespace", p.name)
                }
                Kind::Clone(_) => panic!("{} is not a child probe", p.name),
            }
            at += 1;
        }
    }
    assert_eq!(at, want.len(), "root-listing legs missing from PROBES");
}
