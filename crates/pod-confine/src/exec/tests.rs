use super::*;

fn spec() -> Spec {
    Spec::new()
        .allow("/usr", Access::READ | Access::EXECUTE)
        .allow("/data", Access::READ)
        .allow("/opt/tool/", Access::READ | Access::EXECUTE)
        .allow("//opt//tool", Access::EXECUTE)
        .allow("/srv/run.sh", Access::READ | Access::EXECUTE)
        .allow("/tmp", Access::READ | Access::WRITE)
}

#[test]
fn the_table_holds_each_execute_path_once_and_no_other() {
    assert_eq!(spec().exec_table().paths(), ["/usr", "/opt/tool", "/srv/run.sh"]);
    assert!(Spec::new().exec_table().paths().is_empty());
}

#[test]
fn a_program_runs_under_an_execute_path_and_nowhere_else() {
    let t = spec().exec_table();
    for p in ["/usr/bin/true", "/usr", "/opt/tool/bin/x", "/srv/run.sh"] {
        assert!(t.allows(p), "{p}");
    }
    for p in [
        "/data/x",
        "/tmp/x",
        "/usrx/bin/true",
        "/opt/toolbox",
        "/srv/run.sh.bak",
        "/srv",
        "usr/bin/true",
        "",
        "/usr/../tmp/x",
        "/usr/./bin/true",
    ] {
        assert!(!t.allows(p), "{p}");
    }
    assert!(!Spec::new().exec_table().allows("/usr/bin/true"), "an empty table allowed a program");
    let root = Spec::new().allow("/", Access::EXECUTE).exec_table();
    assert!(root.allows("/any/where") && !root.allows("/a/../b"));
}

#[test]
fn the_text_of_the_table_gives_the_same_answers_with_no_allocation() {
    let t = spec().exec_table();
    let text = t.encode().unwrap();
    assert_eq!(text, "/usr:/opt/tool:/srv/run.sh");
    assert_eq!(ExecTable::decode(&text), t);
    for p in ["/usr/bin/true", "/opt/tool/x", "/srv/run.sh", "/data/x", "/opt/toolbox", "/usr/../x"] {
        assert_eq!(allows_in(text.as_bytes(), p.as_bytes()), t.allows(p), "{p}");
    }
    assert!(!allows_in(b"", b"/usr/bin/true"));
    assert!(!allows_in(b"::", b"/usr/bin/true"));
    let e = Spec::new().allow("/a:b", Access::EXECUTE).exec_table().encode().unwrap_err();
    assert_eq!(e.class, Class::Usage);
    assert!(e.to_string().contains("\"/a:b\""), "{e}");
}

#[test]
fn a_link_in_a_rule_path_resolves_to_its_target() {
    let d = std::env::temp_dir().join(format!("pod-confine-exec-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("real/bin")).unwrap();
    std::os::unix::fs::symlink(d.join("real"), d.join("link")).unwrap();
    let real = std::fs::canonicalize(d.join("real")).unwrap();
    let t = Spec::new()
        .allow(d.join("link").to_str().unwrap(), Access::EXECUTE)
        .allow("/no/such/path", Access::EXECUTE)
        .exec_table()
        .resolved();
    assert_eq!(t.paths(), [real.to_str().unwrap(), "/no/such/path"]);
    assert!(t.allows(real.join("bin/x").to_str().unwrap()));
    let _ = std::fs::remove_dir_all(d);
}
