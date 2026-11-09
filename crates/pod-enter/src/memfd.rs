//! `TOOL.md` section 3.5, [`TODO/deps.md`](../../../TODO/deps.md) T-0909:
//! podvm's own memfd launch path.
//!
//! Three syscalls (`memfd_create`, `write`, `fexecve` by fd), declared here
//! rather than taken from a crate: the fork's value is two mechanisms and a
//! ladder, all smaller re-implemented than carried, and carrying it would
//! import a regression this tree would then have to patch out (an
//! `MFD_CLOEXEC` stub that never sets the flag).
//!
//! The two mechanisms, stated in this project's own words:
//!
//! 1. `MFD_CLOEXEC` is the unconditional base of every `memfd_create` call,
//!    so the memfd never leaks across a later `execve`;
//! 2. a payload is probed for executability **before** it is written, so a
//!    non-executable memfd becomes `EACCES` at the probe rather than a
//!    confusing `fexecve` failure later.
//!
//! ⚠ `#!` scripts fail through fd-exec and route past it: the ladder execs
//! them by path instead of through the memfd.

#![forbid(unsafe_op_in_unsafe_fn)]

use std::path::Path;

pub mod probe;
pub use probe::{memfd_exec, MemfdExec};

use podvm_probe::sys::{self, Sysres};

/// Close-on-exec, from `linux/memfd.h`. Unconditional: every call below sets
/// it, so no later `execve` inherits the memfd.
pub const MFD_CLOEXEC: u64 = 0x0001;

/// Allow seals, from `linux/memfd.h`. Added only when the caller asks for
/// sealing and the kernel accepts it.
pub const MFD_ALLOW_SEALING: u64 = 0x0002;

/// Seal the memfd against exec, from `linux/memfd.h` (Linux 6.3). podvm
/// never sets it; the T-1688 test sets it to prove the denial path.
pub const MFD_NOEXEC_SEAL: u64 = 0x0008;

/// Keep the memfd executable, from `linux/memfd.h` (Linux 6.3). T-1688:
/// under `vm.memfd_noexec=1` a memfd without this flag is sealed against
/// exec, so podvm passes it on each `memfd_create`. A kernel before 6.3
/// answers `EINVAL` to it, and the call is retried without it.
pub const MFD_EXEC: u64 = 0x0010;

/// Whether these bytes route past fd-exec: a `#!` script fails through it,
/// so the ladder execs scripts by path instead of through the memfd.
pub fn has_shebang(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == b'#' && bytes[1] == b'!'
}

/// Whether `path` names something the kernel would execute: a file, with an
/// execute bit set, passing an `X_OK` access check.
///
/// ⚠ Both halves, because either alone mis-answers: for root the bits may
/// read `0o644` while the check still succeeds, and for a non-owner they may
/// read `0o755` while it fails. A path that is not UTF-8 answers false: the
/// probe cannot name it to the kernel through a `&str` buffer, and the
/// ladder's `EACCES` names the probe rather than failing later.
pub fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
        return false;
    }
    let Some(text) = path.to_str() else {
        return false;
    };
    let Some(name) = sys::CBuf::new(text) else {
        return false;
    };
    sys::faccessat(sys::AT_FDCWD as i64, &name, sys::X_OK, 0).is_ok()
}

/// `memfd_create(2)` with [`MFD_CLOEXEC`] always set.
///
/// The call first tries with [`MFD_EXEC`] and, when `allow_sealing` is
/// set, [`MFD_ALLOW_SEALING`]. An `EINVAL` drops one flag and retries:
/// first `MFD_EXEC` (a kernel before 6.3), then sealing (a kernel without
/// it). The image still runs, it just stays unsealed. Each other error
/// is the answer: under `vm.memfd_noexec=2` the kernel refuses
/// `MFD_EXEC` with `EACCES`, and no retry may hide that (T-1688).
///
/// # Safety
///
/// Issues a raw syscall, like every other `podvm_probe::sys` caller.
pub unsafe fn memfd_create(name: &sys::CBuf, allow_sealing: bool) -> Sysres {
    let seal = if allow_sealing { MFD_ALLOW_SEALING } else { 0 };
    let tries = [
        MFD_CLOEXEC | MFD_EXEC | seal,
        MFD_CLOEXEC | seal,
        MFD_CLOEXEC,
    ];
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

/// Write `bytes` to a new memfd, sealed where the kernel accepts it.
///
/// The caller hands the descriptor to the ladder entry and closes its own
/// copy past the fork (see `run_ladder`): a parent copy held past that is a
/// leak, and the child execs from its own.
pub fn stage(bytes: &[u8]) -> crate::Result<i64> {
    stage_raw(bytes).map_err(|f| {
        crate::Error::Runtime(format!(
            "{} failed: {} ({})",
            f.step,
            f.errno.name(),
            f.errno.0
        ))
    })
}

/// The step of [`stage_raw`] that creates the memfd.
pub(crate) const STEP_CREATE: &str = "memfd_create for the payload";

/// The step of [`stage_raw`] that failed, with its errno.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StageFail {
    pub step: &'static str,
    pub errno: sys::Errno,
}

/// [`stage`] with the errno kept. ⛔ One staging path: the T-1688 probe
/// stages its trampoline here, so the probe and the launch use the same
/// flags and seals.
pub(crate) fn stage_raw(bytes: &[u8]) -> Result<i64, StageFail> {
    let fail = |step, errno| StageFail { step, errno };
    let name = sys::CBuf::new("podvm-payload").expect("a literal carries no NUL");
    let fd = unsafe { memfd_create(&name, true) }.map_err(|e| fail(STEP_CREATE, e))?;
    if let Err(e) = unsafe { write_full(fd, bytes) } {
        let _ = sys::close(fd);
        return Err(fail("writing the payload to its memfd", e));
    }
    match seal(fd) {
        Ok(_) => {}
        // A kernel without sealing answers EINVAL; the payload still runs, it
        // just stays unsealed, which is `memfd_create`'s own fallback shape.
        Err(e) if e == sys::EINVAL => {}
        Err(e) => {
            let _ = sys::close(fd);
            return Err(fail("sealing the payload memfd", e));
        }
    }
    Ok(fd)
}

/// Why bytes cannot go through fd-exec, naming what they take instead.
///
/// ⛔ Named rather than a boolean, for the ladder's refusal: a caller told only
/// "no" cannot say whether to exec by path, refuse a forced mode, or fall
/// through, and each of those is a different exit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemfdRefusal {
    /// A `#!` script fails through fd-exec and is exec'd by path instead.
    RoutePastScript,
    /// Not an ELF file at all, with why not.
    NotElf(String),
    /// Dynamically linked: `PT_INTERP` names the loader that must resolve it
    /// from a filesystem, which a memfd never puts in place.
    HasInterp(String),
}

impl std::fmt::Display for MemfdRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemfdRefusal::RoutePastScript => {
                write!(f, "a #! script routes past fd-exec and is exec'd by path")
            }
            MemfdRefusal::NotElf(why) => write!(f, "not an ELF file for fd-exec: {why}"),
            MemfdRefusal::HasInterp(interp) => write!(
                f,
                "dynamically linked against {interp}: a memfd exec never puts \
                 its libraries on disk, so the loader would fail on the first \
                 one it cannot find"
            ),
        }
    }
}

/// A `#!` script refusal with the file and its first line attached.
///
/// TODO/enter.md T-1413. [`MemfdRefusal::RoutePastScript`] is a unit variant:
/// it says a script was met and drops which one, so the refusal downstream
/// cannot name whether podvm picked the entrypoint, a wrapper, or the wrong
/// binary. This carries both: the resolved host path and the first `#!`
/// line. [`eligible`] keeps its shape (bytes only); the caller that owns the
/// rootfs resolves the path and reads the bytes, then attaches them here.
///
/// SEAM for the CLI agent (lifecycle `no_chroot` message): where
/// [`eligible`] answers `RoutePastScript`, resolve the payload path (the
/// same [`crate::ladder::resolve_payload`] [`crate::ladder::payload_bytes`]
/// uses), read its bytes, and render
/// `script_refusal(&path, &bytes).message()`. Longer term the interpreter
/// routes inside the rootfs where the T-1407 map table covers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRefusal {
    /// The resolved payload path, as handed in (host path).
    pub path: String,
    /// The first `#!` line, without the trailing newline.
    pub shebang: String,
}

impl ScriptRefusal {
    /// The refusal sentence: the exact payload path and its `#!` line.
    pub fn message(&self) -> String {
        format!(
            "the payload {} is a #! script ({}), whose interpreter path resolves \
             on the host root without a chroot: the image's script would run \
             under the host's interpreter (TODO/enter.md T-1413)",
            self.path, self.shebang
        )
    }
}

/// How many bytes of a first line the refusal carries.
///
/// Bounded: a hostile first line can run to the payload ceiling, and the
/// refusal is a sentence, not a copy. Longer lines truncate at a character
/// boundary with an ellipsis marker.
pub const MAX_SHEBANG_LINE: usize = 512;

/// Attach the resolved path and the first `#!` line to a script refusal.
///
/// Pure over bytes already in hand, so the unit test and the lifecycle
/// message read the same words. `path` is carried verbatim; `bytes` need
/// not start with `#!` (a caller that already knows it routes past scripts
/// still gets the line it would have named).
pub fn script_refusal(path: &str, bytes: &[u8]) -> ScriptRefusal {
    ScriptRefusal {
        path: path.to_string(),
        shebang: first_shebang_line(bytes),
    }
}

/// The first line of `bytes`: up to the first `\n`, without trailing `\r`,
/// lossy where the bytes are not UTF-8, bounded by [`MAX_SHEBANG_LINE`].
///
/// A first line past the bound truncates at a character boundary with
/// `...` marking it, so the refusal stays a sentence whatever the payload
/// holds.
pub fn first_shebang_line(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|c| *c == b'\n')
        .unwrap_or(bytes.len());
    let mut line = &bytes[..end];
    if line.ends_with(b"\r") {
        line = &line[..line.len() - 1];
    }
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

/// Whether `bytes` may be exec'd from a memfd: a static-PIE with no
/// `PT_INTERP`, reached through the program-header walk in [`crate::abi`].
///
/// ⚠ The architecture is NOT decided here: whether this machine executes the
/// payload's `e_machine` is [`crate::binfmt`]'s question (T-0506), asked with
/// the file in hand rather than guessed from its bytes.
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

/// Write all of `bytes` to `fd`, looping short writes.
///
/// ⚠ The kernel may return short, so one call is not enough: a truncated
/// payload exec'd from the memfd is a corruption, not a short write.
///
/// # Safety
///
/// Issues raw syscalls, like every other `podvm_probe::sys` caller.
pub unsafe fn write_full(fd: i64, bytes: &[u8]) -> Sysres {
    let mut wrote = 0usize;
    while wrote < bytes.len() {
        match sys::write(fd, &bytes[wrote..]) {
            Ok(n) if n > 0 => wrote += n as usize,
            // ⚠ `EINTR` is not a failure: a signal delivered mid-write must not
            // read as the payload being unwritable.
            Err(e) if e == sys::EINTR => continue,
            // A zero write with bytes remaining is not a short write to loop
            // on: nothing progressed, so looping is a hang shaped as patience.
            Ok(_) => return Err(sys::Errno(5)),
            Err(e) => return Err(e),
        }
    }
    Ok(wrote as i64)
}

/// Seal a written memfd against further shrinking, growing and writing.
///
/// A kernel without sealing answers `EINVAL`; the ladder runs the payload
/// unsealed rather than refusing it, which is [`memfd_create`]'s own fallback
/// shape. Sealing is all four seals at once: a payload sealed against writing
/// but still growable is a bound, not a seal.
pub fn seal(fd: i64) -> Sysres {
    sys::fcntl(
        fd,
        sys::F_ADD_SEALS,
        sys::F_SEAL_SEAL | sys::F_SEAL_SHRINK | sys::F_SEAL_GROW | sys::F_SEAL_WRITE,
    )
}

/// Exec the memfd `fd` points at, without resolving a path.
///
/// This is `fexecve` through the number the crates name: `execveat` with the
/// empty path and `AT_EMPTY_PATH` (see [`sys::SYS_EXECVEAT`]). Like `execve`,
/// it returns only on failure, and the error it returns with is the ladder's
/// fallthrough decision.
///
/// ⛔ `empty` is built BEFORE the fork and handed in, because between `clone`
/// and `execve` only async-signal-safe work is permitted and building it
/// allocates.
///
/// # Safety
///
/// `argv` and `envp` must be NUL-terminated arrays of valid C string pointers.
pub unsafe fn exec_fd(
    fd: i64,
    empty: &sys::CBuf,
    argv: *const *const u8,
    envp: *const *const u8,
) -> Sysres {
    unsafe { sys::execveat(fd, empty, argv, envp, sys::AT_EMPTY_PATH) }
}

#[cfg(test)]
mod tests;
