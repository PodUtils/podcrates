//! The probe set: a minimum set of operations, and the Go reference
//! instrument that runs it (`verification/probe/main.go` and
//! `attribute.go` of container-research).
//!
//! The row names are the names of the reference, character for character,
//! so that a run of this crate and a `probe census` / `probe attribute` run
//! diff against each other.
//!
//! Four divergences from the Go instrument, each deliberate and each a defect
//! in it rather than a difference of taste:
//!
//! 1. **The bare `mount(MS_SLAVE,/)` row is not ported.** In the caller's
//!    mount namespace it changes the propagation of `/` and everything under
//!    it, for the machine, permanently. The minimum set names
//!    `mount(tmpfs)` in the current namespace and inside
//!    `clone(CLONE_NEWNS)`; it does not name that one. The `MS_SLAVE`
//!    operation is still measured, inside a private mount namespace where it
//!    cannot escape, which is also where bubblewrap does it.
//! 2. **A probe that mounts, unmounts.** `mount(tmpfs,/mnt)` and
//!    `move_mount(-> /tmp/mm-probe)` succeed on an unconfined host, and the Go
//!    instrument leaves both mounted. A failure to remove one is reported in
//!    the row rather than left silent.
//! 3. **Nothing is overwritten.** Every scratch file is created `O_EXCL`, and
//!    an existing file at that path is a `skip` naming the file, not a probe
//!    that deletes somebody's data to make room for itself.
//! 4. **A failed precondition is a `skip`, never the row's denial.** Where
//!    `fsmount` fails, the Go instrument reports its errno as
//!    `move_mount`'s verdict. That is a defect: move_mount was not measured, so no verdict about it was established.

use crate::sys::{self, CBuf, Errno};
use crate::verdict::Outcome;

/// How a probe is run.
#[derive(Clone, Copy)]
pub enum Kind {
    /// The measurement is `clone(2)` with these flags: was the child built?
    /// The child does nothing but exit.
    Clone(u64),
    /// Run the body in a re-executed child, which is additionally placed in
    /// the namespaces named by the flags. A clone that the flags make fail is
    /// a failed precondition, so the row is a `skip` carrying its errno.
    Child { ns_flags: u64, body: fn() -> Outcome },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// `probe census`: what this machine permits.
    Census,
    /// `probe attribute`: the bogus-argument discriminator and its controls.
    Attribution,
    /// The machine tier's legs: one leg per fact and
    /// a verdict per leg, each measured, never inferred from another leg.
    Machine,
    /// The supervise tier's legs: the tier's own rows, not the census and
    /// attribution rows. A borrowed row answers its own group's contract; a tier leg
    /// answers the tier, so a rename moves the block, the constant and the
    /// assessment together, like the machine legs above.
    Supervise,
}

pub struct Probe {
    pub name: &'static str,
    pub group: Group,
    pub kind: Kind,
}

const NS: u64 = sys::CLONE_NEWNS;
const UTS: u64 = sys::CLONE_NEWUTS;

/// Fixed order. Two runs of this set are diffable only if the rows come out
/// in the same sequence, which is the same reason the Go instrument carries an
/// `order` array beside its map.
///
/// `rustfmt` is turned off for this item and only this item. It is a table,
/// one row per probe, and the default formatting expands each row to four
/// lines: the set stops being readable as a set, which is the one thing this
/// declaration is for.
#[rustfmt::skip]
pub static PROBES: &[Probe] = &[
    // ------------------------------------------------------------- census
    Probe { name: "unshare(CLONE_NEWNS)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_unshare_newns } },
    Probe { name: "unshare(CLONE_NEWUSER)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_unshare_newuser } },
    Probe { name: "unshare(CLONE_NEWPID)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_unshare_newpid } },
    Probe { name: "clone(CLONE_NEWNS)", group: Group::Census, kind: Kind::Clone(NS) },
    Probe { name: "clone(CLONE_NEWUTS|NEWNS)", group: Group::Census, kind: Kind::Clone(UTS | NS) },
    Probe { name: "clone(CLONE_NEWUSER)", group: Group::Census,
            kind: Kind::Clone(sys::CLONE_NEWUSER) },
    // Named by the minimum set and by the banner of a caller, and absent
    // from the Go instrument. The UTS namespace is the
    // one the `chroot` rung can still use, so the banner's `namespaces=` field
    // is a measurement rather than an assumption.
    Probe { name: "clone(CLONE_NEWUTS)", group: Group::Census, kind: Kind::Clone(UTS) },
    Probe { name: "sethostname(in NEWUTS)", group: Group::Census,
            kind: Kind::Child { ns_flags: UTS, body: p_sethostname } },
    Probe { name: "mount(tmpfs,/mnt)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_mount_tmpfs } },
    Probe { name: "mount(tmpfs,/mnt) in clone(NEWNS)", group: Group::Census,
            kind: Kind::Child { ns_flags: NS, body: p_mount_tmpfs } },
    Probe { name: "mount(MS_SLAVE,/) in clone(NEWNS)", group: Group::Census,
            kind: Kind::Child { ns_flags: NS, body: p_mount_slave } },
    Probe { name: "pivot_root(/tmp,/tmp)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_pivot_root } },
    Probe { name: "ptrace(PTRACE_TRACEME)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_ptrace } },
    // The mknod pair. Both, in the same directory: a
    // whiteout that succeeds where a real device number fails proves the
    // denial is capability-based, because no path-scoped policy can tell two
    // device numbers apart at one path.
    Probe { name: "mknod(chr 1:3 /tmp/nodprobe)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_mknod_real } },
    Probe { name: "mknod(chr 0:0 = whiteout)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_mknod_whiteout } },
    Probe { name: "setuid(1000)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_setuid_1000 } },
    Probe { name: "setuid(0)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_setuid_0 } },
    Probe { name: "setgid(0)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_setgid_0 } },
    Probe { name: "setgroups(0,NULL)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_setgroups } },
    Probe { name: "chown(f,0,0)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_chown_0_0 } },
    Probe { name: "chown(f,0,42)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_chown_0_42 } },
    Probe { name: "lchown(f,0,42)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_lchown_0_42 } },
    Probe { name: "chown(f,1000,0)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_chown_1000_0 } },
    Probe { name: "chroot(/tmp)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_chroot } },
    Probe { name: "memfd_create", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_memfd } },
    Probe { name: "memfd_create+exec", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_memfd_exec } },
    Probe { name: "setsid", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_setsid } },
    Probe { name: "prctl(PR_SET_PDEATHSIG)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_pdeathsig } },
    Probe { name: "exec(/tmp/execprobe)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_exec_file } },
    Probe { name: "getrandom", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_getrandom } },
    Probe { name: "write(/etc/probe)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_write_etc } },
    Probe { name: "write into uid-1000-owned dir", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_write_squash } },
    // These two are in THIS set, in the OUTER environment, before any
    // chroot. A pty (`-t`) either works or it does not: the mount table of
    // the confined target shows six device nodes and no `ptmx`, and a mount
    // table does not list plain files, so one stat settles it. The open is
    // the second row because existence is not function: probe the operation
    // that you need.
    Probe { name: "stat(/dev/ptmx)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_stat_ptmx } },
    Probe { name: "open(/dev/ptmx, O_RDWR)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_open_ptmx } },
    // The input of a FUSE rung. Opened, not stat-ed, like the ptmx leg: a node that exists and answers
    // EACCES on open cannot carry the tier. In the outer environment before
    // any chroot, like the ptmx pair above it.
    Probe { name: "open(/dev/fuse, O_RDWR)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_open_fuse } },
    // In the OUTER environment like the ptmx pair: list the root, open an entry in it by name, create a file at
    // its top level. Each leg is its own verdict with its own errno.
    Probe { name: "readdir(/)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_list_root } },
    Probe { name: "open(/bin, O_RDONLY) by name", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_open_by_name } },
    Probe { name: "creat(/, O_CREAT|O_EXCL)", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_creat_toplevel } },
    // A confined target answers EPERM to each TCP
    // bind, loopback or wildcard, which kills every hostfwd-based manager.
    // A census row because it asks what this machine permits, like the
    // ptmx pair above it.
    Probe { name: "bind(127.0.0.1:0)+listen", group: Group::Census,
            kind: Kind::Child { ns_flags: 0, body: p_tcp_listen } },

    // -------------------------------------------------------- attribution
    // A seccomp filter sees the syscall number and six
    // argument registers, cannot dereference a pointer, and runs before the
    // syscall body. An argument the kernel rejects INSIDE the body therefore
    // separates a filter from a policy that runs later.
    Probe { name: "mount(2) bogus target", group: Group::Attribution,
            kind: Kind::Child { ns_flags: 0, body: a_mount_bogus } },
    Probe { name: "umount2(2) bogus target", group: Group::Attribution,
            kind: Kind::Child { ns_flags: 0, body: a_umount_bogus } },
    Probe { name: "pivot_root(2) bogus paths", group: Group::Attribution,
            kind: Kind::Child { ns_flags: 0, body: a_pivot_bogus } },
    Probe { name: "process_vm_readv(bogus pid)", group: Group::Attribution,
            kind: Kind::Child { ns_flags: 0, body: a_process_vm_readv } },
    // The controls travel with the discriminator, in the same run. A control
    // that ran at a different time answers about a different machine state.
    Probe { name: "pidfd_getfd(-1,-1) [control]", group: Group::Attribution,
            kind: Kind::Child { ns_flags: 0, body: a_pidfd_getfd } },
    Probe { name: "kcmp(-1,-1,...) [control]", group: Group::Attribution,
            kind: Kind::Child { ns_flags: 0, body: a_kcmp } },
    Probe { name: "fsopen(tmpfs)", group: Group::Attribution,
            kind: Kind::Child { ns_flags: 0, body: a_fsopen } },
    Probe { name: "fsmount(tmpfs)", group: Group::Attribution,
            kind: Kind::Child { ns_flags: 0, body: a_fsmount } },
    Probe { name: "open_tree(/tmp, CLONE)", group: Group::Attribution,
            kind: Kind::Child { ns_flags: 0, body: a_open_tree } },
    Probe { name: "move_mount(-> bogus dest)", group: Group::Attribution,
            kind: Kind::Child { ns_flags: 0, body: a_move_mount_bogus } },
    Probe { name: "move_mount(-> /tmp/mm-probe)", group: Group::Attribution,
            kind: Kind::Child { ns_flags: 0, body: a_move_mount_real } },
    Probe { name: "openat(detached tmpfs, O_DIRECTORY)", group: Group::Attribution,
            kind: Kind::Child { ns_flags: 0, body: a_openat_detached } },
    Probe { name: "seccomp(NEW_LISTENER)", group: Group::Attribution,
            kind: Kind::Child { ns_flags: 0, body: a_seccomp_listener } },
    Probe { name: "open /proc/self/mem O_RDONLY", group: Group::Attribution,
            kind: Kind::Child { ns_flags: 0, body: a_mem_rdonly } },
    Probe { name: "open /proc/self/mem O_RDWR", group: Group::Attribution,
            kind: Kind::Child { ns_flags: 0, body: a_mem_rdwr } },
    Probe { name: "landlock_create_ruleset(VERSION)", group: Group::Attribution,
            kind: Kind::Child { ns_flags: 0, body: a_landlock } },

    // ------------------------------------------------------------ machine
    // One leg per fact the machine tier needs, each
    // measured by the operation itself: the emulator by running it, the two
    // device nodes by opening them, the file-size bound by reading it, the
    // image space by statfs, the accelerator by asking the emulator.
    // Never infer a leg from another leg. A leg whose precondition is
    // another leg's subject says which one and stops, like every other
    // precondition in this file.
    Probe { name: "qemu-system-x86_64 --version", group: Group::Machine,
            kind: Kind::Child { ns_flags: 0, body: m_qemu_version } },
    Probe { name: "open(/dev/kvm, O_RDWR)", group: Group::Machine,
            kind: Kind::Child { ns_flags: 0, body: m_kvm } },
    Probe { name: "prlimit(RLIMIT_FSIZE)", group: Group::Machine,
            kind: Kind::Child { ns_flags: 0, body: m_fsize } },
    Probe { name: "open(/dev/net/tun, O_RDWR)", group: Group::Machine,
            kind: Kind::Child { ns_flags: 0, body: m_tun } },
    Probe { name: "image space (statfs .)", group: Group::Machine,
            kind: Kind::Child { ns_flags: 0, body: m_space } },
    Probe { name: "qemu-system-x86_64 -accel help", group: Group::Machine,
            kind: Kind::Child { ns_flags: 0, body: m_accel } },

    // ---------------------------------------------------------- supervise
    // The supervise tier's own three legs, each
    // measured by the operation itself: the notification listener by
    // creating one, the argument channel by copying through it, the
    // race-safety by asking for it. Never infer a leg from another leg,
    // and never borrow a row that answers another group's contract.
    // The listener and ptrace rows share their bodies with the census and
    // attribution rows that run the same operation: one implementation, two
    // readings, so the operation cannot drift between the groups.
    Probe { name: "seccomp(NEW_LISTENER) [supervise]", group: Group::Supervise,
            kind: Kind::Child { ns_flags: 0, body: a_seccomp_listener } },
    Probe { name: "process_vm_readv(own pid) [supervise]", group: Group::Supervise,
            kind: Kind::Child { ns_flags: 0, body: s_read_channel } },
    Probe { name: "ptrace(PTRACE_TRACEME) [supervise]", group: Group::Supervise,
            kind: Kind::Child { ns_flags: 0, body: p_ptrace } },
    // Supervision's own two legs beside mediation's three: the lifecycle
    // tracks each direct child through a pidfd and reaps it through
    // waitid(P_PIDFD), so these measure exactly that pair. A leg for a
    // thing that the launcher does not use would green-light a mechanism
    // with no consumer (for example ADDFD).
    Probe { name: "pidfd_open(own pid) [supervise]", group: Group::Supervise,
            kind: Kind::Child { ns_flags: 0, body: s_pidfd_self } },
    Probe { name: "waitid(P_PIDFD, child) [supervise]", group: Group::Supervise,
            kind: Kind::Child { ns_flags: 0, body: s_waitid_child } },
];

/// The machine tier's legs, in the order they run. [`crate::machine`] and the
/// report read the verdicts through these names, so a rename moves all three.
pub const MACHINE_LEGS: &[&str] = &[
    "qemu-system-x86_64 --version",
    "open(/dev/kvm, O_RDWR)",
    "prlimit(RLIMIT_FSIZE)",
    "open(/dev/net/tun, O_RDWR)",
    "image space (statfs .)",
    "qemu-system-x86_64 -accel help",
];

/// The supervise tier's legs, in the order they run. [`crate::supervise`],
/// the report and the selection read the verdicts through these names, so a
/// rename moves all four.
pub const SUPERVISE_LEGS: &[&str] = &[
    "seccomp(NEW_LISTENER) [supervise]",
    "process_vm_readv(own pid) [supervise]",
    "ptrace(PTRACE_TRACEME) [supervise]",
];

/// Supervision's legs beside mediation's: the pair the lifecycle runs on.
/// Read through these names by [`crate::supervise::assess_supervision`],
/// so a rename moves the rows and the reader together.
pub const SUPERVISION_LEGS: &[&str] = &["pidfd_open(own pid) [supervise]", "waitid(P_PIDFD, child) [supervise]"];

pub fn find(name: &str) -> Option<&'static Probe> {
    PROBES.iter().find(|p| p.name == name)
}

// ---------------------------------------------------------------- helpers

/// A path that cannot exist. A syscall that resolves paths and is not filtered
/// must answer `ENOENT` for it.
const BOGUS: &str = "/proc/self/nonexistent-probe-path";

fn c(s: &str) -> CBuf {
    // Every literal below is NUL-free, so this cannot fail. Constructing it
    // through the checked path anyway keeps one constructor.
    CBuf::new(s).expect("probe path literals carry no interior NUL")
}

fn exists(path: &str) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// Create a scratch file the probe owns.
///
/// `O_EXCL`. A probe that deletes whatever is in its way has destroyed data
/// to measure a permission, and the errno it then reports is about a file it
/// created rather than the one that was there.
fn make_scratch(path: &str) -> Result<(), Outcome> {
    match create_exclusive(path, 0o600) {
        Ok(fd) => {
            let _ = sys::close(fd);
            Ok(())
        }
        Err(sys::EEXIST) => Err(already_there(path)),
        Err(e) => Err(Outcome::skip(Some(e), format!("could not create the scratch file {path}"))),
    }
}

fn create_exclusive(path: &str, mode: u64) -> Result<i64, Errno> {
    sys::open(&c(path), sys::O_WRONLY | sys::O_CREAT | sys::O_EXCL, mode)
}

fn already_there(path: &str) -> Outcome {
    Outcome::skip(
        Some(sys::EEXIST),
        format!("{path} already exists and this probe does not overwrite; remove it and re-run"),
    )
}

fn unscratch(path: &str) {
    let _ = sys::unlink(&c(path));
}

/// `/bin/sh` is the interpreter both exec probes need. Its absence is a
/// missing precondition, and the `ENOENT` it would otherwise produce reads
/// exactly like a denial.
fn need_sh() -> Result<(), Outcome> {
    if exists("/bin/sh") {
        Ok(())
    } else {
        Err(Outcome::skip(
            Some(sys::ENOENT),
            "/bin/sh is absent, so there is no interpreter for the payload this \
             probe executes; the exec was not attempted",
        ))
    }
}

/// Run a path as a child process and report what happened to it.
///
/// Through [`sys::shed_after_fork`], because here a probe starts a process
/// with libstd and not with `sys::clone_fork`, and a probe makes one new
/// child for each probe. With no hook, each lock that another thread holds
/// is duplicated into this child at the `fork` and outlives the release of
/// its holder.
fn run_payload(path: &str) -> Outcome {
    let mut cmd = std::process::Command::new(path);
    match sys::shed_after_fork(&mut cmd).status() {
        Ok(st) if st.success() => Outcome::ok(),
        Ok(st) => Outcome::skip(None, format!("the payload {path} ran and ended {st}; the exec itself succeeded")),
        Err(e) => match e.raw_os_error() {
            Some(n) => Outcome::denied(Errno(n)),
            None => Outcome::skip(None, format!("spawning {path} failed: {e}")),
        },
    }
}

/// Which of the three calls behind a detached mount answered.
///
/// Named, not compared as a string: creation, configuration and attachment
/// are separate verdicts, and a
/// caller cannot tell them apart from one errno.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Fsopen,
    Fsconfig,
    Fsmount,
}

impl Step {
    fn label(self) -> &'static str {
        match self {
            Step::Fsopen => "fsopen(tmpfs)",
            Step::Fsconfig => "fsconfig(FSCONFIG_CMD_CREATE)",
            Step::Fsmount => "fsmount(tmpfs)",
        }
    }
}

/// A detached tmpfs mount fd, or the step that failed and its errno.
fn detached_tmpfs() -> Result<i64, (Errno, Step)> {
    let fs = c("tmpfs");
    let fd = unsafe { sys::sys(sys::SYS_FSOPEN, [fs.ptr(), 0, 0, 0, 0, 0]) }.map_err(|e| (e, Step::Fsopen))?;
    let r = unsafe { sys::sys(sys::SYS_FSCONFIG, [fd as u64, sys::FSCONFIG_CMD_CREATE, 0, 0, 0, 0]) };
    if let Err(e) = r {
        let _ = sys::close(fd);
        return Err((e, Step::Fsconfig));
    }
    let mfd = unsafe { sys::sys(sys::SYS_FSMOUNT, [fd as u64, 0, 0, 0, 0, 0]) };
    let _ = sys::close(fd);
    mfd.map_err(|e| (e, Step::Fsmount))
}

fn precondition(step: &str, e: Errno) -> Outcome {
    Outcome::skip(Some(e), format!("{step} failed with {} ({}), so this operation was never attempted", e.name(), e.0))
}

mod census;
use census::*;
mod access;
pub use access::*;
mod attribution;
use attribution::*;

mod machine;
use machine::*;

#[cfg(test)]
mod tests;
