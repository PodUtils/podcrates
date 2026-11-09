use super::*;

/// The fields of a caller in the tests: the digest of an image, and the
/// argv of a detached drive.
#[derive(Clone, Debug, Default, PartialEq, Serialize, serde::Deserialize)]
pub(crate) struct Fields {
    #[serde(default)]
    pub manifest_digest: String,
    #[serde(default)]
    pub userland_exec: Vec<String>,
}

pub(crate) fn scratch(name: &str) -> Settings {
    let d = std::env::temp_dir().join(format!("pod-supervise-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    Settings::new(d)
}

fn make(s: &Settings, name: &str, digest: &str, drive: Vec<String>) -> Container<Fields> {
    create(
        s,
        Some(name),
        vec!["true".into()],
        Vec::new(),
        "/".into(),
        Fields { manifest_digest: digest.into(), userland_exec: drive },
    )
    .unwrap()
}

#[test]
fn an_id_is_sixty_four_hex_characters_and_two_are_not_equal() {
    let a = new_id();
    let b = new_id();
    assert_eq!(a.len(), 64, "{a}");
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "{a}");
    // Two containers of one image are two containers.
    assert_ne!(a, b);
}

#[test]
fn a_generated_name_is_short_and_derived_from_the_id() {
    let s = Settings::new("/nonexistent");
    let id = new_id();
    let n = generated_name(&s, &id);
    assert!(n.starts_with("pod_"));
    assert!(id.starts_with(&n["pod_".len()..]));
    assert_eq!(n.len(), "pod_".len() + 12);
}

/// Create makes the directory of the container, so that a file of the
/// caller can go beside the record at once.
#[test]
fn creating_a_container_makes_its_directory() {
    let s = scratch("mkdir");
    let c = make(&s, "e1", "sha256:0", Vec::new());
    assert!(table::dir(&s, &c.id).is_dir());
    let src = s.root.join("ephemeral.tmp");
    std::fs::write(&src, b"memo").unwrap();
    let dest = table::dir(&s, &c.id).join("memo");
    std::fs::rename(&src, &dest).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"memo");
    let _ = std::fs::remove_dir_all(&s.root);
}

/// `create` stores the fields of the caller beside the record, and a
/// record with an empty field keeps it empty.
#[test]
fn creating_a_container_stores_the_userland_drive() {
    let s = scratch("drive");
    let drive = vec!["/r/lib/ld.so".to_string(), "/r/bin/sh".to_string()];
    let c = make(&s, "u1", "sha256:0", drive.clone());
    assert_eq!(c.userland_exec, drive);
    assert_eq!(get::<Fields>(&s, "u1").unwrap().userland_exec, drive);
    let c = make(&s, "u2", "sha256:0", Vec::new());
    assert!(c.userland_exec.is_empty());
    // A name is used one time.
    let again = create(&s, Some("u1"), Vec::new(), Vec::new(), "/".into(), Fields::default()).expect_err("a second u1");
    assert_eq!(again.class, Class::Usage);
    assert!(again.text().contains("already used"), "{again}");
    let _ = std::fs::remove_dir_all(&s.root);
}

/// The query sees created containers, which hold no lock, and only the
/// digest that it asks for.
#[test]
fn referencing_finds_records_by_manifest_digest() {
    let s = scratch("referrers");
    make(&s, "k1", "sha256:aaa", Vec::new());
    make(&s, "k2", "sha256:aaa", Vec::new());
    make(&s, "k3", "sha256:bbb", Vec::new());
    let by = |d: &str| {
        let mut names: Vec<String> = matching(&s, |c: &Container<Fields>| c.manifest_digest == d)
            .unwrap()
            .iter()
            .map(|c| c.name.clone())
            .collect();
        names.sort();
        names
    };
    assert_eq!(by("sha256:aaa"), vec!["k1".to_string(), "k2".to_string()]);
    assert!(by("sha256:zzz").is_empty());
    assert_eq!(list::<Fields>(&s, true).unwrap().len(), 3);
    assert!(list::<Fields>(&s, false).unwrap().is_empty(), "a created container is not running");
    let _ = std::fs::remove_dir_all(&s.root);
}

/// `logs -f` writes what is there, then what comes, and stops after the
/// container ends. Driven step by step: the crate has no thread, so the
/// test calls `follow_once` and does not race `follow`.
#[test]
fn follow_prints_arrivals_then_stops_at_exit() {
    let s = scratch("follow");
    let c = make(&s, "f1", "sha256:0", Vec::new());
    let log = table::log_path(&s, &c.id);
    std::fs::write(&log, b"first\n").unwrap();
    let mut out = Vec::new();
    let (o1, done1) = logs::follow_once::<Fields>(&s, "f1", &log, 0, &mut out).unwrap();
    assert!(!done1, "a created container is not terminal");
    assert_eq!(out, b"first\n");
    append(&s, &c.id, b"second\n");
    table::update(&s, |t: &mut table::Table<Fields>| {
        let c = t.containers.iter_mut().find(|c| c.name == "f1").expect("the followed container is still recorded");
        c.state = table::State::Exited;
        Ok::<_, Failure>(())
    })
    .unwrap();
    let (_, done2) = logs::follow_once::<Fields>(&s, "f1", &log, o1, &mut out).unwrap();
    assert!(done2, "an exited container ends the follow");
    assert_eq!(out, b"first\nsecond\n");
    assert_eq!(logs::<Fields>(&s, "f1").unwrap(), b"first\nsecond\n");
    let _ = std::fs::remove_dir_all(&s.root);
}

/// The tail offset names the last lines exactly, with no read past the
/// buffer.
#[test]
fn tail_offset_names_the_last_lines() {
    // One newline at the end ends a line; it does not start one.
    assert_eq!(tail_offset(b"a\nb\nc\n", 2), 2);
    assert_eq!(tail_offset(b"a\nb\nc\n", 1), 4);
    assert_eq!(tail_offset(b"a\nb\nc\n", 3), 0);
    assert_eq!(tail_offset(b"a\nb\nc\n", 9), 0);
    // With no newline at the end, the same lines start at the same place.
    assert_eq!(tail_offset(b"a\nb\nc", 2), 2);
    assert_eq!(tail_offset(b"a\nb\nc", 1), 4);
    // One line, an empty input and zero lines are the three edges.
    assert_eq!(tail_offset(b"only\n", 1), 0);
    assert_eq!(tail_offset(b"only", 1), 0);
    assert_eq!(tail_offset(b"", 1), 0);
    assert_eq!(tail_offset(b"a\nb\n", 0), 4);
    assert_eq!(tail_offset(b"\n", 1), 0);
}

/// A second writer for the follow test: the launcher holds the log open,
/// and the test writes too.
fn append(s: &Settings, id: &str, bytes: &[u8]) {
    use std::io::Write;
    let path = table::log_path(s, id);
    let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
    f.write_all(bytes).unwrap();
}

/// Only a launcher that is gone reads as a container that ended.
#[test]
fn only_a_gone_launcher_is_a_stopped_container() {
    assert!(matches!(ended(Ok(Some(3))), Ok(Ended::Code(3))));
    assert!(matches!(ended(Ok(None)), Ok(Ended::Bound)));
    let gone = launcher::WaitError::Gone(failure(Class::Unavailable, "no launcher"));
    assert!(matches!(ended(Err(gone)), Ok(Ended::LauncherGone)));
    // The defect: a failure to bound the wait read as a launcher that is
    // gone, and `stop -t 0` reported a running container as stopped.
    let failed = launcher::WaitError::Failed(failure(Class::Unavailable, "bounding the wait: invalid"));
    match ended(Err(failed)) {
        Err(e) => assert!(e.text().contains("bounding the wait"), "{e}"),
        Ok(_) => panic!("a failed wait must reach the caller"),
    }
}

/// A container that is not running has nothing to signal or stop, and a
/// running one is not removed with no force.
#[test]
fn the_verbs_refuse_what_the_state_does_not_allow() {
    let s = scratch("verbs");
    make(&s, "v1", "sha256:0", Vec::new());
    let e = kill::<Fields>(&s, "v1", 15).expect_err("a signal to a created container");
    assert!(e.text().contains("created and not running"), "{e}");
    let (c, killed) = stop::<Fields>(&s, "v1", 0).unwrap();
    assert_eq!((c.state, killed), (State::Created, false));
    assert_eq!(wait::<Fields>(&s, "v1", 0).unwrap().1, None);
    let gone = remove::<Fields>(&s, "v1", false).unwrap();
    assert!(!table::dir(&s, &gone.id).exists(), "remove left the directory");
    let e = get::<Fields>(&s, "v1").expect_err("a removed container");
    assert_eq!(e.class, Class::NotFound);
    let _ = std::fs::remove_dir_all(&s.root);
}
