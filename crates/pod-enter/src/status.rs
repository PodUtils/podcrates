//! The exit codes of a payload: the codes of the shell for a program that
//! did not start, and docker's translation of a wait status.

use pod_hostprobe::sys;

/// The program exists and the kernel refused to run it (`EACCES`,
/// `ENOEXEC`, `EISDIR`, `EPERM`, `ETXTBSY`). The shell and docker exit
/// 126. A caller that reads 126 does not try another path.
pub const CANNOT_INVOKE: i32 = 126;

/// The program was not found. The shell and docker exit 127. A caller
/// that reads 127 can try another path.
pub const NOT_FOUND: i32 = 127;

/// Was the file there and refused, and not absent?
///
/// The errno decides. `execve` gives `ENOENT` for a path that is not
/// there, and also for a dynamic loader that the binary names and that is
/// not there. `EACCES`, `ENOEXEC`, `EISDIR`, `EPERM` and `ETXTBSY` mean
/// that the file was found and could not run. `ELOOP` and `ENAMETOOLONG`
/// stay with "not found": both are answers about the resolution of the
/// path, not about the file at its end.
pub(crate) fn invocable_but_refused(e: sys::Errno) -> bool {
    matches!(e.0, 1 | 8 | 13 | 21 | 26)
}

/// docker's translation of a wait status into an exit code.
///
/// A payload that a signal ended exits `128 + signal`, which is the rule
/// of the shell and of docker. A code of 0 or 1 for a killed process
/// would hide a SIGKILL from a caller that reads the code.
pub fn exit_status(status: i32) -> i32 {
    if status & 0x7f == 0 { (status >> 8) & 0xff } else { 128 + (status & 0x7f) }
}

/// The word of the forwarding wait for a signal: the name of the two
/// signals that it forwards, `signal N` for each other signal. A
/// terminal's SIGINT can arrive by the forward and by the reap, so the
/// reap can name a signal that the forward did not see.
pub(crate) fn signal_name(signo: i64) -> String {
    match signo {
        n if n == sys::SIGINT as i64 => "SIGINT".to_string(),
        n if n == sys::SIGTERM as i64 => "SIGTERM".to_string(),
        n => format!("signal {n}"),
    }
}
