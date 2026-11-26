use super::*;

// ------------------------------------------------------------ machine

/// The emulator the machine tier drives. Named once, so the two legs that
/// spawn it cannot drift onto two different binaries.
const QEMU: &str = "qemu-system-x86_64";

/// The variable through which a probe child learns the path of the emulator
/// from [`crate::Settings::emulator`]. The parent sets it from the settings
/// alone, so that a variable of the caller never changes the emulator.
pub const EMULATOR_VARIABLE: &str = "POD_HOSTPROBE_EMULATOR";

/// The emulator of one leg: the default name, found on `PATH`, or the path
/// that the caller gave.
struct Emu {
    prog: String,
    given: bool,
}

impl Emu {
    /// The setting in a probe child. An empty value is a path that the
    /// parent could not pass (a NUL or a byte that is not UTF-8).
    fn read() -> Result<Emu, Outcome> {
        match std::env::var_os(EMULATOR_VARIABLE) {
            None => Ok(Emu { prog: QEMU.to_string(), given: false }),
            Some(v) => match v.into_string() {
                Ok(p) if !p.is_empty() => {
                    // A name with no `/` would go to the PATH lookup: the path
                    // of the caller never does.
                    let prog = if p.contains('/') { p } else { format!("./{p}") };
                    Ok(Emu { prog, given: true })
                }
                _ => Err(Outcome::skip(
                    None,
                    "Settings::emulator is not a path that a child can read, so the emulator leg could not run",
                )),
            },
        }
    }

    /// The words for an emulator that is not there.
    fn absent(&self, then: &str) -> Outcome {
        let what = if self.given { format!("no emulator at {}", self.prog) } else { format!("no {QEMU} on PATH") };
        Outcome::skip(Some(sys::ENOENT), format!("{what}, so {then}"))
    }

    /// Run the emulator with `args` and take its stdout.
    ///
    /// Through [`sys::shed_after_fork`], like [`run_payload`]: libstd forks
    /// inside `output()`, and without the hook every lock another thread
    /// holds is duplicated into the child at the fork.
    fn output(&self, args: &[&str]) -> EmuAnswer {
        let mut cmd = std::process::Command::new(&self.prog);
        cmd.args(args);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());
        let out = match sys::shed_after_fork(&mut cmd).output() {
            Ok(o) => o,
            Err(e) => match e.raw_os_error() {
                // The lookup is `execvp`'s, so ENOENT is the binary's absence
                // and not a statement about the machine's acceleration.
                Some(n) if n == sys::ENOENT.0 => return EmuAnswer::Absent,
                Some(n) => return EmuAnswer::Unusable(Errno(n)),
                None => return EmuAnswer::Failed(format!("spawning {} failed: {e}", self.prog)),
            },
        };
        if !out.status.success() {
            return EmuAnswer::Exited(out.status.code().unwrap_or(-1));
        }
        EmuAnswer::Said(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

/// What running the emulator answered: its stdout, or why there is none.
enum EmuAnswer {
    Said(String),
    Absent,
    Unusable(Errno),
    Exited(i32),
    Failed(String),
}

pub(super) fn m_qemu_version() -> Outcome {
    let emu = match Emu::read() {
        Ok(e) => e,
        Err(o) => return o,
    };
    match emu.output(&["--version"]) {
        EmuAnswer::Said(out) => match out.lines().next().map(str::trim).filter(|l| !l.is_empty()) {
            Some(line) => {
                let line: String = line.chars().take(160).collect();
                // The path that ran, so a row of a store emulator says which.
                Outcome::ok_with(if emu.given { format!("{line} ({})", emu.prog) } else { line })
            }
            None => Outcome::skip(None, format!("{} --version ran and answered nothing", emu.prog)),
        },
        EmuAnswer::Absent => emu.absent("the emulator leg could not run"),
        EmuAnswer::Unusable(e) => Outcome::denied(e),
        EmuAnswer::Exited(code) => Outcome::skip(
            None,
            format!(
                "{} --version ran and exited {code}; the emulator \
                 runs but did not answer",
                emu.prog
            ),
        ),
        EmuAnswer::Failed(why) => Outcome::skip(None, why),
    }
}

pub(super) fn m_kvm() -> Outcome {
    // OPENED, not stat-ed. A node that exists and answers EACCES on open is
    // not acceleration. Absence is the answer too, so ENOENT is a
    // denial rather than a missing precondition, like `stat(/dev/ptmx)`.
    // An open device must also run a guest of this build's architecture:
    // QEMU user mode passes the device of another one through (`kvm`).
    match sys::open(&c("/dev/kvm"), sys::O_RDWR | sys::O_CLOEXEC, 0) {
        Ok(fd) => {
            let out = super::kvm::runs_native_guest(fd);
            let _ = sys::close(fd);
            out
        }
        Err(e) => Outcome::denied(e),
    }
}

pub(super) fn m_fsize() -> Outcome {
    match sys::prlimit(sys::RLIMIT_FSIZE) {
        Ok((cur, max)) => Outcome::ok_with(format!("RLIMIT_FSIZE cur={} max={}", rlim(cur), rlim(max))),
        Err(e) => Outcome::denied(e),
    }
}

/// An rlimit value in words. `RLIM64_INFINITY` is not a size, so digits
/// would be a number that looks plausible and is not.
fn rlim(v: u64) -> String {
    if v == u64::MAX { "infinity".to_string() } else { v.to_string() }
}

pub(super) fn m_tun() -> Outcome {
    // Opened the same way as the kvm leg: a node that cannot be opened cannot
    // carry the tier's network.
    open_probe("/dev/net/tun", sys::O_RDWR | sys::O_CLOEXEC)
}

pub(super) fn m_space() -> Outcome {
    // Blocks and inodes both, for the directory the caller runs in, named in
    // the row. The destination that the tier will use is not known when the
    // probe runs; its consumer checks its own.
    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => match e.raw_os_error() {
            Some(n) => {
                return Outcome::skip(
                    Some(Errno(n)),
                    "the working directory could not be read, so no space was measured",
                );
            }
            None => return Outcome::skip(None, format!("the working directory could not be read: {e}")),
        },
    };
    let path = cwd.to_string_lossy().into_owned();
    let Some(p) = CBuf::new(&path) else {
        return Outcome::skip(None, "the working directory contains a NUL and cannot reach the kernel");
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
    let emu = match Emu::read() {
        Ok(e) => e,
        Err(o) => return o,
    };
    // Ask the emulator rather than deciding from the kvm leg. An
    // accelerator the build was compiled without is as absent as a missing
    // node, and the node alone cannot say which builds carry what.
    match emu.output(&["-accel", "help"]) {
        EmuAnswer::Said(out) => {
            let accels: Vec<&str> = out.lines().map(str::trim).filter(|l| !l.is_empty() && !l.ends_with(':')).collect();
            if accels.is_empty() {
                Outcome::skip(None, format!("{} -accel help ran and listed no accelerator", emu.prog))
            } else if emu.given {
                Outcome::ok_with(format!("accelerators: {} ({})", accels.join(" "), emu.prog))
            } else {
                Outcome::ok_with(format!("accelerators: {}", accels.join(" ")))
            }
        }
        EmuAnswer::Absent => emu.absent("the accelerator list could not be read"),
        EmuAnswer::Unusable(e) => Outcome::denied(e),
        EmuAnswer::Exited(code) => {
            Outcome::skip(None, format!("{} -accel help ran and exited {code}; the list is unread", emu.prog))
        }
        EmuAnswer::Failed(why) => Outcome::skip(None, why),
    }
}
