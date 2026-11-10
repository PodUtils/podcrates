//! Who the payload thinks it is, and whether anything reads the file that says
//! so.
//!
//! - [`passwd_and_group`] -- [`TODO/complete.md`](../../../TODO/complete.md)
//!   T-0404;
//! - [`nsswitch`] -- T-0410, and ⭐ **it is the entry that decides whether T-0404
//!   does anything at all.**
//!
//! ⛔ **On a glibc rootfs a supplied `/etc/passwd` is a no-op unless
//! `/etc/nsswitch.conf` names `files`.** Measured, with a user name no
//! distribution ships so a hit cannot come from the image's own file:
//! `experiments/results/nsswitch-contract.txt` check A, one pinned
//! `ubuntu:20.04`, one static glibc probe, the same supplied `/etc/passwd`:
//!
//! ```text
//! nsswitch=files  -> FOUND
//! nsswitch=other  -> NOTFOUND
//! ```
//!
//! ⚠ A statically linked payload does not escape it. The probe above is
//! `-static` and still answered `NOTFOUND`: static linking removes the
//! `PT_INTERP`, not the NSS dispatcher.
//!
//! ⚠ musl has no NSS plugin system and reads `/etc/passwd` directly, which is
//! why `alpine:3.22` ships no `nsswitch.conf` at all and needs no fixup.
//! `experiments/results/across/` reads the same question across eleven pinned
//! images: **10 of 11** read a supplied `/etc/passwd`, over **6** distinct
//! `passwd` shapes.

use crate::write::{Kind, Root};
use crate::{Action, Fixup, Libc, Report, Result};

/// Where each libc's loader lives. ⛔ Probed, never inferred from a
/// distribution name in `/etc/os-release`: a rootfs is whatever it holds.
///
/// ⚠ ONE GLOB PER TEST, and that is a measured lesson rather than style:
/// `experiments/125-across-distributions.sh` reported `unknown` for every glibc
/// row until it stopped combining them, because `/lib` is a symlink to
/// `usr/lib` on debian 12.
const GLIBC_MARKERS: &[&str] = &[
    "lib/libc.so.6",
    "lib64/libc.so.6",
    "usr/lib64/libc.so.6",
    "lib/x86_64-linux-gnu/libc.so.6",
    "lib/aarch64-linux-gnu/libc.so.6",
    "usr/lib/x86_64-linux-gnu/libc.so.6",
    "usr/lib/aarch64-linux-gnu/libc.so.6",
];

const MUSL_MARKERS: &[&str] = &[
    "lib/ld-musl-x86_64.so.1",
    "lib/ld-musl-aarch64.so.1",
    "lib/libc.musl-x86_64.so.1",
    "lib/libc.musl-aarch64.so.1",
];

pub fn probe_libc(root: &Root) -> Result<Libc> {
    let any = |ps: &[&str]| -> Result<bool> {
        for p in ps {
            if !root.kind(p)?.is_missing() {
                return Ok(true);
            }
        }
        Ok(false)
    };
    // ⛔ musl first. A rootfs can hold both -- `gcompat` on alpine ships a glibc
    // shim -- and the loader that runs the payload is the one it was linked
    // against. Naming musl where both are present is the safe error: it makes
    // podvm write an `nsswitch.conf` it did not need rather than skip one it
    // did. ⚠ Which is why the glibc arm is checked anyway, below.
    let musl = any(MUSL_MARKERS)?;
    let glibc = any(GLIBC_MARKERS)?;
    Ok(match (glibc, musl) {
        (true, _) => Libc::Glibc,
        (false, true) => Libc::Musl,
        (false, false) => Libc::Unknown,
    })
}

// ---------------------------------------------------------------- T-0404
//
// ⭐ The maintainer's fix, not this project's guess.
// `references/indigo-dc__udocker` tracker #141, comment of 2020-01-08:
// `run --containerauth` made passwd and group changes PERSIST in the container,
// and the reporter confirmed that is what fixed package installation. The
// failure it fixes is in the same thread, 2018-08-31: "packages that deploy
// their own service users during install/config and throw out errors when they
// try to do so, stopping the install process."
//
// The other half is tooling that dies before reaching any syscall wall:
// `references/garywill__treesandbox/tree/src/initializing.py:69` calls
// `pwd.getpwuid(uid).pw_name` unguarded at initialization, and
// `references/indigo-dc__udocker/tree/udocker/helper/hostinfo.py:19-24` returns
// `""` on KeyError, which
// `references/indigo-dc__udocker/tree/udocker/engine/base.py:422-427` turns
// into `Error: invalid syntax for user`.

/// ⛔ Synthesized only where the file is ABSENT. An image's own `/etc/passwd`
/// is the image's, and the payload's later writes to it must survive, which is
/// the whole of tracker #141.
const MINIMAL_PASSWD: &str = "\
root:x:0:0:root:/root:/bin/sh
nobody:x:65534:65534:nobody:/nonexistent:/sbin/nologin
";

const MINIMAL_GROUP: &str = "\
root:x:0:
nogroup:x:65534:
";

pub(crate) fn passwd_and_group(root: &Root, r: &mut Report) {
    crate::record(r, "T-0404", "identity", run_identity(root));
}

fn run_identity(root: &Root) -> Result<Vec<Fixup>> {
    let mut out = Vec::new();
    for (path, body) in [("etc/passwd", MINIMAL_PASSWD), ("etc/group", MINIMAL_GROUP)] {
        match root.kind(path)? {
            Kind::Regular { mode, .. } => {
                // ⭐ The persistence half of #141. A file the image shipped
                // read-only is one `useradd` cannot write, and the install
                // stops there. podvm widens the owner bit and says so.
                if mode & 0o200 == 0 {
                    let bytes = root.read(path)?.unwrap_or_default();
                    let w = root.write(path, &bytes, (mode & 0o7777) | 0o200)?;
                    out.push(
                        Fixup::new("T-0404", "identity", path, crate::act(w)).why(format!(
                            "the image shipped it mode {:04o}, which its own useradd \
                             cannot write. Content unchanged; the owner write bit is \
                             set so a package that creates a service user succeeds \
                             and the change persists",
                            mode & 0o7777
                        )),
                    );
                } else {
                    out.push(
                        Fixup::new("T-0404", "identity", path, Action::Unchanged).why(
                            "the image ships it, and it is writable, so the payload's \
                                  own useradd works and persists",
                        ),
                    );
                }
            }
            other => {
                let w = root.write(path, body.as_bytes(), 0o644)?;
                out.push(
                    Fixup::new("T-0404", "identity", path, crate::act(w))
                        .why(format!(
                            "the image has no {path} ({}), so tooling that calls \
                             getpwuid(0) dies before reaching any syscall wall. \
                             podvm synthesized a minimal one. note: Only 0:0 is a real \
                             id on this machine; every other id in it is the \
                             image's convention",
                            match other {
                                Kind::Missing => "absent".to_string(),
                                k => format!("{k:?}"),
                            }
                        ))
                        .degraded(),
                );
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- T-0410

/// The line `passwd:` and `group:` must start with for the supplied file to be
/// read at all.
const FILES: &str = "files";

/// What podvm writes where a glibc rootfs has no `nsswitch.conf`.
///
/// ⚠ Minimal on purpose: `files` for the two databases this layer supplies, and
/// `dns` for hosts because that is what `/etc/resolv.conf` is for. Nothing else
/// is claimed.
const MINIMAL_NSSWITCH: &str = "\
# Written by podvm (TODO/complete.md T-0410): glibc resolves a user through
# NSS, and only `files` reads /etc/passwd. Without this line the /etc/passwd
# podvm supplies is never consulted.
passwd:     files
group:      files
shadow:     files
hosts:      files dns
";

pub(crate) fn nsswitch(root: &Root, r: &mut Report) {
    let libc = r.libc;
    crate::record(r, "T-0410", "nsswitch", run_nsswitch(root, libc));
}

fn run_nsswitch(root: &Root, libc: Libc) -> Result<Vec<Fixup>> {
    let path = "etc/nsswitch.conf";
    let present = root.read_text(path)?;

    // 1. no nsswitch.conf and no glibc: musl, and it reads /etc/passwd directly.
    if present.is_none() && libc != Libc::Glibc {
        return Ok(vec![Fixup::new(
            "T-0410",
            "nsswitch",
            path,
            Action::Skipped,
        )
        .why(format!(
            "this rootfs is {} and ships no nsswitch.conf. musl has no NSS plugin \
             system and reads /etc/passwd directly, so there is nothing to fix",
            libc.word()
        ))]);
    }

    // 4. glibc and no nsswitch.conf: write a minimal one.
    let Some(text) = present else {
        // ⛔ `reach` and not `Kind::Dir`. Found by the door sweep of
        // 2026-09-09 after `experiments/85-completion-symlink-escape.sh` door D
        // caught the same shape in the CA fixup: a test on the last component
        // answers "not a directory" both for an image that has NO `/etc` and for
        // one whose `/etc` is a symlink podvm cannot follow, and only the first
        // of those is quiet.
        match root.reach("etc")? {
            crate::write::Reach::Yes => {}
            crate::write::Reach::Absent => {
                return Ok(vec![Fixup::new(
                    "T-0410",
                    "nsswitch",
                    path,
                    Action::Skipped,
                )
                .why("this image has no /etc directory")])
            }
            crate::write::Reach::Unreachable(why) => {
                return Ok(vec![Fixup::new("T-0410", "nsswitch", path, Action::Failed)
                    .why(why)
                    .degraded()])
            }
        }
        let w = root.write(path, MINIMAL_NSSWITCH.as_bytes(), 0o644)?;
        return Ok(vec![Fixup::new("T-0410", "nsswitch", path, crate::act(w))
            .why(
                "this rootfs carries glibc and shipped no nsswitch.conf, so nothing \
             would have read the /etc/passwd T-0404 supplies. podvm wrote a \
             minimal one naming files",
            )]);
    };

    let edited = prepend_files(&text);
    match edited {
        // 2. present and `passwd` already names `files` first.
        None => Ok(vec![Fixup::new(
            "T-0410",
            "nsswitch",
            path,
            Action::Unchanged,
        )
        .why(
            "passwd and group already name files first, so a supplied /etc/passwd \
              is read",
        )]),
        // 3. present and it does not. ⛔ EDIT, never replace: a rootfs naming
        // `sss` or `systemd` may be doing so for a reason podvm cannot see.
        Some(new) => {
            let w = root.write(path, new.as_bytes(), 0o644)?;
            Ok(vec![Fixup::new("T-0410", "nsswitch", path, crate::act(w))
                .why(
                    "passwd or group did not name files FIRST, so the /etc/passwd \
                 T-0404 supplies would have been ignored. podvm prepended files \
                 and kept every other service on the line",
                )])
        }
    }
}

/// Prepend `files` to the `passwd` and `group` lines, or `None` where both
/// already begin with it.
///
/// ⚠ **"names files" is not enough; it has to be FIRST.** `passwd: sss files`
/// asks sss before the file podvm supplied, and a directory service that
/// answers for a different user is a wrong answer rather than a missing one.
/// Where `files` appears later it is MOVED to the front rather than duplicated:
/// glibc tolerates the duplicate, and a line carrying `files` twice is one no
/// reader can tell podvm from the image on.
pub fn prepend_files(text: &str) -> Option<String> {
    let mut changed = false;
    let mut out = String::with_capacity(text.len() + 16);
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let tail: &str = &line[body.len()..];
        let Some((key, rest)) = body.split_once(':') else {
            out.push_str(line);
            continue;
        };
        if !matches!(key.trim(), "passwd" | "group") {
            out.push_str(line);
            continue;
        }
        // ⚠ A comment after the services is part of the line and is kept: an
        // image annotating why it names `sss` is documentation podvm is not
        // entitled to drop.
        let (services, comment) = match rest.split_once('#') {
            Some((s, c)) => (s, Some(c)),
            None => (rest, None),
        };
        let mut words: Vec<&str> = services.split_whitespace().collect();
        if words.first() == Some(&FILES) {
            out.push_str(line);
            continue;
        }
        words.retain(|w| *w != FILES);
        words.insert(0, FILES);
        out.push_str(&format!("{key}:{}{}", " ".repeat(6), words.join(" ")));
        if let Some(c) = comment {
            out.push_str(&format!(" #{c}"));
        }
        out.push_str(tail);
        changed = true;
    }
    changed.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> String {
        let p = std::env::temp_dir().join(format!("podvm-id-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("etc")).unwrap();
        p.to_string_lossy().to_string()
    }

    /// ⭐ The six distinct `passwd` shapes
    /// `experiments/125-across-distributions.sh` measured across eleven pinned
    /// images, driven through the editor. ⛔ These strings are the measurement,
    /// not examples: each is what one row's `/etc/nsswitch.conf` actually says.
    #[test]
    fn every_measured_passwd_shape_ends_up_naming_files_first() {
        let measured = [
            "passwd: files systemd",
            "passwd:         compat systemd",
            "passwd:     files systemd",
            "passwd:     files sss systemd",
            "passwd: files",
            "passwd:\tfiles",
        ];
        for m in measured {
            let got = prepend_files(m).unwrap_or_else(|| m.to_string());
            let services = got.split_once(':').unwrap().1;
            assert_eq!(
                services.split_whitespace().next(),
                Some("files"),
                "{m} -> {got}"
            );
            // ⛔ Never twice.
            assert_eq!(
                services
                    .split_whitespace()
                    .filter(|w| *w == "files")
                    .count(),
                1,
                "{m} -> {got}"
            );
        }
    }

    /// ⛔ The one that matters: `compat` does NOT read the supplied file, and
    /// the measurement says so. Every other service on the line survives.
    #[test]
    fn compat_gets_files_in_front_and_keeps_the_rest() {
        let got =
            prepend_files("passwd:         compat systemd\ngroup: compat\nhosts: dns\n").unwrap();
        assert!(got.contains("passwd:      files compat systemd\n"), "{got}");
        assert!(got.contains("group:      files compat\n"), "{got}");
        // ⚠ hosts is not this fixup's, and it is untouched.
        assert!(got.contains("hosts: dns\n"), "{got}");
    }

    #[test]
    fn a_line_already_naming_files_first_is_left_byte_identical() {
        assert!(prepend_files("passwd:     files systemd\ngroup:      files\n").is_none());
    }

    #[test]
    fn a_trailing_comment_survives_the_edit() {
        let got = prepend_files("passwd: sss # the site directory\n").unwrap();
        assert!(got.contains("files sss # the site directory"), "{got}");
    }

    /// ⚠ Case 1 of T-0410's four: a musl rootfs is left completely alone.
    #[test]
    fn a_musl_rootfs_gets_no_nsswitch_conf() {
        let d = scratch("musl");
        let root = Root::open(&d).unwrap();
        let fs = run_nsswitch(&root, Libc::Musl).unwrap();
        assert_eq!(fs[0].action, Action::Skipped);
        assert!(!std::path::Path::new(&format!("{d}/etc/nsswitch.conf")).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// ⚠ Case 4: a glibc rootfs with no file at all gets one.
    #[test]
    fn a_glibc_rootfs_with_no_nsswitch_conf_gets_a_minimal_one() {
        let d = scratch("glibc");
        let root = Root::open(&d).unwrap();
        let fs = run_nsswitch(&root, Libc::Glibc).unwrap();
        assert_eq!(fs[0].action, Action::Created);
        let got = std::fs::read_to_string(format!("{d}/etc/nsswitch.conf")).unwrap();
        assert!(got.contains("passwd:     files"), "{got}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// ⭐ T-0404's persistence half: a read-only `/etc/passwd` is what stops an
    /// install, and the content must not change while the mode does.
    #[test]
    fn a_read_only_passwd_is_made_writable_without_changing_a_byte() {
        use std::os::unix::fs::PermissionsExt;
        let d = scratch("ro");
        let p = format!("{d}/etc/passwd");
        std::fs::write(&p, b"root:x:0:0:root:/root:/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o444)).unwrap();
        let root = Root::open(&d).unwrap();
        let fs = run_identity(&root).unwrap();
        let f = fs.iter().find(|f| f.path == "etc/passwd").unwrap();
        assert_eq!(f.action, Action::Rewrote);
        assert_eq!(
            std::fs::read(&p).unwrap(),
            b"root:x:0:0:root:/root:/bin/sh\n"
        );
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o200,
            0o200
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_absent_passwd_is_synthesized_and_marked_degraded() {
        let d = scratch("nopasswd");
        let root = Root::open(&d).unwrap();
        let fs = run_identity(&root).unwrap();
        assert!(fs.iter().all(|f| f.action == Action::Created));
        assert!(fs.iter().all(|f| f.degraded));
        let got = std::fs::read_to_string(format!("{d}/etc/passwd")).unwrap();
        assert!(got.starts_with("root:x:0:0:"), "{got}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
