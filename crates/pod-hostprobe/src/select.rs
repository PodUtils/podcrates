//! Mode selection, and one switch that turns each degradation into a
//! refusal.
//!
//! **A weaker mode must never satisfy a stronger request.** A user who
//! believes they have namespaces when they have a `chroot` is worse off than
//! one who is told the truth, and an automated user cannot notice the
//! difference the way a human skimming a log might.
//!
//! The rung is derived from the probe results, never from a privilege that
//! usually implies them. Two mechanisms come from other projects, and the
//! architecture of neither:
//!
//! - ruri (`src/include/ruri.h`): `ruri_warn_on_error` is non-fatal by
//!   default and, under
//!   `ruri_flag(force_panic)`, warns and then exits at the same site. **One
//!   switch converts every degradation into a refusal.** [`Strictness`] is
//!   that switch. `ruri`'s `show` argument is
//!   `!ruri_flag(disable_warnings)`, so a flag can silence its degradation
//!   notices; a caller suppresses its banner by configuration and never by
//!   default, because a silenced degradation is `sandlock`'s failure mode with
//!   a flag in front of it.
//! - udocker (`udocker/engine/execmode.py`): `get_mode()`'s precedence is force, then the per-container file, then a
//!   config override, then a per-architecture default, then `DEFAULT`.
//!   [`Selection::choose`] is the same precedence with the probe as its source.

use crate::Findings;
use crate::probes::Group;
use crate::verdict::{Outcome, Verdict};

/// The ladder of modes, strongest first. The order is the
/// comparison: `Rung::Namespace < Rung::Chroot` is what "a weaker mode must
/// never satisfy a stronger request" is enforced with.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Rung {
    Namespace,
    Supervise,
    Chroot,
    /// Entered without `chroot(2)`: the payload runs with the host's root,
    /// its working directory inside the image. Weaker than chroot, which
    /// is what the order says.
    Userland,
    Interpose,
    Unsupported,
}

impl Rung {
    pub fn word(self) -> &'static str {
        match self {
            Rung::Namespace => "namespace",
            Rung::Supervise => "supervise",
            Rung::Chroot => "chroot",
            Rung::Userland => "userland",
            Rung::Interpose => "interpose",
            Rung::Unsupported => "unsupported",
        }
    }

    /// What the rung must never claim.
    pub fn must_never_claim(self) -> &'static str {
        match self {
            // The rung is mount-only, so this
            // guards what it must never imply: user, pid or network
            // isolation, or a /proc it does not mount.
            Rung::Namespace => "user, pid or network isolation, or a /proc filesystem",
            Rung::Supervise => "anything whose arguments it cannot read; on this runtime, exec remapping",
            Rung::Chroot => "process, network, IPC or mount isolation, or a /proc filesystem",
            Rung::Userland => {
                "isolation of any kind, a private root, or host-absolute paths resolving inside the image"
            }
            Rung::Interpose => "any security property whatsoever",
            Rung::Unsupported => "",
        }
    }
}

/// `ruri`'s inversion, as one switch. Under [`Strictness::Refuse`] a rung
/// below the requested floor is an error rather than a notice.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Strictness {
    /// Report the rung achieved and continue. The banner still names every
    /// degradation; it is never silent.
    Warn,
    /// Refuse anything below the floor.
    Refuse,
}

/// Why a rung was not reached. One per rung considered and rejected, so the
/// diagnostic names the unmet requirement rather than the conclusion.
pub struct Rejection {
    pub rung: Rung,
    pub requirement: &'static str,
    pub evidence: String,
}

pub struct Selection {
    pub rung: Rung,
    pub rejected: Vec<Rejection>,
    /// True only when every control in the attribution set answered. A
    /// probe whose control has stopped answering has stopped discriminating,
    /// and it must say so and not report a mechanism.
    pub controls_answered: bool,
    pub control_notes: Vec<String>,
}

/// The verdict of one row, by name, or `None` when the set carries no such row.
fn row<'a>(f: &'a Findings, name: &str) -> Option<&'a Outcome> {
    f.rows.iter().find(|(n, _)| *n == name).map(|(_, o)| o)
}

fn is(f: &Findings, name: &str, want: Verdict) -> bool {
    row(f, name).map(|o| o.verdict == want) == Some(true)
}

fn ok(f: &Findings, name: &str) -> bool {
    is(f, name, Verdict::Ok)
}

/// How a row reads in a rejection: the verdict and, for a denial, its errno.
fn evidence(f: &Findings, name: &str) -> String {
    match row(f, name) {
        None => format!("{name}=(not probed)"),
        Some(o) => match (o.verdict, o.errno) {
            (Verdict::Ok, _) => format!("{name}=ok"),
            (Verdict::Denied, Some(e)) => format!("{name}={}", e.name()),
            (Verdict::Denied, None) => format!("{name}=denied"),
            (Verdict::Skip, _) => format!("{name}=skip"),
        },
    }
}

fn evidence_of(f: &Findings, names: &[&str]) -> String {
    names.iter().map(|n| evidence(f, n)).collect::<Vec<_>>().join(" ")
}

impl Selection {
    /// Derive the rung. Each rung's requirement is the **operation**, never
    /// a privilege that usually implies it.
    pub fn choose(f: &Findings) -> Selection {
        let mut rejected = Vec::new();

        // -- namespace: namespace creation AND mounts AND ID maps -----------
        //
        // "mounts" is the attach half and not the create half. A machine where
        // fsmount succeeds and move_mount is denied has detached mounts it
        // cannot use.
        let ns_created = ok(f, "unshare(CLONE_NEWNS)") || ok(f, "clone(CLONE_NEWNS)");
        let mounts = ok(f, "mount(tmpfs,/mnt)") || ok(f, "mount(tmpfs,/mnt) in clone(NEWNS)");
        if ns_created && mounts {
            return Selection { rung: Rung::Namespace, rejected, ..Selection::controls(f) };
        }
        rejected.push(Rejection {
            rung: Rung::Namespace,
            requirement: "namespace creation and mounts and ID maps for what was asked",
            evidence: evidence_of(
                f,
                &[
                    "unshare(CLONE_NEWNS)",
                    "clone(CLONE_NEWNS)",
                    "mount(tmpfs,/mnt)",
                    "mount(tmpfs,/mnt) in clone(NEWNS)",
                ],
            ),
        });

        // -- supervise: three legs, and the tier is refused if any is missing
        //
        // Never fall back per call. The third leg comes from sandlock issue
        // #27: the only known fix for
        // seccomp-notify's TOCTOU race needs PTRACE_SEIZE on every thread of
        // every process in the sandbox before a Continue, so a runtime where
        // ptrace is filtered has no way to make the tier race-safe.
        // The legs are the tier's own rows (`Group::Supervise`), never the
        // census and attribution rows that answer other groups' contracts.
        let listener = ok(f, "seccomp(NEW_LISTENER) [supervise]");
        let arg_channel = ok(f, "process_vm_readv(own pid) [supervise]");
        let race_safe = ok(f, "ptrace(PTRACE_TRACEME) [supervise]");
        if listener && arg_channel && race_safe {
            return Selection { rung: Rung::Supervise, rejected, ..Selection::controls(f) };
        }
        rejected.push(Rejection {
            rung: Rung::Supervise,
            requirement: "a notification listener, a channel to read the child's syscall \
                          arguments, and ptrace to make a Continue race-safe",
            evidence: evidence_of(
                f,
                &[
                    "seccomp(NEW_LISTENER) [supervise]",
                    "process_vm_readv(own pid) [supervise]",
                    "ptrace(PTRACE_TRACEME) [supervise]",
                ],
            ),
        });

        // -- chroot ---------------------------------------------------------
        if ok(f, "chroot(/tmp)") {
            return Selection { rung: Rung::Chroot, rejected, ..Selection::controls(f) };
        }
        rejected.push(Rejection {
            rung: Rung::Chroot,
            requirement: "CAP_SYS_CHROOT, a prepared rootfs, payload syscalls permitted",
            evidence: evidence_of(f, &["chroot(/tmp)"]),
        });

        // -- interpose ------------------------------------------------------
        //
        // The rung needs a dynamically linked payload and sufficient libc
        // coverage, neither of which exists until there is a payload. What the
        // probe can establish is that this machine can execute one at all.
        if ok(f, "exec(/tmp/execprobe)") || ok(f, "memfd_create+exec") {
            return Selection { rung: Rung::Interpose, rejected, ..Selection::controls(f) };
        }
        rejected.push(Rejection {
            rung: Rung::Interpose,
            requirement: "a payload this machine can execute at all",
            evidence: evidence_of(f, &["exec(/tmp/execprobe)", "memfd_create+exec"]),
        });

        Selection { rung: Rung::Unsupported, rejected, ..Selection::controls(f) }
    }

    /// The discipline of the discriminator, evaluated over the controls that
    /// travelled in this same run.
    fn controls(f: &Findings) -> Selection {
        let mut notes = Vec::new();
        let mut answered = true;
        for (name, want) in
            [("pidfd_getfd(-1,-1) [control]", crate::sys::EBADF), ("kcmp(-1,-1,...) [control]", crate::sys::ESRCH)]
        {
            match row(f, name) {
                None => {
                    answered = false;
                    notes.push(format!("{name} did not run"));
                }
                Some(o) if o.verdict == Verdict::Skip => {
                    answered = false;
                    notes.push(format!("{name} could not answer: {}", o.reason));
                }
                Some(o) if o.errno == Some(want) => {}
                Some(o) => {
                    answered = false;
                    notes.push(format!(
                        "{name} answered {} where the discriminator needs {}; \
                         it has stopped separating a filter from a policy",
                        o.errno.map(|e| e.name()).unwrap_or_else(|| "ok".into()),
                        want.name()
                    ));
                }
            }
        }
        Selection { rung: Rung::Unsupported, rejected: Vec::new(), controls_answered: answered, control_notes: notes }
    }

    /// The floor `--strict` holds to: nothing below `namespace` satisfies a
    /// request that asked for isolation.
    pub const STRICT_FLOOR: Rung = Rung::Namespace;

    pub fn meets(&self, floor: Rung) -> bool {
        self.rung <= floor
    }

    /// Exit code for a probe command: 0 always for a plain run, because the probe ran and answered; non-zero under `--strict`
    /// when the rung is below the floor, so a caller can gate without parsing.
    ///
    /// **This code carries the rung and only the rung.**
    /// [`Selection::controls_answered`] is a separate statement about the
    /// diagnostic and it does not move this number. Folding the two together
    /// would make `--strict` fail on every kernel built without
    /// `CONFIG_CHECKPOINT_RESTORE`, where `kcmp(2)` is absent and the host is
    /// otherwise fully capable, and a caller could no longer tell the two
    /// apart from one exit code. A caller reports `controls_answered` apart.
    pub fn exit_code(&self, strictness: Strictness) -> i32 {
        match strictness {
            Strictness::Warn => 0,
            Strictness::Refuse if self.meets(Self::STRICT_FLOOR) => 0,
            Strictness::Refuse => 1,
        }
    }
}

/// What the selected rung provides, derived from the probes rather than
/// asserted. The banner of a caller is these six fields.
pub struct Provides {
    pub namespaces: String,
    pub mounts: String,
    pub devices: String,
    pub ownership: String,
    pub network: String,
    pub pids: String,
}

impl Provides {
    pub fn of(rung: Rung, f: &Findings) -> Provides {
        // The namespace rung is mount-only: one mount namespace with a
        // private tmpfs on /tmp, and no user, pid or network namespace. The
        // cells state the rung, not the machine: "as configured" here can
        // pass a chroot off as namespaces.
        let namespaces = if rung == Rung::Namespace {
            "mount-only".to_string()
        } else if ok(f, "clone(CLONE_NEWUTS)") && ok(f, "sethostname(in NEWUTS)") {
            "uts-only".to_string()
        } else {
            "none".to_string()
        };
        // What the RUNG provides, not what the machine permits. Only the
        // `namespace` rung sets a mount topology up, so at every rung below it
        // the caller mounts nothing, however attachable this machine's mounts
        // turn out to be. Report the mode that you achieved. The
        // machine's own four verdicts are in the evidence block beneath, where
        // they are a measurement rather than a claim about the mode.
        //
        // The rung's topology is fixed (private, tmpfs on
        // `/tmp`), so the cell names it rather than the machine's mount
        // verdicts, which would read as the rung providing them.
        let mounts = if rung == Rung::Namespace { "private, tmpfs on /tmp".to_string() } else { "none".to_string() };
        let devices = if ok(f, "mknod(chr 1:3 /tmp/nodprobe)") { "real".to_string() } else { "shimmed".to_string() };
        let ownership = if ok(f, "chown(f,0,42)") { "real".to_string() } else { "virtualized+sidecar".to_string() };
        // Not measured, and stated as what the rung does rather than as a
        // reading. The namespace rung is mount-only: no network
        // or pid namespace is created at any rung, so both are always the
        // caller's.
        Provides {
            namespaces,
            mounts,
            devices,
            ownership,
            network: "host-shared".to_string(),
            pids: "host-shared".to_string(),
        }
    }
}

/// Every attribution row that has a control, for the report.
pub fn control_rows() -> impl Iterator<Item = &'static str> {
    crate::probes::PROBES
        .iter()
        .filter(|p| p.group == Group::Attribution && p.name.contains("[control]"))
        .map(|p| p.name)
}

#[cfg(test)]
mod tests;
