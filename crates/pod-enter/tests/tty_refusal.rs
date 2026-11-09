//! `experiments/251-tty-refusal-no-ptmx.sh` as an integration test.
//!
//! `run -t` refuses by name where /dev/ptmx is unusable, with the exit
//! code from the process that produced it. The gate reads only
//! `podvm_probe::probes::ptmx_usable`: an Ok open row promises a pty and
//! anything else refuses. Two halves. The first always runs: synthetic
//! findings prove both arms, usable promises and unusable refuses,
//! including that existence is not function. The second arranges a machine
//! with no usable ptmx, a private mount namespace where a fresh tmpfs
//! covers the pty path, and drives the trigger there. The refusal triggers
//! only with no ptmx, and the lane has a usable /dev/ptmx, so where the
//! cover cannot be arranged the drive reports SKIP with its reason and
//! passes, which is the script exit-2 contract. No live engine, no mounts
//! outside a self-created user and mount namespace.

use podvm_probe::probes::ptmx_usable;
use podvm_probe::verdict::Outcome;
use podvm_probe::Findings;

/// Every fork in this file serialises behind one lock. `clone_fork` allows
/// the child async-signal-safe work only, and a sibling thread inside the
/// allocator at the fork instant would hang the child on its first
/// allocation. Holding this across the fork leaves only the harness idle
/// threads outside it.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn findings(rows: Vec<(&'static str, Outcome)>) -> Findings {
    Findings {
        rows,
        ..Findings::empty()
    }
}

/// Both arms of the gate. Only the open row with Ok promises a pty: a
/// denial is the refusal itself, a skip never ran, and the stat row does
/// not answer. The consumer half of the probe unit test beside it.
#[test]
fn refusal_triggers_only_where_the_open_row_is_denied() {
    let _serialised = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let open = "open(/dev/ptmx, O_RDWR)";
    let stat = "stat(/dev/ptmx)";
    assert!(
        ptmx_usable(&findings(vec![(open, Outcome::ok())])),
        "an Ok open row must promise a pty"
    );
    for errno in [1, 2, 13] {
        assert!(
            !ptmx_usable(&findings(vec![(
                open,
                Outcome::denied(podvm_probe::sys::Errno(errno))
            )])),
            "a denied open row must trigger the refusal, errno {errno}"
        );
    }
    assert!(
        !ptmx_usable(&findings(vec![(open, Outcome::skip(None, "no run"))])),
        "a skipped open row must trigger the refusal"
    );
    assert!(
        !ptmx_usable(&findings(vec![])),
        "a missing open row must trigger the refusal"
    );
    assert!(
        !ptmx_usable(&findings(vec![
            (stat, Outcome::ok()),
            (open, Outcome::denied(podvm_probe::sys::EACCES)),
        ])),
        "a present node that will not open must still trigger the refusal"
    );
}

/// The cover child: where ptmx already refuses, the trigger holds with no
/// cover. Else it covers the pty path with a fresh tmpfs and the covered
/// open must refuse with the trigger holding. Exit 0 drove the refusal,
/// 1 a wrong outcome, 2 the cover cannot be arranged. No unwrap inside: a
/// panic past the fork may hang on a duplicated lock, so every shortfall
/// exits by number.
#[cfg(target_os = "linux")]
fn child_drive() -> ! {
    use podvm_probe::sys;
    let fail = |n: i32| -> ! { sys::exit_group(n) };
    let ptmx = match sys::CBuf::new("/dev/ptmx") {
        Some(c) => c,
        None => fail(2),
    };
    let flags = sys::O_RDWR | sys::O_NOCTTY;
    match sys::open(&ptmx, flags, 0) {
        Err(e) => {
            let f = Findings {
                rows: vec![("open(/dev/ptmx, O_RDWR)", Outcome::denied(e))],
                ..Findings::empty()
            };
            if ptmx_usable(&f) {
                fail(1);
            }
            fail(0);
        }
        Ok(fd) => {
            let _ = sys::close(fd);
        }
    }
    // The script layout rule: a symlink into /dev/pts is covered at
    // /dev/pts, a directory at itself, anything else cannot be covered.
    let cover: &str = match std::fs::symlink_metadata("/dev/ptmx") {
        Ok(m) if m.file_type().is_symlink() => match std::fs::read_link("/dev/ptmx") {
            Ok(target) if target.to_string_lossy().contains("pts") => "/dev/pts",
            _ => fail(2),
        },
        Ok(m) if m.is_dir() => "/dev/ptmx",
        _ => fail(2),
    };
    if sys::unshare(sys::CLONE_NEWUSER | sys::CLONE_NEWNS).is_err() {
        fail(2);
    }
    if std::fs::write("/proc/self/uid_map", "0 0 1\n").is_err() {
        fail(2);
    }
    let _ = std::fs::write("/proc/self/setgroups", "deny");
    if std::fs::write("/proc/self/gid_map", "0 0 1\n").is_err() {
        fail(2);
    }
    // Privatise before covering: without it the tmpfs below propagates to
    // the host mount table on shared-propagated roots and hides the host
    // pty path machine-wide instead of inside this namespace.
    let empty = sys::cempty();
    let slash = match sys::CBuf::new("/") {
        Some(c) => c,
        None => fail(2),
    };
    if sys::mount(&empty, &slash, &empty, sys::MS_REC | sys::MS_PRIVATE).is_err() {
        fail(2);
    }
    let source = match sys::CBuf::new("tmpfs") {
        Some(c) => c,
        None => fail(2),
    };
    let fstype = match sys::CBuf::new("tmpfs") {
        Some(c) => c,
        None => fail(2),
    };
    let target = match sys::CBuf::new(cover) {
        Some(c) => c,
        None => fail(2),
    };
    if sys::mount(&source, &target, &fstype, 0).is_err() {
        fail(2);
    }
    match sys::open(&ptmx, flags, 0) {
        Ok(fd) => {
            let _ = sys::close(fd);
            fail(1);
        }
        Err(e) => {
            let f = Findings {
                rows: vec![("open(/dev/ptmx, O_RDWR)", Outcome::denied(e))],
                ..Findings::empty()
            };
            if ptmx_usable(&f) {
                fail(1);
            }
            fail(0);
        }
    }
}

/// The covered drive. Asserts the refusal trigger only where no ptmx can
/// be arranged, and reports SKIP with its reason where the cover cannot
/// be arranged: unknown ptmx layout, refused unshare, refused maps, or a
/// refused mount. The lane has a usable /dev/ptmx and refuses unshare, so
/// it always takes the SKIP arm here.
#[cfg(target_os = "linux")]
#[test]
fn covered_ptmx_drives_the_refusal_where_the_cover_can_be_arranged() {
    use podvm_probe::sys;
    let _serialised = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let pid = unsafe { sys::clone_fork(sys::SIGCHLD) }.expect("fork for the cover child");
    if pid == 0 {
        child_drive();
    }
    let mut status = 0;
    sys::wait4(pid, &mut status).expect("wait for the cover child");
    if (status & 0x7f) != 0 {
        panic!("the cover child died by signal");
    }
    match (status >> 8) & 0xff {
        0 => {}
        2 => {
            eprintln!(
                "SKIP: the pty cover cannot be arranged here, so the refusal has no machine without ptmx to trigger on"
            );
        }
        c => panic!("the cover child exited {c}"),
    }
}

/// The cover off Linux. Hiding the pty path needs user and mount
/// namespaces, so elsewhere it reports SKIP with its reason and passes.
#[cfg(not(target_os = "linux"))]
#[test]
fn covered_ptmx_drives_the_refusal_where_the_cover_can_be_arranged() {
    eprintln!("SKIP: the pty cover needs Linux user and mount namespaces");
}
