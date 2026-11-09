use super::*;

#[test]
fn an_id_is_sixty_four_hex_characters_and_two_are_not_equal() {
    let a = new_id();
    let b = new_id();
    assert_eq!(a.len(), 64, "{a}");
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "{a}");
    // ⛔ Two containers of one image are two containers. An id derived from
    // content would collide for exactly the case M4's acceptance drives
    // twenty times.
    assert_ne!(a, b);
}

#[test]
fn a_generated_name_is_short_and_derived_from_the_id() {
    let id = new_id();
    let n = generated_name(&id);
    assert!(n.starts_with("podvm_"));
    assert!(id.starts_with(&n["podvm_".len()..]));
}

/// T-0810: creating a container makes its directory, or the memo rename
/// beside the record fails with ENOENT and no record can be inspected.
#[test]
fn creating_a_container_makes_its_directory() {
    let d = std::env::temp_dir().join(format!("podvm-mkdir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let s = Store::open(&d).unwrap();
    let c = create(
        &s,
        Some("e1"),
        "img",
        "sha256:0",
        "/tmp",
        vec!["true".into()],
        Vec::new(),
        "/".into(),
        "chroot",
        Vec::new(),
        0,
        Vec::new(),
    )
    .unwrap();
    assert!(table::dir(&s, &c.id).is_dir());
    // The shape the cli rename needs: a file moves beside the record.
    let src = d.join("ephemeral.tmp");
    std::fs::write(&src, b"memo").unwrap();
    let dest = table::memo_path(&s, &c.id);
    std::fs::rename(&src, &dest).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"memo");
    let _ = std::fs::remove_dir_all(&d);
}

/// TODO/enter.md T-1411. `create` stores the detached userland drive
/// beside the record, and a record without one keeps an empty drive:
/// the launcher branches on exactly this.
#[test]
fn creating_a_container_stores_the_userland_drive() {
    let d = std::env::temp_dir().join(format!("podvm-drive-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let s = Store::open(&d).unwrap();
    let drive = vec!["/r/lib/ld.so".to_string(), "/r/bin/sh".to_string()];
    let c = create(
        &s,
        Some("u1"),
        "img",
        "sha256:0",
        "/r",
        vec!["sh".into()],
        vec!["PODVM_MAPS=/:/r".into()],
        "/".into(),
        "userland",
        Vec::new(),
        0,
        drive.clone(),
    )
    .unwrap();
    assert_eq!(c.userland_exec, drive);
    assert_eq!(get(&s, "u1").unwrap().userland_exec, drive);
    let c = create(
        &s,
        Some("u2"),
        "img",
        "sha256:0",
        "/r",
        vec!["true".into()],
        Vec::new(),
        "/".into(),
        "chroot",
        Vec::new(),
        0,
        Vec::new(),
    )
    .unwrap();
    assert!(c.userland_exec.is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

/// T-1322: the reference query sees created containers, which hold no
/// lock, and only the digest it is asked about.
#[test]
fn referencing_finds_records_by_manifest_digest() {
    let d = std::env::temp_dir().join(format!("podvm-referrers-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let s = Store::open(&d).unwrap();
    let mk = |name: &str, digest: &str| {
        create(
            &s,
            Some(name),
            "img",
            digest,
            "/tmp",
            vec!["true".into()],
            Vec::new(),
            "/".into(),
            "chroot",
            Vec::new(),
            0,
            Vec::new(),
        )
        .unwrap()
    };
    mk("k1", "sha256:aaa");
    mk("k2", "sha256:aaa");
    mk("k3", "sha256:bbb");
    let mut names: Vec<String> = referencing(&s, "sha256:aaa")
        .unwrap()
        .iter()
        .map(|c| c.name.clone())
        .collect();
    names.sort();
    assert_eq!(names, vec!["k1".to_string(), "k2".to_string()]);
    assert!(referencing(&s, "sha256:zzz").unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

/// T-1318: `logs -f` prints what is already there, then what arrives,
/// and stops after the container ends. Driven step by step: no thread
/// anywhere in this crate, so the test interleaves with `follow_once`
/// instead of racing `follow`.
#[test]
fn follow_prints_arrivals_then_stops_at_exit() {
    let d = std::env::temp_dir().join(format!("podvm-follow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let s = Store::open(&d).unwrap();
    let c = create(
        &s,
        Some("f1"),
        "img",
        "sha256:0",
        "/tmp",
        vec!["true".into()],
        Vec::new(),
        "/".into(),
        "chroot",
        Vec::new(),
        0,
        Vec::new(),
    )
    .unwrap();
    let log = table::log_path(&s, &c.id);
    std::fs::write(&log, b"first\n").unwrap();
    let mut out = Vec::new();
    let (o1, done1) = follow_once(&s, "f1", &log, 0, &mut out).unwrap();
    assert!(!done1, "a created container is not terminal");
    assert_eq!(out, b"first\n");
    append(&s, &c.id, b"second\n");
    table::update(&s, |t| {
        let c = t
            .containers
            .iter_mut()
            .find(|c| c.name == "f1")
            .expect("the followed container is still recorded");
        c.state = table::State::Exited;
        Ok(())
    })
    .unwrap();
    let (_, done2) = follow_once(&s, "f1", &log, o1, &mut out).unwrap();
    assert!(done2, "an exited container ends the follow");
    assert_eq!(out, b"first\nsecond\n");
    let _ = std::fs::remove_dir_all(&d);
}

/// T-1337: the tail offset names the last lines exactly, with no
/// read past the buffer and no thread anywhere near it.
#[test]
fn tail_offset_names_the_last_lines() {
    // One trailing newline terminates rather than starting a line.
    assert_eq!(tail_offset(b"a\nb\nc\n", 2), 2);
    assert_eq!(tail_offset(b"a\nb\nc\n", 1), 4);
    assert_eq!(tail_offset(b"a\nb\nc\n", 3), 0);
    assert_eq!(tail_offset(b"a\nb\nc\n", 9), 0);
    // Without the trailing newline the same lines start the same way.
    assert_eq!(tail_offset(b"a\nb\nc", 2), 2);
    assert_eq!(tail_offset(b"a\nb\nc", 1), 4);
    // One line, empty input and zero lines are the three edges.
    assert_eq!(tail_offset(b"only\n", 1), 0);
    assert_eq!(tail_offset(b"only", 1), 0);
    assert_eq!(tail_offset(b"", 1), 0);
    assert_eq!(tail_offset(b"a\nb\n", 0), 4);
    assert_eq!(tail_offset(b"\n", 1), 0);
}

/// Append-only writer for the follow test: the launcher holds the log
/// open in production, and the test is the second writer.
fn append(store: &Store, id: &str, bytes: &[u8]) {
    use std::io::Write;
    let path = table::log_path(store, id);
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    f.write_all(bytes).unwrap();
}

/// T-1691 (issue 215): only a gone launcher reads as a container that ended.
#[test]
fn only_a_gone_launcher_is_a_stopped_container() {
    assert!(matches!(ended(Ok(Some(3))), Ok(Ended::Code(3))));
    assert!(matches!(ended(Ok(None)), Ok(Ended::Bound)));
    let gone = launcher::WaitError::Gone(Error("no launcher".into()));
    assert!(matches!(ended(Err(gone)), Ok(Ended::LauncherGone)));
    // ⛔ The defect: a failure to bound the wait read as the launcher gone,
    // and `stop -t 0` reported a running container as stopped.
    let failed = launcher::WaitError::Failed(Error("bounding the wait: invalid".into()));
    match ended(Err(failed)) {
        Err(e) => assert!(e.0.contains("bounding the wait"), "{e}"),
        Ok(_) => panic!("a failed wait must reach the caller"),
    }
}
