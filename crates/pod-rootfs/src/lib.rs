//! `TOOL.md` section 6.4: environment completion. Milestone M5,
//! [`TODO/milestones.md`](../../../TODO/milestones.md) T-1106 and
//! [`TODO/complete.md`](../../../TODO/complete.md) T-0401 to T-0411.
//!
//! ⭐ **The layer that makes the runtime useful without the user learning
//! anything.** An extracted OCI rootfs is not a working environment on a
//! machine that denies `mknod`, `mount` and `chown`: it has no `/dev/null`, its
//! resolver is the build host's, its `/etc/mtab` is a symlink into a `/proc`
//! that is not mounted, and on a glibc rootfs whether anything reads
//! `/etc/passwd` at all is decided by a file most images do not ship.
//!
//! Three rules decide whether this crate is right rather than merely finished:
//!
//! 1. ⛔ **Every write goes through [`write::Root`]**, which resolves each
//!    component with symlinks refused and replaces by unlink-then-create. T-0405
//!    is that mechanism and not a special case for `/etc/mtab`;
//!    ⚠ and where a fixup needs a COMMAND rather than a write, this crate
//!    returns a [`Step`] and the caller runs it. T-0412: a library that forks
//!    and chroots is one no test can drive;
//! 2. ⛔ **Every fixup is reported, on the banner, per file.** podvm has
//!    edited a file inside somebody else's image and the payload can see it.
//!    [`TODO/cli.md`](../../../TODO/cli.md) T-0804's honesty rules have no
//!    exception for a helpful edit;
//! 3. ⛔ **A fixup never changes the payload's exit code.** T-0409 and T-0802:
//!    the exit code is the payload's answer, and a runtime that improves it is
//!    lying in the one field an automated caller reads first.
//!
//! ⚠ **It is idempotent and it runs on every `run` and `exec`.** The rootfs is
//! content-addressed and shared between containers
//! ([`TODO/image.md`](../../../TODO/image.md) T-0204), so a fixup made once at
//! extraction time would be one container's decision imposed on every later
//! one. Running it every time also repairs the failure T-0401 exists for: a
//! `/dev/null` that a previous payload turned into a growing regular file is
//! truncated back to nothing.

#![forbid(unsafe_op_in_unsafe_fn)]

pub mod devices;
pub mod identity;
pub mod net;
pub mod pkg;
pub mod reach;
pub mod sources;
pub mod write;

use std::fmt;

/// ⚠ One error type, and it carries a sentence rather than a code. Every
/// failure here is reported to a human or an agent reading stderr, and the
/// caller's decision is the same for all of them: say so and go on, or, under
/// `--strict`, refuse.
#[derive(Debug)]
pub enum Error {
    Complete(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Complete(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// What the caller asked for. ⛔ Defaults are the conservative ones: a field
/// that changes somebody else's image defaults to the value that changes least.
#[derive(Debug, Clone)]
pub struct Options {
    /// The container's name, for `/etc/hosts`. T-0403.
    pub container_name: Option<String>,
    /// `--add-host name:ip`, repeatable. T-0403.
    pub add_hosts: Vec<String>,
    /// T-0411. `--no-source-fixup` clears it, and then any rewrite a previous
    /// run made is **undone** rather than merely not repeated.
    pub source_fixup: bool,
    /// T-0407. ⭐ Append this machine's announced CA bundle to the image's own
    /// trust store, so an `https://` package source verifies where the machine
    /// intercepts TLS. `--no-host-cas` clears it. ⚠ It does nothing at all
    /// unless the machine announced a bundle through `$SSL_CERT_FILE`,
    /// `$CURL_CA_BUNDLE` or `$REQUESTS_CA_BUNDLE`.
    pub host_cas: bool,
    /// Where the host's resolver is read from. ⚠ A field rather than a constant
    /// so the tests can drive T-0402 without a host `/etc/resolv.conf`.
    pub host_resolv_conf: String,
    /// ⭐ T-0412. May podvm ask the caller to run a COMMAND inside the rootfs?
    /// `--no-steps` clears it, and then every fixup whose remedy is a command
    /// reports what the caller gave up instead of asking for one.
    ///
    /// ⛔ It gates the fixup and not only the command. The openSUSE arm writes
    /// one PEM per root into a hash-indexed directory, and a certificate in
    /// such a directory that nothing has hash-linked is a file no TLS stack
    /// reads: writing it and then refusing to link it would leave litter in
    /// somebody else's image for no benefit.
    pub steps: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            container_name: None,
            add_hosts: Vec::new(),
            source_fixup: true,
            host_cas: true,
            host_resolv_conf: "/etc/resolv.conf".to_string(),
            steps: true,
        }
    }
}

/// What one fixup did. ⛔ Four outcomes and not two: "already right" and
/// "podvm changed it" are different sentences, and "podvm could not" is a
/// third state that must never read as either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// podvm created a file that was not there.
    Created,
    /// podvm replaced a file the image shipped.
    Rewrote,
    /// podvm put back what the image shipped, undoing an earlier fixup.
    Restored,
    /// ⭐ T-0412. podvm ran a [`Step`] inside the rootfs and it succeeded.
    /// ⚠ It counts as a change: a command podvm executed inside somebody
    /// else's image edited it, and the container record has to carry that as
    /// much as it carries a file podvm wrote.
    Ran,
    /// Nothing to do: the image already carries what podvm would have written.
    Unchanged,
    /// podvm did not act, and the reason is in the detail.
    Skipped,
    /// podvm tried and could not. ⛔ Never silently folded into `Skipped`.
    Failed,
}

impl Action {
    pub fn word(self) -> &'static str {
        match self {
            Action::Created => "created",
            Action::Rewrote => "rewrote",
            Action::Restored => "restored",
            Action::Ran => "ran",
            Action::Unchanged => "unchanged",
            Action::Skipped => "skipped",
            Action::Failed => "failed",
        }
    }

    /// Did this fixup change a byte in the image?
    pub fn changed(self) -> bool {
        matches!(
            self,
            Action::Created | Action::Rewrote | Action::Restored | Action::Ran
        )
    }
}

/// One line of the completion record.
#[derive(Debug, Clone)]
pub struct Fixup {
    /// The entry that owns it, so a reader can find the reasoning.
    pub entry: &'static str,
    /// A stable, greppable id.
    pub id: &'static str,
    /// Rootfs-relative, with no leading slash, or `-` where the fixup is not
    /// about one file.
    pub path: String,
    pub action: Action,
    pub detail: String,
    /// ⭐ T-0804. True where the result is not what the payload would get on a
    /// machine that could do the real thing. `--strict` refuses the run when
    /// any of these is set, so a caller demanding real semantics does not have
    /// to parse the banner.
    pub degraded: bool,
}

impl Fixup {
    fn new(entry: &'static str, id: &'static str, path: &str, action: Action) -> Fixup {
        Fixup {
            entry,
            id,
            path: path.to_string(),
            action,
            detail: String::new(),
            degraded: false,
        }
    }

    fn why(mut self, detail: impl Into<String>) -> Fixup {
        self.detail = detail.into();
        self
    }

    fn degraded(mut self) -> Fixup {
        self.degraded = true;
        self
    }
}

/// A command podvm needs run **inside** the rootfs, which this crate cannot
/// run itself.
///
/// ⭐ [`TODO/complete.md`](../../../TODO/complete.md) T-0412. The completion
/// layer runs on the host, before the chroot, so every fixup it can make is a
/// write. Two are not: `pacman-key --init` runs `gpg` inside the rootfs
/// (T-0406), and a hash-indexed CA directory is indexed by a program that reads
/// the certificates (T-0412). This crate returns them and the caller orders
/// them, because the banner has to name a command before it runs and only the
/// caller knows what else it is about to print.
///
/// ⛔ **An argv, never a shell line.** Nothing here is word-split, so no
/// filename inside the image can become an argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// The entry that owns it, so a reader can find the reasoning.
    pub entry: &'static str,
    /// A stable, greppable id, shared with the fixup row that asked for it.
    pub id: &'static str,
    /// ⛔ Absolute inside the rootfs, and the program was found there by this
    /// crate rather than looked up on a `PATH` at exec time: a step announced
    /// on the banner and then not found is a command podvm promised and did
    /// not run.
    pub argv: Vec<String>,
    /// What the payload gets if it runs, said in the banner before it does.
    pub why: String,
}

/// Everything the completion layer did to one rootfs, and what it could not do
/// from outside it.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub fixups: Vec<Fixup>,
    /// ⭐ T-0412. Commands the caller runs inside the rootfs, in this order,
    /// before the payload.
    pub steps: Vec<Step>,
    /// What [`identity::probe`] found, because three other fixups branch on it
    /// and `inspect` prints it.
    pub libc: Libc,
}

impl Report {
    /// The banner half. ⛔ One line per fixup that CHANGED something, and one
    /// summary line. A run that changed nothing prints the summary alone, so
    /// the absence of edits is stated rather than inferred from silence.
    pub fn banner(&self) -> String {
        let mut s = String::new();
        for f in &self.fixups {
            if !f.action.changed() && f.action != Action::Failed {
                continue;
            }
            s.push_str(&format!(
                "podvm: complete: {} {} ({}, {}){}\n",
                f.action.word(),
                if f.path.is_empty() { "-" } else { &f.path },
                f.id,
                f.entry,
                if f.detail.is_empty() {
                    String::new()
                } else {
                    format!(": {}", f.detail)
                }
            ));
        }
        // ⭐ T-0412 rule 3. Every step is NAMED HERE, before the caller runs it,
        // because podvm is about to execute a command inside somebody else's
        // image that the caller did not write.
        for (i, st) in self.steps.iter().enumerate() {
            s.push_str(&format!(
                "podvm: complete: step {} of {}: `{}` ({}, {}): {}\n",
                i + 1,
                self.steps.len(),
                st.argv.join(" "),
                st.id,
                st.entry,
                st.why
            ));
        }
        let changed = self.fixups.iter().filter(|f| f.action.changed()).count();
        let failed = self
            .fixups
            .iter()
            .filter(|f| f.action == Action::Failed)
            .count();
        let degraded = self.degradations().len();
        s.push_str(&format!(
            "podvm: complete: {} fixup(s) applied to this image's rootfs, {} \
             degraded, {} failed, {} step(s) to run inside it; libc={}. refused: These \
             are edits podvm made inside somebody else's image and the payload \
             can see them\n",
            changed,
            degraded,
            failed,
            self.steps.len(),
            self.libc.word()
        ));
        s
    }

    /// ⭐ T-0804's input. Every fixup whose result is not what a machine that
    /// could do the real thing would give the payload.
    pub fn degradations(&self) -> Vec<&Fixup> {
        self.fixups
            .iter()
            .filter(|f| f.degraded && f.action != Action::Skipped)
            .collect()
    }

    /// The record as JSON, for `inspect` and for a test that has to assert on
    /// it. ⚠ Hand-serialised for the same reason the ownership sidecar is: the
    /// field order is the contract.
    pub fn json(&self) -> String {
        let rows: Vec<serde_json::Value> = self
            .fixups
            .iter()
            .map(|f| {
                serde_json::json!({
                    "entry": f.entry,
                    "id": f.id,
                    "path": f.path,
                    "action": f.action.word(),
                    "detail": f.detail,
                    "degraded": f.degraded,
                })
            })
            .collect();
        let steps: Vec<serde_json::Value> = self
            .steps
            .iter()
            .map(|s| {
                serde_json::json!({
                    "entry": s.entry,
                    "id": s.id,
                    "argv": s.argv,
                    "why": s.why,
                })
            })
            .collect();
        serde_json::json!({ "libc": self.libc.word(), "fixups": rows, "steps": steps }).to_string()
    }
}

/// Which C library the rootfs carries, measured rather than inferred from a
/// distribution name.
///
/// ⭐ T-0410 turns on this: musl reads `/etc/passwd` directly and glibc goes
/// through NSS, so the same supplied file is read on one and ignored on the
/// other. `experiments/results/nsswitch-contract.txt` is the measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Libc {
    Glibc,
    Musl,
    /// Both, which a multi-arch or a mixed rootfs can be, and neither, which a
    /// distroless or a scratch image is. ⛔ Not folded into a guess.
    #[default]
    Unknown,
}

impl Libc {
    pub fn word(self) -> &'static str {
        match self {
            Libc::Glibc => "glibc",
            Libc::Musl => "musl",
            Libc::Unknown => "unknown",
        }
    }
}

/// Prepare `rootfs` for a payload, and say what was done.
///
/// ⛔ **Never fails the run on a fixup that could not be applied.** A rootfs
/// podvm could not write a resolver into is one the payload may still be able
/// to use, and refusing here would make podvm less useful than the bare
/// `chroot` it replaces. The failure is a `Failed` row, the banner names it, and
/// `--strict` is how a caller turns it into a refusal.
pub fn complete(rootfs: &str, opts: &Options) -> Result<Report> {
    let root = write::Root::open(rootfs)?;
    let mut r = Report {
        libc: identity::probe_libc(&root)?,
        ..Report::default()
    };

    // ⚠ Order matters in exactly one place: the identity fixups must see the
    // rootfs as the image shipped it, and the package-manager fixups may read
    // what the identity ones wrote. Everything else is independent.
    devices::apply(&root, &mut r);
    net::resolver(&root, opts, &mut r);
    net::hosts(&root, opts, &mut r);
    identity::passwd_and_group(&root, &mut r);
    identity::nsswitch(&root, &mut r);
    net::mtab(&root, &mut r);
    pkg::apply(&root, opts, &mut r);
    sources::apply(&root, opts, &mut r);
    Ok(r)
}

/// One [`Action`] per [`write::Wrote`], in one place.
///
/// ⛔ It lived in four modules before this and that is exactly the shape
/// `docs/conventions/code.md` refuses: four copies of a mapping, and the one
/// nobody exercises is the one that diverges.
pub(crate) fn act(w: write::Wrote) -> Action {
    match w {
        write::Wrote::Created => Action::Created,
        write::Wrote::Replaced => Action::Rewrote,
        write::Wrote::Unchanged => Action::Unchanged,
    }
}

/// Collect a fixup that may have failed, so a call site is one line rather than
/// a `match` per fixup.
fn record(r: &mut Report, entry: &'static str, id: &'static str, out: Result<Vec<Fixup>>) {
    record_steps(r, entry, id, out.map(|fs| (fs, Vec::new())))
}

/// The same, for a fixup that may also ask for a [`Step`].
///
/// ⛔ A fixup that could not be applied contributes NO step. The two travel
/// together through one `Result` for exactly that reason: a step asking for a
/// command whose write half failed is a command run for nothing.
fn record_steps(
    r: &mut Report,
    entry: &'static str,
    id: &'static str,
    out: Result<(Vec<Fixup>, Vec<Step>)>,
) {
    match out {
        Ok((fs, st)) => {
            r.fixups.extend(fs);
            r.steps.extend(st);
        }
        Err(e) => r.fixups.push(
            Fixup::new(entry, id, "", Action::Failed)
                .why(e.to_string())
                .degraded(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_that_changed_nothing_still_says_so() {
        let r = Report::default();
        let b = r.banner();
        assert!(b.contains("0 fixup(s) applied"), "{b}");
    }

    /// ⛔ The distinction T-0804 reads. A `Skipped` degradation is not one:
    /// nothing was done, so there is nothing for `--strict` to refuse.
    #[test]
    fn a_skipped_fixup_is_not_a_degradation() {
        let mut r = Report::default();
        r.fixups.push(
            Fixup::new("T-0401", "dev-shim", "dev/null", Action::Skipped)
                .why("already a character device")
                .degraded(),
        );
        r.fixups
            .push(Fixup::new("T-0401", "dev-shim", "dev/zero", Action::Created).degraded());
        assert_eq!(r.degradations().len(), 1);
        assert_eq!(r.degradations()[0].path, "dev/zero");
    }
}
