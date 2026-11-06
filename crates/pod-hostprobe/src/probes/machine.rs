use super::*;

// ------------------------------------------------------------ machine

/// The emulator the machine tier drives. Named once, so the two legs that
/// spawn it cannot drift onto two different binaries.
const QEMU: &str = "qemu-system-x86_64";

/// What running the emulator answered: its stdout, or why there is none.
enum EmuAnswer {
    Said(String),
    Absent,
    Unusable(Errno),
    Exited(i32),
    Failed(String),
}

/// Run the emulator with `args` and take its stdout.
///
/// ⛔ Through [`sys::shed_after_fork`], like [`run_payload`]: libstd forks
/// inside `output()`, and without the hook every lock another thread holds is
/// duplicated into the child at the fork. `TODO/image.md` T-0215.
fn qemu_output(args: &[&str]) -> EmuAnswer {
    let mut cmd = std::process::Command::new(QEMU);
    cmd.args(args);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::null());
    let out = match sys::shed_after_fork(&mut cmd).output() {
        Ok(o) => o,
        Err(e) => match e.raw_os_error() {
            // The lookup is `execvp`'s, so ENOENT is the binary's absence from
            // PATH and not a statement about the machine's acceleration.
            Some(n) if n == sys::ENOENT.0 => return EmuAnswer::Absent,
            Some(n) => return EmuAnswer::Unusable(Errno(n)),
            None => return EmuAnswer::Failed(format!("spawning {QEMU} failed: {e}")),
        },
    };
    if !out.status.success() {
        return EmuAnswer::Exited(out.status.code().unwrap_or(-1));
    }
    EmuAnswer::Said(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub(super) fn m_qemu_version() -> Outcome {
    match qemu_output(&["--version"]) {
        EmuAnswer::Said(out) => match out.lines().next().map(str::trim).filter(|l| !l.is_empty()) {
            Some(line) => Outcome::ok_with(line.chars().take(160).collect::<String>()),
            None => Outcome::skip(
                None,
                "qemu-system-x86_64 --version ran and answered nothing",
            ),
        },
        EmuAnswer::Absent => Outcome::skip(
            Some(sys::ENOENT),
            "no qemu-system-x86_64 on PATH, so the emulator leg could not run",
        ),
        EmuAnswer::Unusable(e) => Outcome::denied(e),
        EmuAnswer::Exited(code) => Outcome::skip(
            None,
            format!(
                "qemu-system-x86_64 --version ran and exited {code}; the emulator \
                 runs but did not answer"
            ),
        ),
        EmuAnswer::Failed(why) => Outcome::skip(None, why),
    }
}

pub(super) fn m_kvm() -> Outcome {
    // ⛔ OPENED, not stat-ed. A node that exists and answers EACCES on open is
    // not acceleration. T-1301. Absence is the answer too, so ENOENT is a
    // denial rather than a missing precondition, like `stat(/dev/ptmx)`.
    open_probe("/dev/kvm", sys::O_RDWR | sys::O_CLOEXEC)
}

pub(super) fn m_fsize() -> Outcome {
    match sys::prlimit(sys::RLIMIT_FSIZE) {
        Ok((cur, max)) => {
            Outcome::ok_with(format!("RLIMIT_FSIZE cur={} max={}", rlim(cur), rlim(max)))
        }
        Err(e) => Outcome::denied(e),
    }
}

/// An rlimit value in words. `RLIM64_INFINITY` is not a size, so printing it
/// as digits would be the plausible-looking number AGENTS.md absolute 3
/// refuses.
fn rlim(v: u64) -> String {
    if v == u64::MAX {
        "infinity".to_string()
    } else {
        v.to_string()
    }
}

pub(super) fn m_tun() -> Outcome {
    // Opened the same way as the kvm leg: a node that cannot be opened cannot
    // carry the tier's network.
    open_probe("/dev/net/tun", sys::O_RDWR | sys::O_CLOEXEC)
}

pub(super) fn m_space() -> Outcome {
    // Blocks and inodes both, per `TODO/RULES.md` section 8, for the directory
    // the caller runs in, named in the row. A destination the tier will use is
    // not known at probe time; T-1302's consumer checks its own.
    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => match e.raw_os_error() {
            Some(n) => {
                return Outcome::skip(
                    Some(Errno(n)),
                    "the working directory could not be read, so no space was measured",
                )
            }
            None => {
                return Outcome::skip(
                    None,
                    format!("the working directory could not be read: {e}"),
                )
            }
        },
    };
    let path = cwd.to_string_lossy().into_owned();
    let Some(p) = CBuf::new(&path) else {
        return Outcome::skip(
            None,
            "the working directory contains a NUL and cannot reach the kernel",
        );
    };
    match sys::statfs(&p) {
        Ok(s) => Outcome::ok_with(format!(
            "{} blocks of {} B free, {} inodes free at {path}",
            s.f_bavail, s.f_bsize, s.f_ffree
        )),
        Err(e) => Outcome::denied(e),
    }
}

pub(super) fn m_accel() -> Outcome {
    // ⭐ Ask the emulator rather than deciding from the kvm leg. An
    // accelerator the build was compiled without is as absent as a missing
    // node, and the node alone cannot say which builds carry what.
    match qemu_output(&["-accel", "help"]) {
        EmuAnswer::Said(out) => {
            let accels: Vec<&str> = out
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.ends_with(':'))
                .collect();
            if accels.is_empty() {
                Outcome::skip(
                    None,
                    "qemu-system-x86_64 -accel help ran and listed no accelerator",
                )
            } else {
                Outcome::ok_with(format!("accelerators: {}", accels.join(" ")))
            }
        }
        EmuAnswer::Absent => Outcome::skip(
            Some(sys::ENOENT),
            "no qemu-system-x86_64 on PATH, so the accelerator list could not be read",
        ),
        EmuAnswer::Unusable(e) => Outcome::denied(e),
        EmuAnswer::Exited(code) => Outcome::skip(
            None,
            format!("qemu-system-x86_64 -accel help ran and exited {code}; the list is unread"),
        ),
        EmuAnswer::Failed(why) => Outcome::skip(None, why),
    }
}
