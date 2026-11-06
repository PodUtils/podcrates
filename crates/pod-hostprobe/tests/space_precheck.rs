//! `140-space-precheck.sh` clause 4 as an integration test: inodes are
//! checked as well as blocks, on a real filesystem rather than a double.
//!
//! Two halves. The first always runs: `space::require` refuses an
//! impossible byte ask and an impossible inode ask on an ordinary temp
//! directory, each naming the destination. That half proves the refusal
//! logic, both arms, on a real filesystem. The second stages true inode
//! pressure on a fresh tmpfs inside a forked child (user and mount
//! namespaces, so the test process never mounts anything itself) and
//! refuses a modest ask there. Where pressure is unreachable under the
//! fill cap the drive reports SKIP with its reason and passes, which is the
//! script's own exit-2 contract: an unarranged fixture is not a failure.
//! Measured in the lane 2026-10-01: a fresh tmpfs carries 4,082,283 inodes
//! and fills 20,000 files in 0.09 s, so the 30,000-file cap below always
//! takes the SKIP arm there. Filling 4M files would take ~20 s and ~3 GiB
//! of kernel slab per runner, which is not a polite test, so the cap stays
//! and the refusal under true pressure is covered by the first half's
//! inode arm through the same code path.

use podvm_image::space;
use podvm_probe::sys;

/// ⛔ Both tests in this binary run under one lock. `clone_fork`'s contract
/// (`sys.rs`) allows the child async-signal-safe work only: no allocation,
/// no format, no lock. The pressure child below allocates (`CBuf`, `fs`,
/// `space`), so a sibling thread inside the allocator at the fork instant
/// would hang the child on its first allocation. Holding this across the
/// fork leaves only the harness's idle threads outside it, which is the
/// same reason `store.rs` serialises its forking test behind `STORE_TESTS`.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn fresh_tmp(prefix: &str) -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Clause 4's arms, on an ordinary filesystem: bytes short, then inodes
/// short while bytes are roomy. Both refusals name the destination.
#[test]
fn require_refuses_short_bytes_and_short_inodes_naming_the_destination() {
    let _serialised = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = fresh_tmp("roomy");
    let at = dir.to_string_lossy().into_owned();
    let e = space::require(&at, "test-image", u64::MAX / 4, 0, 0).unwrap_err();
    let text = format!("{e}");
    assert!(text.contains(&at), "{text}");
    assert!(text.contains("free"), "{text}");
    let e = space::require(&at, "test-image", 1, u64::MAX / 2, 0).unwrap_err();
    let text = format!("{e}");
    assert!(text.contains(&at), "{text}");
    assert!(text.contains("inodes"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The true clause-4 drive: a tmpfs filled to inode pressure refuses a
/// modest ask it would grant roomy. Runs in a forked child under user and
/// mount namespaces; the child exits 0 on a driven refusal, 1 on a wrong
/// outcome, and 2 where the fixture cannot be arranged (no userns, no
/// mount, or a tmpfs too roomy to exhaust under the fill cap).
#[test]
fn inode_pressure_on_a_fresh_tmpfs_refuses_a_modest_ask() {
    let _serialised = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = fresh_tmp("pressure");
    let at = dir.to_string_lossy().into_owned();
    let pid = unsafe { sys::clone_fork(sys::SIGCHLD) }.expect("fork for the mount child");
    if pid == 0 {
        child_drive(&at);
    }
    let mut status = 0;
    sys::wait4(pid, &mut status).expect("wait for the mount child");
    let _ = std::fs::remove_dir_all(&dir);
    if (status & 0x7f) == 0 {
        match (status >> 8) & 0xff {
            0 => return,
            2 => {
                eprintln!("SKIP: the tmpfs drive cannot be arranged here; see the child log");
                return;
            }
            c => panic!("the mount child exited {c}"),
        }
    }
    panic!("the mount child died by signal");
}

fn child_drive(at: &str) -> ! {
    let fail = |n: i32| -> ! {
        sys::exit_group(n);
    };
    if sys::unshare(sys::CLONE_NEWUSER | sys::CLONE_NEWNS).is_err() {
        fail(2);
    }
    // ⛔ A raw `unshare(2)` writes no id maps, unlike `unshare(1)`. Until
    // they exist the child is nobody and the tmpfs mount is refused, which
    // the lane proved: without these three writes the drive always takes
    // the SKIP arm below in 0.00 s. Root maps to itself; nothing here
    // grants what the parent did not already hold.
    if std::fs::write("/proc/self/uid_map", "0 0 1\n").is_err() {
        fail(2);
    }
    let _ = std::fs::write("/proc/self/setgroups", "deny");
    if std::fs::write("/proc/self/gid_map", "0 0 1\n").is_err() {
        fail(2);
    }
    let source = sys::CBuf::new("tmpfs").unwrap();
    let target = match sys::CBuf::new(at) {
        Some(c) => c,
        None => fail(2),
    };
    let fstype = sys::CBuf::new("tmpfs").unwrap();
    if sys::mount(&source, &target, &fstype, 0).is_err() {
        fail(2);
    }
    // Fill inodes until few are free, under a cap that bounds the drive on
    // a roomy tmpfs. Files stay empty: content pages would exhaust bytes
    // first on a big tmpfs and stage the wrong pressure. Bytes are checked
    // alongside anyway: running out of bytes first means this filesystem
    // cannot stage inode pressure.
    let mut made = 0u32;
    let roomy = loop {
        let free = match space::read(at) {
            Ok(have) => have,
            Err(_) => fail(1),
        };
        if free.free.inodes <= 64 {
            eprintln!("DRIVE: inode pressure staged with {made} files");
            break false;
        }
        if made >= 30_000 {
            eprintln!("DRIVE: roomy tmpfs, {} inodes still free", free.free.inodes);
            break true;
        }
        if std::fs::File::create(format!("{at}/filler.{made}")).is_err() {
            break true;
        }
        made += 1;
    };
    if roomy {
        fail(2);
    }
    match space::require(at, "test-image", 1, 1_000, 0) {
        Err(e) if format!("{e}").contains("inodes") => fail(0),
        other => {
            let _ = other;
            fail(1);
        }
    }
}
