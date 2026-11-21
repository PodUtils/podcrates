//! The exit codes of a payload: the codes of the shell for a program that
//! did not start, and docker's translation of a wait status. The error of a
//! step that the child of the fork reported.

use pod_hostprobe::sys;

use crate::{Error, Plan};

/// The program exists and the kernel refused to run it (`EPERM`, `E2BIG`,
/// `ENOEXEC`, `EACCES`, `EISDIR`, `EINVAL`, `ETXTBSY`, `ELIBBAD`). The
/// shell and docker exit 126. A caller that reads 126 does not try another
/// path.
pub const CANNOT_INVOKE: i32 = 126;

/// The program was not found. The shell and docker exit 127. A caller
/// that reads 127 can try another path.
pub const NOT_FOUND: i32 = 127;

/// Was the file there and refused, and not absent?
///
/// The errno decides. `execve` gives `ENOENT` for a path that is not
/// there, and also for a dynamic loader that the binary names and that is
/// not there. These mean that the file was found and could not run:
///
/// * `EPERM` (1): a mount with `nosuid`, or a security module.
/// * `E2BIG` (7): the argv and the environment are too large for the
///   kernel. The command is there; each path refuses the same strings.
/// * `ENOEXEC` (8): a format that the kernel does not know.
/// * `EACCES` (13): no execute permission, or a file that is not regular.
/// * `EISDIR` (21): the ELF interpreter is a directory.
/// * `EINVAL` (22): the ELF names more than one interpreter.
/// * `ETXTBSY` (26): the file is open for a write.
/// * `ELIBBAD` (80): the ELF interpreter has a bad format.
///
/// `ELOOP`, `ENAMETOOLONG` and `ENOTDIR` stay with "not found": each is an
/// answer about the resolution of the path, not about the file at its
/// end. `ENOMEM`, `EIO`, `EMFILE`, `ENFILE` and `EFAULT` stay too: each is
/// a failure of the host, not an answer about the file.
pub(crate) fn invocable_but_refused(e: sys::Errno) -> bool {
    matches!(e.0, 1 | 7 | 8 | 13 | 21 | 22 | 26 | 80)
}

/// docker's translation of a wait status into an exit code.
///
/// A payload that a signal ended exits `128 + signal`, which is the rule
/// of the shell and of docker. A code of 0 or 1 for a killed process
/// would hide a SIGKILL from a caller that reads the code.
pub fn exit_status(status: i32) -> i32 {
    if status & 0x7f == 0 { (status >> 8) & 0xff } else { 128 + (status & 0x7f) }
}

/// The word of the forwarding wait for a signal: the name of each signal
/// that it forwards, `signal N` for each other signal. A
/// terminal's SIGINT can arrive by the forward and by the reap, so the
/// reap can name a signal that the forward did not see.
pub(crate) fn signal_name(signo: i64) -> String {
    match signo {
        n if n == sys::SIGHUP as i64 => "SIGHUP".to_string(),
        n if n == sys::SIGINT as i64 => "SIGINT".to_string(),
        n if n == sys::SIGTERM as i64 => "SIGTERM".to_string(),
        n if n == sys::SIGQUIT as i64 => "SIGQUIT".to_string(),
        n if n == sys::SIGUSR1 as i64 => "SIGUSR1".to_string(),
        n if n == sys::SIGUSR2 as i64 => "SIGUSR2".to_string(),
        n if n == sys::SIGALRM as i64 => "SIGALRM".to_string(),
        n if n == sys::SIGPWR as i64 => "SIGPWR".to_string(),
        n => format!("signal {n}"),
    }
}

/// The error for a step that the child reported.
pub(crate) fn step_failure(plan: &Plan, step_byte: u8, errno: sys::Errno, at: u8) -> Error {
    // A refused nested user namespace names the ids of the plan.
    if let Some(e) = crate::ns_user::nested_failure(plan, step_byte, errno) {
        return e;
    }
    // A refused limit, `no_new_privs` or the death signal: the call, by
    // name, and the payload did not run.
    let call = match step_byte {
        12 => plan.limits.get(usize::from(at)).map(|l| format!("setrlimit({})", l.resource.name())),
        13 => Some("prctl(PR_SET_NO_NEW_PRIVS)".to_string()),
        14 => Some("prctl(PR_SET_PDEATHSIG)".to_string()),
        _ => None,
    };
    if let Some(call) = call {
        return Error::runtime(
            crate::errno_class(errno),
            format!("the payload could not be started: {call}: {} ({})", errno.name(), errno.0),
        );
    }
    let step = match step_byte {
        1 => "fchdir onto the rootfs descriptor",
        2 => "chroot into the rootfs",
        3 => "chdir(\"/\") after the chroot",
        5 => "execveat of the memfd",
        6 => "execve of the userland entry argv",
        11 => "chdir to the working directory",
        _ => "execve of every candidate path",
    };
    // A working directory that went missing names the directory, so the
    // refusal says which `-w` failed.
    if step_byte == 11 {
        return Error::runtime(
            crate::errno_class(errno),
            format!(
                "the working directory {:?} could not be entered: {step} failed with {} ({})",
                plan.working_dir,
                errno.name(),
                errno.0
            ),
        );
    }
    let text = format!("the payload could not be started: {step} failed with {} ({})", errno.name(), errno.0);
    if !matches!(step_byte, 4..=6) {
        return Error::runtime(crate::errno_class(errno), text);
    }
    let named = format!("{:?}: {text}", plan.argv.first().map(String::as_str).unwrap_or(""));
    // 126 and not 127, as docker: `docker run alpine /etc/passwd` exits
    // 126. A caller that reads 127 tries another path; one that reads 126
    // does not.
    if invocable_but_refused(errno) { Error::CannotInvoke(named) } else { Error::NotFound(named) }
}

#[cfg(test)]
mod tests;
