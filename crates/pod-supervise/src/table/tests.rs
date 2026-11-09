use super::*;

fn c(id: &str, name: &str, state: State) -> Container {
    Container {
        id: id.to_string(),
        name: name.to_string(),
        image: "img".into(),
        manifest_digest: "sha256:0".into(),
        rootfs: "/tmp".into(),
        argv: vec!["true".into()],
        env: Vec::new(),
        working_dir: "/".into(),
        created_at: "now".into(),
        started_at: None,
        finished_at: None,
        state,
        pid: None,
        launcher_pid: None,
        exit_code: None,
        noticed: None,
        rung: "chroot".into(),
        userland_exec: Vec::new(),
        completion: Vec::new(),
        completion_degraded: 0,
    }
}

#[test]
fn a_name_a_full_id_and_a_prefix_all_resolve() {
    let t = Table {
        podvm_containers: TABLE_VERSION,
        containers: vec![c("abcdef0123", "one", State::Created)],
    };
    assert_eq!(find(&t, "one").unwrap().id, "abcdef0123");
    assert_eq!(find(&t, "abcdef0123").unwrap().id, "abcdef0123");
    assert_eq!(find(&t, "abc").unwrap().id, "abcdef0123");
    assert!(find(&t, "nope").is_err());
}

/// ⛔ Never the first match. A caller that gets the wrong container from an
/// ambiguous prefix has no way to find out.
#[test]
fn an_ambiguous_prefix_is_an_error_and_names_the_candidates() {
    let t = Table {
        podvm_containers: TABLE_VERSION,
        containers: vec![
            c("abcdef0123", "one", State::Created),
            c("abcff09999", "two", State::Created),
        ],
    };
    let e = find(&t, "abc").unwrap_err().0;
    assert!(e.contains("names 2 containers"), "{e}");
    assert!(e.contains("abcdef0123"), "{e}");
    // ⚠ And an exact name still wins over a prefix that would be ambiguous.
    assert_eq!(find(&t, "two").unwrap().id, "abcff09999");
}

#[test]
fn the_status_column_never_invents_an_exit_code() {
    let mut x = c("a", "n", State::Dead);
    assert!(x.status().contains("not watching"), "{}", x.status());
    x.state = State::Exited;
    assert_eq!(x.status(), "Exited");
    x.exit_code = Some(3);
    assert_eq!(x.status(), "Exited (3)");
}

/// ⭐ T-0710: the memo lives beside the container record, on the host, and
/// the environment names the constant descriptor number for it.
#[test]
fn the_memo_lives_beside_the_container_record() {
    let d = std::env::temp_dir().join(format!("podvm-memo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let s = Store::open(&d).unwrap();
    let p = memo_path(&s, "abc123");
    assert!(p.ends_with("containers/abc123/ownership.memo"), "{p:?}");
    assert_eq!(memo_fd_env(), "PODVM_MEMO_FD=17");
    assert_eq!(MEMO_CHILD_FD, 17);
    // Creating twice keeps what is there: a second entry must not truncate
    // the first process's ownership memory.
    ensure_memo_file(&p).unwrap();
    std::fs::write(&p, b"first").unwrap();
    ensure_memo_file(&p).unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), b"first");
    let _ = std::fs::remove_dir_all(&d);
}

/// TODO/enter.md T-1411. A table written before the userland drive
/// still parses, with an empty drive: old records enter through
/// `spawn_selected` exactly as before.
#[test]
fn a_table_without_the_userland_drive_still_parses() {
    let t: Table = serde_json::from_value(serde_json::json!({
        "podvm_containers": TABLE_VERSION,
        "containers": [{
            "id": "abc",
            "name": "one",
            "image": "img",
            "manifest_digest": "sha256:0",
            "rootfs": "/tmp",
            "argv": ["true"],
            "env": [],
            "working_dir": "/",
            "created_at": "now",
            "started_at": null,
            "finished_at": null,
            "state": "created",
            "pid": null,
            "launcher_pid": null,
            "exit_code": null,
            "noticed": null,
            "rung": "chroot"
        }]
    }))
    .unwrap();
    assert!(t.containers[0].userland_exec.is_empty());
}

fn tally_rec(dev: u64, ino: u64, uid: u32, gid: u32, set: u32) -> Vec<u8> {
    let mut r = Vec::with_capacity(32);
    r.extend(dev.to_le_bytes());
    r.extend(ino.to_le_bytes());
    r.extend(uid.to_le_bytes());
    r.extend(gid.to_le_bytes());
    r.extend(set.to_le_bytes());
    r.extend(0u32.to_le_bytes());
    r
}

fn tally_cont(path: &[u8; 16]) -> Vec<u8> {
    let mut r = Vec::with_capacity(32);
    r.extend(u64::MAX.to_le_bytes());
    r.extend(0u64.to_le_bytes());
    r.extend(path);
    r
}

/// T-0708: the tally scan counts complete records and drops torn chains.
#[test]
fn the_tally_scan_counts_complete_records_only() {
    let d = std::env::temp_dir().join(format!("podvm-tally-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let p = d.join("memo");
    let mut b = Vec::new();
    // An ownership record passes through untouched.
    b.extend(tally_rec(7, 9, 0, 0, 0));
    // Two mknods, one unshare, one clone.
    b.extend(tally_rec(u64::MAX, TALLY_MKNOD, 0, 0, 0));
    b.extend(tally_rec(u64::MAX, TALLY_MKNOD, 0o644, 0, 0));
    b.extend(tally_rec(u64::MAX, TALLY_UNSHARE, 0x20000, 0, 0));
    b.extend(tally_rec(u64::MAX, TALLY_CLONE, 0x20000, 0, 0));
    // A mount with a one-record chain each side.
    b.extend(tally_rec(u64::MAX, TALLY_MOUNT, 0, 1, 1));
    b.extend(tally_cont(b"/src\0\0\0\0\0\0\0\0\0\0\0\0"));
    b.extend(tally_cont(b"/tgt\0\0\0\0\0\0\0\0\0\0\0\0"));
    // A header broken by an ownership record: a torn chain, dropped.
    b.extend(tally_rec(u64::MAX, TALLY_MOUNT, 0, 1, 0));
    b.extend(tally_rec(7, 9, 0, 0, 0));
    // A header at EOF with its chain missing: dropped too.
    b.extend(tally_rec(u64::MAX, TALLY_MOUNT, 0, 1, 0));
    // An unknown kind: skipped.
    b.extend(tally_rec(u64::MAX, 9, 0, 0, 0));
    // T-0413: two served /proc answers with their detail words.
    b.extend(tally_rec(u64::MAX, TALLY_PROC, 1, 0, 0));
    b.extend(tally_rec(u64::MAX, TALLY_PROC, 5, 0, 0));
    std::fs::write(&p, &b).unwrap();
    assert_eq!(
        emulated_counts(&p),
        EmulatedCounts {
            mknod: 2,
            mount: 1,
            unshare: 1,
            clone: 1,
            procfs: 2,
        }
    );
    // A missing file counts nothing rather than failing.
    assert_eq!(
        emulated_counts(&d.join("absent")),
        EmulatedCounts::default()
    );
    let _ = std::fs::remove_dir_all(&d);
}
