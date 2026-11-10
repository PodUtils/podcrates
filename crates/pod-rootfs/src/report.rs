//! The record of one completion: a row for each fixup, the commands that
//! the caller runs inside the root, and the C library of the root.

/// Each id of a [`Fixup`] and of a [`Step`] that the crate gives. A caller
/// that maps an id to a name of its own (podvm: the entry of its record)
/// checks its map against this list.
pub const IDS: &[&str] = &[
    "dev-node",
    "dev-shim",
    "dev-fd-link",
    "resolv.conf",
    "hosts",
    "identity",
    "nsswitch",
    "mtab",
    "pacman",
    "pacman-keyring",
    "apt",
    "apt-sandbox",
    "zypper",
    "zypper-index",
    "ca-bundle",
    "ca-hash-dir",
    "source-scheme",
    "source-reach",
];

/// What one fixup did. "The file was already right", "the crate changed
/// it" and "the crate could not" are three different statements, and the
/// last one never reads as one of the first two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// The crate created a file that was not there.
    Created,
    /// The crate replaced a file of the image.
    Rewrote,
    /// The crate put back the file of the image, and undid an earlier fixup.
    Restored,
    /// The caller ran a [`Step`] inside the root and it succeeded. It is a
    /// change: a command changed the image.
    Ran,
    /// Nothing to do: the image already holds what the crate writes.
    Unchanged,
    /// The crate did nothing, and the detail says why.
    Skipped,
    /// The crate tried and could not.
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
        matches!(self, Action::Created | Action::Rewrote | Action::Restored | Action::Ran)
    }
}

/// One row of the record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixup {
    /// A stable id of the fixup (`dev-shim`, `resolv.conf`, `ca-bundle`).
    pub id: &'static str,
    /// Relative to the root, with no leading slash, or empty where the row is
    /// not about one file.
    pub path: String,
    pub action: Action,
    pub detail: String,
    /// True where the result is not what the payload gets on a host that can
    /// do the real thing (a regular file in place of a device node, a CA of
    /// the host in the trust store of the image).
    pub degraded: bool,
}

impl Fixup {
    pub fn new(id: &'static str, path: &str, action: Action) -> Fixup {
        Fixup { id, path: path.to_string(), action, detail: String::new(), degraded: false }
    }

    pub fn why(mut self, detail: impl Into<String>) -> Fixup {
        self.detail = detail.into();
        self
    }

    pub fn degraded(mut self) -> Fixup {
        self.degraded = true;
        self
    }
}

/// A command that the caller runs **inside** the root, before the payload.
///
/// The crate runs on the host, before the payload enters the root, so each
/// fixup that it can make is a write. Two are not: `pacman-key --init` runs
/// `gpg` inside the root, and a hash-indexed CA directory gets its links from
/// a program that reads the certificates.
///
/// An argv, never a shell line: no file name of the image becomes an
/// argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// The id of the fixup that asked for it.
    pub id: &'static str,
    /// The program is an absolute path inside the root that the crate found
    /// there, not a name for a `PATH` search at exec time.
    pub argv: Vec<String>,
    /// What the payload gets when the step runs.
    pub why: String,
}

/// Each row of one completion, and what the crate cannot do from outside
/// the root.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub fixups: Vec<Fixup>,
    /// The commands that the caller runs inside the root, in this order,
    /// before the payload.
    pub steps: Vec<Step>,
    /// What [`crate::identity::probe_libc`] found.
    pub libc: Libc,
}

impl Report {
    /// Each row whose result is not what a host that can do the real thing
    /// gives the payload. A skipped row did nothing, so it is not one.
    pub fn degradations(&self) -> Vec<&Fixup> {
        self.fixups.iter().filter(|f| f.degraded && f.action != Action::Skipped).collect()
    }

    /// The count of the rows that changed a byte in the image.
    pub fn changed(&self) -> usize {
        self.fixups.iter().filter(|f| f.action.changed()).count()
    }

    /// The count of the failed rows.
    pub fn failed(&self) -> usize {
        self.fixups.iter().filter(|f| f.action == Action::Failed).count()
    }
}

/// The C library of the root, found on disk and not from the name of a
/// distribution.
///
/// musl reads `/etc/passwd` directly; glibc goes through NSS, so a supplied
/// `/etc/passwd` is read on one and can be ignored on the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Libc {
    Glibc,
    Musl,
    /// Both (a root with more than one architecture) or neither (a
    /// distroless or a scratch image).
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A skipped degraded row is not a degradation: nothing was done.
    #[test]
    fn a_skipped_fixup_is_not_a_degradation() {
        let mut r = Report::default();
        r.fixups.push(Fixup::new("dev-shim", "dev/null", Action::Skipped).why("already a character device").degraded());
        r.fixups.push(Fixup::new("dev-shim", "dev/zero", Action::Created).degraded());
        assert_eq!(r.degradations().len(), 1);
        assert_eq!(r.degradations()[0].path, "dev/zero");
        assert_eq!(r.changed(), 1);
        assert_eq!(r.failed(), 0);
    }

    /// Each id that the source gives is in [`IDS`], and each id of [`IDS`]
    /// is in the source.
    #[test]
    fn each_id_of_the_source_is_listed() {
        let sources = [
            include_str!("devices.rs"),
            include_str!("identity.rs"),
            include_str!("net.rs"),
            include_str!("pkg.rs"),
            include_str!("pkg/bundle.rs"),
            include_str!("pkg/hash_dir.rs"),
            include_str!("sources.rs"),
        ];
        let mut found = Vec::new();
        for text in sources {
            for head in ["Fixup::new(\"", "id: \"", "record(r, \"", "record_steps(r, \"", "let id = \""] {
                for (at, _) in text.match_indices(head) {
                    let rest = &text[at + head.len()..];
                    found.push(rest[..rest.find('"').unwrap()].to_string());
                }
            }
        }
        for id in &found {
            assert!(IDS.contains(&id.as_str()), "{id} is not in IDS");
        }
        for id in IDS {
            assert!(found.iter().any(|f| f == id), "{id} of IDS is not in the source");
        }
    }
}
