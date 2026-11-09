//! What the payload will be: the argv, the environment, the working directory,
//! and the banner that says what podvm is and is not doing.
//!
//! ⭐ **docker's rules for combining an image's `Entrypoint` and `Cmd` with the
//! caller's arguments are not obvious and are not podvm's to invent.** They are
//! implemented here as data and asserted, because getting them wrong runs
//! something the caller did not ask for and the failure looks like the image's.

use crate::Fds;

/// How the program will be looked for inside the new root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Program {
    /// The argument had a `/` in it, so it is a path and `PATH` is not consulted.
    Absolute(String),
    /// A bare name, to be looked for along `PATH`.
    OnPath(String),
}

pub struct Plan {
    pub argv: Vec<String>,
    pub env: Vec<String>,
    pub working_dir: String,
    pub fds: Fds,
    pub banner: String,
    /// The `PATH` the program is looked for along, already split.
    pub path_dirs: Vec<String>,
}

impl Plan {
    /// The request variable: names the launch rung the caller wants
    /// ([`TODO/packaging.md`](../../../TODO/packaging.md) T-1003). Read from
    /// the process environment before the plan is built, never from the plan
    /// itself: a packed app launching another must not hand it a directive.
    pub const MODE_REQUEST_VAR: &'static str = "PODVM_MODE";

    /// The result variable: names the rung actually entered. Set beside every
    /// exec and always scrubbed from what the image or the caller declared, so
    /// the two names can never agree by inheritance.
    pub const MODE_ACTIVE_VAR: &'static str = "PODVM_ACTIVE_MODE";
    /// The image's `Entrypoint` and `Cmd`, combined with what the caller wrote.
    ///
    /// ⛔ **docker's rules, and each one has a reason a caller relies on:**
    ///
    /// | image has | caller wrote | what runs |
    /// | --- | --- | --- |
    /// | `Entrypoint` | nothing | `Entrypoint` + `Cmd` |
    /// | `Entrypoint` | `args` | `Entrypoint` + `args`, and `Cmd` is DROPPED |
    /// | no `Entrypoint` | nothing | `Cmd` |
    /// | no `Entrypoint` | `args` | `args`, and `Cmd` is DROPPED |
    ///
    /// ⚠ The row that surprises people is the second: an image's `Cmd` is the
    /// **default arguments to its entrypoint**, so a caller supplying arguments
    /// replaces them. Appending both would pass the image's defaults *and* the
    /// caller's, which is how `docker run img --help` ends up running the
    /// default command with `--help` bolted on.
    pub fn argv_for(
        entrypoint: Option<&[String]>,
        cmd: Option<&[String]>,
        caller: &[String],
        override_entrypoint: Option<&str>,
    ) -> Vec<String> {
        // ⚠ `--entrypoint` REPLACES the image's, and then the image's `Cmd` is
        // dropped too: docker's rule, because the default arguments belonged to
        // the entrypoint that is no longer running.
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

    /// The image's `Env`, then the caller's, so the caller wins.
    ///
    /// ⛔ **A later assignment of the same name wins and the earlier one is
    /// removed rather than shadowed.** `execve` takes an array, not a map, and a
    /// duplicate name in it is resolved by whichever the libc's `getenv` finds
    /// first, which is not the same on glibc and musl. podvm decides here so
    /// the answer does not depend on the payload's libc.
    pub fn env_for(image: &[String], caller: &[String]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for e in image.iter().chain(caller.iter()) {
            let name = e.split('=').next().unwrap_or("");
            out.retain(|existing: &String| existing.split('=').next().unwrap_or("") != name);
            out.push(e.clone());
        }
        // ⭐ T-1003. The request variable and any stale result claim are scrubbed
        // from what the image or the caller declared: the request is read from
        // the process environment before the plan is built, and the result is
        // set beside the exec by [`Plan::set_active_mode`]. Either one reaching
        // the payload from here is a directive surviving into a nested
        // `podvm run`, which reproduces only under nesting.
        out.retain(|e| {
            let name = e.split('=').next().unwrap_or("");
            name != Self::MODE_REQUEST_VAR && name != Self::MODE_ACTIVE_VAR
        });
        // ⭐ **DOCKER'S DEFAULTS, WHERE THE IMAGE DECLARES NONE**, and this was
        // found by running something rather than by reading a specification.
        //
        // ⛔ Measured on 2026-09-09 against
        // `quay.io/rockylinux/rockylinux:9`, whose config `Env` is
        // `container=oci` and nothing else. `docker run ... env` prints `PATH`,
        // `HOME` and `HOSTNAME`; `podvm run ... env` printed `container=oci`
        // alone. The payload's shell then sets a PATH of its own **without
        // exporting it**, so `gcc` was found and started with no `PATH` in its
        // environment at all -- and gcc resolves its own installation directory
        // by searching `$PATH` for `argv[0]`. With nothing to search it fell
        // back to RELATIVE search directories:
        //
        //     programs: =../libexec/gcc/x86_64-redhat-linux/11/
        //
        // resolved against `/`, and every compile died with `cannot execute
        // 'cc1': execvp: No such file or directory` while `cc1` sat in
        // `/usr/libexec/...` with its execute bit set. ⚠ `almalinux:9`, the same
        // gcc from the same family, worked, because its image config declares a
        // `PATH`. A one-image test would have found nothing.
        //
        // ⚠ `HOSTNAME` is deliberately NOT added, where docker sets it. podvm
        // has no UTS namespace, so `gethostname(2)` returns this machine's name;
        // an environment variable disagreeing with the syscall is worse than a
        // missing one, and `TOOL.md` section 4.1 forbids output that implies an
        // isolation podvm does not have.
        fn has(out: &[String], name: &str) -> bool {
            out.iter()
                .any(|e| e.split('=').next().unwrap_or("") == name)
        }
        if !has(&out, "PATH") {
            out.push(format!("PATH={}", Self::DEFAULT_PATH));
        }
        if !has(&out, "HOME") {
            // docker takes HOME from the user's passwd entry; podvm runs as
            // uid 0 and cannot change to anything else (T-0704), so this is
            // root's and it is the same value docker produces for root.
            out.push("HOME=/root".to_string());
        }
        out
    }

    /// Where `PATH` comes from, with a default that is not invented.
    ///
    /// ⚠ The fallback is the one POSIX names and every base image sets anyway.
    /// It is here so a payload whose image config carries no `Env` still finds
    /// `/bin/sh`, and it is a **documented default** rather than a silent one:
    /// the banner prints the `PATH` that was used.
    pub const DEFAULT_PATH: &'static str =
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

    /// Mark the entered rung on the way to `execve`, and scrub the request
    /// beside it.
    ///
    /// The payload sees `PODVM_ACTIVE_MODE=<entered>` and never `PODVM_MODE`,
    /// so a nested `podvm run` starts unforced. A stale result claim already in
    /// the vector is replaced, never kept: only the rung about to be entered
    /// may report itself.
    pub fn set_active_mode(env: &mut Vec<String>, entered: &str) {
        env.retain(|e| {
            let name = e.split('=').next().unwrap_or("");
            name != Self::MODE_REQUEST_VAR && name != Self::MODE_ACTIVE_VAR
        });
        env.push(format!("{}={entered}", Self::MODE_ACTIVE_VAR));
    }

    pub fn path_from(env: &[String]) -> Vec<String> {
        let raw = env
            .iter()
            .rev()
            .find_map(|e| e.strip_prefix("PATH="))
            .unwrap_or(Self::DEFAULT_PATH);
        raw.split(':')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// How the first argument will be looked for.
    pub fn program(&self) -> Option<Program> {
        let first = self.argv.first()?;
        if first.contains('/') {
            Some(Program::Absolute(first.clone()))
        } else {
            Some(Program::OnPath(first.clone()))
        }
    }

    /// Every path `execve` will be tried against, in order.
    ///
    /// ⛔ Built before the fork and tried after it. T-0502: resolving in the
    /// parent names the outer tree, and the outer tree is gone.
    pub fn program_candidates(&self) -> Vec<String> {
        match self.program() {
            None => Vec::new(),
            Some(Program::Absolute(p)) => vec![p],
            Some(Program::OnPath(name)) => self
                .path_dirs
                .iter()
                .map(|d| format!("{}/{name}", d.trim_end_matches('/')))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {

    /// ⭐ The defect M5's ten-distribution sweep found, as a test.
    ///
    /// ⛔ An image whose config declares no `PATH` -- `quay.io/rockylinux/rockylinux:9`
    /// declares `container=oci` and nothing else -- must still hand its payload
    /// docker's default, or the payload's own shell sets one WITHOUT exporting
    /// it and every child process runs with no `PATH` at all. gcc resolves its
    /// installation directory by searching `$PATH` for `argv[0]`, so it then
    /// emitted relative search paths and could not execute `cc1`.
    #[test]
    fn an_image_with_no_path_still_gets_dockers_default() {
        let env = Plan::env_for(&["container=oci".to_string()], &[]);
        assert!(
            env.iter()
                .any(|e| e == &format!("PATH={}", Plan::DEFAULT_PATH)),
            "{env:?}"
        );
        assert!(env.iter().any(|e| e == "HOME=/root"), "{env:?}");
        assert!(env.iter().any(|e| e == "container=oci"), "{env:?}");
        // ⚠ And NOT HOSTNAME: podvm has no UTS namespace, so an environment
        // variable naming one would disagree with gethostname(2).
        assert!(!env.iter().any(|e| e.starts_with("HOSTNAME=")), "{env:?}");
    }

    /// ⛔ The image's own value wins, and so does the caller's over it. A
    /// default that overwrote either would be podvm deciding something the
    /// image already decided.
    #[test]
    fn a_declared_path_and_home_are_never_replaced_by_the_default() {
        let env = Plan::env_for(
            &["PATH=/mine".to_string(), "HOME=/somewhere".to_string()],
            &[],
        );
        assert!(env.contains(&"PATH=/mine".to_string()), "{env:?}");
        assert!(env.contains(&"HOME=/somewhere".to_string()), "{env:?}");
        assert_eq!(env.iter().filter(|e| e.starts_with("PATH=")).count(), 1);

        let env = Plan::env_for(&["PATH=/mine".to_string()], &["PATH=/theirs".to_string()]);
        assert_eq!(env.iter().filter(|e| e.starts_with("PATH=")).count(), 1);
        assert!(env.contains(&"PATH=/theirs".to_string()), "{env:?}");
    }
    use super::*;

    fn v(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn dockers_entrypoint_and_cmd_rules_are_the_ones_implemented() {
        let ep = v(&["/ep"]);
        let cmd = v(&["a", "b"]);

        // Entrypoint + nothing -> Entrypoint + Cmd
        assert_eq!(
            Plan::argv_for(Some(&ep), Some(&cmd), &[], None),
            v(&["/ep", "a", "b"])
        );
        // ⛔ Entrypoint + args -> Entrypoint + args, and Cmd is DROPPED. An
        // image's Cmd is the default ARGUMENTS to its entrypoint, so a caller
        // supplying arguments replaces them rather than appending to them.
        assert_eq!(
            Plan::argv_for(Some(&ep), Some(&cmd), &v(&["z"]), None),
            v(&["/ep", "z"])
        );
        // no Entrypoint + nothing -> Cmd
        assert_eq!(Plan::argv_for(None, Some(&cmd), &[], None), v(&["a", "b"]));
        // no Entrypoint + args -> args
        assert_eq!(
            Plan::argv_for(None, Some(&cmd), &v(&["z"]), None),
            v(&["z"])
        );
        // --entrypoint replaces, and drops Cmd with it
        assert_eq!(
            Plan::argv_for(Some(&ep), Some(&cmd), &[], Some("/other")),
            v(&["/other"])
        );
        assert_eq!(
            Plan::argv_for(Some(&ep), Some(&cmd), &v(&["z"]), Some("/other")),
            v(&["/other", "z"])
        );
    }

    #[test]
    fn an_empty_entrypoint_array_is_not_the_same_as_an_absent_one() {
        // ⛔ An image that sets `"Entrypoint": []` has explicitly CLEARED its
        // parent's, and treating that as unset would run the parent's.
        let empty: Vec<String> = Vec::new();
        let cmd = v(&["c"]);
        assert_eq!(
            Plan::argv_for(Some(&empty), Some(&cmd), &[], None),
            v(&["c"])
        );
    }

    #[test]
    fn the_caller_wins_an_environment_name_and_the_image_copy_is_removed() {
        // ⛔ Removed, not shadowed: `execve` takes an array, and which of two
        // entries with one name `getenv` finds is not the same on glibc and
        // musl. podvm decides here so the payload's libc cannot.
        let out = Plan::env_for(&v(&["A=1", "B=2"]), &v(&["A=9"]));
        // ⚠ The docker defaults are appended after the declared names, so the
        // assertion is on the declared ones in order rather than on the whole
        // vector: `an_image_with_no_path_still_gets_dockers_default` owns the
        // other half.
        let declared: Vec<String> = out
            .iter()
            .filter(|e| e.starts_with("A=") || e.starts_with("B="))
            .cloned()
            .collect();
        assert_eq!(declared, v(&["B=2", "A=9"]));
        assert_eq!(out.iter().filter(|e| e.starts_with("A=")).count(), 1);
    }

    #[test]
    fn path_comes_from_the_environment_and_falls_back_to_a_stated_default() {
        assert_eq!(Plan::path_from(&v(&["PATH=/x:/y"])), v(&["/x", "/y"]),);
        // ⚠ The LAST assignment wins, matching env_for's own rule.
        assert_eq!(Plan::path_from(&v(&["PATH=/a", "PATH=/b"])), v(&["/b"]));
        assert!(Plan::path_from(&[]).contains(&"/bin".to_string()));
    }

    /// ⭐ T-1003. The request variable never reaches the payload from the image
    /// or the caller, from either side, and neither does a stale result claim:
    /// both are read or set outside this vector, so either one surviving here
    /// is a directive reaching a nested `podvm run`.
    #[test]
    fn the_mode_request_and_any_stale_result_are_scrubbed() {
        let env = Plan::env_for(&["PODVM_MODE=memfd".to_string()], &["A=1".to_string()]);
        assert!(!env.iter().any(|e| e.starts_with("PODVM_MODE=")), "{env:?}");
        assert!(env.iter().any(|e| e == "A=1"), "{env:?}");

        let env = Plan::env_for(&[], &["PODVM_MODE=cache".to_string()]);
        assert!(!env.iter().any(|e| e.starts_with("PODVM_MODE=")), "{env:?}");

        let env = Plan::env_for(&["PODVM_ACTIVE_MODE=memfd".to_string()], &[]);
        assert!(
            !env.iter().any(|e| e.starts_with("PODVM_ACTIVE_MODE=")),
            "{env:?}"
        );
    }

    /// ⭐ T-1003. Request in, result out, no request leak: the entered rung is
    /// reported under a different name, and a stale claim is replaced rather
    /// than doubled.
    #[test]
    fn the_entered_rung_is_reported_under_a_different_name() {
        let mut env = v(&["PODVM_MODE=memfd", "A=1"]);
        Plan::set_active_mode(&mut env, "chroot");
        assert!(
            env.contains(&"PODVM_ACTIVE_MODE=chroot".to_string()),
            "{env:?}"
        );
        assert!(!env.iter().any(|e| e.starts_with("PODVM_MODE=")), "{env:?}");
        assert!(env.contains(&"A=1".to_string()), "{env:?}");

        Plan::set_active_mode(&mut env, "memfd");
        assert_eq!(
            env.iter()
                .filter(|e| e.starts_with("PODVM_ACTIVE_MODE="))
                .count(),
            1,
            "{env:?}"
        );
        assert!(
            env.contains(&"PODVM_ACTIVE_MODE=memfd".to_string()),
            "{env:?}"
        );
    }

    #[test]
    fn a_name_with_a_slash_is_a_path_and_never_searched_on_path() {
        let mut p = Plan {
            argv: v(&["/bin/echo", "hi"]),
            env: Vec::new(),
            working_dir: String::new(),
            fds: Fds::default(),
            banner: String::new(),
            path_dirs: v(&["/usr/bin", "/bin"]),
        };
        assert_eq!(p.program_candidates(), v(&["/bin/echo"]));

        p.argv = v(&["echo"]);
        assert_eq!(p.program_candidates(), v(&["/usr/bin/echo", "/bin/echo"]));

        // ⚠ A relative path with a slash is still a path, not a PATH lookup:
        // `./x` means the working directory's `x`.
        p.argv = v(&["./x"]);
        assert_eq!(p.program_candidates(), v(&["./x"]));
    }
}
