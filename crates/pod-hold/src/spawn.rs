//! The start of a program under the filter.
//!
//! A seccomp filter is per thread unless TSYNC is used. A helper thread
//! installs the filter and forks there: the child inherits the filter, the
//! threads of the supervisor have none, and the notification descriptor
//! stays in the table of the supervisor, so no `SCM_RIGHTS` handshake is
//! needed (the child's own `sendmsg` is itself stopped). The child
//! allocates nothing between the `fork` and the `execve`.

use std::ffi::{CString, OsStr};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use pod_core::{Class, Failure};
use pod_hostprobe::sys::Errno;

use crate::failure;
use crate::filter::{Calls, install};
use crate::level::Hold;

/// The search path when the caller gives none (the default of `execvp`).
pub const DEFAULT_PATH: &str = "/usr/local/bin:/usr/bin:/bin";

/// Find `name` as `execvp` does: a name with a `/` is that path; another
/// name is looked up in the directories of `path` (the `PATH` of the
/// caller; `None` gives [`DEFAULT_PATH`]). Each is a regular file with an
/// execute bit, else a failure with the class of the errno of `execve`.
pub fn resolve(name: &str, path: Option<&OsStr>) -> Result<PathBuf, Failure> {
    if name.contains('/') {
        let what = format!("the program {name:?}");
        return match std::fs::metadata(name) {
            Ok(meta) if runnable(&meta) => Ok(PathBuf::from(name)),
            // execve gives EACCES for a file that is not regular and for a
            // file with no execute bit.
            Ok(_) => Err(failure::errno(
                exec_class(libc::EACCES),
                &format!("{what}: not a regular file with an execute bit"),
                libc::EACCES,
            )),
            Err(e) => Err(failure::caused(exec_class(e.raw_os_error().unwrap_or(0)), what, e)),
        };
    }
    let path = path.unwrap_or(OsStr::new(DEFAULT_PATH));
    for dir in std::env::split_paths(path) {
        let candidate = dir.join(name);
        if let Ok(meta) = std::fs::metadata(&candidate)
            && runnable(&meta)
        {
            return Ok(candidate);
        }
    }
    Err(failure::host(Class::NotFound, format!("the program {name:?} is not in PATH")))
}

fn runnable(meta: &std::fs::Metadata) -> bool {
    meta.is_file() && meta.permissions().mode() & 0o111 != 0
}

/// The class of an errno of `execve`. The split of a file that was found
/// and refused from one that is absent is the one of the shell (126 and
/// 127), in `Errno::exec_refused`.
fn exec_class(errno: i32) -> Class {
    if !Errno(errno).exec_refused() {
        return match errno {
            libc::ENOENT | libc::ENOTDIR | libc::ELOOP | libc::ENAMETOOLONG => Class::NotFound,
            _ => Class::Unavailable,
        };
    }
    match errno {
        libc::EACCES | libc::EPERM => Class::Permission,
        libc::E2BIG => Class::Usage,
        libc::ETXTBSY => Class::TempFail,
        // ENOEXEC, ELIBBAD, EISDIR, EINVAL: a file that this host cannot run.
        _ => Class::Unsupported,
    }
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

/// Start `program` under the filter of `calls`. A failed `execve` is a
/// failure with its errno and the class of the errno.
pub fn start(program: Program, calls: &Calls) -> Result<Started, Failure> {
    let calls = calls.clone();
    let count = calls.numbers().len();
    // A good execve closes the end of the child (FD_CLOEXEC): the parent
    // reads no byte. A failed one writes its errno. The filter of the
    // helper thread stops `close`, so this thread holds both ends.
    let (read_end, write_end) = cloexec_pipe()?;
    let child_end = write_end.as_raw_fd();
    let path = program.path.clone();
    let helper = std::thread::Builder::new()
        .name("pod-hold-filter".into())
        // The helper closes no descriptor after the filter: the listener
        // comes back to this thread also when the fork fails.
        .spawn(move || -> Result<(OwnedFd, Result<i32, Failure>), Failure> {
            let argv: Vec<*const libc::c_char> =
                program.argv.iter().map(|c| c.as_ptr()).chain(std::iter::once(std::ptr::null())).collect();
            let envp: Vec<*const libc::c_char> =
                program.env.iter().map(|c| c.as_ptr()).chain(std::iter::once(std::ptr::null())).collect();
            let listener = install(&calls)?;
            // SAFETY: the child calls only execve, write and _exit, which
            // are async-signal-safe; the filter stops none of them.
            let pid = unsafe { libc::fork() };
            if pid < 0 {
                return Ok((listener, Err(failure::last_os("fork"))));
            }
            if pid == 0 {
                // SAFETY: the pointers live until execve; the arrays end
                // with a null pointer.
                unsafe {
                    libc::execve(program.path.as_ptr(), argv.as_ptr(), envp.as_ptr());
                    let errno = (*libc::__errno_location()).to_ne_bytes();
                    libc::write(child_end, errno.as_ptr().cast(), errno.len());
                    libc::_exit(127)
                }
            }
            Ok((listener, Ok(pid)))
        })
        .map_err(|e| failure::host(Class::of_io(e.kind()), format!("the filter thread: {e}")))?;
    let (listener, pid) = helper.join().map_err(|_| failure::host(Class::Unavailable, "the filter thread died"))??;
    drop(write_end);
    let pid = pid?;
    if let Some(errno) = exec_errno(&read_end) {
        // SAFETY: waitpid of the child of this fork, which ends now.
        unsafe { libc::waitpid(pid, std::ptr::null_mut(), 0) };
        let what = format!("the program {path:?} could not be started: execve: {}", Errno(errno).name());
        return Err(failure::errno(exec_class(errno), &what, errno));
    }
    Ok(Started { listener, pid, hold: Hold::seccomp(pid, count) })
}

/// A pipe whose two ends have `FD_CLOEXEC`.
fn cloexec_pipe() -> Result<(OwnedFd, OwnedFd), Failure> {
    let mut fds = [0; 2];
    // SAFETY: pipe2 writes two descriptors into the array.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(failure::last_os("pipe2"));
    }
    // SAFETY: two new descriptors that nothing else owns.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// The errno that the child wrote, or `None` at the end of the pipe (a good
/// `execve`).
fn exec_errno(read_end: &OwnedFd) -> Option<i32> {
    let mut bytes = [0u8; 4];
    let mut got = 0;
    while got < bytes.len() {
        // SAFETY: a read into the rest of a buffer of this frame.
        let n = unsafe { libc::read(read_end.as_raw_fd(), bytes[got..].as_mut_ptr().cast(), bytes.len() - got) };
        match n {
            0 => return None,
            n if n > 0 => got += n as usize,
            _ if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted => {}
            _ => return None,
        }
    }
    Some(i32::from_ne_bytes(bytes))
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
        // A path with a `/` is checked as a bare name is.
        let shown = |p: &Path| p.to_str().unwrap().to_owned();
        assert_eq!(resolve(&shown(&tool), None).unwrap(), tool);
        assert_eq!(resolve(&shown(&plain), None).unwrap_err().class, Class::Permission, "no execute bit");
        assert_eq!(resolve(&shown(&dir), None).unwrap_err().class, Class::Permission, "a directory");
        assert_eq!(resolve("./x/y", Some(&path)).unwrap_err().class, Class::NotFound);
        assert_eq!(resolve("absent", None).unwrap_err().class, Class::NotFound);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_nul_byte_is_a_usage_failure() {
        let e = Program::new(Path::new("/bin/true"), &["true", "a\0b"], &[] as &[&str]).unwrap_err();
        assert_eq!(e.class, Class::Usage);
    }
}
