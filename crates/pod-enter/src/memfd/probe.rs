//! Does a memfd execute on this host?
//!
//! A probe that makes a memfd and closes it proves that `memfd_create`
//! answers, not that the descriptor executes. Under `vm.memfd_noexec=1`
//! such a probe promises the memfd rung, and the launch then fails late,
//! at the step "execveat of the memfd". This probe stages a small static
//! ELF through [`super::stage_raw`] (the flags and the seals of the
//! launch), forks, and executes it through [`super::exec_fd`] (the
//! `execveat` of the launch). The answer is measured one time for each
//! process: a health row, the ladder and the entry check read the same
//! answer, so they cannot disagree.
//!
//! A probe that cannot run to an answer (no pipe, no fork, an unknown wait
//! status) is [`MemfdExec::Unmeasured`]. It never reads as a denial.
//!
//! The trampoline exists for x86_64 and aarch64. On each other
//! architecture the probe stages bytes that no loader takes and reads
//! `ENOEXEC` as an exec: the checks of the exec permission passed, and no
//! code ran.

use std::sync::OnceLock;

use pod_hostprobe::sys::{self, Errno};

use crate::Names;

/// The sysctl that seals each new memfd against exec (Linux 6.3 and
/// later): 0 allows exec, 1 makes `MFD_NOEXEC_SEAL` the default, 2
/// refuses each `memfd_create` without `MFD_NOEXEC_SEAL`.
pub const SYSCTL_PATH: &str = "/proc/sys/vm/memfd_noexec";

/// The exit code of the trampoline. Not 0: a status of 42 proves that
/// the trampoline itself ran, and no other path in the probe child
/// exits with it.
pub const TRAMPOLINE_EXIT: i32 = 42;

/// The exit code of the probe child when `execveat` returns.
const EXEC_FAILED: i32 = 127;

/// The name of the memfd of the trampoline, and the `argv[0]` of it.
const PROBE_LABEL: &str = "memfd-probe";

/// The bound on the wait for the probe child, in milliseconds.
const WAIT_BOUND_MS: u32 = 2000;

/// What the probe measured. Each arm carries the sysctl value as read
/// beside the measurement; `None` is an unreadable sysctl.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemfdExec {
    /// The trampoline ran from a memfd and exited with [`TRAMPOLINE_EXIT`].
    Exec { sysctl: Option<u8> },
    /// The memfd was made, and `execveat` of it failed with `errno`.
    NoExec { sysctl: Option<u8>, errno: Errno },
    /// `memfd_create` failed with `errno`.
    NoMemfd { sysctl: Option<u8>, errno: Errno },
    /// The probe could not run to an answer: `step` failed with `why`.
    Unmeasured { step: &'static str, why: String },
}

impl MemfdExec {
    /// True only where the trampoline ran. The ladder promises the memfd
    /// rung on this and on nothing else.
    pub fn runs(&self) -> bool {
        matches!(self, MemfdExec::Exec { .. })
    }
}

/// The three states of the answer: a health row maps them to ok, fail
/// and note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    Holds,
    Denied,
    Unmeasured,
}

/// The text for one measurement. `fix` is `Some` exactly where the
/// standing is [`Standing::Denied`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub standing: Standing,
    pub detail: String,
    pub fix: Option<String>,
}

impl Answer {
    /// The detail and the fix as one sentence, for a refusal.
    pub fn sentence(&self) -> String {
        match &self.fix {
            Some(fix) => format!("{}; fix: {fix}", self.detail),
            None => self.detail.clone(),
        }
    }
}

/// The fix line where `vm.memfd_noexec=2` is the cause. `MFD_EXEC` does
/// not help under 2: the kernel refuses it too.
fn fix_sysctl_2(names: &Names) -> String {
    format!(
        "set vm.memfd_noexec to 0 (`sysctl -w vm.memfd_noexec=0` as root, in this pid namespace \
         or a parent one), or use another rung: run with chroot(2) allowed, or with {} unset",
        names.mode_request
    )
}

/// The fix line where the cause is not known.
fn fix_unknown(names: &Names) -> String {
    format!(
        "use another rung: run with chroot(2) allowed, or with {} unset; a seccomp or LSM policy \
         that denies memfd_create or execveat to {} can also be the cause",
        names.mode_request, names.product
    )
}

/// The sysctl part of a detail.
fn sysctl_words(sysctl: Option<u8>) -> String {
    match sysctl {
        Some(v) => format!("vm.memfd_noexec={v}"),
        None => "vm.memfd_noexec is not readable".to_string(),
    }
}

/// The text and the fix line for one measurement. Pure: a health row and
/// the refusals read this one mapping.
///
/// The entry gives `MFD_EXEC` to its own `memfd_create`
/// ([`super::memfd_create`]), so value 1 does not stop it. Value 2
/// refuses each memfd without `MFD_NOEXEC_SEAL`, also with `MFD_EXEC`, so
/// value 2 is the cause where `memfd_create` fails with `EACCES` or
/// `EPERM`. A denial with value 0 or with no readable value is "denied,
/// cause unknown", never the sysctl.
pub fn answer(names: &Names, m: &MemfdExec) -> Answer {
    let denied = |detail: String, fix: String| Answer { standing: Standing::Denied, detail, fix: Some(fix) };
    match m {
        MemfdExec::Exec { sysctl: Some(1) } => Answer {
            standing: Standing::Holds,
            detail: format!(
                "a memfd executes through execveat(AT_EMPTY_PATH): vm.memfd_noexec=1 seals a memfd \
                 against exec by default, and {} passes MFD_EXEC on its own memfd_create",
                names.product
            ),
            fix: None,
        },
        MemfdExec::Exec { sysctl } => Answer {
            standing: Standing::Holds,
            detail: format!("a memfd executes through execveat(AT_EMPTY_PATH) ({})", sysctl_words(*sysctl)),
            fix: None,
        },
        MemfdExec::NoMemfd { sysctl: Some(2), errno } if *errno == sys::EACCES || *errno == sys::EPERM => denied(
            format!(
                "denied by vm.memfd_noexec=2: the kernel refuses each memfd that is not sealed \
                 against exec, so memfd_create with MFD_EXEC fails with {} ({})",
                errno.name(),
                errno.0
            ),
            fix_sysctl_2(names),
        ),
        MemfdExec::NoExec { sysctl: Some(2), errno } => denied(
            format!(
                "denied by vm.memfd_noexec=2: execveat of the memfd fails with {} ({}), because \
                 the kernel seals each memfd against exec",
                errno.name(),
                errno.0
            ),
            fix_sysctl_2(names),
        ),
        MemfdExec::NoMemfd { sysctl, errno } => denied(
            format!(
                "denied, cause unknown: memfd_create fails with {} ({}) ({}{})",
                errno.name(),
                errno.0,
                sysctl_words(*sysctl),
                not_the_cause(names, *sysctl)
            ),
            fix_unknown(names),
        ),
        MemfdExec::NoExec { sysctl, errno } => denied(
            format!(
                "denied, cause unknown: execveat of the memfd fails with {} ({}) ({}{})",
                errno.name(),
                errno.0,
                sysctl_words(*sysctl),
                not_the_cause(names, *sysctl)
            ),
            fix_unknown(names),
        ),
        MemfdExec::Unmeasured { step, why } => {
            Answer { standing: Standing::Unmeasured, detail: format!("not measured: {step} failed: {why}"), fix: None }
        }
    }
}

/// Why a readable value of the sysctl is not the cause of a denial.
fn not_the_cause(names: &Names, sysctl: Option<u8>) -> String {
    match sysctl {
        Some(0) => ", which allows exec".to_string(),
        Some(1) => format!(", which {} meets with MFD_EXEC", names.product),
        _ => String::new(),
    }
}

/// The measurement, made one time for each process.
///
/// The answer is kept for the process: the entry check, the ladder and a
/// health row read one answer, so a second measurement cannot disagree
/// with the first. The answer is a fact of the kernel and of the policy
/// of the process, which do not change while the process runs.
pub fn memfd_exec() -> &'static MemfdExec {
    static ONCE: OnceLock<MemfdExec> = OnceLock::new();
    ONCE.get_or_init(measure)
}

/// Read `vm.memfd_noexec`, best effort. A kernel before 6.3 has no such
/// file, and that reads as `None`.
pub fn read_sysctl() -> Option<u8> {
    std::fs::read_to_string(SYSCTL_PATH).ok()?.trim().parse::<u8>().ok()
}

/// Stage the trampoline the way the launch stages a payload, then run it.
fn measure() -> MemfdExec {
    let sysctl = read_sysctl();
    let fd = match super::stage_raw(PROBE_LABEL, &TRAMPOLINE) {
        Ok(fd) => fd,
        Err(fail) if fail.step == super::STEP_CREATE => return MemfdExec::NoMemfd { sysctl, errno: fail.errno },
        Err(fail) => {
            return MemfdExec::Unmeasured { step: fail.step, why: format!("{} ({})", fail.errno.name(), fail.errno.0) };
        }
    };
    let m = measure_fd(fd, sysctl);
    let _ = sys::close(fd);
    m
}

/// Execute the memfd `fd` in a forked child and classify the result.
/// The caller keeps and closes `fd`.
pub(crate) fn measure_fd(fd: i64, sysctl: Option<u8>) -> MemfdExec {
    let unmeasured =
        |step: &'static str, e: Errno| MemfdExec::Unmeasured { step, why: format!("{} ({})", e.name(), e.0) };
    // Each thing that the child touches exists before the fork: between
    // clone and execve only async-signal-safe work is permitted.
    let empty = sys::CBuf::new("").expect("a literal carries no NUL");
    let argv0 = sys::CBuf::new(PROBE_LABEL).expect("a literal carries no NUL");
    let argv: [*const u8; 2] = [argv0.ptr() as *const u8, std::ptr::null()];
    let envp: [*const u8; 1] = [std::ptr::null()];
    let mut pipe = [0i32; 2];
    if let Err(e) = sys::pipe2(&mut pipe, sys::O_CLOEXEC) {
        return unmeasured("pipe2 for the errno channel", e);
    }
    let (r, w) = (i64::from(pipe[0]), i64::from(pipe[1]));
    let pid = match unsafe { sys::clone_fork(sys::SIGCHLD) } {
        Ok(0) => {
            // The child. The write end is close-on-exec, so a successful
            // exec closes it with nothing written.
            if let Err(e) = unsafe { super::exec_fd(fd, &empty, argv.as_ptr(), envp.as_ptr()) } {
                let msg = [(e.0 & 0xff) as u8, ((e.0 >> 8) & 0xff) as u8];
                let _ = sys::write(w, &msg);
            }
            sys::exit_group(EXEC_FAILED)
        }
        Ok(pid) => pid,
        Err(e) => {
            let _ = sys::close(r);
            let _ = sys::close(w);
            return unmeasured("clone(2) for the probe child", e);
        }
    };
    let _ = sys::close(w);
    let status = wait_bounded(pid);
    // The child is gone, so no writer is left and this read cannot block.
    let mut buf = [0u8; 2];
    let got = loop {
        match sys::read(r, &mut buf) {
            Err(e) if e == sys::EINTR => continue,
            other => break other,
        }
    };
    let _ = sys::close(r);
    let status = match status {
        Ok(st) => st,
        Err(why) => return MemfdExec::Unmeasured { step: "the wait for the probe child", why },
    };
    if let Ok(2) = got {
        let errno = Errno(i32::from(buf[0]) | (i32::from(buf[1]) << 8));
        if !RUNS_CODE && errno == ENOEXEC {
            return MemfdExec::Exec { sysctl };
        }
        return MemfdExec::NoExec { sysctl, errno };
    }
    classify_status(status, sysctl)
}

/// The wait status of a child that exec'd: the trampoline's own code is
/// [`MemfdExec::Exec`]; each other status is unmeasured, because the
/// exec happened and something else ended the child.
fn classify_status(status: i32, sysctl: Option<u8>) -> MemfdExec {
    let exited = status & 0x7f == 0;
    if exited && (status >> 8) & 0xff == TRAMPOLINE_EXIT {
        return MemfdExec::Exec { sysctl };
    }
    MemfdExec::Unmeasured {
        step: "the exit status of the trampoline",
        why: format!("wait status {status:#06x}, not an exit with {TRAMPOLINE_EXIT}"),
    }
}

/// Reap `pid` within [`WAIT_BOUND_MS`]. Past the bound the child is
/// killed and reaped, and the result is an error with the bound.
fn wait_bounded(pid: i64) -> Result<i32, String> {
    const WNOHANG: u64 = 1;
    const SIGKILL: i64 = 9;
    let mut st = 0i32;
    for _ in 0..WAIT_BOUND_MS {
        let ptr = &mut st as *mut i32 as u64;
        match unsafe { sys::sys(sys::SYS_WAIT4, [pid as u64, ptr, WNOHANG, 0, 0, 0]) } {
            Ok(0) => std::thread::sleep(std::time::Duration::from_millis(1)),
            Ok(_) => return Ok(st),
            Err(e) if e == sys::EINTR => continue,
            Err(e) => return Err(format!("wait4 failed with {} ({})", e.name(), e.0)),
        }
    }
    let _ = sys::kill(pid, SIGKILL);
    loop {
        match sys::wait4(pid, &mut st) {
            Err(e) if e == sys::EINTR => continue,
            _ => break,
        }
    }
    Err(format!("the child did not exit within {WAIT_BOUND_MS} ms and was killed"))
}

/// Whether [`TRAMPOLINE`] is a program for this architecture. Where it
/// is not, the probe stages bytes that no loader takes, and `ENOEXEC`
/// from `execveat` reads as [`MemfdExec::Exec`]: the kernel passed its
/// exec permission checks (the mode bits, the exec seal, the LSM hooks)
/// and only then looked for a loader.
const RUNS_CODE: bool = cfg!(any(target_arch = "x86_64", target_arch = "aarch64"));

/// `ENOEXEC`: no loader takes the bytes.
const ENOEXEC: Errno = Errno(8);

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
pub use elf::TRAMPOLINE;

/// Bytes that no loader takes, for an architecture without a trampoline.
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub const TRAMPOLINE: [u8; 16] = [0; 16];

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
mod elf {
    /// The base virtual address of the trampoline's one `PT_LOAD`.
    const BASE: u64 = 0x40_0000;

    /// The size of the ELF64 header plus one program header: the code
    /// starts here.
    pub(super) const CODE_AT: usize = 64 + 56;

    /// The machine code: `exit_group(42)`.
    ///
    /// x86_64: `mov edi, 42` (bf 2a 00 00 00), `mov eax, 231` (b8 e7 00
    /// 00 00, `__NR_exit_group`), `syscall` (0f 05).
    #[cfg(target_arch = "x86_64")]
    const CODE: [u8; 12] = [0xbf, 0x2a, 0x00, 0x00, 0x00, 0xb8, 0xe7, 0x00, 0x00, 0x00, 0x0f, 0x05];
    /// `EM_X86_64`.
    #[cfg(target_arch = "x86_64")]
    pub(super) const MACHINE: u16 = 62;

    /// aarch64: `movz x0, #42` (0xd2800540), `movz x8, #94` (0xd2800bc8,
    /// `__NR_exit_group`), `svc #0` (0xd4000001), each little-endian. The
    /// code starts at offset 120, which is 4-byte aligned.
    #[cfg(target_arch = "aarch64")]
    const CODE: [u8; 12] = [0x40, 0x05, 0x80, 0xd2, 0xc8, 0x0b, 0x80, 0xd2, 0x01, 0x00, 0x00, 0xd4];
    /// `EM_AARCH64`.
    #[cfg(target_arch = "aarch64")]
    pub(super) const MACHINE: u16 = 183;

    /// The length of the trampoline file.
    pub(super) const LEN: usize = CODE_AT + CODE.len();

    /// The trampoline: a static `ET_EXEC` ELF64 with one `PT_LOAD` (read
    /// and execute) that maps the whole file at [`BASE`] and enters at the
    /// code. No `PT_INTERP`, no sections. It calls `exit_group(42)` and
    /// nothing else.
    pub const TRAMPOLINE: [u8; LEN] = trampoline();

    const fn put(b: &mut [u8; LEN], at: usize, v: &[u8]) {
        let mut i = 0;
        while i < v.len() {
            b[at + i] = v[i];
            i += 1;
        }
    }

    const fn trampoline() -> [u8; LEN] {
        let mut b = [0u8; LEN];
        // e_ident: the magic, ELFCLASS64 (2), ELFDATA2LSB (1), EV_CURRENT
        // (1), ELFOSABI_NONE (0); the pad bytes stay 0.
        put(&mut b, 0, &[0x7f, b'E', b'L', b'F', 2, 1, 1, 0]);
        put(&mut b, 16, &2u16.to_le_bytes()); // e_type: ET_EXEC
        put(&mut b, 18, &MACHINE.to_le_bytes()); // e_machine
        put(&mut b, 20, &1u32.to_le_bytes()); // e_version: EV_CURRENT
        put(&mut b, 24, &(BASE + CODE_AT as u64).to_le_bytes()); // e_entry
        put(&mut b, 32, &64u64.to_le_bytes()); // e_phoff: after the header
        // e_shoff (40) and e_flags (48) stay 0: no sections, no flags.
        put(&mut b, 52, &64u16.to_le_bytes()); // e_ehsize
        put(&mut b, 54, &56u16.to_le_bytes()); // e_phentsize
        put(&mut b, 56, &1u16.to_le_bytes()); // e_phnum
        // e_shentsize, e_shnum and e_shstrndx (58..64) stay 0.
        put(&mut b, 64, &1u32.to_le_bytes()); // p_type: PT_LOAD
        put(&mut b, 68, &5u32.to_le_bytes()); // p_flags: PF_R | PF_X
        // p_offset (72) stays 0: the segment maps the file from its start.
        put(&mut b, 80, &BASE.to_le_bytes()); // p_vaddr
        put(&mut b, 88, &BASE.to_le_bytes()); // p_paddr
        put(&mut b, 96, &(LEN as u64).to_le_bytes()); // p_filesz
        put(&mut b, 104, &(LEN as u64).to_le_bytes()); // p_memsz
        put(&mut b, 112, &0x1000u64.to_le_bytes()); // p_align
        put(&mut b, CODE_AT, &CODE);
        b
    }
}

#[cfg(test)]
mod tests;
