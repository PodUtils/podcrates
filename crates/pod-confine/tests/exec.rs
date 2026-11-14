//! The exec of a confined child on the kernel rung (PodUtils/podvm#344): a
//! static ELF, a dynamic ELF and a script, inside and outside an execute
//! path. The kernel executes each file itself, so Landlock checks `EXECUTE`
//! on the program (and on a script), and the answer of
//! `ExecTable::allows` is the answer of the kernel. A host with no Landlock
//! skips with a line that says so.
#![cfg(target_os = "linux")]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use pod_confine::{Access, MIN_ABI, Spec, apply};
use pod_hostprobe::sys::{self as k, CBuf};

const EACCES: i32 = 13;

fn tool(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|d| d.join(name))
        .find(|p| p.is_file())
        .and_then(|p| std::fs::canonicalize(p).ok())
}

fn put(p: &Path, bytes: &[u8]) {
    std::fs::write(p, bytes).unwrap();
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The exit code of a child that applies `spec` and executes `program`:
/// the code of the program, or the errno of a refused `execve`.
fn run_confined(spec: &pod_confine::Compiled, program: &Path) -> i32 {
    let path = CBuf::new(program.to_str().unwrap()).unwrap();
    let argv: [*const u8; 2] = [path.ptr() as *const u8, std::ptr::null()];
    let envp: [*const u8; 1] = [std::ptr::null()];
    // SAFETY: the child makes system calls on data made before the fork,
    // and exits; it does not allocate or take a lock.
    match unsafe { k::clone_fork(k::SIGCHLD) }.expect("a fork") {
        0 => {
            if apply(spec).is_err() {
                k::exit_group(100);
            }
            // SAFETY: both arrays end with a null pointer, and `path` lives.
            match unsafe { k::execve(&path, argv.as_ptr(), envp.as_ptr()) } {
                Ok(_) => k::exit_group(101),
                Err(e) => k::exit_group(e.0),
            }
        }
        pid => {
            let mut status = 0;
            k::wait4(pid, &mut status).expect("the wait of the child");
            assert_eq!(status & 0x7f, 0, "the child of {} ended on a signal: {status}", program.display());
            (status >> 8) & 0xff
        }
    }
}

#[test]
fn an_execute_path_holds_a_static_elf_a_dynamic_elf_and_a_script() {
    let abi = match pod_confine::check() {
        Ok(abi) if abi >= MIN_ABI => abi,
        Ok(abi) => return pod_testkit::host::skip_line(&format!("Landlock ABI {abi} is under {MIN_ABI}")),
        Err(e) => return pod_testkit::host::skip_line(&e.to_string()),
    };
    let Some(elf) = pod_testkit::elf::static_exit0() else {
        return pod_testkit::host::skip_line("no static ELF of this architecture in the test");
    };
    let (Some(dynamic), Some(sh)) = (tool("true"), tool("sh")) else {
        return pod_testkit::host::skip_line("no `true` or `sh` on the PATH");
    };
    if !pod_testkit::elf::is_dynamic(&std::fs::read(&dynamic).unwrap_or_default()) {
        return pod_testkit::host::skip_line(&format!("{} has no PT_INTERP", dynamic.display()));
    }
    let d = std::env::temp_dir().join(format!("pod-confine-exec-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let (inside, outside) = (d.join("exec"), d.join("read"));
    let script = format!("#!{}\nexit 0\n", sh.display());
    for dir in [&inside, &outside] {
        std::fs::create_dir_all(dir).unwrap();
        put(&dir.join("static"), &elf);
        put(&dir.join("dynamic"), &std::fs::read(&dynamic).unwrap());
        put(&dir.join("script"), script.as_bytes());
    }
    // Each file reads; the system directories (the loader, the libraries,
    // the shell) and `inside` execute.
    let mut spec = Spec::new().allow("/", Access::READ).allow(inside.to_str().unwrap(), Access::READ | Access::EXECUTE);
    for sys in ["/usr", "/lib", "/lib64", "/bin"] {
        if let Ok(real) = std::fs::canonicalize(sys) {
            spec = spec.allow(real.to_str().unwrap(), Access::READ | Access::EXECUTE);
        }
    }
    let table = spec.exec_table().resolved();
    let compiled = spec.compile(abi).unwrap();
    for (dir, want) in [(&inside, 0), (&outside, EACCES)] {
        for name in ["static", "dynamic", "script"] {
            let p = std::fs::canonicalize(dir.join(name)).unwrap();
            assert_eq!(run_confined(&compiled, &p), want, "{}", p.display());
            assert_eq!(
                table.allows(p.to_str().unwrap()),
                want == 0,
                "the table and the kernel differ: {}",
                p.display()
            );
        }
    }
    let _ = std::fs::remove_dir_all(d);
}
