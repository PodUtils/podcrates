//! The rule `exec=` for each file that the kernel would execute for one
//! argv (PodUtils/podvm#613): by host paths ([`check_exec_argv`]) and by
//! guest paths through a table ([`check_exec_guest`]).

use std::os::unix::fs::symlink;
use std::path::PathBuf;

use pod_confine::{Access, Spec};

use super::*;
use crate::userland::Volume;
use crate::{Fds, Names};

/// A scratch tree, removed on drop.
struct Tree(PathBuf);

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Tree {
    fn new(tag: &str) -> Tree {
        let d = std::env::temp_dir().join(format!("pod-enter-program-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Tree(std::fs::canonicalize(&d).unwrap())
    }

    fn put(&self, rel: &str, bytes: &[u8]) {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    fn host(&self, rel: &str) -> String {
        self.0.join(rel).display().to_string()
    }
}

fn plan(confine: Option<Spec>, working_dir: &str, path_dirs: &[&str]) -> Plan {
    Plan {
        argv: Vec::new(),
        env: Vec::new(),
        working_dir: working_dir.to_string(),
        fds: Fds::default(),
        banner: String::new(),
        path_dirs: path_dirs.iter().map(|d| d.to_string()).collect(),
        names: Names::DEFAULT,
        limits: Vec::new(),
        no_new_privs: false,
        die_with_parent: false,
        confine,
        hostname: None,
        volumes: Vec::new(),
        userns_ids: None,
        pid_namespace: false,
        relay_title: None,
    }
}

fn v(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

/// The image of podvm's test (PodUtils/podvm@T-2013), with a volume, the
/// interpreter of a `#!` line and musl's loader as a link to `libc.so`.
/// Each row: the argv by guest paths, and whether the rule holds.
#[test]
fn the_first_program_by_guest_paths_takes_the_rule_of_exec() {
    let t = Tree::new("guest");
    let vol = Tree::new("guest-vol");
    for f in ["bin/sh", "lib64/ld-linux-x86-64.so.2", "sc/m", "tmp/m", "usr/lib/x86_64-linux-musl/libc.so"] {
        t.put(f, b"x");
    }
    t.put("sc/script", b"#!/bin/sh\nexit 0\n");
    t.put("sc/badscript", b"#!/tmp/m -x\n");
    t.put("bin/ldscript", b"#!/lib64/ld-linux-x86-64.so.2 /tmp/m\n");
    t.put("bin/ldok", b"#!/lib64/ld-linux-x86-64.so.2\n");
    t.put("bin/deep", b"#!/bin/deep\n");
    std::fs::create_dir_all(t.0.join("lib")).unwrap();
    symlink("/usr/lib/x86_64-linux-musl/libc.so", t.0.join("lib/ld-musl-x86_64.so.1")).unwrap();
    symlink("/lib64/ld-linux-x86-64.so.2", t.0.join("bin/run")).unwrap();
    vol.put("tool", b"x");
    let maps = Maps::new(
        &Names::DEFAULT,
        t.0.to_str().unwrap(),
        &[Volume { guest: "/vol".to_string(), host: vol.0.display().to_string() }],
    )
    .unwrap();
    let spec = Spec::new()
        .allow(t.host("sc"), Access::READ)
        .allow(t.host("tmp"), Access::READ | Access::WRITE)
        .allow(t.host("bin"), Access::READ | Access::EXECUTE)
        .allow(t.host("lib64"), Access::READ | Access::EXECUTE)
        .allow(t.host("usr/lib"), Access::READ | Access::EXECUTE)
        .allow(t.host("sc/script"), Access::EXECUTE)
        .allow(t.host("sc/badscript"), Access::EXECUTE)
        .allow(vol.host("tool"), Access::EXECUTE);
    let p = plan(Some(spec), "/tmp", &["/bin", "/sc"]);
    let ld = "/lib64/ld-linux-x86-64.so.2";
    let musl = "/lib/ld-musl-x86_64.so.1";
    let rows: Vec<(Vec<String>, bool)> = vec![
        (v(&["/bin/sh", "-c", "true"]), true),
        (v(&["sh"]), true),
        (v(&["/sc/m"]), false),
        (v(&["m"]), false),
        (v(&["./m"]), false),
        (v(&["../bin/sh"]), true),
        (v(&["/vol/tool"]), true),
        (v(&[ld, "/sc/m"]), false),
        (v(&[ld, "--library-path", "/tmp", "/tmp/m"]), false),
        (v(&[ld, "m"]), false),
        (v(&[ld, "./m"]), false),
        (v(&[ld, "/nowhere/x"]), false),
        (v(&[ld, "/bin/sh"]), true),
        (v(&[ld, "/vol/tool"]), true),
        (v(&[ld, "--version"]), true),
        // A link named as no loader, to the loader.
        (v(&["/bin/run", "/tmp/m"]), false),
        // musl's loader as a link to `libc.so`: the name as given counts.
        (v(&[musl, "/tmp/m"]), false),
        (v(&["/usr/lib/x86_64-linux-musl/libc.so", "/tmp/m"]), false),
        (v(&[musl, "/bin/sh"]), true),
        // A loader as the program of a loader.
        (v(&[ld, ld, "/tmp/m"]), false),
        // The interpreter of a `#!` line, and the loader as one.
        (v(&["script"]), true),
        (v(&["/sc/badscript"]), false),
        (v(&["ldscript"]), false),
        (v(&["ldok"]), true),
        (v(&["/bin/deep"]), false),
        (v(&["/nowhere/x"]), true),
        (v(&["nowhere"]), true),
        (Vec::new(), true),
    ];
    for (argv, holds) in rows {
        let got = check_exec_guest(&p, &maps, &argv);
        assert_eq!(got.is_ok(), holds, "{argv:?}: {got:?}");
        if let Err(e) = got {
            assert!(matches!(e, Error::CannotInvoke(_)), "{argv:?}: {e:?}");
        }
    }
    // No confinement: each program runs.
    let open = plan(None, "/tmp", &["/bin"]);
    assert!(check_exec_guest(&open, &maps, &v(&["/sc/m"])).is_ok());
    assert!(check_exec_guest(&open, &maps, &v(&[ld, "/sc/m"])).is_ok());
}

/// The same rule by host paths: the program, its `#!` interpreter and the
/// program of the loader; a file that does not resolve is
/// [`Error::NotFound`].
#[test]
fn check_exec_argv_takes_the_rule_for_the_program_of_the_loader() {
    let t = Tree::new("host");
    for f in ["exec/ld-linux-x86-64.so.2", "exec/prog", "read/prog", "exec/sh"] {
        t.put(f, b"x");
    }
    t.put("exec/script", format!("#!{} -e\n", t.host("exec/sh")).as_bytes());
    t.put("exec/outscript", format!("#!{}\n", t.host("read/prog")).as_bytes());
    let spec = Spec::new().allow(t.host(""), Access::READ).allow(t.host("exec"), Access::READ | Access::EXECUTE);
    let p = plan(Some(spec), "/", &[]);
    let (ld, inside, outside) = (t.host("exec/ld-linux-x86-64.so.2"), t.host("exec/prog"), t.host("read/prog"));
    // (argv, the answer: 0 for `Ok`, else the exit code of the error)
    let rows: Vec<(Vec<String>, i32)> = vec![
        (v(&[&inside]), 0),
        (v(&[&outside]), 126),
        (v(&[&ld, &inside]), 0),
        (v(&[&ld, &outside]), 126),
        (v(&[&ld, "--preload", &outside, &inside]), 0),
        (v(&[&ld, "prog"]), 126),
        (v(&[&t.host("exec/script")]), 0),
        (v(&[&t.host("exec/outscript")]), 126),
        (v(&[&t.host("exec/absent")]), 127),
    ];
    for (argv, want) in rows {
        let got = check_exec_argv(&p, &argv);
        let code = match &got {
            Ok(()) => 0,
            Err(Error::CannotInvoke(_)) => 126,
            Err(Error::NotFound(_)) => 127,
            Err(_) => -1,
        };
        assert_eq!(code, want, "{argv:?}: {got:?}");
    }
    // The program of the loader is named in the refusal.
    let e = check_exec_argv(&p, &v(&[&ld, &outside])).unwrap_err().to_string();
    assert!(e.contains(&outside) && e.contains("dynamic loader"), "{e}");
    assert!(check_exec_argv(&plan(None, "/", &[]), &v(&[&ld, &outside])).is_ok());
}

/// The kernel follows five `#!` lines and refuses a sixth with `ELOOP`
/// (`fs/exec.c`, `exec_binprm`: `depth > 5`; a chain of five scripts runs on
/// Linux 6.18). The check follows the same count (T-1033).
#[test]
fn a_chain_of_five_scripts_takes_the_rule_and_a_sixth_level_is_eloop() {
    let t = Tree::new("chain");
    t.put("exec/sh", b"x");
    let chain = |tag: &str, n: usize| -> String {
        let mut next = t.host("exec/sh");
        for k in (0..n).rev() {
            let name = format!("exec/{tag}{k}");
            t.put(&name, format!("#!{next}\n").as_bytes());
            next = t.host(&name);
        }
        next
    };
    let (five, six) = (chain("five", 5), chain("six", 6));
    let spec = Spec::new().allow(t.host(""), Access::READ).allow(t.host("exec"), Access::READ | Access::EXECUTE);
    let p = plan(Some(spec), "/", &[]);
    assert!(check_exec_argv(&p, &v(&[&five])).is_ok(), "{:?}", check_exec_argv(&p, &v(&[&five])));
    let e = check_exec_argv(&p, &v(&[&six])).unwrap_err();
    assert!(matches!(e, Error::CannotInvoke(_)) && e.to_string().contains("ELOOP"), "{e}");
}
