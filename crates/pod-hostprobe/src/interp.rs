//! Which instrument answered: this machine, or an emulator standing in for it.
//!
//! **A probe run under `qemu-user` measures the emulator, and its rung is
//! not the rung of the machine.** Measured: `qemu-aarch64` answers `EINVAL`
//! to `clone(CLONE_NEWNS)`, `ENOSYS` to `fsmount` and `move_mount`, and
//! reports `Seccomp: 0` and `NoNewPrivs: 0` however the host is confined.
//! Each of those is a statement about QEMU.
//!
//! An ordinary user gets there: a run of an arm64 image on an amd64 host
//! executes its payload, and each probe inside it, under this emulator.
//!
//! ## What a probe can detect, measured
//!
//! Measured with the tools of an arm64 rootfs under `qemu-aarch64-static`
//! on an amd64 host:
//!
//! | asked | answer under the emulator | usable? |
//! | --- | --- | --- |
//! | `/proc/cpuinfo` | `model name: ARMv8 Processor rev 0 (v8l)` | no, emulated |
//! | `readlink /proc/self/exe` | the guest binary's own path | no, emulated |
//! | `/proc/self/maps` | guest mappings only; qemu's own are filtered out | no |
//! | `ls /proc/sys/fs/binfmt_misc` | the **host's** registrations | yes |
//!
//! **So the answer is conditional, and it says so.** The crate cannot prove
//! that it runs natively. It can find that this machine routes binaries of
//! its own architecture through an interpreter, which is what
//! [`Interpreter::Emulated`] reports. The other state is
//! [`Interpreter::NoEvidence`] and it carries what was checked, because "no
//! evidence of an emulator" and "measured natively" are different sentences.
//!
//! The crate reads no environment variable: the caller gives a function
//! that tells which of [`QEMU_VARS`] are set.

use crate::binfmt;

/// The environment variables that `qemu-user` reads. Their presence is
/// evidence of an explicit `qemu-<arch> ./binary`, which leaves no
/// `binfmt_misc` trace at all because no registration was involved.
pub const QEMU_VARS: &[&str] =
    &["QEMU_LD_PREFIX", "QEMU_CPU", "QEMU_GUEST_BASE", "QEMU_UNAME", "QEMU_STACK_SIZE", "QEMU_LOG", "QEMU_STRACE"];

/// Which instrument produced a measurement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Interpreter {
    /// Nothing found. **Absence of evidence, not evidence of absence**, so
    /// `checked` names what was asked.
    NoEvidence { checked: Vec<String> },
    /// This machine hands binaries of this crate's own architecture to
    /// `interpreter`, so a measurement taken here is very likely its.
    Emulated { interpreter: String, evidence: String },
}

impl Interpreter {
    pub fn is_emulated(&self) -> bool {
        matches!(self, Interpreter::Emulated { .. })
    }

    /// The one word that goes into the cache key and into `--json`.
    ///
    /// **`none` is not a claim of nativeness.** It is the absence of an
    /// interpreter that the crate could find, and it keys the cache so that an answer
    /// measured under an emulator can never be served to one that was not.
    pub fn key(&self) -> String {
        match self {
            Interpreter::NoEvidence { .. } => "none".to_string(),
            Interpreter::Emulated { interpreter, .. } => interpreter.clone(),
        }
    }

    /// One line for the evidence block, or `None` where there is nothing to say
    /// beyond what every other line already says.
    pub fn note(&self) -> Option<String> {
        match self {
            Interpreter::NoEvidence { .. } => None,
            Interpreter::Emulated { interpreter, evidence } => Some(format!(
                "refused: MEASURED BY {interpreter}, NOT BY THIS MACHINE. {evidence}. Every \
                 row above is that emulator's answer: it answers for the syscalls it \
                 implements and reports its own confinement, whatever this host's is"
            )),
        }
    }
}

/// Ask whether an emulator is in the way.
///
/// Two checks, in this order, because they cover two different routes in and
/// only one of them leaves a `binfmt_misc` trace.
///
/// `is_set` tells whether a variable of the environment of the process is
/// set, for example `|v| std::env::var_os(v).is_some()`.
pub fn detect(is_set: &dyn Fn(&str) -> bool) -> Interpreter {
    let mut checked = Vec::new();

    // 1. An explicit `qemu-<arch> ./binary`. No registration is involved, so
    //    binfmt_misc says nothing; what qemu does leave is its own environment.
    let set: Vec<&str> = QEMU_VARS.iter().copied().filter(|v| is_set(v)).collect();
    if !set.is_empty() {
        return Interpreter::Emulated {
            interpreter: format!("a qemu-user emulator (named by ${})", set.join(", $")),
            evidence: format!(
                "the environment carries {}, which qemu-user reads and nothing else sets",
                set.join(", ")
            ),
        };
    }
    checked.push(format!("no qemu-user environment variable is set ({})", QEMU_VARS.join(", ")));

    // 2. The binfmt_misc route, which is how a run of a foreign image gets
    //    there, and therefore the one that matters in practice.
    //    This file is the HOST's and qemu does not emulate it.
    if !std::path::Path::new(binfmt::BINFMT_DIR).is_dir() {
        checked.push(format!(
            "{} is not mounted, so this kernel routes nothing through an interpreter \
             and nothing tells what it would route",
            binfmt::BINFMT_DIR
        ));
        return Interpreter::NoEvidence { checked };
    }
    let all = binfmt::registrations();
    for r in &all {
        if r.machine == Some(binfmt::SELF_MACHINE) {
            return Interpreter::Emulated {
                interpreter: r.interpreter.clone(),
                evidence: format!(
                    "the enabled binfmt_misc registration {:?} selects ELF machine \
                     0x{:x}, which is this binary's own, so this kernel hands binaries \
                     like this one to that interpreter",
                    r.name,
                    binfmt::SELF_MACHINE
                ),
            };
        }
    }
    checked.push(format!(
        "none of the {} enabled binfmt_misc registration(s) selects ELF machine 0x{:x}, \
         which is this binary's own",
        all.len(),
        binfmt::SELF_MACHINE
    ));
    Interpreter::NoEvidence { checked }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whatever this machine is, `detect` answers one of two states and never
    /// panics, and both carry their reason. A test that demanded one of them
    /// would be a test about the machine.
    #[test]
    fn detect_answers_with_its_evidence_either_way() {
        match detect(&|v| std::env::var_os(v).is_some()) {
            Interpreter::NoEvidence { checked } => {
                assert!(!checked.is_empty(), "no evidence, and nothing was checked");
                assert!(checked.iter().all(|c| !c.is_empty()));
            }
            Interpreter::Emulated { interpreter, evidence } => {
                assert!(!interpreter.is_empty());
                assert!(!evidence.is_empty());
            }
        }
    }

    /// The key is what stops one instrument's answer being served to
    /// another, so the two states must never produce the same one.
    #[test]
    fn the_two_states_never_key_alike() {
        let native = Interpreter::NoEvidence { checked: vec!["x".into()] };
        let qemu = Interpreter::Emulated { interpreter: "/usr/bin/qemu-aarch64-static".into(), evidence: "e".into() };
        assert_ne!(native.key(), qemu.key());
        assert!(native.note().is_none(), "no evidence is not a claim");
        assert!(qemu.note().unwrap().contains("NOT BY THIS MACHINE"));
    }

    /// The variable route is measurable here without an emulator, because it
    /// is only an environment read. The test gives the environment: it sets
    /// no variable of the process.
    #[test]
    fn an_explicit_qemu_invocation_is_seen_through_its_environment() {
        let var = "QEMU_STRACE";
        let got = detect(&|v| v == var);
        match got {
            Interpreter::Emulated { evidence, .. } => assert!(evidence.contains(var), "{evidence}"),
            other => panic!("{other:?}"),
        }
    }
}
