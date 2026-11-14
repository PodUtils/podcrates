//! The exec table of the entry with no chroot (PodUtils/podvm#344): the
//! loader of the image maps a program, and the interpreter of a script
//! reads it, so Landlock sees neither; [`check_exec`] and the variable of
//! the table answer for them.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use pod_confine::{Access, Spec};

use super::*;
use crate::userland::Maps;
use crate::{Fds, Names, spawn_userland};

/// The files of a test: a static ELF, a dynamic ELF and a script, in a
/// directory with execute (`inside`) and in one with read only
/// (`outside`).
struct Files {
    dir: PathBuf,
    inside: PathBuf,
    outside: PathBuf,
    /// The host's loader of the dynamic ELF.
    loader: String,
    sh: String,
}

impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn found(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|d| d.join(name))
        .find(|p| p.is_file())
        .and_then(|p| std::fs::canonicalize(p).ok())
}

/// The files, or a skip line and `None` on a host with no `true` of a
/// loader, no `sh`, or an architecture with no static ELF of the kit.
fn files(tag: &str) -> Option<Files> {
    let skip = |why: &str| {
        pod_testkit::host::skip_line(why);
        None
    };
    let Some(elf) = pod_testkit::elf::static_exit0() else { return skip("no static ELF of this architecture") };
    let (Some(dynamic), Some(sh)) = (found("true"), found("sh")) else { return skip("no `true` or `sh` on the PATH") };
    let Ok(Some(interp)) = crate::abi::interp_of_file(dynamic.to_str().unwrap()) else {
        return skip("`true` of the host has no PT_INTERP");
    };
    let Ok(loader) = std::fs::canonicalize(&interp) else { return skip("the loader of `true` does not resolve") };
    let dir = std::env::temp_dir().join(format!("pod-enter-exec-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (inside, outside) = (dir.join("exec"), dir.join("read"));
    let script = format!("#!{}\nexit 0\n", sh.display());
    for d in [&inside, &outside] {
        std::fs::create_dir_all(d).unwrap();
        for (name, bytes) in
            [("static", elf.clone()), ("dynamic", std::fs::read(&dynamic).unwrap()), ("script", script.clone().into())]
        {
            std::fs::write(d.join(name), bytes).unwrap();
            std::fs::set_permissions(d.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let loader = loader.display().to_string();
    Some(Files { dir, inside, outside, loader, sh: sh.display().to_string() })
}

/// Each path reads, the directory of the test writes, and the system
/// directories and `inside` execute: host paths, as the entry with no
/// chroot takes them.
fn spec(f: &Files) -> Spec {
    let mut s = Spec::new()
        .allow("/", Access::READ)
        .allow(f.dir.to_str().unwrap(), Access::READ | Access::WRITE)
        .allow(f.inside.to_str().unwrap(), Access::READ | Access::EXECUTE);
    for sys in ["/usr", "/lib", "/lib64", "/bin"] {
        if let Ok(real) = std::fs::canonicalize(sys) {
            s = s.allow(real.to_str().unwrap(), Access::READ | Access::EXECUTE);
        }
    }
    s
}

fn plan(confine: Option<Spec>, env: Vec<String>) -> Plan {
    Plan {
        argv: vec!["payload".to_string()],
        env,
        working_dir: "/".to_string(),
        fds: Fds::default(),
        banner: String::new(),
        path_dirs: vec!["/bin".to_string()],
        names: Names::DEFAULT,
        limits: Vec::new(),
        no_new_privs: false,
        die_with_parent: false,
        confine,
    }
}

/// Run `argv` on the entry with no chroot under `plan`: the exit code, or
/// the error of the entry.
fn run(f: &Files, plan: &Plan, argv: &[&str], fd: Option<i64>) -> Result<i32> {
    let m = Maps::of_root(f.dir.to_str().unwrap());
    let root = RootDir::open(m.rootfs()).unwrap();
    let argv = argv.iter().map(|a| a.to_string()).collect();
    spawn_userland(&root, plan, &m, argv, "userland", fd, &mut std::io::sink())?.wait()
}

fn landlock() -> bool {
    match pod_confine::check() {
        Ok(_) => true,
        Err(e) => {
            pod_testkit::host::skip_line(&e.to_string());
            false
        }
    }
}

fn path(d: &Path, name: &str) -> String {
    d.join(name).display().to_string()
}

#[test]
fn check_exec_answers_for_each_kind_of_program_inside_and_outside() {
    let Some(f) = files("check") else { return };
    let confined = plan(Some(spec(&f)), Vec::new());
    for name in ["static", "dynamic", "script"] {
        assert!(check_exec(&confined, &path(&f.inside, name)).is_ok(), "{name}");
        let e = check_exec(&confined, &path(&f.outside, name)).unwrap_err();
        assert!(matches!(e, Error::CannotInvoke(_)), "{name}: {e:?}");
        assert_eq!(e.payload_code(), Some(crate::status::CANNOT_INVOKE));
        assert!(e.to_string().contains("outside each execute path"), "{e}");
        assert!(check_exec(&plan(None, Vec::new()), &path(&f.outside, name)).is_ok(), "{name} with no confinement");
    }
    // A link into `inside` from `outside` runs the file of `inside`.
    std::os::unix::fs::symlink(f.inside.join("static"), f.outside.join("link")).unwrap();
    assert!(check_exec(&confined, &path(&f.outside, "link")).is_ok());
    let e = check_exec(&confined, &path(&f.outside, "missing")).unwrap_err();
    assert!(matches!(e, Error::NotFound(_)), "{e:?}");
}

/// The kernel executes the loader and the interpreter, not the program:
/// a program outside each execute path runs through them. The kernel
/// refuses the same program by its own exec. This is the gap that
/// [`check_exec`] and the table of the interposer close.
#[test]
fn the_loader_and_the_interpreter_run_a_program_that_landlock_does_not_see() {
    let Some(f) = files("gap") else { return };
    if !landlock() {
        return;
    }
    let p = plan(Some(spec(&f)), Vec::new());
    for name in ["static", "dynamic", "script"] {
        assert_eq!(run(&f, &p, &[&path(&f.inside, name)], None).unwrap(), 0, "{name} inside");
        let e = run(&f, &p, &[&path(&f.outside, name)], None).unwrap_err();
        assert!(matches!(e, Error::CannotInvoke(_)), "{name} outside: {e:?}");
    }
    assert_eq!(run(&f, &p, &[&f.loader, &path(&f.outside, "dynamic")], None).unwrap(), 0);
    assert_eq!(run(&f, &p, &[&f.sh, &path(&f.outside, "script")], None).unwrap(), 0);
    // A memfd is on no path of a rule, and Landlock does not check its
    // exec: a static program staged from `outside` runs too.
    let fd = crate::memfd::stage(&Names::DEFAULT, &std::fs::read(f.outside.join("static")).unwrap()).unwrap();
    let staged = run(&f, &p, &[], Some(fd));
    let _ = pod_hostprobe::sys::close(fd);
    assert_eq!(staged.unwrap(), 0);
}

#[test]
fn the_payload_gets_the_exec_table_and_no_value_of_the_caller() {
    let Some(f) = files("var") else { return };
    let out = path(&f.dir, "table");
    let script = format!("printf %s \"${{POD_ENTER_EXEC_TABLE-unset}}\" > {out}");
    let forged = vec!["POD_ENTER_EXEC_TABLE=/".to_string(), "PATH=/usr/bin:/bin".to_string()];
    assert_eq!(run(&f, &plan(None, forged.clone()), &[&f.sh, "-c", &script], None).unwrap(), 0);
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "unset", "a value of the caller reached the payload");
    if !landlock() {
        return;
    }
    let s = spec(&f);
    let want = s.exec_table().resolved().encode().unwrap();
    assert_eq!(run(&f, &plan(Some(s), forged), &[&f.sh, "-c", &script], None).unwrap(), 0);
    let got = std::fs::read_to_string(&out).unwrap();
    assert_eq!(got, want);
    assert!(pod_confine::exec::allows_in(got.as_bytes(), path(&f.inside, "static").as_bytes()));
    assert!(!pod_confine::exec::allows_in(got.as_bytes(), path(&f.outside, "static").as_bytes()));
}
