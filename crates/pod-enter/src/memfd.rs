//! The memfd launch: `memfd_create`, `write`, and the exec of the
//! descriptor.
//!
//! The three system calls are here, not in a dependency. Two rules:
//!
//! 1. `MFD_CLOEXEC` is set on each `memfd_create`, so the memfd never
//!    crosses a later `execve`.
//! 2. The caller checks that a payload can execute before it writes it,
//!    so a memfd that cannot execute is `EACCES` at the check and not a
//!    late failure of `execveat`.
//!
//! A `#!` script fails through the exec of a descriptor, so the ladder
//! execs a script by its path.

use std::path::Path;

use pod_hostprobe::sys::{self, Sysres};

use crate::Names;

pub mod probe;
pub use probe::{MemfdExec, memfd_exec};

/// Close-on-exec, from `linux/memfd.h`. Each call below sets it.
pub const MFD_CLOEXEC: u64 = 0x0001;

/// Allow seals, from `linux/memfd.h`. Only where the caller asks for seals
/// and the kernel accepts it.
pub const MFD_ALLOW_SEALING: u64 = 0x0002;

/// Seal the memfd against exec, from `linux/memfd.h` (Linux 6.3). The
/// entry never sets it; a test sets it to prove the path of a denial.
pub const MFD_NOEXEC_SEAL: u64 = 0x0008;

/// Keep the memfd executable, from `linux/memfd.h` (Linux 6.3). Under
/// `vm.memfd_noexec=1` a memfd without this flag is sealed against exec,
/// so each `memfd_create` of the entry gives it. A kernel before 6.3
/// answers `EINVAL`, and the call is done again without it.
pub const MFD_EXEC: u64 = 0x0010;

/// Do these bytes go past the exec of a descriptor? A `#!` script fails
/// through it, so the ladder execs a script by its path.
pub fn has_shebang(bytes: &[u8]) -> bool {
    bytes.starts_with(b"#!")
}

/// Does `path` name a file that the kernel would execute: a file, with an
/// execute bit, that passes an `X_OK` access check?
///
/// Both halves: for root the bits can be `0o644` while the check passes,
/// and for a user that is not the owner the bits can be `0o755` while the
/// check fails. A path that is not UTF-8 gives false.
pub fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
        return false;
    }
    let Some(name) = path.to_str().and_then(sys::CBuf::new) else {
        return false;
    };
    sys::faccessat(sys::AT_FDCWD as i64, &name, sys::X_OK, 0).is_ok()
}

/// `memfd_create(2)` with [`MFD_CLOEXEC`] set.
///
/// The first try gives [`MFD_EXEC`], and [`MFD_ALLOW_SEALING`] where
/// `allow_sealing` is set. An `EINVAL` drops one flag and tries again:
/// first `MFD_EXEC` (a kernel before 6.3), then the seals (a kernel with
/// no seals). The image still runs, with no seals. Each other error is the
/// answer: under `vm.memfd_noexec=2` the kernel refuses `MFD_EXEC` with
/// `EACCES`, and no second try may hide that.
///
/// # Safety
///
/// It does a raw system call, as each caller of `pod_hostprobe::sys`.
pub unsafe fn memfd_create(name: &sys::CBuf, allow_sealing: bool) -> Sysres {
    let seal = if allow_sealing { MFD_ALLOW_SEALING } else { 0 };
    let tries = [MFD_CLOEXEC | MFD_EXEC | seal, MFD_CLOEXEC | seal, MFD_CLOEXEC];
    let mut answered = Err(sys::EINVAL);
    for (i, flags) in tries.into_iter().enumerate() {
        if i > 0 && flags == tries[i - 1] {
            continue;
        }
        answered = unsafe { sys::sys(sys::SYS_MEMFD_CREATE, [name.ptr(), flags, 0, 0, 0, 0]) };
        if answered != Err(sys::EINVAL) {
            break;
        }
    }
    answered
}

/// Write `bytes` to a new memfd named `names.memfd_label`, sealed where
/// the kernel accepts it.
///
/// The caller gives the descriptor to the ladder entry, which closes the
/// copy of the parent after the fork ([`crate::run_ladder`]).
pub fn stage(names: &Names, bytes: &[u8]) -> crate::Result<i64> {
    stage_raw(names.memfd_label, bytes).map_err(|f| crate::errno_failure(&format!("{} failed", f.step), f.errno))
}

/// The step of [`stage_raw`] that makes the memfd.
pub(crate) const STEP_CREATE: &str = "memfd_create for the payload";

/// The step of [`stage_raw`] that failed, with its errno.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StageFail {
    pub step: &'static str,
    pub errno: sys::Errno,
}

/// [`stage`] with the errno kept. The probe stages its trampoline here
/// too, so the probe and the launch use the same flags and seals.
pub(crate) fn stage_raw(label: &str, bytes: &[u8]) -> Result<i64, StageFail> {
    let fail = |step, errno| StageFail { step, errno };
    let name = sys::CBuf::new(label).ok_or(fail(STEP_CREATE, sys::EINVAL))?;
    let fd = unsafe { memfd_create(&name, true) }.map_err(|e| fail(STEP_CREATE, e))?;
    if let Err(e) = unsafe { write_full(fd, bytes) } {
        let _ = sys::close(fd);
        return Err(fail("writing the payload to its memfd", e));
    }
    match seal(fd) {
        Ok(_) => {}
        // A kernel with no seals answers EINVAL; the payload runs with no
        // seals, as in `memfd_create`.
        Err(e) if e == sys::EINVAL => {}
        Err(e) => {
            let _ = sys::close(fd);
            return Err(fail("sealing the payload memfd", e));
        }
    }
    Ok(fd)
}

/// Why bytes cannot go through the exec of a descriptor, with what they
/// take in its place.
///
/// A named reason, not a boolean: a caller that gets only "no" cannot
/// tell whether to exec by path, refuse a forced mode or fall through,
/// and each one is a different exit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemfdRefusal {
    /// A `#!` script fails through the exec of a descriptor; the entry
    /// execs it by path.
    RoutePastScript,
    /// Not an ELF file, with the reason.
    NotElf(String),
    /// Dynamically linked: `PT_INTERP` names the loader, which must find
    /// the libraries on a file system, and a memfd never puts them there.
    HasInterp(String),
}

impl std::fmt::Display for MemfdRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemfdRefusal::RoutePastScript => write!(f, "a #! script routes past fd-exec and is exec'd by path"),
            MemfdRefusal::NotElf(why) => write!(f, "not an ELF file for fd-exec: {why}"),
            MemfdRefusal::HasInterp(interp) => write!(
                f,
                "dynamically linked against {interp}: a memfd exec never puts its libraries on \
                 disk, so the loader would fail on the first one it cannot find"
            ),
        }
    }
}

/// The refusal of a `#!` script, with the file and its first line.
///
/// [`MemfdRefusal::RoutePastScript`] says that the entry found a script,
/// not which one. This value carries both: the resolved host path and the
/// first `#!` line. [`eligible`] keeps its form (bytes only); the caller
/// that owns the root file system resolves the path
/// ([`crate::ladder::resolve_payload`]), reads the bytes, and gives them
/// to [`script_refusal`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRefusal {
    /// The resolved path of the payload on the host, as given.
    pub path: String,
    /// The first `#!` line, with no newline at its end.
    pub shebang: String,
}

impl ScriptRefusal {
    /// The refusal: the exact path of the payload and its `#!` line.
    pub fn message(&self) -> String {
        format!(
            "the payload {} is a #! script ({}), whose interpreter path resolves on the host root \
             without a chroot: the image's script would run under the host's interpreter",
            self.path, self.shebang
        )
    }
}

/// The number of bytes of a first line that the refusal carries. A
/// hostile first line can be as long as the payload, and the refusal is a
/// sentence, not a copy.
pub const MAX_SHEBANG_LINE: usize = 512;

/// The path and the first `#!` line for the refusal of a script. `bytes`
/// need not start with `#!`.
pub fn script_refusal(path: &str, bytes: &[u8]) -> ScriptRefusal {
    ScriptRefusal { path: path.to_string(), shebang: first_shebang_line(bytes) }
}

/// The first line of `bytes`: up to the first `\n`, with no `\r` at its
/// end, lossy where the bytes are not UTF-8, cut at [`MAX_SHEBANG_LINE`]
/// on a character boundary with `...` after it.
pub fn first_shebang_line(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|c| *c == b'\n').unwrap_or(bytes.len());
    let line = &bytes[..end];
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let mut text = String::from_utf8_lossy(line).to_string();
    if text.len() > MAX_SHEBANG_LINE {
        let mut cut = MAX_SHEBANG_LINE;
        while cut > 0 && !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("...");
    }
    text
}

/// Can `bytes` execute from a memfd: an ELF file with no `PT_INTERP`,
/// from the walk of the program headers in [`crate::abi`]?
///
/// The architecture is not decided here: [`crate::binfmt`] answers whether
/// this machine runs the `e_machine` of the payload.
pub fn eligible(bytes: &[u8]) -> Result<(), MemfdRefusal> {
    if has_shebang(bytes) {
        return Err(MemfdRefusal::RoutePastScript);
    }
    match crate::abi::interp_of("payload", bytes) {
        Ok(None) => Ok(()),
        Ok(Some(interp)) => Err(MemfdRefusal::HasInterp(interp)),
        Err(e) => Err(MemfdRefusal::NotElf(format!("{e}"))),
    }
}

/// Write all of `bytes` to `fd`. The kernel can write less than asked, so
/// one call is not enough: a payload with a missing end that executes from
/// the memfd is a corruption.
///
/// # Safety
///
/// It does raw system calls, as each caller of `pod_hostprobe::sys`.
pub unsafe fn write_full(fd: i64, bytes: &[u8]) -> Sysres {
    let mut wrote = 0usize;
    while wrote < bytes.len() {
        match sys::write(fd, &bytes[wrote..]) {
            // `n` is at most the length of the slice, so it fits a usize.
            Ok(n) if n > 0 => wrote += n as usize,
            // A signal during the write is not a failure of the write.
            Err(e) if e == sys::EINTR => continue,
            // A write of zero with bytes left made no progress: a loop
            // would hang.
            Ok(_) => return Err(sys::Errno(5)),
            Err(e) => return Err(e),
        }
    }
    Ok(wrote as i64)
}

/// Seal a written memfd against shrink, growth and write.
///
/// A kernel with no seals answers `EINVAL`; the ladder runs the payload
/// with no seals, as [`memfd_create`] does. The four seals go on at one
/// time: a payload sealed against writes that can still grow is not
/// sealed.
pub fn seal(fd: i64) -> Sysres {
    sys::fcntl(fd, sys::F_ADD_SEALS, sys::F_SEAL_SEAL | sys::F_SEAL_SHRINK | sys::F_SEAL_GROW | sys::F_SEAL_WRITE)
}

/// Exec the memfd `fd` with no path.
///
/// This is `fexecve`: `execveat` with the empty path and `AT_EMPTY_PATH`.
/// As `execve`, it returns only on a failure. The caller makes `empty`
/// before the fork: between `clone` and `execve` only async-signal-safe
/// work is permitted, and the build of it allocates.
///
/// # Safety
///
/// `argv` and `envp` must be arrays of valid C string pointers that end
/// with a null pointer.
pub unsafe fn exec_fd(fd: i64, empty: &sys::CBuf, argv: *const *const u8, envp: *const *const u8) -> Sysres {
    unsafe { sys::execveat(fd, empty, argv, envp, sys::AT_EMPTY_PATH) }
}

#[cfg(test)]
mod tests;
