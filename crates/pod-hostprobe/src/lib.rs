//! The probes of a Linux host, and the raw system calls under them.
//!
//! Four rules make the probes correct:
//!
//! 1. Each probe runs in a disposable child ([`child`]).
//! 2. A probe records the errno, never a boolean ([`sys::Errno`]).
//! 3. "Could not run" is a third state, and it never reads as "denied"
//!    ([`verdict::Verdict::Skip`]).
//! 4. The controls run with the bogus-argument discriminator, so that a
//!    probe that stops to discriminate says so ([`select::Selection`]).
//!
//! The crate is Linux only. On another target it is empty.
//!
//! [`sys`] uses no libc wrapper: a wrapper (`setuid(3)` of glibc or musl)
//! gives the errno of the library, and a probe needs the errno of the
//! kernel.
#![cfg(target_os = "linux")]
#![forbid(unsafe_op_in_unsafe_fn)]

pub mod binfmt;
pub mod child;
pub mod exec;
pub mod identity;
pub mod interp;
pub mod machine;
pub mod mounts;
pub mod probes;
pub mod select;
pub mod space;
pub mod supervise;
pub mod sys;
pub mod verdict;
pub mod writable;

use verdict::Outcome;

/// The values that the caller gives to the probes. The caller fills it one
/// time and gives it to each call that needs it.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The private argument with which a probe child starts. The binary
    /// must read it before all other arguments, and then call
    /// [`run_child`] with the name that follows it.
    pub child_flag: String,
    /// The `argv[0]` of a probe child.
    pub child_name: String,
    /// The name of the setting with which the user gives a different
    /// directory, for example `$STORE`. The messages of [`space`] and
    /// [`exec`] name it. `None`: they name no setting.
    pub directory_setting: Option<String>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            child_flag: "--hostprobe-child".to_string(),
            child_name: "pod-hostprobe".to_string(),
            directory_setting: None,
        }
    }
}

/// All that one run measured.
pub struct Findings {
    /// Each probe, in the fixed order of [`probes::PROBES`].
    pub rows: Vec<(&'static str, Outcome)>,
    pub identity: identity::Identity,
    pub writable: Vec<writable::WriteProbe>,
    /// The path from which the probe children started, for the report.
    pub self_exe: String,
}

impl Findings {
    /// An empty set, for a test that builds one field at a time.
    pub fn empty() -> Findings {
        Findings {
            rows: Vec::new(),
            identity: identity::Identity::default(),
            writable: Vec::new(),
            self_exe: String::new(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&Outcome> {
        self.rows.iter().find(|(n, _)| *n == name).map(|(_, o)| o)
    }
}

/// Run the whole set: one disposable child for each probe, in a fixed
/// order. `candidates` are the directories of the write probe, as
/// [`writable::candidates`] gives them.
pub fn run(settings: &Settings, candidates: &[(String, String)]) -> Findings {
    let spawner = child::Spawner::new(settings);
    let rows = probes::PROBES
        .iter()
        .map(|p| {
            let outcome = match p.kind {
                probes::Kind::Clone(flags) => spawner.clone_accepted(flags),
                probes::Kind::Child { ns_flags, .. } => spawner.run_probe(p.name, ns_flags),
            };
            (p.name, outcome)
        })
        .collect();
    Findings {
        rows,
        identity: identity::read(),
        writable: writable::probe(candidates),
        self_exe: spawner.exe_source.clone(),
    }
}

/// The work of a probe child: run one probe in this process, write its line
/// to `out` (the standard output of the child), and return the exit status.
///
/// The line and the exit status must agree. [`child::Spawner`] refuses a
/// child whose line and exit status do not agree: such a child gives no
/// verdict. A name that is not a probe gives a skip line and status 2.
pub fn run_child(name: &str, out: &mut dyn std::io::Write) -> i32 {
    let outcome = match probes::find(name) {
        None => Outcome::skip(None, format!("no probe named {name:?}")),
        Some(probe) => match probe.kind {
            probes::Kind::Child { body, .. } => body(),
            // The measurement of a `Clone` probe is the clone of the
            // parent, so a child has nothing to run. The parent and the
            // table do not agree: that is a defect, not a denial.
            probes::Kind::Clone(_) => {
                Outcome::skip(None, format!("{name:?} is measured by the parent's clone(2) and has no child body"))
            }
        },
    };
    let _ = writeln!(out, "{}", verdict::encode(&outcome));
    let _ = out.flush();
    outcome.verdict.exit_code()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_child_probe_runs_and_answers_in_a_child() {
        // The real path: one real child for each probe. The test checks the
        // shape of the answers, not that this host permits a thing. libtest
        // does not know the child flag, so each child row is a skip here.
        let f = run(&Settings::default(), &writable::candidates(&[]));
        assert_eq!(f.rows.len(), probes::PROBES.len());
        for (name, o) in &f.rows {
            if o.verdict == verdict::Verdict::Denied {
                assert!(o.errno.is_some(), "{name} denied with no errno");
            }
        }
    }

    #[test]
    fn a_probe_name_that_does_not_exist_exits_two() {
        let mut out = Vec::new();
        assert_eq!(run_child("no such probe", &mut out), 2);
        let line = String::from_utf8(out).unwrap();
        let got = verdict::decode(line.trim_end()).unwrap();
        assert_eq!(got.verdict, verdict::Verdict::Skip);
        assert!(got.reason.contains("no such probe"), "{line}");
    }
}
