//! `TODO/supervise.md` T-0606: the supervise tier as legs, and the refusal rule.
//!
//! One leg per fact the tier needs, each measured, never inferred from
//! another leg and never borrowed from another group's contract. Mediation
//! (the notify tier: listener, argument channel, race-safety) is refused
//! when any of its legs is missing, and the refusal names the leg: not a
//! reduced supervise tier, and never a per-call fallback that reports success
//! for mediation it never performed. Supervision (the lifecycle: a pidfd per
//! child, waitid for its status) is assessed beside it through
//! [`assess_supervision`], so a machine that cannot mediate can still track.
//! The legs run as [`crate::probes`] rows in [`crate::probes::Group::Supervise`]
//! order; this module reads their verdicts back through
//! [`crate::probes::SUPERVISE_LEGS`] and
//! [`crate::probes::SUPERVISION_LEGS`], so a rename moves the rows, the
//! constants and this assessment together.
//!
//! ⛔ A row that is absent is a missing leg, not a passing one. Reading that
//! absence as anything but missing would be the silent non-mediation T-0606
//! exists to refuse.

use crate::verdict::Outcome;
use crate::Findings;

/// One leg's verdict. `None` where the findings carry no such row: the leg
/// was never measured, which is missing rather than any verdict.
pub struct Leg {
    pub name: &'static str,
    pub outcome: Option<Outcome>,
}

impl Leg {
    fn missing(&self) -> bool {
        match &self.outcome {
            Some(o) => o.verdict != crate::verdict::Verdict::Ok,
            None => true,
        }
    }

    fn detail(&self) -> String {
        match &self.outcome {
            Some(o) => match (o.verdict, o.errno) {
                (crate::verdict::Verdict::Ok, _) => format!("{}=ok", self.name),
                (crate::verdict::Verdict::Denied, Some(e)) => {
                    format!("{}={}", self.name, e.name())
                }
                (crate::verdict::Verdict::Denied, None) => {
                    format!("{}=denied", self.name)
                }
                (crate::verdict::Verdict::Skip, _) if o.reason.is_empty() => {
                    format!("{}=skip", self.name)
                }
                (crate::verdict::Verdict::Skip, _) => {
                    format!("{}=skip: {}", self.name, o.reason)
                }
            },
            None => format!("{}=(not probed)", self.name),
        }
    }
}

/// What the legs established: every leg with its verdict, and the names of
/// the ones that are missing.
pub struct Assessment {
    pub legs: Vec<Leg>,
    /// The legs that are missing, in run order. Empty exactly when the tier
    /// holds.
    pub missing: Vec<&'static str>,
    /// Whose refusal this is: "supervise tier" for mediation, "supervision"
    /// for lifecycle tracking. Two assessments, two sentences, one shape.
    scope: &'static str,
}

impl Assessment {
    /// True when any leg is missing: a denial, a skip, or a row that is
    /// absent. A supervise tier without a listener, without an argument
    /// channel, or without race-safety is three different products depending
    /// on which leg failed, so there is no partial tier.
    pub fn refused(&self) -> bool {
        !self.missing.is_empty()
    }

    /// The refusal, naming every missing leg with what it measured. `None`
    /// where the tier holds.
    pub fn refusal(&self) -> Option<String> {
        if self.missing.is_empty() {
            return None;
        }
        let mut out = String::from(self.scope);
        out.push_str(" refused:");
        for leg in &self.legs {
            if self.missing.contains(&leg.name) {
                out.push_str(&format!(" {}", leg.detail()));
            }
        }
        Some(out)
    }
}

/// Read the supervise legs out of a finished run.
pub fn assess(f: &Findings) -> Assessment {
    assess_through(f, crate::probes::SUPERVISE_LEGS, "supervise tier")
}

/// Read the supervision legs out of a finished run: the pair the
/// lifecycle runs on, refused on its own where the launcher cannot
/// track, available where it can even as mediation is refused.
pub fn assess_supervision(f: &Findings) -> Assessment {
    assess_through(f, crate::probes::SUPERVISION_LEGS, "supervision")
}

fn assess_through(f: &Findings, legs: &[&'static str], scope: &'static str) -> Assessment {
    let legs: Vec<Leg> = legs
        .iter()
        .map(|&name| Leg {
            name,
            outcome: f.get(name).cloned(),
        })
        .collect();
    let missing: Vec<&'static str> = legs
        .iter()
        .filter(|l| l.missing())
        .map(|l| l.name)
        .collect();
    Assessment {
        legs,
        missing,
        scope,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys;

    fn findings(rows: &[(&'static str, Outcome)]) -> Findings {
        Findings {
            rows: rows.to_vec(),
            ..Findings::empty()
        }
    }

    fn all_ok() -> Vec<(&'static str, Outcome)> {
        crate::probes::SUPERVISE_LEGS
            .iter()
            .map(|&name| (name, Outcome::ok()))
            .collect()
    }

    fn supervision_ok() -> Vec<(&'static str, Outcome)> {
        crate::probes::SUPERVISION_LEGS
            .iter()
            .map(|&name| (name, Outcome::ok()))
            .collect()
    }

    #[test]
    fn three_clear_legs_hold_the_tier() {
        let a = assess(&findings(&all_ok()));
        assert_eq!(a.legs.len(), 3);
        assert!(a.missing.is_empty());
        assert!(!a.refused());
        assert!(a.refusal().is_none());
    }

    #[test]
    fn a_denied_leg_refuses_and_names_its_errno() {
        let mut rows = all_ok();
        rows[0] = (
            "seccomp(NEW_LISTENER) [supervise]",
            Outcome::denied(sys::EACCES),
        );
        let a = assess(&findings(&rows));
        assert!(a.refused());
        assert_eq!(a.missing, vec!["seccomp(NEW_LISTENER) [supervise]"]);
        let refusal = a.refusal().expect("a refusal prints");
        assert!(
            refusal.contains("seccomp(NEW_LISTENER) [supervise]=EACCES"),
            "{refusal}"
        );
    }

    #[test]
    fn a_skipped_leg_refuses_and_names_its_reason() {
        // The shape on a machine where the leg could not run: missing rather
        // than any verdict about the tier.
        let mut rows = all_ok();
        rows[1] = (
            "process_vm_readv(own pid) [supervise]",
            Outcome::skip(Some(sys::ENOENT), "no readv here"),
        );
        let a = assess(&findings(&rows));
        assert!(a.refused());
        assert_eq!(a.missing, vec!["process_vm_readv(own pid) [supervise]"]);
        let refusal = a.refusal().expect("a refusal prints");
        assert!(refusal.contains("no readv here"), "{refusal}");
    }

    #[test]
    fn a_row_that_is_absent_is_missing_rather_than_any_verdict() {
        // A document written before the legs existed: two legs measured, the
        // third never run. Its absence refuses the tier; it is not read as a
        // denial and not as a pass.
        let mut rows = all_ok();
        rows.pop();
        let a = assess(&findings(&rows));
        assert!(a.refused());
        assert_eq!(a.missing, vec!["ptrace(PTRACE_TRACEME) [supervise]"]);
        let refusal = a.refusal().expect("a refusal prints");
        assert!(
            refusal.contains("ptrace(PTRACE_TRACEME) [supervise]=(not probed)"),
            "{refusal}"
        );
    }

    #[test]
    fn the_refusal_names_every_missing_leg_in_run_order() {
        let mut rows = all_ok();
        rows[0] = (
            "seccomp(NEW_LISTENER) [supervise]",
            Outcome::denied(sys::EACCES),
        );
        rows[2] = (
            "ptrace(PTRACE_TRACEME) [supervise]",
            Outcome::skip(Some(sys::ENOENT), "no ptrace here"),
        );
        let a = assess(&findings(&rows));
        assert_eq!(
            a.missing,
            vec![
                "seccomp(NEW_LISTENER) [supervise]",
                "ptrace(PTRACE_TRACEME) [supervise]",
            ]
        );
        let refusal = a.refusal().expect("a refusal prints");
        assert!(
            refusal.contains("seccomp(NEW_LISTENER) [supervise]=EACCES"),
            "{refusal}"
        );
        assert!(
            refusal.contains("ptrace(PTRACE_TRACEME) [supervise]=skip"),
            "{refusal}"
        );
        assert!(
            refusal.find("seccomp").unwrap() < refusal.find("ptrace").unwrap(),
            "{refusal}"
        );
    }

    /// TODO/supervise.md T-0606. Two clear legs hold supervision: the
    /// lifecycle's own pair, read through its own names.
    #[test]
    fn two_clear_legs_hold_supervision() {
        let a = assess_supervision(&findings(&supervision_ok()));
        assert_eq!(a.legs.len(), 2);
        assert!(a.missing.is_empty());
        assert!(!a.refused());
        assert!(a.refusal().is_none());
    }

    /// A denied supervision leg refuses supervision in its own sentence,
    /// not mediation's: two assessments, two refusals, one shape.
    #[test]
    fn a_denied_supervision_leg_refuses_supervision() {
        let mut rows = supervision_ok();
        rows[0] = (
            "pidfd_open(own pid) [supervise]",
            Outcome::denied(sys::EPERM),
        );
        let a = assess_supervision(&findings(&rows));
        assert!(a.refused());
        let refusal = a.refusal().expect("a refusal prints");
        assert!(refusal.starts_with("supervision refused:"), "{refusal}");
        assert!(
            refusal.contains("pidfd_open(own pid) [supervise]=EPERM"),
            "{refusal}"
        );
    }

    /// The split itself, on the target shape: mediation refused where
    /// its legs are denied, supervision available where its pair holds.
    /// No degraded mediation, no silent Continue, and lifecycle
    /// tracking stands where the launcher runs it.
    #[test]
    fn denied_notify_legs_refuse_mediation_beside_available_supervision() {
        let notify = vec![
            (
                "seccomp(NEW_LISTENER) [supervise]",
                Outcome::denied(sys::EPERM),
            ),
            (
                "process_vm_readv(own pid) [supervise]",
                Outcome::denied(sys::EPERM),
            ),
            (
                "ptrace(PTRACE_TRACEME) [supervise]",
                Outcome::denied(sys::EPERM),
            ),
        ];
        let mediation = assess(&findings(&notify));
        assert!(mediation.refused());
        assert!(mediation
            .refusal()
            .expect("a refusal prints")
            .starts_with("supervise tier refused:"),);
        let supervision = assess_supervision(&findings(&supervision_ok()));
        assert!(!supervision.refused());
        assert!(supervision.refusal().is_none());
    }
}
