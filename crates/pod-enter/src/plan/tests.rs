use super::*;

const N: Names = Names::DEFAULT;

fn v(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| (*s).to_string()).collect()
}

/// An image whose config declares no `PATH` still gives its payload
/// docker's default. With none, the shell of the payload sets a `PATH`
/// that it does not export, and each child process runs with no `PATH`.
#[test]
fn an_image_with_no_path_still_gets_dockers_default() {
    let env = Plan::env_for(&N, &["container=oci".to_string()], &[]);
    assert!(env.iter().any(|e| e == &format!("PATH={}", Plan::DEFAULT_PATH)), "{env:?}");
    assert!(env.iter().any(|e| e == "HOME=/root"), "{env:?}");
    assert!(env.iter().any(|e| e == "container=oci"), "{env:?}");
    // No HOSTNAME: there is no UTS namespace, so the variable would
    // disagree with gethostname(2).
    assert!(!env.iter().any(|e| e.starts_with("HOSTNAME=")), "{env:?}");
}

/// The value of the image wins, and the value of the caller wins over it.
#[test]
fn a_declared_path_and_home_are_never_replaced_by_the_default() {
    let env = Plan::env_for(&N, &["PATH=/mine".to_string(), "HOME=/somewhere".to_string()], &[]);
    assert!(env.contains(&"PATH=/mine".to_string()), "{env:?}");
    assert!(env.contains(&"HOME=/somewhere".to_string()), "{env:?}");
    assert_eq!(env.iter().filter(|e| e.starts_with("PATH=")).count(), 1);

    let env = Plan::env_for(&N, &["PATH=/mine".to_string()], &["PATH=/theirs".to_string()]);
    assert_eq!(env.iter().filter(|e| e.starts_with("PATH=")).count(), 1);
    assert!(env.contains(&"PATH=/theirs".to_string()), "{env:?}");
}

#[test]
fn dockers_entrypoint_and_cmd_rules_are_the_ones_implemented() {
    let ep = v(&["/ep"]);
    let cmd = v(&["a", "b"]);

    // Entrypoint + nothing -> Entrypoint + Cmd
    assert_eq!(Plan::argv_for(Some(&ep), Some(&cmd), &[], None), v(&["/ep", "a", "b"]));
    // Entrypoint + args -> Entrypoint + args, and Cmd is dropped: the Cmd
    // of an image is the default arguments of its entrypoint.
    assert_eq!(Plan::argv_for(Some(&ep), Some(&cmd), &v(&["z"]), None), v(&["/ep", "z"]));
    // no Entrypoint + nothing -> Cmd
    assert_eq!(Plan::argv_for(None, Some(&cmd), &[], None), v(&["a", "b"]));
    // no Entrypoint + args -> args
    assert_eq!(Plan::argv_for(None, Some(&cmd), &v(&["z"]), None), v(&["z"]));
    // --entrypoint replaces, and drops Cmd with it
    assert_eq!(Plan::argv_for(Some(&ep), Some(&cmd), &[], Some("/other")), v(&["/other"]));
    assert_eq!(Plan::argv_for(Some(&ep), Some(&cmd), &v(&["z"]), Some("/other")), v(&["/other", "z"]));
}

#[test]
fn an_empty_entrypoint_array_is_not_the_same_as_an_absent_one() {
    // An image that sets `"Entrypoint": []` cleared the entrypoint of its
    // parent; an unset one would run the parent's.
    let empty: Vec<String> = Vec::new();
    let cmd = v(&["c"]);
    assert_eq!(Plan::argv_for(Some(&empty), Some(&cmd), &[], None), v(&["c"]));
}

#[test]
fn the_caller_wins_an_environment_name_and_the_image_copy_is_removed() {
    let out = Plan::env_for(&N, &v(&["A=1", "B=2"]), &v(&["A=9"]));
    // The docker defaults come after the declared names, so the check is
    // on the declared names in order.
    let declared: Vec<String> = out.iter().filter(|e| e.starts_with("A=") || e.starts_with("B=")).cloned().collect();
    assert_eq!(declared, v(&["B=2", "A=9"]));
    assert_eq!(out.iter().filter(|e| e.starts_with("A=")).count(), 1);
}

#[test]
fn path_comes_from_the_environment_and_falls_back_to_a_stated_default() {
    assert_eq!(Plan::path_from(&v(&["PATH=/x:/y"])), v(&["/x", "/y"]));
    // The last value wins, as in env_for.
    assert_eq!(Plan::path_from(&v(&["PATH=/a", "PATH=/b"])), v(&["/b"]));
    assert!(Plan::path_from(&[]).contains(&"/bin".to_string()));
}

/// The request variable never reaches the payload from the image or from
/// the caller, and an old active claim does not either.
#[test]
fn the_mode_request_and_any_stale_result_are_scrubbed() {
    let request = format!("{}=", N.mode_request);
    let active = format!("{}=", N.mode_active);
    let env = Plan::env_for(&N, &[format!("{request}memfd")], &["A=1".to_string()]);
    assert!(!env.iter().any(|e| e.starts_with(&request)), "{env:?}");
    assert!(env.iter().any(|e| e == "A=1"), "{env:?}");

    let env = Plan::env_for(&N, &[], &[format!("{request}cache")]);
    assert!(!env.iter().any(|e| e.starts_with(&request)), "{env:?}");

    let env = Plan::env_for(&N, &[format!("{active}memfd")], &[]);
    assert!(!env.iter().any(|e| e.starts_with(&active)), "{env:?}");
}

/// The request goes out and the entered rung comes in under the other
/// name; an old claim is replaced, not doubled.
#[test]
fn the_entered_rung_is_reported_under_a_different_name() {
    let mut env = vec![format!("{}=memfd", N.mode_request), "A=1".to_string()];
    Plan::set_active_mode(&N, &mut env, "chroot");
    assert!(env.contains(&format!("{}=chroot", N.mode_active)), "{env:?}");
    assert!(!env.iter().any(|e| e.starts_with(&format!("{}=", N.mode_request))), "{env:?}");
    assert!(env.contains(&"A=1".to_string()), "{env:?}");

    Plan::set_active_mode(&N, &mut env, "memfd");
    let active = format!("{}=", N.mode_active);
    assert_eq!(env.iter().filter(|e| e.starts_with(&active)).count(), 1, "{env:?}");
    assert!(env.contains(&format!("{active}memfd")), "{env:?}");
}

#[test]
fn a_name_with_a_slash_is_a_path_and_never_searched_on_path() {
    let mut p = Plan {
        argv: v(&["/bin/echo", "hi"]),
        env: Vec::new(),
        working_dir: String::new(),
        fds: Fds::default(),
        banner: String::new(),
        path_dirs: v(&["/usr/bin", "/bin"]),
        names: N,
        limits: Vec::new(),
        no_new_privs: false,
        die_with_parent: false,
    };
    assert_eq!(p.program_candidates(), v(&["/bin/echo"]));

    p.argv = v(&["echo"]);
    assert_eq!(p.program_candidates(), v(&["/usr/bin/echo", "/bin/echo"]));

    // A relative path with a slash is a path, not a lookup in PATH: `./x`
    // is the `x` of the working directory.
    p.argv = v(&["./x"]);
    assert_eq!(p.program_candidates(), v(&["./x"]));
}
