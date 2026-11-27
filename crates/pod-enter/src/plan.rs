//! What the payload will be: the argv, the environment, the working
//! directory, and the banner that says what the entry does and does not
//! do.
//!
//! docker's rules for the combination of the `Entrypoint` and the `Cmd` of
//! an image with the arguments of the caller are not obvious. They are
//! here as data, with tests, because a wrong rule runs a command that the
//! caller did not ask for, and the failure looks like a failure of the
//! image.

use crate::names::var_name;
use crate::{Fds, Names};

/// How the child looks for the program inside the new root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Program {
    /// The argument has a `/`, so it is a path and `PATH` is not used.
    Absolute(String),
    /// A bare name, to look for along `PATH`.
    OnPath(String),
}

pub struct Plan {
    pub argv: Vec<String>,
    pub env: Vec<String>,
    pub working_dir: String,
    pub fds: Fds,
    pub banner: String,
    /// The `PATH` along which the child looks for the program, split.
    pub path_dirs: Vec<String>,
    /// The names of the caller: the entry removes the mode variables from
    /// `env` and sets the active mode variable beside the exec.
    pub names: Names,
    /// The resource limits of the payload, set in its child before the
    /// exec ([`crate::limits`]); at most [`crate::limits::MOST`].
    pub limits: Vec<crate::Limit>,
    /// `PR_SET_NO_NEW_PRIVS` in the child before the exec.
    pub no_new_privs: bool,
    /// `PR_SET_PDEATHSIG(SIGKILL)` in the child, first after the fork: the
    /// payload ends with the thread that forked it, so a supervisor that is
    /// killed leaves no payload behind. Only for a caller that forks from a
    /// thread that lives as long as the payload (the launcher of
    /// `pod-supervise`): the signal fires at the exit of that thread.
    pub die_with_parent: bool,
    /// The confinement of the payload ([`pod_confine::Spec`]): compiled
    /// before the fork, applied in the child after the limits,
    /// `no_new_privs` and the root change, as the last step before the
    /// exec. Each rung with a chroot (the chroot sequence, the namespace
    /// rung, the ladder) takes guest paths; the entry with no chroot takes
    /// host paths. A host with no Landlock refuses the entry: the payload
    /// never runs unconfined.
    pub confine: Option<pod_confine::Spec>,
    /// The host name of the payload ([`crate::uts`]): the namespace rung
    /// enters a UTS namespace and sets it before the exec. Every other entry
    /// refuses a plan with a name, and the namespace rung does not fall back
    /// to chroot with one: the payload never sees the host's name instead.
    pub hostname: Option<String>,
    /// The volumes of the payload, bound in this order on the namespace
    /// rung ([`VolumeBind`], `ns_volume`). A later volume can be under an
    /// earlier one. A join of a run binds none: the mount namespace of the
    /// run holds them. Each other entry refuses a plan with a volume, and
    /// the namespace rung does not fall back to chroot with one: the
    /// payload never runs with a volume missing.
    pub volumes: Vec<VolumeBind>,
    /// The tmpfs mounts of the payload on the namespace rung, each with a
    /// byte limit ([`TmpfsMount`], `ns_tmpfs`), mounted in this order
    /// before the volumes. A join of a run mounts none: the mount namespace
    /// of the run holds them. Each other entry refuses a plan with one, and
    /// the namespace rung does not fall back to chroot with one.
    pub tmpfs: Vec<TmpfsMount>,
    /// The uid and the gid of the payload inside the user namespace of the
    /// namespace rung (`ns_user`); `None` is 0. The one-line maps then map
    /// the effective ids of the caller to these ids: the files of the
    /// caller show as owned by them, and the `execve` of a uid that is not
    /// 0 drops each capability of the payload. A caller that is uid 0 of a
    /// user namespace that is not the initial one gets them on each entry
    /// with a chroot through a nested user namespace (`<uid> 0 1`), after
    /// the mounts and before the root change. A host root (the initial
    /// namespace), a join of a run (the map of the run holds) and the entry
    /// with no chroot keep the ids of the caller. `u32::MAX` is refused
    /// before the fork: the kernel maps no such id.
    pub userns_ids: Option<(u32, u32)>,
    /// A PID namespace of the payload on the namespace rung (`ns_pid`): a
    /// fresh `proc` of it on `/proc`, the payload as pid 2 under an init
    /// of the entry, and a join of the run lands in it. Each other entry
    /// refuses the plan, and the namespace rung does not fall back to
    /// chroot with it: the payload never sees the processes of the host.
    pub pid_namespace: bool,
    /// The command line and the name that the init of [`Plan::pid_namespace`]
    /// and the relay of a join of such a run show in `/proc/PID`, with no
    /// environment ([`crate::title`]); `None` is the product of
    /// [`Plan::names`]. Each other entry reads none.
    pub relay_title: Option<String>,
    /// A read-only root of the payload on the namespace rung (`ns_root`):
    /// the root mount is remounted read-only after each mount of the rung,
    /// and `/dev` of the tree, each volume that is not `read_only`, each
    /// tmpfs and `/tmp` stay writable. A join of a run lands on the root of
    /// the run. Each other entry refuses the plan, and the namespace rung
    /// does not fall back to chroot with it: the payload never runs on a
    /// writable root.
    pub read_only_root: bool,
}

/// One volume of the namespace rung: a recursive bind of the host path
/// `host` on the guest path `guest` (`-v HOST:GUEST[:ro]`).
///
/// `host` is an absolute path that exists. A directory gives a directory
/// as the mount point, each other file an empty file. `guest` is an
/// absolute normal path that is not `/`; the child makes each missing
/// directory of it, and refuses a path through a symbolic link of the
/// image. `read_only` remounts the bind read-only and keeps each flag of
/// the mount that a user namespace locks. The mounts under `host` keep
/// their own flags, as docker's `ro`.
///
/// `host_base`: a directory that the caller checked, which holds `host`.
/// Then `host` opens under it with `openat2(RESOLVE_BENEATH |
/// RESOLVE_NO_SYMLINKS)`, before the fork and again in the child, and the
/// bind takes that descriptor, not the path: a link that the job of a unit
/// makes in the source after the check of the caller cannot redirect the
/// bind (PodUtils/podrunner#277). A link under the base is a refusal that
/// names the volume.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VolumeBind {
    pub host: String,
    pub guest: String,
    pub read_only: bool,
    pub host_base: Option<String>,
}

/// One tmpfs of the namespace rung: a file system in memory of at most
/// `bytes` bytes on the guest path `guest` (`--tmpfs GUEST:size=N`), with
/// `nosuid` and `nodev`.
///
/// `guest` is an absolute normal path that is not `/`, and not at or under
/// the guest path of a volume; the child makes each missing directory of
/// it, and refuses a path through a symbolic link of the image. The kernel
/// rounds `bytes` up to whole pages. After the mount the child reads the
/// size back, and a mount with no such limit stops the payload: the
/// payload never runs with no limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmpfsMount {
    pub guest: String,
    pub bytes: std::num::NonZeroU64,
}

impl Plan {
    /// The `Entrypoint` and the `Cmd` of the image, with the arguments of
    /// the caller.
    ///
    /// docker's rules:
    ///
    /// | image has | caller wrote | what runs |
    /// | --- | --- | --- |
    /// | `Entrypoint` | nothing | `Entrypoint` + `Cmd` |
    /// | `Entrypoint` | `args` | `Entrypoint` + `args`; `Cmd` is dropped |
    /// | no `Entrypoint` | nothing | `Cmd` |
    /// | no `Entrypoint` | `args` | `args`; `Cmd` is dropped |
    ///
    /// The second row: the `Cmd` of an image is the default arguments of
    /// its entrypoint, so arguments of the caller replace them. With both,
    /// `docker run img --help` would run the default command with `--help`
    /// added.
    pub fn argv_for(
        entrypoint: Option<&[String]>,
        cmd: Option<&[String]>,
        caller: &[String],
        override_entrypoint: Option<&str>,
    ) -> Vec<String> {
        // `--entrypoint` replaces the entrypoint of the image, and the
        // `Cmd` of the image goes too: its default arguments belonged to
        // the entrypoint that no longer runs.
        if let Some(e) = override_entrypoint {
            let mut v = vec![e.to_string()];
            v.extend_from_slice(caller);
            return v;
        }
        let mut v: Vec<String> = entrypoint.map(<[String]>::to_vec).unwrap_or_default();
        if caller.is_empty() {
            v.extend(cmd.map(<[String]>::to_vec).unwrap_or_default());
        } else {
            v.extend_from_slice(caller);
        }
        v
    }

    /// The `Env` of the image, then the variables of the caller, so the
    /// caller wins.
    ///
    /// A later value of a name replaces the earlier one; the earlier one is
    /// removed, not hidden. `execve` takes an array, not a map, and a name
    /// that is in it two times resolves to the entry that `getenv` finds
    /// first, which is not the same on glibc and musl. The entry decides
    /// here, so the answer does not depend on the C library of the payload.
    pub fn env_for(names: &Names, image: &[String], caller: &[String]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for e in image.iter().chain(caller.iter()) {
            let name = var_name(e);
            out.retain(|existing: &String| var_name(existing) != name);
            out.push(e.clone());
        }
        // The request variable and an old active claim go out of what the
        // image or the caller declared: the caller reads the request before
        // it builds the plan, and the entry sets the active claim beside
        // the exec ([`Plan::set_active_mode`]). Either one in the payload
        // would be a directive to a nested run.
        out.retain(|e| {
            let name = var_name(e);
            name != names.mode_request && name != names.mode_active
        });
        // docker's defaults where the image declares none. An image whose
        // `Env` has no `PATH` (`quay.io/rockylinux/rockylinux:9` declares
        // only `container=oci`) gets docker's `PATH`: with none, the shell
        // of the payload sets a `PATH` that it does not export, gcc then
        // finds no `PATH` to find its own directory, and each compile
        // fails to execute `cc1`.
        //
        // `HOSTNAME` is not added, as docker does: the entry has no UTS
        // namespace, so `gethostname(2)` gives the name of the host, and a
        // variable that disagrees with the system call is worse than no
        // variable.
        let has = |out: &[String], name: &str| out.iter().any(|e| var_name(e) == name);
        if !has(&out, "PATH") {
            out.push(format!("PATH={}", Self::DEFAULT_PATH));
        }
        if !has(&out, "HOME") {
            // docker takes `HOME` from the passwd entry of the user. With
            // no `Plan::userns_ids` the payload runs as uid 0, so this is
            // the value that docker gives for root. A caller that names
            // other ids gives the `HOME` of that user in `caller`.
            out.push("HOME=/root".to_string());
        }
        out
    }

    /// The `PATH` for an image with none: the value that POSIX names and
    /// that each base image sets. A banner can print the `PATH` that the
    /// entry used.
    pub const DEFAULT_PATH: &'static str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

    /// Set the active mode variable to the entered rung, and remove the
    /// request variable beside it.
    ///
    /// The payload sees the active mode and never the request, so a nested
    /// run starts with no request. An older active claim in the vector is
    /// replaced: only the rung that the entry enters can report itself.
    pub fn set_active_mode(names: &Names, env: &mut Vec<String>, entered: &str) {
        env.retain(|e| {
            let name = var_name(e);
            name != names.mode_request && name != names.mode_active
        });
        env.push(format!("{}={entered}", names.mode_active));
    }

    /// The directories of the last `PATH` in `env`, or of
    /// [`Plan::DEFAULT_PATH`].
    pub fn path_from(env: &[String]) -> Vec<String> {
        let raw = env.iter().rev().find_map(|e| e.strip_prefix("PATH=")).unwrap_or(Self::DEFAULT_PATH);
        raw.split(':').filter(|s| !s.is_empty()).map(str::to_string).collect()
    }

    /// How the child looks for the first argument.
    pub fn program(&self) -> Option<Program> {
        let first = self.argv.first()?;
        if first.contains('/') { Some(Program::Absolute(first.clone())) } else { Some(Program::OnPath(first.clone())) }
    }

    /// Each path that the child gives to `execve`, in order.
    ///
    /// Built before the fork and tried after it: a path that the parent
    /// resolves names the outer tree, which is gone after the chroot.
    pub fn program_candidates(&self) -> Vec<String> {
        match self.program() {
            None => Vec::new(),
            Some(Program::Absolute(p)) => vec![p],
            Some(Program::OnPath(name)) => {
                self.path_dirs.iter().map(|d| format!("{}/{name}", d.trim_end_matches('/'))).collect()
            }
        }
    }
}

#[cfg(test)]
mod tests;
