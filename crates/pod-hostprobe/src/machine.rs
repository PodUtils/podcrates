//! The machine tier as legs, and the refusal rule.
//!
//! One leg per fact the tier needs, each measured, never inferred from
//! another leg. The legs run as [`crate::probes`] rows in
//! [`crate::probes::Group::Machine`] order; this module reads their
//! verdicts back through [`crate::probes::MACHINE_LEGS`], so a rename moves
//! the rows, the constant and this assessment together.
//!
//! The assessment establishes a profile, not just a refusal. `full` needs
//! every leg plus `kvm` in the emulator's own accelerator list; `tcg`
//! needs the emulator, `tcg` in that list, the file-size bound and image
//! space, with kvm and tun reported but not blocking. A TCG boundary is
//! the emulator process, never hardware isolation, and the banner says
//! so. Refusal stays only where no profile holds: no usable accelerator
//! at all, or no space, or the emulator missing.
//!
//! A row that is absent is a missing leg, not a passing one. A document
//! written before the legs existed carries no verdict for them, and reading
//! that absence as anything but missing would be silent non-isolation.

use crate::Findings;
use crate::verdict::{Outcome, Verdict};

/// Which machine profile the legs establish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Every leg holds and the emulator lists `kvm`: acceleration and tun.
    Full,
    /// The emulator lists `tcg` with space and the file-size bound
    /// holding; kvm and tun may be missing. The boundary is the emulator
    /// process, never hardware isolation.
    Tcg,
}

impl Profile {
    pub fn word(self) -> &'static str {
        match self {
            Profile::Full => "full",
            Profile::Tcg => "tcg",
        }
    }
}

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

    /// One leg's verdict in words. Public for a doctor command that prints
    /// the legs: a second renderer would drift from this one.
    pub fn detail(&self) -> String {
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

/// What the legs established: every leg with its verdict, the profile
/// they establish, and the names of the legs blocking each shape.
pub struct Assessment {
    pub legs: Vec<Leg>,
    /// The full-tier gaps, in run order: the legs that did not hold. A
    /// leg the TCG profile does not need still gaps the full one here:
    /// a weaker profile running is not full holding. Empty where every
    /// leg holds, which the full profile additionally narrows with the
    /// emulator's own kvm listing.
    pub missing: Vec<&'static str>,
    /// The profile the legs establish: full where nothing is missing,
    /// TCG where the emulator, its `tcg` accelerator, the file-size
    /// bound and image space hold without kvm or tun, none elsewhere.
    pub profile: Option<Profile>,
    /// The legs blocking every profile, in run order: the required legs
    /// (emulator, accelerator list, file-size bound, image space) that
    /// did not hold. Empty exactly where a profile holds.
    pub tcg_blocked_by: Vec<&'static str>,
}

impl Assessment {
    /// True when no profile runs: a denial, a skip, or a row that is
    /// absent on a required leg. kvm or tun missing alone never refuses:
    /// that is the TCG profile, not a refusal.
    pub fn refused(&self) -> bool {
        self.profile.is_none()
    }

    /// The refusal, naming the legs blocking every profile with what
    /// they measured: the required legs that did not hold, and the kvm
    /// leg where no kvm acceleration holds with them. `None` where a
    /// profile holds.
    ///
    /// The string states its verdict one time ("machine tier refused:"),
    /// so a printer adds no verdict word before it. `; ` separates the
    /// legs: the reason of a skipped leg is a sentence, and a space alone
    /// runs the name of the next leg into it.
    pub fn refusal(&self) -> Option<String> {
        if self.profile.is_some() {
            return None;
        }
        let kvm_holds = accel_usable(&self.legs, "kvm");
        let legs: Vec<String> = self
            .legs
            .iter()
            .filter(|leg| self.tcg_blocked_by.contains(&leg.name) || (leg.name == KVM_LEG && !kvm_holds))
            .map(|leg| refusal_detail(&self.legs, leg))
            .collect();
        Some(format!("machine tier refused: {}", legs.join("; ")))
    }

    /// True where the tun leg holds: the TCG profile carries real tun
    /// networking rather than user-mode networking.
    pub fn tun_ok(&self) -> bool {
        leg_ok(&self.legs, TUN_LEG)
    }
}

/// The leg names this assessment reads. They spell
/// [`crate::probes::MACHINE_LEGS`], which owns them: a rename moves the
/// rows, the constant and every use below together. `assess` carries a
/// debug assertion on each, so a drift fails fast in tests rather than
/// assessing the wrong legs.
///
/// Public for a doctor command, which classifies
/// and remedies by leg: matching its own copies of these strings would
/// be a second list that drifts, and the one nobody reads is the one
/// that does.
pub const EMU_LEG: &str = "qemu-system-x86_64 --version";
pub const KVM_LEG: &str = "open(/dev/kvm, O_RDWR)";
pub const FSIZE_LEG: &str = "prlimit(RLIMIT_FSIZE)";
pub const TUN_LEG: &str = "open(/dev/net/tun, O_RDWR)";
pub const SPACE_LEG: &str = "image space (statfs .)";
pub const ACCEL_LEG: &str = "qemu-system-x86_64 -accel help";

/// The accelerator words the emulator listed, read from the leg's reason
/// (`accelerators: kvm tcg ...`, the shape the accel leg writes). Empty
/// where the leg never listed any: an `Ok` without a list promises no
/// accelerator.
pub fn accelerators_in(reason: &str) -> Vec<&str> {
    let body = reason.strip_prefix("accelerators:").unwrap_or(reason);
    body.split_whitespace().collect()
}

fn leg_ok(legs: &[Leg], name: &str) -> bool {
    legs.iter().any(|l| {
        l.name == name
            && matches!(
                &l.outcome,
                Some(o) if o.verdict == Verdict::Ok
            )
    })
}

/// Whether the named accelerator is both listed by the emulator and, for
/// `kvm`, opened on its node. The list alone cannot say the caller may
/// use it, and the node alone cannot say the build carries it: the accel
/// leg asks the emulator, the kvm leg opens the node, and the
/// profile needs both halves of each claim.
fn accel_usable(legs: &[Leg], accel: &str) -> bool {
    let listed = legs
        .iter()
        .find(|l| l.name == ACCEL_LEG)
        .map(|l| match &l.outcome {
            Some(o) if o.verdict == Verdict::Ok => accelerators_in(&o.reason).contains(&accel),
            _ => false,
        })
        .unwrap_or(false);
    if !listed {
        return false;
    }
    match accel {
        "kvm" => leg_ok(legs, KVM_LEG),
        _ => true,
    }
}

/// Read the machine legs out of a finished run.
pub fn assess(f: &Findings) -> Assessment {
    for name in [EMU_LEG, KVM_LEG, FSIZE_LEG, TUN_LEG, SPACE_LEG, ACCEL_LEG] {
        debug_assert!(crate::probes::MACHINE_LEGS.contains(&name), "{name} is not a machine leg");
    }
    let legs: Vec<Leg> =
        crate::probes::MACHINE_LEGS.iter().map(|&name| Leg { name, outcome: f.get(name).cloned() }).collect();
    let missing: Vec<&'static str> = legs.iter().filter(|l| l.missing()).map(|l| l.name).collect();
    let kvm = accel_usable(&legs, "kvm");
    let tcg = accel_usable(&legs, "tcg");
    // The TCG blockers, in run order: the emulator, its `tcg`
    // accelerator, the file-size bound and image space. kvm and tun
    // never block a profile; they decide between them.
    let mut tcg_blocked_by = Vec::new();
    for leg in &legs {
        let holds = match leg.name {
            EMU_LEG => leg_ok(&legs, EMU_LEG),
            ACCEL_LEG => tcg,
            FSIZE_LEG => leg_ok(&legs, FSIZE_LEG),
            SPACE_LEG => leg_ok(&legs, SPACE_LEG),
            _ => true,
        };
        if !holds {
            tcg_blocked_by.push(leg.name);
        }
    }
    let profile = if missing.is_empty() && kvm {
        Some(Profile::Full)
    } else if tcg_blocked_by.is_empty() {
        Some(Profile::Tcg)
    } else {
        None
    };
    Assessment { legs, missing, profile, tcg_blocked_by }
}

/// The refusal line for one blocking leg. The leg's own detail, except
/// where the verdict is `Ok` and the accelerator list is what blocks:
/// an `ok` inside a refusal would read as the tier holding.
fn refusal_detail(legs: &[Leg], leg: &Leg) -> String {
    if let Some(o) = &leg.outcome
        && o.verdict == Verdict::Ok
    {
        if leg.name == ACCEL_LEG && !accelerators_in(&o.reason).contains(&"tcg") {
            let listed = o.reason.strip_prefix("accelerators:").unwrap_or(&o.reason).trim();
            let listed = if listed.is_empty() { "no list" } else { listed };
            return format!("{}: tcg not listed ({listed})", leg.name);
        }
        if leg.name == KVM_LEG && !accel_usable(legs, "kvm") {
            return format!("{}: the node opens but the emulator lists no kvm", leg.name);
        }
    }
    leg.detail()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys;

    fn findings(rows: &[(&'static str, Outcome)]) -> Findings {
        Findings { rows: rows.to_vec(), ..Findings::empty() }
    }

    fn all_ok() -> Vec<(&'static str, Outcome)> {
        // The accel leg carries the producer's list format, like a real
        // run: an `Ok` without a list promises no accelerator.
        crate::probes::MACHINE_LEGS
            .iter()
            .map(|&name| {
                if name == ACCEL_LEG {
                    (name, Outcome::ok_with("accelerators: kvm tcg xen"))
                } else {
                    (name, Outcome::ok())
                }
            })
            .collect()
    }

    #[test]
    fn six_clear_legs_hold_the_full_tier() {
        let a = assess(&findings(&all_ok()));
        assert_eq!(a.legs.len(), 6);
        assert!(a.missing.is_empty());
        assert_eq!(a.profile, Some(Profile::Full));
        assert!(a.tcg_blocked_by.is_empty());
        assert!(!a.refused());
        assert!(a.refusal().is_none());
        assert!(a.tun_ok());
    }

    #[test]
    fn a_denied_kvm_leg_runs_tcg_without_a_refusal() {
        // The lane shape: the node is absent but the emulator lists tcg.
        // kvm still gaps the full tier here; it just no longer refuses.
        let mut rows = all_ok();
        rows[1] = ("open(/dev/kvm, O_RDWR)", Outcome::denied(sys::ENOENT));
        let a = assess(&findings(&rows));
        assert_eq!(a.profile, Some(Profile::Tcg));
        assert!(!a.refused());
        assert!(a.refusal().is_none());
        assert_eq!(a.missing, vec!["open(/dev/kvm, O_RDWR)"]);
    }

    #[test]
    fn a_denied_tun_leg_runs_tcg_without_a_refusal() {
        let mut rows = all_ok();
        rows[3] = ("open(/dev/net/tun, O_RDWR)", Outcome::denied(sys::ENOENT));
        let a = assess(&findings(&rows));
        assert_eq!(a.profile, Some(Profile::Tcg));
        assert!(a.refusal().is_none());
        assert!(!a.tun_ok());
    }

    #[test]
    fn an_accelerator_list_without_tcg_blocks_every_profile() {
        let mut rows = all_ok();
        rows[1] = ("open(/dev/kvm, O_RDWR)", Outcome::denied(sys::ENOENT));
        rows[5] = ("qemu-system-x86_64 -accel help", Outcome::ok_with("accelerators: kvm"));
        let a = assess(&findings(&rows));
        assert_eq!(a.profile, None);
        assert!(a.refused());
        let refusal = a.refusal().expect("a refusal prints");
        assert!(refusal.contains("tcg not listed"), "{refusal}");
        assert!(refusal.contains("open(/dev/kvm, O_RDWR)=ENOENT"), "{refusal}");
        assert!(refusal.find("kvm").unwrap() < refusal.find("accel").unwrap(), "{refusal}");
    }

    #[test]
    fn a_kvm_node_the_emulator_does_not_list_is_not_full() {
        // The node opens but the build carries no kvm: full claims
        // acceleration it cannot drive, so the profile is TCG.
        let mut rows = all_ok();
        rows[5] = ("qemu-system-x86_64 -accel help", Outcome::ok_with("accelerators: tcg"));
        let a = assess(&findings(&rows));
        assert_eq!(a.profile, Some(Profile::Tcg));
        assert!(a.refusal().is_none());
        assert!(a.missing.is_empty());
    }

    #[test]
    fn a_skipped_emulator_blocks_every_profile() {
        // The common shape on a machine with no emulator: both emulator
        // legs could not run, which is missing rather than any verdict
        // about acceleration.
        let mut rows = all_ok();
        rows[0] = ("qemu-system-x86_64 --version", Outcome::skip(Some(sys::ENOENT), "no qemu-system-x86_64 on PATH"));
        rows[5] = (
            "qemu-system-x86_64 -accel help",
            Outcome::skip(
                Some(sys::ENOENT),
                "no qemu-system-x86_64 on PATH, so the accelerator list could not be read",
            ),
        );
        let a = assess(&findings(&rows));
        assert_eq!(a.profile, None);
        assert!(a.refused());
        assert_eq!(a.tcg_blocked_by, vec!["qemu-system-x86_64 --version", "qemu-system-x86_64 -accel help"]);
        let refusal = a.refusal().expect("a refusal prints");
        assert!(refusal.contains("no qemu-system-x86_64 on PATH"), "{refusal}");
    }

    /// PodUtils/podvm#337: the legs are separated where the reason of a leg
    /// ends, and the verdict comes one time. A space alone ran "could not
    /// run" into the name of the next leg.
    #[test]
    fn the_legs_of_a_refusal_are_separated_where_a_reason_ends() {
        let mut rows = all_ok();
        let emu = "no qemu-system-x86_64 on PATH, so the emulator leg could not run";
        let accel = "no qemu-system-x86_64 on PATH, so the accelerator list could not be read";
        rows[0] = ("qemu-system-x86_64 --version", Outcome::skip(Some(sys::ENOENT), emu));
        rows[1] = ("open(/dev/kvm, O_RDWR)", Outcome::denied(sys::ENOENT));
        rows[5] = ("qemu-system-x86_64 -accel help", Outcome::skip(Some(sys::ENOENT), accel));
        let refusal = assess(&findings(&rows)).refusal().expect("a refusal prints");
        assert_eq!(
            refusal,
            format!(
                "machine tier refused: qemu-system-x86_64 --version=skip: {emu}; \
                 open(/dev/kvm, O_RDWR)=ENOENT; qemu-system-x86_64 -accel help=skip: {accel}"
            )
        );
        assert_eq!(refusal.matches("refused").count(), 1, "{refusal}");
    }

    #[test]
    fn a_row_that_is_absent_is_missing_rather_than_any_verdict() {
        // A document written before the legs existed: five legs measured, the
        // sixth never run. Its absence refuses every profile; it is not read
        // as a denial and not as a pass.
        let mut rows = all_ok();
        rows.pop();
        let a = assess(&findings(&rows));
        assert_eq!(a.profile, None);
        assert!(a.refused());
        assert_eq!(a.missing, vec!["qemu-system-x86_64 -accel help"]);
        let refusal = a.refusal().expect("a refusal prints");
        assert!(refusal.contains("qemu-system-x86_64 -accel help=(not probed)"), "{refusal}");
    }

    #[test]
    fn the_accelerator_list_reads_words_not_substrings() {
        assert_eq!(accelerators_in("accelerators: kvm tcg xen"), vec!["kvm", "tcg", "xen"]);
        assert_eq!(accelerators_in(""), Vec::<&str>::new());
        // `tcg-3d` is not `tcg`: a substring match would arm TCG on a
        // list that never offered it.
        assert!(!accelerators_in("accelerators: kvm tcg-3d").contains(&"tcg"));
    }
}
