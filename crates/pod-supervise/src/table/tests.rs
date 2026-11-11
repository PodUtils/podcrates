use super::*;

use crate::tests::{Fields, scratch};

fn c(id: &str, name: &str, state: State) -> Container<Fields> {
    Container {
        id: id.to_string(),
        name: name.to_string(),
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
        wall_ms: None,
        end: None,
        of: Fields { manifest_digest: "sha256:0".into(), userland_exec: Vec::new() },
    }
}

#[test]
fn a_name_a_full_id_and_a_prefix_all_resolve() {
    let s = Settings::new("/nonexistent");
    let t = Table { version: TABLE_VERSION, containers: vec![c("abcdef0123", "one", State::Created)] };
    assert_eq!(find(&s, &t, "one").unwrap().id, "abcdef0123");
    assert_eq!(find(&s, &t, "abcdef0123").unwrap().id, "abcdef0123");
    assert_eq!(find(&s, &t, "abc").unwrap().id, "abcdef0123");
    assert!(find(&s, &t, "nope").is_err());
}

/// Never the first match: a caller that gets the wrong container from a
/// prefix of two ids cannot find out.
#[test]
fn an_ambiguous_prefix_is_an_error_and_names_the_candidates() {
    let s = Settings::new("/nonexistent");
    let t = Table {
        version: TABLE_VERSION,
        containers: vec![c("abcdef0123", "one", State::Created), c("abcff09999", "two", State::Created)],
    };
    let e = find(&s, &t, "abc").unwrap_err().text().to_string();
    assert!(e.contains("names 2 containers"), "{e}");
    assert!(e.contains("abcdef0123"), "{e}");
    // An exact name still wins over a prefix of two ids.
    assert_eq!(find(&s, &t, "two").unwrap().id, "abcff09999");
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

/// The version goes under the key of the settings, first; a file with no
/// version under that key does not parse.
#[test]
fn the_table_file_keeps_its_version_under_the_key_of_the_settings() {
    let mut s = Settings::new("/nonexistent");
    s.table_key = "tool_containers".into();
    let t = Table { version: TABLE_VERSION, containers: vec![c("abc", "one", State::Created)] };
    let bytes = encode(&s, &t).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.trim_start().starts_with("{\n  \"tool_containers\": 1,"), "{text}");
    let back: Table<Fields> = decode(&s, &bytes).unwrap();
    assert_eq!(back.containers.len(), 1);
    assert_eq!(back.containers[0].manifest_digest, "sha256:0");
    s.table_key = "other".into();
    let e = decode::<Fields>(&s, &bytes).unwrap_err();
    assert_eq!(e.class, Class::Protocol);
    assert!(e.text().contains("no version under \"other\""), "{e}");
}

/// A record with only the fields of the crate parses, with the defaults of
/// the fields of the caller.
#[test]
fn a_table_without_the_userland_drive_still_parses() {
    let s = Settings::new("/nonexistent");
    let bytes = serde_json::to_vec(&serde_json::json!({
        "pod_supervise": TABLE_VERSION,
        "containers": [{
            "id": "abc",
            "name": "one",
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
    let t: Table<Fields> = decode(&s, &bytes).unwrap();
    assert!(t.containers[0].userland_exec.is_empty());
}

/// A running record whose lock no launcher holds is `Dead`, with the time
/// of the notice and no exit code; a record whose lock is held stays
/// `Running`.
#[test]
fn a_running_record_with_a_free_lock_is_dead_and_a_held_lock_keeps_it_running() {
    let s = scratch("reconcile");
    std::fs::create_dir_all(&s.root).unwrap();
    let t = Table {
        version: TABLE_VERSION,
        containers: vec![
            c("aaa1", "free", State::Running),
            c("bbb2", "held", State::Running),
            c("ccc3", "nolock", State::Running),
        ],
    };
    update(&s, |x: &mut Table<Fields>| {
        *x = t.clone();
        Ok::<_, Failure>(())
    })
    .unwrap();
    std::fs::create_dir_all(dir(&s, "aaa1")).unwrap();
    std::fs::write(lock_path(&s, "aaa1"), b"").unwrap();
    let held = Lock::try_exclusive(&lock_path(&s, "bbb2")).unwrap().expect("a free lock");
    let r: Table<Fields> = reconcile(&s).unwrap();
    let by = |n: &str| r.containers.iter().find(|c| c.name == n).unwrap().clone();
    assert_eq!(by("free").state, State::Dead);
    assert!(by("free").noticed.is_some() && by("free").exit_code.is_none());
    assert_eq!(by("held").state, State::Running);
    assert_eq!(by("nolock").state, State::Dead);
    assert!(by("free").status().contains("not watching"), "{}", by("free").status());
    drop(held);
    let _ = std::fs::remove_dir_all(&s.root);
}

/// A crafted id cannot name a directory out of the directory of the
/// containers.
#[test]
fn a_crafted_id_names_no_directory_outside_the_containers() {
    let s = scratch("contain");
    std::fs::create_dir_all(dir(&s, "good")).unwrap();
    std::fs::create_dir_all(s.root.join("outside")).unwrap();
    assert!(contained(&s, "good").is_some());
    assert!(contained(&s, "../outside").is_none());
    assert!(contained(&s, "").is_none());
    assert!(contained(&s, "absent").is_none());
    let _ = std::fs::remove_dir_all(&s.root);
}
