//! The completion of a root file system: the files that a payload reads
//! before it can work, made from the host and not from inside the root.
//!
//! An extracted OCI root file system is not a working environment on a host
//! that refuses `mknod`, `mount` and `chown`: it has no `/dev/null`, its
//! resolver is the resolver of the build host, its `/etc/mtab` is a symlink
//! into a `/proc` that is not mounted, and on a glibc root a file that most
//! images do not ship decides if a program reads `/etc/passwd` at all.
//!
//! Three rules:
//!
//! 1. **Each write goes through [`write::Root`]**. It resolves each
//!    directory component held in the root (`openat2(RESOLVE_IN_ROOT)`),
//!    opens the last component with `O_NOFOLLOW`, and replaces a file with
//!    an unlink and a create. A fixup that needs a COMMAND inside the root
//!    gives a [`Step`]: the caller runs it. A library that forks and enters
//!    a root is a library that no test can drive.
//! 2. **Each fixup is in the [`Report`], one row for each file.** The crate
//!    changed a file in an image of somebody else, and the payload can see
//!    the change. A row that is not what a host with the real thing gives is
//!    `degraded`.
//! 3. **A fixup does not fail the run.** A fixup that the crate cannot apply
//!    is a [`Action::Failed`] row. The caller decides what a failed or a
//!    degraded row means.
//!
//! The completion is idempotent, and the caller runs it before each start of
//! a payload: a root file system can be shared between containers, so a
//! fixup made one time at extraction is the decision of one container for
//! each later one. Each run also repairs a `/dev/null` that a payload turned
//! into a regular file.
//!
//! The caller gives each setting ([`Settings`]): the names that the crate
//! writes into the root ([`Names`]), the CA bundle of the host
//! ([`HostBundle`]), the resolver and the mount table of the host, and the
//! directory for the originals of the package sources. The question "does
//! this mirror speak HTTPS" is the caller's ([`reach::Probe`]): the crate
//! makes no request. The crate reads no variable and prints nothing. Each
//! failure is a [`pod_core::Failure`] at `Stage::Host` ([`error`]).
//!
//! The crate is Linux only: on another target it is empty.
#![cfg(target_os = "linux")]
#![forbid(unsafe_op_in_unsafe_fn)]

pub mod devices;
pub mod error;
pub mod identity;
pub mod net;
pub mod pkg;
pub mod reach;
mod report;
pub mod sources;
pub mod write;

#[cfg(test)]
mod testing;

use std::path::{Path, PathBuf};

pub use error::Result;
pub use report::{Action, Fixup, IDS, Libc, Report, Step};

/// The names that the crate writes into a root file system. A second run
/// finds the files of the first by these names, so a caller keeps its
/// values from one release to the next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Names {
    /// The name of the caller in the head comment of each file that the
    /// crate writes (`nsswitch.conf`, the drop-in of apt, a CA file).
    pub tool: String,
    /// The text before each line that the crate comments out (the
    /// `DownloadUser` lines of `pacman.conf`).
    pub comment: String,
    /// The drop-in of apt that sets `APT::Sandbox::User`, relative to the
    /// root.
    pub apt_drop_in: String,
    /// The line before the host bundle that the crate appends to a CA
    /// bundle of the image. A bundle that holds it is not appended again.
    pub host_ca_marker: String,
    /// The prefix of each certificate file that the crate writes into a
    /// hash-indexed CA directory. The crate removes a file with this prefix
    /// that the host bundle no longer holds.
    pub host_ca_prefix: String,
    /// The prefix of the temporary name of a device node before its rename
    /// into place.
    pub scratch_prefix: String,
}

impl Default for Names {
    fn default() -> Names {
        Names {
            tool: "pod-rootfs".to_string(),
            comment: "# pod-rootfs: ".to_string(),
            apt_drop_in: "etc/apt/apt.conf.d/99pod-rootfs".to_string(),
            host_ca_marker: "# pod-rootfs: the CA bundle of the host".to_string(),
            host_ca_prefix: "pod-rootfs-host-ca-".to_string(),
            scratch_prefix: ".pod-rootfs-mknod-".to_string(),
        }
    }
}

/// The CA bundle of the host, which the caller found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostBundle {
    /// Where the bundle is on the host, for the rows.
    pub path: String,
    /// The variable that named the bundle (`SSL_CERT_FILE`), where one did.
    /// A host that names its bundle tells each tool which roots to use: a
    /// proxy that signs TLS again. `None`: a bundle at a default path, which
    /// tells nothing. The crate appends a bundle to the trust store of an
    /// image only when a variable named it.
    pub announced_by: Option<String>,
    /// The PEM bytes.
    pub bytes: Vec<u8>,
}

/// The values that the caller gives. The defaults change the least in the
/// image of somebody else.
#[derive(Debug, Clone)]
pub struct Settings {
    pub names: Names,
    /// The settings of `pod-hostprobe`. The space message of the device
    /// shims names its `directory_setting`.
    pub host: pod_hostprobe::Settings,
    /// The name of the container, for `/etc/hosts`.
    pub container_name: Option<String>,
    /// More lines of `/etc/hosts`, each `name:address` (docker's
    /// `--add-host`).
    pub add_hosts: Vec<String>,
    /// Change `http://` package sources to `https://`. When false, the crate
    /// puts back the original of each source that an earlier run changed.
    pub source_fixup: bool,
    /// Append the host bundle to the trust store of the image, when a
    /// variable named the bundle, and write it into an image that has no
    /// bundle. When false, the crate writes neither.
    pub host_cas: bool,
    /// The caller runs the [`Step`]s. When false, a fixup that needs a
    /// command is not done, and its row says what the payload does not get.
    pub steps: bool,
    /// The CA bundle of the host. `None`: the host has none.
    pub host_bundle: Option<HostBundle>,
    /// The resolver of the host.
    pub host_resolv_conf: PathBuf,
    /// The mount table of the host.
    pub mounts: PathBuf,
    /// The directory for the originals of the package sources that the
    /// crate changes. Keep it outside the root: a payload can change a file
    /// in the root, and the crate then puts back what the payload wrote.
    /// `None`: the crate does not change a package source.
    pub originals: Option<PathBuf>,
    /// The upper layer of a root of two layers, as overlayfs mounts them.
    /// With it, the root of [`complete`] is the lower layer: each read sees
    /// this layer over it, each write goes to this layer, and the lower
    /// layer is never opened for write ([`write::layers`]). `None`: one
    /// root, which takes the writes.
    pub upper: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            names: Names::default(),
            host: pod_hostprobe::Settings::default(),
            container_name: None,
            add_hosts: Vec::new(),
            source_fixup: true,
            host_cas: true,
            steps: true,
            host_bundle: None,
            host_resolv_conf: PathBuf::from("/etc/resolv.conf"),
            mounts: PathBuf::from("/proc/self/mounts"),
            originals: None,
            upper: None,
        }
    }
}

/// Prepare the root file system at `rootfs` for a payload, and say what was
/// done. `probe` answers if a mirror speaks HTTPS. With
/// [`Settings::upper`], `rootfs` is the lower layer of two.
///
/// A fixup that the crate cannot apply is a [`Action::Failed`] row, not a
/// failure: a root that did not get a resolver can still be of use to the
/// payload. The failure of this function is a root that does not open, or a
/// setting that cannot be used.
pub fn complete(rootfs: &Path, settings: &Settings, probe: &mut dyn reach::Probe) -> Result<Report> {
    check_names(&settings.names)?;
    let root = match &settings.upper {
        Some(upper) => write::Root::layers(rootfs, upper)?,
        None => write::Root::open(rootfs)?,
    };
    let mut r = Report { libc: identity::probe_libc(&root)?, ..Report::default() };

    // The identity fixups see the root as the image shipped it, and the
    // package fixups can read what the identity fixups wrote. The others do
    // not depend on each other.
    devices::apply(&root, settings, &mut r);
    net::resolver(&root, settings, &mut r);
    net::hosts(&root, settings, &mut r);
    identity::passwd_and_group(&root, &mut r);
    identity::nsswitch(&root, &settings.names, &mut r);
    net::mtab(&root, settings, &mut r);
    pkg::apply(&root, settings, &mut r);
    sources::apply(&root, settings, probe, &mut r);
    Ok(r)
}

/// A name with a `/` or a line end writes outside the place that it names.
fn check_names(n: &Names) -> Result<()> {
    let one_line = [("tool", &n.tool), ("comment", &n.comment), ("host_ca_marker", &n.host_ca_marker)];
    for (what, v) in one_line {
        if v.contains(['\n', '\r', '\0']) {
            return Err(error::usage(format!("the name {what} {v:?} holds a line end or a NUL")));
        }
    }
    for (what, v) in [("host_ca_prefix", &n.host_ca_prefix), ("scratch_prefix", &n.scratch_prefix)] {
        if v.is_empty() || v.contains(['/', '\n', '\r', '\0']) || v == "." || v == ".." {
            return Err(error::usage(format!("the name {what} {v:?} is not one file name")));
        }
    }
    if n.host_ca_marker.trim().is_empty() {
        return Err(error::usage("the name host_ca_marker is empty"));
    }
    write::check_relative(&n.apt_drop_in)
}

/// The [`Action`] of a [`write::Wrote`].
pub(crate) fn act(w: write::Wrote) -> Action {
    match w {
        write::Wrote::Created => Action::Created,
        write::Wrote::Replaced => Action::Rewrote,
        write::Wrote::Unchanged => Action::Unchanged,
    }
}

/// Add the rows of a fixup to the report, or one failed row.
fn record(r: &mut Report, id: &'static str, out: Result<Vec<Fixup>>) {
    record_steps(r, id, out.map(|fs| (fs, Vec::new())))
}

/// The same, for a fixup that can also ask for a [`Step`]. A fixup that
/// failed gives no step: a command for a write that did not happen does
/// nothing of use.
fn record_steps(r: &mut Report, id: &'static str, out: Result<(Vec<Fixup>, Vec<Step>)>) {
    match out {
        Ok((fs, st)) => {
            r.fixups.extend(fs);
            r.steps.extend(st);
        }
        Err(e) => r.fixups.push(Fixup::new(id, "", Action::Failed).why(e.to_string()).degraded()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Scratch, no_in_root};

    /// Each fixup gives its rows, and a second run of the same settings
    /// changes nothing: the completion runs before each start.
    #[test]
    fn a_second_completion_changes_nothing() {
        if no_in_root() {
            return;
        }
        let d = Scratch::new("lib-twice");
        std::fs::create_dir_all(d.at("rootfs/etc/apt")).unwrap();
        std::fs::write(d.at("rootfs/etc/apt/sources.list"), "deb http://deb.debian.org/debian bookworm main\n")
            .unwrap();
        let resolv = d.at("resolv.conf");
        std::fs::write(&resolv, "nameserver 10.1.2.3\n").unwrap();
        let settings = Settings { host_resolv_conf: resolv, originals: Some(d.at("originals")), ..Settings::default() };
        let first = complete(&d.at("rootfs"), &settings, &mut reach::Fixed(true)).unwrap();
        for id in ["resolv.conf", "hosts", "identity", "mtab", "apt-sandbox", "source-scheme"] {
            assert!(first.fixups.iter().any(|f| f.id == id && f.action.changed()), "{id}: {first:#?}");
        }
        assert_eq!(first.failed(), 0, "{first:#?}");
        let second = complete(&d.at("rootfs"), &settings, &mut reach::Fixed(true)).unwrap();
        assert_eq!(second.changed(), 0, "{second:#?}");
    }

    /// A name that writes outside its place is a refused setting.
    #[test]
    fn a_name_with_a_slash_or_a_line_end_is_refused() {
        let bad = [
            Names { host_ca_prefix: "../x".into(), ..Names::default() },
            Names { scratch_prefix: "".into(), ..Names::default() },
            Names { host_ca_marker: "# a\nb".into(), ..Names::default() },
            Names { apt_drop_in: "/etc/apt/apt.conf.d/99x".into(), ..Names::default() },
            Names { apt_drop_in: "etc/../../x".into(), ..Names::default() },
        ];
        for n in bad {
            let e = check_names(&n).unwrap_err();
            assert!(matches!(e.class, pod_core::Class::Usage | pod_core::Class::Policy), "{e}");
        }
        check_names(&Names::default()).unwrap();
    }
}
