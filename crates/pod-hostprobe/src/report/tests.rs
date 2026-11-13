use super::*;
use crate::sys;
use serde_json::{Value, json};

/// A set in which each field of the object has a value that is not its
/// default, so that a field that `write` or `read` drops changes the text.
fn fake() -> (Findings, Selection) {
    let findings = Findings {
        rows: vec![
            ("unshare(CLONE_NEWNS)", Outcome::denied(sys::EPERM)),
            ("clone(CLONE_NEWNS)", Outcome::ok()),
            ("chroot(/tmp)", Outcome::ok_with("an extra fact")),
            ("mount(tmpfs,/mnt)", Outcome::skip(Some(sys::ENOENT), "a \"quoted\" reason,\ttab, newline\n, é")),
            ("pidfd_getfd(-1,-1) [control]", Outcome::denied(sys::EBADF)),
            ("kcmp(-1,-1,...) [control]", Outcome::skip(None, "kcmp(2) is absent")),
        ],
        identity: Identity {
            uid: 1000,
            gid: -1,
            groups: vec![0, 65534],
            uid_map: Some("0 1000 1".to_string()),
            gid_map: None,
            setgroups: Some("deny".to_string()),
            cap_eff: Some("000001ffffffffff".to_string()),
            cap_prm: Some("0000000000000000".to_string()),
            cap_bnd: Some("000001ffffffffff".to_string()),
            seccomp: Some("2".to_string()),
            seccomp_filters: Some("1".to_string()),
            no_new_privs: Some("1".to_string()),
            threads: Some("4".to_string()),
            unreadable: vec!["/proc/self/gid_map: EACCES".to_string()],
        },
        writable: vec![
            WriteProbe {
                path: "/tmp".to_string(),
                source: "fixed".to_string(),
                outcome: Outcome::ok(),
                space: Some(Space { blocks_free: u64::MAX, block_size: 4096, inodes_free: 12 }),
                space_error: None,
            },
            WriteProbe {
                path: "/state".to_string(),
                source: "$HOME".to_string(),
                outcome: Outcome::denied(sys::EACCES),
                space: None,
                space_error: Some(sys::ENOENT),
            },
        ],
        self_exe: "/proc/self/exe".to_string(),
    };
    let selection = Selection::choose(&findings);
    (findings, selection)
}

fn written() -> Value {
    let (f, s) = fake();
    serde_json::from_str(&write(&f, &s)).unwrap()
}

fn read_value(v: &Value) -> Result<(Findings, Selection), ReadError> {
    read(&v.to_string())
}

#[test]
fn a_fake_set_reads_back_to_the_same_text() {
    let (f, s) = fake();
    let text = write(&f, &s);
    assert!(!text.contains('\n'), "{text}");
    assert!(text.starts_with(&format!("{{\"schema\":{SCHEMA},")), "{text}");
    let (f2, s2) = read(&text).unwrap();
    assert_eq!(write(&f2, &s2), text);
    // The fixture selects chroot, so it carries the two rejections above it.
    assert_eq!(s2.rung, Rung::Chroot);
    assert_eq!(s2.rejected.len(), 2);
    for r in &s2.rejected {
        assert_eq!(r.requirement, r.rung.requirement());
    }
    assert_eq!(s2.controls_answered, s.controls_answered);
    assert_eq!(f2.identity.groups, vec![0, 65534]);
    assert_eq!(f2.writable[1].space_error, Some(sys::ENOENT));
}

#[test]
fn each_row_name_is_the_name_of_its_probe() {
    let (f, s) = fake();
    let (f2, _) = read(&write(&f, &s)).unwrap();
    assert_eq!(f2.rows.len(), f.rows.len());
    for (name, _) in &f2.rows {
        let probe = probes::find(name).unwrap();
        assert!(std::ptr::eq(*name, probe.name), "{name} is not the str of PROBES");
    }
}

#[test]
fn the_findings_of_this_host_read_back_to_the_same_text() {
    let f = crate::run(&crate::Settings::default(), &crate::writable::candidates(&[]));
    let s = Selection::choose(&f);
    let text = write(&f, &s);
    let (f2, s2) = read(&text).unwrap();
    assert_eq!(f2.rows.len(), probes::PROBES.len());
    assert_eq!(s2.rung, s.rung);
    assert_eq!(write(&f2, &s2), text);
}

#[test]
fn the_keys_in_another_order_and_white_space_read() {
    let v = written();
    let pretty = serde_json::to_string_pretty(&v).unwrap();
    let (f, s) = read(&pretty).unwrap();
    let (f0, s0) = fake();
    assert_eq!(write(&f, &s), write(&f0, &s0));
}

#[test]
fn a_row_that_probes_does_not_name_is_refused() {
    let mut v = written();
    v["findings"]["rows"][1]["name"] = json!("frobnicate(2)");
    assert_eq!(read_value(&v).err(), Some(ReadError::UnknownRow("frobnicate(2)".to_string())));
}

#[test]
fn a_row_that_comes_two_times_is_refused() {
    let mut v = written();
    let first = v["findings"]["rows"][0].clone();
    v["findings"]["rows"].as_array_mut().unwrap().push(first);
    assert_eq!(read_value(&v).err(), Some(ReadError::DuplicateRow("unshare(CLONE_NEWNS)".to_string())));
}

#[test]
fn an_unknown_version_is_refused() {
    let mut v = written();
    v["schema"] = json!(SCHEMA + 1);
    assert_eq!(read_value(&v).err(), Some(ReadError::Version(Some(u64::from(SCHEMA) + 1))));
    v["schema"] = json!(SCHEMA.to_string());
    assert_eq!(read_value(&v).err(), Some(ReadError::Version(None)));
    v.as_object_mut().unwrap().remove("schema");
    assert_eq!(read_value(&v).err(), Some(ReadError::Version(None)));
}

#[test]
fn a_version_is_refused_before_the_form() {
    // A later version can change each other field.
    assert_eq!(read(r#"{"schema":2,"rows":7}"#).err(), Some(ReadError::Version(Some(2))));
}

#[test]
fn an_unknown_word_is_refused() {
    let mut v = written();
    v["findings"]["rows"][0]["verdict"] = json!("maybe");
    assert_eq!(read_value(&v).err(), Some(ReadError::Word { field: "verdict", word: "maybe".to_string() }));
    let mut v = written();
    v["findings"]["writable"][0]["verdict"] = json!("Ok");
    assert_eq!(read_value(&v).err(), Some(ReadError::Word { field: "verdict", word: "Ok".to_string() }));
    let mut v = written();
    v["selection"]["rung"] = json!("root");
    assert_eq!(read_value(&v).err(), Some(ReadError::Word { field: "rung", word: "root".to_string() }));
    let mut v = written();
    v["selection"]["rejected"][0]["rung"] = json!("vm");
    assert_eq!(read_value(&v).err(), Some(ReadError::Word { field: "rung", word: "vm".to_string() }));
}

#[test]
fn a_denial_without_an_errno_is_refused() {
    let mut v = written();
    v["findings"]["rows"][0]["errno"] = Value::Null;
    assert_eq!(read_value(&v).err(), Some(ReadError::DenialWithoutErrno("unshare(CLONE_NEWNS)".to_string())));
    let mut v = written();
    v["findings"]["writable"][1]["errno"] = Value::Null;
    assert_eq!(read_value(&v).err(), Some(ReadError::DenialWithoutErrno("/state".to_string())));
}

#[test]
fn a_text_of_another_form_is_refused() {
    for text in ["", "not json", "[1]", "null", "{\"schema\":1}"] {
        assert!(matches!(read(text), Err(ReadError::Form(_) | ReadError::Version(_))), "{text:?}");
    }
    assert!(matches!(read("[1]"), Err(ReadError::Form(_))));
    assert!(matches!(read("{\"schema\":1}"), Err(ReadError::Form(_))));
    // An unknown field is a field of another version.
    let mut v = written();
    v["findings"]["identity"]["boot_id"] = json!("x");
    assert!(matches!(read_value(&v), Err(ReadError::Form(_))));
}

#[test]
fn each_word_maps_back_to_its_value() {
    for v in VERDICTS {
        assert_eq!(outcome_of(v.word(), Some(1), String::new(), "x").unwrap().verdict, v);
    }
    for r in RUNGS {
        assert_eq!(rung_of(r.word()).unwrap(), r);
    }
}

#[test]
fn a_refusal_names_what_it_refused() {
    let e = ReadError::UnknownRow("frobnicate(2)".to_string());
    assert!(e.to_string().contains("frobnicate(2)"), "{e}");
    assert!(ReadError::Version(Some(9)).to_string().contains('9'));
}
