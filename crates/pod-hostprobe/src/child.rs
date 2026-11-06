//! The disposable child. Each probe runs in a newly forked child, because a
//! successful `unshare`, `chroot` or `setuid` changes the prober and several
//! of them have no undo.
//!
//! The child starts `/proc/self/exe` again with a private argument, and the
//! parent takes the verdict from the child. The child writes its errno on
//! stdout and sets its exit status, and the parent **cross-checks the two**.
//! A child sets its exit status from the operation, or the parent reads no
//! verdict from it; a child that disagrees with itself gives no verdict
//! either, so that is a `skip` and never a denial.
//!
//! The fork is `clone(2)` with no `CLONE_VM`, not `std::process::Command`.
//! The probe set must *attempt* `clone(CLONE_NEWNS)` and
//! `clone(CLONE_NEWUSER)` with those flags, and the standard library cannot
//! set them. One spawn path serves both kinds of probe, not two that drift.

use std::os::unix::ffi::OsStrExt;

use crate::sys::{self, CBuf, Errno};
use crate::verdict::{Outcome, Verdict, decode};

/// How a child ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Exited(i32),
    Signalled(i32),
}

impl Status {
    fn from_wait(status: i32) -> Status {
        if status & 0x7f == 0 { Status::Exited((status >> 8) & 0xff) } else { Status::Signalled(status & 0x7f) }
    }

    fn describe(self) -> String {
        match self {
            Status::Exited(c) => format!("exit {c}"),
            Status::Signalled(s) => format!("signal {s}"),
        }
    }
}

/// Everything a child run produced.
struct Run {
    stdout: Vec<u8>,
    status: Status,
}

pub struct Spawner {
    /// The path re-executed for a probe child, and where it came from, so a
    /// report says which. `/proc/self/exe` is a magic link the kernel resolves
    /// itself, which is why it is preferred: it keeps working where the
    /// binary's own path does not resolve.
    exe: Option<CBuf>,
    pub exe_source: String,
    envp_owned: Vec<CBuf>,
    /// `Settings::child_flag` and `Settings::child_name`, made before any
    /// fork. `None` where the value holds a NUL.
    child_flag: Option<CBuf>,
    child_name: Option<CBuf>,
}

impl Spawner {
    /// The environment of this process goes to each child as it is: the
    /// child must see the same `PATH` and the same variables as the caller.
    pub fn new(settings: &crate::Settings) -> Spawner {
        let (exe, source) = Self::find_self();
        let envp_owned = std::env::vars_os()
            .filter_map(|(k, v)| {
                let mut s = Vec::new();
                s.extend_from_slice(k.as_bytes());
                s.push(b'=');
                s.extend_from_slice(v.as_bytes());
                // An interior NUL cannot reach a C array intact. Dropping the
                // variable is the honest move; carrying a truncated one is not.
                if s.contains(&0) {
                    return None;
                }
                String::from_utf8(s).ok().and_then(|s| CBuf::new(&s))
            })
            .collect();
        Spawner {
            exe,
            exe_source: source,
            envp_owned,
            child_flag: CBuf::new(&settings.child_flag),
            child_name: CBuf::new(&settings.child_name),
        }
    }

    fn find_self() -> (Option<CBuf>, String) {
        let magic = "/proc/self/exe";
        if std::fs::metadata(magic).is_ok()
            && let Some(c) = CBuf::new(magic)
        {
            return (Some(c), magic.to_string());
        }
        if let Ok(p) = std::env::current_exe()
            && let Some(s) = p.to_str()
            && let Some(c) = CBuf::new(s)
        {
            return (Some(c), format!("current_exe {s}"));
        }
        // argv[0], and only when it names a path rather than a PATH lookup.
        if let Some(a0) = std::env::args().next()
            && a0.contains('/')
            && let Some(c) = CBuf::new(&a0)
        {
            return (Some(c), format!("argv[0] {a0}"));
        }
        (None, "unavailable".to_string())
    }

    /// Probe whether a `clone(2)` with these flags is accepted.
    ///
    /// The verdict is the clone's own errno. The child does nothing but
    /// `exit_group(0)`: the question is whether the kernel built the child,
    /// not what it then did.
    pub fn clone_accepted(&self, flags: u64) -> Outcome {
        let pid = match unsafe { sys::clone_fork(flags | sys::SIGCHLD) } {
            Ok(0) => sys::exit_group(0),
            Ok(pid) => pid,
            Err(e) => return Outcome::denied(e),
        };
        match reap(pid) {
            Ok(Status::Exited(0)) => Outcome::ok(),
            // The clone was accepted, so this is not a denial of the thing
            // probed: the skip says so.
            Ok(other) => Outcome::skip(
                None,
                format!(
                    "clone(2) was accepted and the child ended {}; the clone is \
                     the measurement and it succeeded",
                    other.describe()
                ),
            ),
            Err(e) => Outcome::skip(Some(e), "wait4 on the cloned child failed"),
        }
    }

    /// Run one named probe in a fresh child and read back its verdict.
    ///
    /// `ns_flags` places the child in the namespaces the probe needs. A clone
    /// the flags make fail is a **failed precondition**, so the row is a
    /// `skip` carrying the clone's errno: the probe never ran, and
    /// this must not read as a denial.
    pub fn run_probe(&self, name: &str, ns_flags: u64) -> Outcome {
        let exe = match &self.exe {
            Some(e) => e,
            None => {
                return Outcome::skip(
                    None,
                    "no path to re-execute this binary: /proc/self/exe is not \
                     readable and argv[0] does not name a path",
                );
            }
        };
        let Some(arg_flag) = &self.child_flag else {
            return Outcome::skip(None, "the child flag of the settings is not a C string");
        };
        let arg_name = match CBuf::new(name) {
            Some(c) => c,
            None => return Outcome::skip(None, "internal: probe name contains a NUL and cannot cross execve"),
        };
        let Some(argv0) = &self.child_name else {
            return Outcome::skip(None, "the child name of the settings is not a C string");
        };

        // Everything the child touches after the clone exists before it.
        // Between clone and execve only async-signal-safe work is permitted,
        // so no allocation may happen there.
        let argv: Vec<*const u8> =
            vec![argv0.ptr() as *const u8, arg_flag.ptr() as *const u8, arg_name.ptr() as *const u8, std::ptr::null()];
        let mut envp: Vec<*const u8> = self.envp_owned.iter().map(|c| c.ptr() as *const u8).collect();
        envp.push(std::ptr::null());

        let run = match self.exec_child(exe, ns_flags, &argv, &envp) {
            Ok(r) => r,
            Err((what, e)) => return Outcome::skip(Some(e), what),
        };
        Self::interpret(name, run)
    }

    /// The one fork point. Returns the child's stdout and how it ended.
    fn exec_child(
        &self,
        exe: &CBuf,
        ns_flags: u64,
        argv: &[*const u8],
        envp: &[*const u8],
    ) -> Result<Run, (String, Errno)> {
        let mut fds = [0i32; 2];
        // NOT `O_CLOEXEC`. The child closes the read end itself, so the flag
        // buys nothing, and it introduces a case that loses the verdict: where
        // the caller started this process with fd 1 closed, `pipe2` hands out fd 1
        // for one end, and a close-on-exec fd 1 is a child that writes its
        // answer into a descriptor the exec has already taken away.
        sys::pipe2(&mut fds, 0).map_err(|e| ("pipe2 for the child's verdict failed".to_string(), e))?;
        let (r, w) = (fds[0] as i64, fds[1] as i64);

        let pid = match unsafe { sys::clone_fork(ns_flags | sys::SIGCHLD) } {
            Ok(0) => {
                // ---- child. Async-signal-safe work only, then execve. ----
                // Both guards are for the same case: `pipe2` returns the
                // lowest free descriptors, so with fd 1 closed one end lands on
                // it. `dup2(1, 1)` is a no-op that returns 1 WITHOUT closing,
                // and the unconditional `close` after it would then shut the
                // verdict channel. Where an end already is fd 1 there is
                // nothing to move and nothing to close.
                if w != 1 {
                    let _ = sys::dup2(w, 1);
                    let _ = sys::close(w);
                }
                if r != 1 {
                    let _ = sys::close(r);
                }
                let _ = unsafe { sys::execve(exe, argv.as_ptr(), envp.as_ptr()) };
                // execve returned, so it failed. 127 is the shell's convention
                // for "could not execute", and the parent reads it as a harness
                // failure rather than as a denial.
                sys::exit_group(127)
            }
            Ok(pid) => pid,
            Err(e) => {
                let _ = sys::close(r);
                let _ = sys::close(w);
                let what = if ns_flags == 0 {
                    "clone(2) for the probe child failed".to_string()
                } else {
                    format!(
                        "clone(2) with flags 0x{ns_flags:x} was refused, so the \
                         namespace this probe needs was never entered and the \
                         operation was never attempted"
                    )
                };
                return Err((what, e));
            }
        };

        let _ = sys::close(w);
        let stdout = drain(r);
        let _ = sys::close(r);
        let status = reap(pid).map_err(|e| ("wait4 on the probe child failed".to_string(), e))?;
        Ok(Run { stdout, status })
    }

    /// Where the two halves meet. The spoken verdict and
    /// the exit status must agree; anything else is "could not run".
    fn interpret(name: &str, run: Run) -> Outcome {
        let text = String::from_utf8_lossy(&run.stdout);
        let line = text.lines().find(|l| l.starts_with(crate::verdict::WIRE_MAGIC));
        let Some(line) = line else {
            return Outcome::skip(
                None,
                format!(
                    "the child for {name:?} ended {} and said nothing this \
                     parent can read; no verdict was established",
                    run.status.describe()
                ),
            );
        };
        let spoken = match decode(line) {
            Ok(o) => o,
            Err(why) => {
                return Outcome::skip(
                    None,
                    format!("the child for {name:?} spoke a line this parent cannot read: {why}"),
                );
            }
        };
        let Status::Exited(code) = run.status else {
            return Outcome::skip(
                spoken.errno,
                format!(
                    "the child for {name:?} said {:?} and then ended on {}; a \
                     verdict and a signal are not one answer",
                    spoken.verdict.word(),
                    run.status.describe()
                ),
            );
        };
        match Verdict::from_exit_code(code) {
            Some(v) if v == spoken.verdict => spoken,
            _ => Outcome::skip(
                spoken.errno,
                format!(
                    "the child for {name:?} said {:?} and exited {code}; the \
                     spoken verdict and the exit status disagree, so neither \
                     is a measurement",
                    spoken.verdict.word()
                ),
            ),
        }
    }
}

/// Read a descriptor to EOF. The child writes one short line, well under
/// `PIPE_BUF`, so this cannot deadlock against a child still writing.
fn drain(fd: i64) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match sys::read(fd, &mut buf) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buf[..n as usize]),
            Err(sys::EINTR) => continue,
            Err(_) => break,
        }
        if out.len() > 64 * 1024 {
            break; // a child that will not stop talking is not a verdict
        }
    }
    out
}

fn reap(pid: i64) -> Result<Status, Errno> {
    let mut status = 0i32;
    loop {
        match sys::wait4(pid, &mut status) {
            Ok(_) => return Ok(Status::from_wait(status)),
            Err(sys::EINTR) => continue,
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_exit_decodes_as_exited_zero() {
        assert_eq!(Status::from_wait(0), Status::Exited(0));
        assert_eq!(Status::from_wait(2 << 8), Status::Exited(2));
        assert_eq!(Status::from_wait(9), Status::Signalled(9));
    }

    #[test]
    fn a_child_that_says_nothing_is_a_skip_and_not_a_denial() {
        let run = Run { stdout: Vec::new(), status: Status::Exited(1) };
        let out = Spawner::interpret("mount(tmpfs,/mnt)", run);
        assert_eq!(out.verdict, Verdict::Skip);
    }

    #[test]
    fn a_child_whose_status_contradicts_its_line_establishes_nothing() {
        let line = crate::verdict::encode(&Outcome::denied(crate::sys::EPERM));
        let run = Run { stdout: line.into_bytes(), status: Status::Exited(0) };
        let out = Spawner::interpret("chroot(/tmp)", run);
        assert_eq!(out.verdict, Verdict::Skip);
        assert!(out.reason.contains("disagree"));
    }

    #[test]
    fn an_agreeing_child_is_taken_at_its_word() {
        let line = crate::verdict::encode(&Outcome::denied(crate::sys::EPERM));
        let run = Run { stdout: line.into_bytes(), status: Status::Exited(1) };
        let out = Spawner::interpret("chroot(/tmp)", run);
        assert_eq!(out.verdict, Verdict::Denied);
        assert_eq!(out.errno, Some(crate::sys::EPERM));
    }
}
