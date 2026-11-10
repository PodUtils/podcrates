//! The start of a program under the filter.
//!
//! A seccomp filter is per thread unless TSYNC is used. A helper thread
//! installs the filter and forks there: the child inherits the filter, the
//! threads of the supervisor have none, and the notification descriptor
//! stays in the table of the supervisor, so no `SCM_RIGHTS` handshake is
//! needed (the child's own `sendmsg` is itself stopped). The child
//! allocates nothing between the `fork` and the `execve`.

use std::ffi::{CString, OsStr};
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use pod_core::{Class, Failure};

use crate::failure;
use crate::filter::{Calls, install};
use crate::level::Hold;

/// The search path when the caller gives none (the default of `execvp`).
pub const DEFAULT_PATH: &str = "/usr/local/bin:/usr/bin:/bin";

/// Find `name` as `execvp` does: a name with a `/` is used as it is; another
/// name is looked up in the directories of `path` (the `PATH` of the
/// caller; `None` gives [`DEFAULT_PATH`]).
pub fn resolve(name: &str, path: Option<&OsStr>) -> Result<PathBuf, Failure> {
    if name.contains('/') {
        return Ok(PathBuf::from(name));
    }
    let path = path.unwrap_or(OsStr::new(DEFAULT_PATH));
    for dir in std::env::split_paths(path) {
        let candidate = dir.join(name);
        if let Ok(meta) = std::fs::metadata(&candidate)
            && meta.is_file()
            && meta.permissions().mode() & 0o111 != 0
        {
            return Ok(candidate);
        }
    }
    Err(failure::host(Class::NotFound, format!("the program {name:?} is not in PATH")))
}

/// A program to start, made before the fork: its path, its arguments and
/// its environment.
#[derive(Clone, Debug)]
pub struct Program {
    path: CString,
    argv: Vec<CString>,
    env: Vec<CString>,
}

fn c_string(what: &str, bytes: &[u8]) -> Result<CString, Failure> {
    CString::new(bytes).map_err(|_| failure::host(Class::Usage, format!("{what} has a NUL byte")))
}

impl Program {
    /// The program at `path` (resolved: see [`resolve`]) with `argv` (the
    /// first is its name) and the environment `env` (`NAME=VALUE`). The
    /// caller makes the environment: the crate reads none.
    pub fn new(path: &Path, argv: &[impl AsRef<OsStr>], env: &[impl AsRef<OsStr>]) -> Result<Program, Failure> {
        let path = c_string("the program path", path.as_os_str().as_bytes())?;
        let argv = argv.iter().map(|a| c_string("an argument", a.as_ref().as_bytes())).collect::<Result<_, _>>()?;
        let env = env.iter().map(|e| c_string("a variable", e.as_ref().as_bytes())).collect::<Result<_, _>>()?;
        Ok(Program { path, argv, env })
    }

    /// The program with the environment as C strings.
    pub fn with_env(path: &Path, argv: &[impl AsRef<OsStr>], env: Vec<CString>) -> Result<Program, Failure> {
        let mut program = Program::new(path, argv, &[] as &[&OsStr])?;
        program.env = env;
        Ok(program)
    }
}

/// A program that runs under the filter.
#[derive(Debug)]
pub struct Started {
    /// The notification descriptor ([`crate::listen::Listener::new`]).
    pub listener: OwnedFd,
    /// The pid of the program. The caller reaps it.
    pub pid: i32,
    /// The level of the hold and its proof.
    pub hold: Hold,
}

/// Start `program` under the filter of `calls`.
pub fn start(program: Program, calls: &Calls) -> Result<Started, Failure> {
    let calls = calls.clone();
    let count = calls.numbers().len();
    let helper = std::thread::Builder::new()
        .name("pod-hold-filter".into())
        .spawn(move || -> Result<(OwnedFd, i32), Failure> {
            let argv: Vec<*const libc::c_char> =
                program.argv.iter().map(|c| c.as_ptr()).chain(std::iter::once(std::ptr::null())).collect();
            let envp: Vec<*const libc::c_char> =
                program.env.iter().map(|c| c.as_ptr()).chain(std::iter::once(std::ptr::null())).collect();
            let listener = install(&calls)?;
            // SAFETY: the child calls only execve and _exit, which are
            // async-signal-safe.
            let pid = unsafe { libc::fork() };
            if pid < 0 {
                return Err(failure::last_os("fork"));
            }
            if pid == 0 {
                // SAFETY: the pointers live until execve; the arrays end
                // with a null pointer.
                unsafe {
                    libc::execve(program.path.as_ptr(), argv.as_ptr(), envp.as_ptr());
                    libc::_exit(127)
                }
            }
            Ok((listener, pid))
        })
        .map_err(|e| failure::host(Class::of_io(e.kind()), format!("the filter thread: {e}")))?;
    let (listener, pid) = helper.join().map_err(|_| failure::host(Class::Unavailable, "the filter thread died"))??;
    Ok(Started { listener, pid, hold: Hold::seccomp(pid, count) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_found_in_the_path_of_the_caller() {
        let dir = std::env::temp_dir().join(format!("pod-hold-resolve-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tool = dir.join("tool");
        std::fs::write(&tool, "#!/bin/sh\n").unwrap();
        let plain = dir.join("plain");
        std::fs::write(&plain, "").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = std::env::join_paths(["/nonexistent".into(), dir.clone()]).unwrap();
        assert_eq!(resolve("tool", Some(&path)).unwrap(), tool);
        assert_eq!(resolve("plain", Some(&path)).unwrap_err().class, Class::NotFound, "not executable");
        assert_eq!(resolve("./x/y", Some(&path)).unwrap(), PathBuf::from("./x/y"));
        assert_eq!(resolve("absent", None).unwrap_err().class, Class::NotFound);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_nul_byte_is_a_usage_failure() {
        let e = Program::new(Path::new("/bin/true"), &["true", "a\0b"], &[] as &[&str]).unwrap_err();
        assert_eq!(e.class, Class::Usage);
    }
}
