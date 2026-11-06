//! `TODO/probe.md` T-0103: probe creation and attachment separately.
//!
//! A runtime that asks only "does `mount(2)` work" learns less than one
//! syscall's worth more effort would have told it, and then reports "no
//! mounts" where the truth is "mounts you can create and never attach".
//!
//! ⭐ Four verdicts, not one. **Use** is the fourth and is the one a runtime
//! that stops at "creatable" gets wrong: a detached mount that cannot be
//! opened through is not a usable private filesystem.

use crate::verdict::{Outcome, Verdict};
use crate::Findings;

pub struct Mounts {
    pub create: Outcome,
    pub configure: Outcome,
    pub attach: Outcome,
    pub r#use: Outcome,
}

/// The row each half is read from. One read path: these are the attribution
/// probes that already ran, never a second set of calls that would answer
/// about a different moment.
const CREATE_ROW: &str = "fsopen(tmpfs)";
const CONFIGURE_ROW: &str = "fsmount(tmpfs)";
const ATTACH_ROW: &str = "move_mount(-> /tmp/mm-probe)";
const USE_ROW: &str = "openat(detached tmpfs, O_DIRECTORY)";

fn take(f: &Findings, name: &str) -> Outcome {
    f.rows
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, o)| o.clone())
        .unwrap_or_else(|| {
            Outcome::skip(None, format!("the probe set carries no row named {name:?}"))
        })
}

impl Mounts {
    pub fn of(f: &Findings) -> Mounts {
        Mounts {
            create: take(f, CREATE_ROW),
            configure: take(f, CONFIGURE_ROW),
            attach: take(f, ATTACH_ROW),
            r#use: take(f, USE_ROW),
        }
    }

    pub fn rows(&self) -> [(&'static str, &Outcome); 4] {
        [
            ("create", &self.create),
            ("configure", &self.configure),
            ("attach", &self.attach),
            ("use", &self.r#use),
        ]
    }

    /// The banner's `mounts=` field.
    pub fn summary(&self) -> String {
        let attached = self.attach.verdict == Verdict::Ok;
        let usable = self.r#use.verdict == Verdict::Ok;
        let made = self.configure.verdict == Verdict::Ok;
        match (made, attached, usable) {
            (_, true, _) => "full".to_string(),
            (true, false, true) => "detached-only".to_string(),
            (true, false, false) => "detached-only, and not openable".to_string(),
            (false, _, _) => "none".to_string(),
        }
    }

    /// ⭐ The free attribution `TOOL.md` section 6.1 rule 8 asks for, and the
    /// sentence that stops the next reader hunting for a capability to acquire.
    pub fn capability_note(&self) -> Option<&'static str> {
        if self.configure.verdict == Verdict::Ok && self.attach.verdict != Verdict::Ok {
            Some(
                "fsmount(2) succeeded, which runs the same may_mount() check \
                 move_mount(2) and unshare(CLONE_NEWNS) use, so the attach denial \
                 on this machine is NOT a capability problem and no capability \
                 will clear it",
            )
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys;

    fn f(rows: &[(&'static str, Outcome)]) -> Findings {
        Findings {
            rows: rows.to_vec(),
            ..Findings::empty()
        }
    }

    #[test]
    fn creatable_but_unattachable_is_not_reported_as_no_mounts() {
        let m = Mounts::of(&f(&[
            (CREATE_ROW, Outcome::ok()),
            (CONFIGURE_ROW, Outcome::ok()),
            (ATTACH_ROW, Outcome::denied(sys::EPERM)),
            (USE_ROW, Outcome::ok()),
        ]));
        assert_eq!(m.summary(), "detached-only");
        assert!(m.capability_note().is_some());
    }

    #[test]
    fn a_machine_that_cannot_create_reports_none() {
        let m = Mounts::of(&f(&[
            (CREATE_ROW, Outcome::denied(sys::EPERM)),
            (CONFIGURE_ROW, Outcome::denied(sys::EPERM)),
            (
                ATTACH_ROW,
                Outcome::skip(Some(sys::EPERM), "fsmount failed"),
            ),
            (USE_ROW, Outcome::skip(Some(sys::EPERM), "fsmount failed")),
        ]));
        assert_eq!(m.summary(), "none");
        assert!(m.capability_note().is_none());
    }

    #[test]
    fn a_missing_row_is_a_skip_rather_than_a_denial() {
        let m = Mounts::of(&f(&[]));
        assert_eq!(m.create.verdict, Verdict::Skip);
        assert_eq!(m.summary(), "none");
    }
}
