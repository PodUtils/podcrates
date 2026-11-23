//! The hardening of a unit of systemd: the lines that `systemd-analyze
//! security` scores, each on by default. A program that needs a line
//! relaxed says which line and why ([`Hardening::relax`]): the unit keeps
//! the line as a comment with the reason, so that a reader and
//! [`crate::systemd::audit`] see each gap. The base is podssh's hardening of
//! a system unit (`node_install/system_text.rs`).
//!
//! A user manager of systemd applies a line that needs a mount namespace
//! only through a user namespace, and a host with no user namespaces then
//! refuses the start. A unit of the user takes the lines of seccomp alone
//! ([`Need::Seccomp`]); a unit of the system takes each line.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What the manager needs to apply a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// A filter of seccomp, or a setting of the process: each manager.
    Seccomp,
    /// A mount namespace or a privilege: the manager of the system.
    System,
}

/// One line of the hardening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    pub key: &'static str,
    pub value: &'static str,
    pub need: Need,
}

const fn line(key: &'static str, value: &'static str, need: Need) -> Line {
    Line { key, value, need }
}

/// Each line of the hardening, in the order of the unit.
pub const LINES: &[Line] = &[
    line("NoNewPrivileges", "yes", Need::Seccomp),
    line("CapabilityBoundingSet", "", Need::System),
    line("AmbientCapabilities", "", Need::System),
    line("RestrictSUIDSGID", "yes", Need::Seccomp),
    line("ProtectSystem", "strict", Need::System),
    line("ProtectHome", "tmpfs", Need::System),
    line("PrivateTmp", "yes", Need::System),
    line("PrivateDevices", "yes", Need::System),
    line("ProtectKernelTunables", "yes", Need::System),
    line("ProtectKernelModules", "yes", Need::System),
    line("ProtectKernelLogs", "yes", Need::System),
    line("ProtectControlGroups", "yes", Need::System),
    line("ProtectClock", "yes", Need::System),
    line("ProtectHostname", "yes", Need::System),
    line("ProtectProc", "invisible", Need::System),
    line("RestrictNamespaces", "yes", Need::Seccomp),
    line("RestrictRealtime", "yes", Need::Seccomp),
    line("LockPersonality", "yes", Need::Seccomp),
    line("MemoryDenyWriteExecute", "yes", Need::Seccomp),
    line("SystemCallArchitectures", "native", Need::Seccomp),
    line("SystemCallFilter", "@system-service", Need::Seccomp),
    line("SystemCallErrorNumber", "EPERM", Need::Seccomp),
    line("RestrictAddressFamilies", "AF_INET AF_INET6 AF_UNIX", Need::Seccomp),
    line("RemoveIPC", "yes", Need::System),
    line("UMask", "0077", Need::Seccomp),
];

/// The address families that a program may add, each with its reason
/// (`AF_NETLINK`: the kernel gives the addresses of the host by netlink).
const FAMILIES: &[&str] = &["AF_NETLINK", "AF_PACKET", "AF_VSOCK"];

/// The hardening of one unit: the lines that are relaxed, the address
/// families added, and the paths that the lines must let through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hardening {
    relaxed: BTreeMap<&'static str, String>,
    families: Vec<(&'static str, String)>,
    /// `ReadWritePaths=`: written where `ProtectSystem=strict` makes the tree
    /// read-only.
    pub write: Vec<PathBuf>,
    /// `BindPaths=`: under a home that `ProtectHome=tmpfs` hides, written.
    pub bind: Vec<PathBuf>,
    /// `BindReadOnlyPaths=`: under a home, read.
    pub bind_read: Vec<PathBuf>,
    /// The lines that a program adds past the table (`PrivateUsers=yes`,
    /// `ProcSubset=pid`, `IPAddressDeny=any`), in their order.
    extra: Vec<(String, String)>,
    /// True: the named account of the unit is the program's own
    /// ([`Hardening::service_account`]).
    own_account: bool,
}

impl Hardening {
    /// Relax the line `key`, with the reason that the unit says. A key that
    /// is not a line of [`LINES`] is refused.
    pub fn relax(&mut self, key: &str, why: &str) -> Result<&mut Hardening, pod_core::Failure> {
        let Some(line) = LINES.iter().find(|l| l.key == key) else {
            return Err(crate::model::usage(format!("{key}: no line of the hardening has this key")));
        };
        if key == "SystemCallFilter" {
            // The number of the error goes with the filter.
            self.relaxed.insert("SystemCallErrorNumber", why.to_string());
        }
        self.relaxed.insert(line.key, why.to_string());
        Ok(self)
    }

    /// Add the address family `family` (`AF_NETLINK`, `AF_PACKET`,
    /// `AF_VSOCK`), with the reason that the unit says.
    pub fn family(&mut self, family: &str, why: &str) -> Result<&mut Hardening, pod_core::Failure> {
        let Some(known) = FAMILIES.iter().find(|f| **f == family) else {
            return Err(crate::model::usage(format!("{family}: give one of {}", FAMILIES.join(", "))));
        };
        if !self.families.iter().any(|(f, _)| f == known) {
            self.families.push((known, why.to_string()));
        }
        Ok(self)
    }

    /// Add the line `KEY=VALUE` past the table, for each scope. A key of the
    /// table is refused: relax it, or keep its value.
    pub fn add(&mut self, key: &str, value: &str) -> Result<&mut Hardening, pod_core::Failure> {
        let fine = !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric());
        if !fine || LINES.iter().any(|l| l.key == key) {
            return Err(crate::model::usage(format!("{key}: give a key of letters and digits that the table has not")));
        }
        self.extra.push((key.to_string(), value.to_string()));
        Ok(self)
    }

    /// Say that the named account of a unit of the system (`User=NAME`) is
    /// the program's own, which no person uses: the unit keeps
    /// `RemoveIPC=yes`. With no call, a unit with a named account relaxes
    /// that line, as the account can be a person's, whose IPC a stop of the
    /// unit would remove. A unit with `DynamicUser=yes` keeps it.
    pub fn service_account(&mut self) -> &mut Hardening {
        self.own_account = true;
        self
    }

    /// The lines that a program added past the table.
    pub fn added(&self) -> impl Iterator<Item = (&str, &str)> {
        self.extra.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// The reason why `key` is relaxed, when it is.
    pub fn relaxed(&self, key: &str) -> Option<&str> {
        self.relaxed.get(key).map(String::as_str)
    }

    /// Each path that the lines let through, with its key.
    pub fn paths(&self) -> impl Iterator<Item = (&'static str, &Path)> {
        let write = self.write.iter().map(|p| ("ReadWritePaths", p.as_path()));
        let bind = self.bind.iter().map(|p| ("BindPaths", p.as_path()));
        let read = self.bind_read.iter().map(|p| ("BindReadOnlyPaths", p.as_path()));
        write.chain(bind).chain(read)
    }

    /// The lines of the unit for a manager of `system` (true) or of a user,
    /// which runs as the named account `named` when one is given: each line,
    /// or its comment when it is relaxed; the families after their reasons;
    /// then the paths.
    pub(crate) fn lines(&self, system: bool, named: Option<&str>) -> Vec<String> {
        let mut out = vec!["# The hardening, which systemd-analyze security scores.".to_string()];
        if !system {
            out.push(
                "# A user manager applies a line of a mount namespace only in a user namespace: the lines of seccomp \
                 alone."
                    .to_string(),
            );
        }
        for line in LINES.iter().filter(|l| system || l.need == Need::Seccomp) {
            let mut text = format!("{}={}", line.key, line.value);
            if line.key == "RestrictAddressFamilies" {
                for (family, why) in &self.families {
                    out.push(format!("# {family}: {why}."));
                    text.push(' ');
                    text.push_str(family);
                }
            }
            let person = named.filter(|_| line.key == "RemoveIPC" && !self.own_account);
            match (self.relaxed.get(line.key), person) {
                (Some(why), _) => out.push(format!("# Relaxed, as {why}: {text}")),
                (None, Some(user)) => out.push(format!(
                    "# Relaxed, as the account {user} can be a person's, whose IPC a stop would remove: {text}"
                )),
                (None, None) => out.push(text),
            }
        }
        for (key, value) in &self.extra {
            out.push(format!("{key}={value}"));
        }
        if system {
            for (key, path) in self.paths() {
                out.push(format!("{key}=-{}", path.display()));
            }
        }
        out
    }
}
