//! `TOOL.md` section 6.1: the probe set, disposable children, mode selection.
//! The work is in [`TODO/probe.md`](../../../TODO/probe.md), T-0101 to T-0110.
//!
//! ⭐ **Four rules decide whether this crate is right rather than merely
//! finished**, and each is one entry:
//!
//! 1. every probe runs in a disposable child ([`child`], T-0101);
//! 2. the errno is recorded, never a boolean ([`sys::Errno`], T-0101);
//! 3. "could not run" is a third state and never reads as "denied"
//!    ([`verdict::Verdict::Skip`], T-0109);
//! 4. the controls travel with the bogus-argument discriminator, so a probe
//!    that has stopped discriminating says so ([`select::Selection`], T-0102).
//!
//! ⛔ No dependencies. `TODO/deps.md` T-0901 rules that the section 6.1 probe
//! set is declared by hand first and the binary measured, before a syscall
//! crate is considered against that number.
#![forbid(unsafe_op_in_unsafe_fn)]

pub mod binfmt;
pub mod child;
pub mod exit;
pub mod identity;
pub mod interp;
pub mod json;
pub mod machine;
pub mod mounts;
pub mod nongoals;
pub mod probes;
pub mod report;
pub mod select;
pub mod supervise;
pub mod sys;
pub mod verdict;
pub mod writable;

use verdict::Outcome;

/// The private argument a probe child is re-executed with.
///
/// ⚠ It is not a public verb. `podvm probe --help` names it as internal, and
/// the CLI dispatches it before anything else so a child cannot fall into the
/// ordinary argument surface.
pub const CHILD_FLAG: &str = "--probe-child";

/// Everything one run measured.
pub struct Findings {
    /// Every probe, in the fixed order of [`probes::PROBES`].
    pub rows: Vec<(&'static str, Outcome)>,
    pub identity: identity::Identity,
    pub writable: Vec<writable::WriteProbe>,
    /// Where the probe children were re-executed from, for the report.
    pub self_exe: String,
}

impl Findings {
    /// An empty set, for building one field at a time in a test.
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

/// Run the whole set. One disposable child per probe, in a fixed order.
pub fn run() -> Findings {
    let spawner = child::Spawner::new();
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
        writable: writable::probe(),
        self_exe: spawner.exe_source.clone(),
    }
}

/// What a probe child does: run exactly one probe, in this process, and report
/// it on stdout and in the exit status.
///
/// ⛔ Both, and they must agree. [`child::Spawner`] refuses a child whose line
/// and exit status disagree, because a child that contradicts itself has
/// established no verdict. `TODO/probe.md` T-0109 rule 1.
pub fn run_child(name: &str) -> i32 {
    let Some(probe) = probes::find(name) else {
        eprintln!("podvm: no probe named {name:?}");
        return 2;
    };
    let outcome = match probe.kind {
        probes::Kind::Child { body, .. } => body(),
        // A `Clone` probe's measurement is the clone the parent performed, so
        // there is nothing for a child to run. Reaching here means the parent
        // and this table disagree, which is a defect and not a denial.
        probes::Kind::Clone(_) => Outcome::skip(
            None,
            format!("{name:?} is measured by the parent's clone(2) and has no child body"),
        ),
    };
    println!("{}", verdict::encode(&outcome));
    outcome.verdict.exit_code()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_child_probe_runs_and_answers_in_a_child() {
        // ⭐ Reality as the acceptance gate: this drives the shipping path,
        // one real forked child per probe, and asserts the shape of what comes
        // back rather than that any particular machine permits anything.
        let f = run();
        assert_eq!(f.rows.len(), probes::PROBES.len());
        for (name, o) in &f.rows {
            if o.verdict == verdict::Verdict::Denied {
                assert!(o.errno.is_some(), "{name} denied with no errno");
            }
        }
    }

    #[test]
    fn a_probe_name_that_does_not_exist_exits_two() {
        assert_eq!(run_child("no such probe"), 2);
    }
}
