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
            // docker takes `HOME` from the passwd entry of the user. The
            // payload runs as uid 0, so this is the value of root, the
            // value that docker gives for root.
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
