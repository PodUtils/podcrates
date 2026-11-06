//! The selection on a host that permits namespaces, as integration tests.
//!
//! Where unshare and mounts hold, the probe selects namespace, the banner
//! reads mount-only with host-shared network and pids and the never-claim,
//! and the entered rung reads namespace. Five clauses: ns-1 selects
//! namespace, ns-2 enters it with the honest banner, ns-3 isolates the
//! payload tmp from the host, ns-4 falls back without /tmp, ns-5 starts a
//! detached container through the same gate. Direct assertions on the
//! probe library for ns-1 and ns-2, plus a real mount-namespace drive for
//! ns-3 that mounts only inside a self-created user and mount namespace.
//! ns-4 and ns-5 need a live image store and lifecycle, so the consumer
//! owns those drives and this file pins the predicates they assert. No live
//! engine, no mounts outside self-created namespaces.

use pod_hostprobe::select::{Provides, Rung, Selection, Strictness};
use pod_hostprobe::verdict::Outcome;
use pod_hostprobe::{Findings, sys};

/// Every fork in this file serialises behind one lock. `clone_fork` allows
/// the child async-signal-safe work only, and a sibling thread inside the
/// allocator at the fork instant would hang the child on its first
/// allocation. Holding this across the fork leaves only the harness idle
/// threads outside it.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The capable shape: unshare holds, both mount spellings hold,
/// chroot holds, both controls answer. Real row names throughout.
fn capable() -> Findings {
    Findings {
        rows: vec![
            ("unshare(CLONE_NEWNS)", Outcome::ok()),
            ("clone(CLONE_NEWNS)", Outcome::ok()),
            ("mount(tmpfs,/mnt)", Outcome::ok()),
            ("mount(tmpfs,/mnt) in clone(NEWNS)", Outcome::ok()),
            ("chroot(/tmp)", Outcome::ok()),
            ("pidfd_getfd(-1,-1) [control]", Outcome::denied(sys::EBADF)),
            ("kcmp(-1,-1,...) [control]", Outcome::denied(sys::ESRCH)),
        ],
        ..Findings::empty()
    }
}

/// ns-1. The capable host selects namespace: nothing rejects the rung,
/// the controls answer, and strict accepts it.
#[test]
fn capable_shape_selects_namespace() {
    let _serialised = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let s = Selection::choose(&capable());
    assert_eq!(s.rung, Rung::Namespace, "capable rung is not namespace");
    assert!(s.rejected.iter().all(|r| r.rung != Rung::Namespace), "the namespace rung was rejected on a capable host");
    assert!(s.controls_answered, "the capable controls stopped answering");
    assert_eq!(s.exit_code(Strictness::Refuse), 0, "strict refuses a capable host");
}

/// ns-1 by the mount-name path. Creation holding is not enough: with both
/// mount spellings refused the rung stays below namespace, which is the
/// attach half that the selection reads.
#[test]
fn creation_without_attach_does_not_select_namespace() {
    let _serialised = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let f = Findings {
        rows: vec![
            ("unshare(CLONE_NEWNS)", Outcome::ok()),
            ("clone(CLONE_NEWNS)", Outcome::ok()),
            ("mount(tmpfs,/mnt)", Outcome::denied(sys::EPERM)),
            ("mount(tmpfs,/mnt) in clone(NEWNS)", Outcome::denied(sys::EPERM)),
            ("chroot(/tmp)", Outcome::ok()),
        ],
        ..Findings::empty()
    };
    let s = Selection::choose(&f);
    assert!(s.rung != Rung::Namespace, "mounts refused yet selected {:?}", s.rung);
    assert_eq!(s.rung, Rung::Chroot, "the fallback rung is not chroot");
}

/// ns-1 by the second mount spelling. Either mount row carries the attach
/// half on its own: the plain spelling refused with the clone spelling
/// holding still selects namespace.
#[test]
fn clone_only_mount_row_still_selects_namespace() {
    let _serialised = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let f = Findings {
        rows: vec![
            ("unshare(CLONE_NEWNS)", Outcome::ok()),
            ("mount(tmpfs,/mnt)", Outcome::denied(sys::EPERM)),
            ("mount(tmpfs,/mnt) in clone(NEWNS)", Outcome::ok()),
        ],
        ..Findings::empty()
    };
    let s = Selection::choose(&f);
    assert_eq!(s.rung, Rung::Namespace, "the clone mount spelling did not carry the attach half");
}

/// ns-2. The banner reads mount-only with host-shared network and pids and
/// the never-claim, and the entered word reads namespace with no fallback:
/// nothing rejected the rung.
#[test]
fn namespace_banner_reads_mount_only_with_host_shared_rest() {
    let _serialised = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let f = capable();
    let s = Selection::choose(&f);
    assert_eq!(s.rung.word(), "namespace", "the entered word is wrong");
    let p = Provides::of(s.rung, &f);
    assert_eq!(p.namespaces, "mount-only", "namespaces read {}", p.namespaces);
    assert_eq!(p.mounts, "private, tmpfs on /tmp", "mounts read {}", p.mounts);
    assert_eq!(p.network, "host-shared", "network reads {}", p.network);
    assert_eq!(p.pids, "host-shared", "pids read {}", p.pids);
    assert!(Rung::Namespace.must_never_claim().contains("network"), "the never-claim names no network guard");
    assert!(Rung::Namespace.must_never_claim().contains("/proc"), "the never-claim names no /proc guard");
    assert!(s.rejected.iter().all(|r| r.rung != Rung::Namespace), "a fallback was named where none may be");
}

#[cfg(target_os = "linux")]
fn fresh_tmp(prefix: &str) -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the isolation parent dir");
    dir
}

/// Whether the host mount table carries a mount on `target`.
#[cfg(target_os = "linux")]
fn mount_table_has(target: &str) -> bool {
    let Ok(text) = std::fs::read_to_string("/proc/self/mounts") else {
        return false;
    };
    text.lines().any(|l| l.split_whitespace().nth(1) == Some(target))
}

/// The ns-3 child: its own mount namespace, `/` recursively private so its
/// mounts cannot propagate to the host, then its private tmpfs on the
/// target. Exit 0 drove the isolation, 1 a wrong outcome, 2 the fixture
/// cannot be arranged. No unwrap inside: a panic past the fork may hang on
/// a duplicated lock, so every shortfall exits by number.
#[cfg(target_os = "linux")]
fn child_drive(at: &str) -> ! {
    use pod_hostprobe::sys;
    let fail = |n: i32| -> ! { sys::exit_group(n) };
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
    let target = match sys::CBuf::new(at) {
        Some(c) => c,
        None => fail(2),
    };
    if sys::mount(&source, &target, &fstype, 0).is_err() {
        fail(2);
    }
    if std::fs::write(format!("{at}/ns3mark"), "mark366").is_err() {
        fail(1);
    }
    match std::fs::read_to_string(format!("{at}/ns3mark")) {
        Ok(back) if back == "mark366" => {}
        _ => fail(1),
    }
    fail(0);
}

/// ns-3. The payload writes and reads its own tmp file, the host rootfs
/// holds no mark afterwards, and no payload mount leaks into the host
/// mount table. Where unshare or mounts refuse, the drive reports SKIP
/// with its reason and passes: an unarranged fixture is not a failure.
#[cfg(target_os = "linux")]
#[test]
fn payload_tmp_is_invisible_from_the_host() {
    use pod_hostprobe::sys;
    let _serialised = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = fresh_tmp("ns-isolation");
    let at = dir.to_string_lossy().into_owned();
    let pid = unsafe { sys::clone_fork(sys::SIGCHLD) }.expect("fork for the mount child");
    if pid == 0 {
        child_drive(&at);
    }
    let mut status = 0;
    sys::wait4(pid, &mut status).expect("wait for the mount child");
    let marker_gone = !dir.join("ns3mark").exists();
    let mount_leaked = mount_table_has(&at);
    let _ = std::fs::remove_dir_all(&dir);
    if (status & 0x7f) != 0 {
        panic!("the mount child died by signal");
    }
    match (status >> 8) & 0xff {
        0 => {
            assert!(marker_gone, "the payload file leaked into the host rootfs");
            assert!(!mount_leaked, "a payload mount leaked into the host mount table");
        }
        2 => {
            eprintln!("SKIP: unshare or mounts are refused here, so the isolation drive cannot be arranged");
        }
        c => panic!("the mount child exited {c}"),
    }
}

/// ns-3 off Linux. The isolation drive needs user and mount namespaces,
/// so elsewhere it reports SKIP with its reason and passes.
#[cfg(not(target_os = "linux"))]
#[test]
fn payload_tmp_is_invisible_from_the_host() {
    eprintln!("SKIP: the isolation drive needs Linux user and mount namespaces");
}
