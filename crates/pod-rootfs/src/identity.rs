//! Who the payload is, and if a program reads the file that says so.
//!
//! - `passwd_and_group`, for `/etc/passwd` and `/etc/group`;
//! - `nsswitch`, for `/etc/nsswitch.conf`, which decides if the first one
//!   does anything.
//!
//! **On a glibc root a supplied `/etc/passwd` does nothing unless
//! `/etc/nsswitch.conf` names `files`.** With a user name that no
//! distribution ships, one static glibc probe in one `ubuntu:20.04` and the
//! same `/etc/passwd`: `passwd: files` finds the user, `passwd: other` does
//! not. A static link does not change it: it removes `PT_INTERP`, not the
//! NSS dispatcher.
//!
//! musl has no NSS and reads `/etc/passwd` directly, so `alpine` ships no
//! `nsswitch.conf` and needs no fixup.

use crate::write::{Kind, Reach, Root};
use crate::{Action, Fixup, Libc, Names, Report, Result};

/// The places of a C library, for each architecture: `lib`, `lib64`,
/// `lib32`, their `usr` forms, and in each the directories of a multiarch
/// layout (`x86_64-linux-gnu`, `arm-linux-gnueabihf`,
/// `powerpc64le-linux-gnu`). One path for each test: `/lib` is a symlink to
/// `usr/lib` on debian 12, and a probe that joins the two finds nothing.
const LIB_DIRS: &[&str] = &["lib", "lib64", "lib32", "usr/lib", "usr/lib64", "usr/lib32"];

/// Is a C library of `glibc` in the root, on any architecture?
fn has_glibc(root: &Root) -> Result<bool> {
    for dir in LIB_DIRS {
        if !root.kind(&format!("{dir}/libc.so.6"))?.is_missing() {
            return Ok(true);
        }
        for sub in root.list(dir)?.iter().filter(|n| n.contains("-linux-gnu")) {
            if !root.kind(&format!("{dir}/{sub}/libc.so.6"))?.is_missing() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Is the loader of musl in the root, on any architecture
/// (`ld-musl-riscv64.so.1`, `libc.musl-armv7.so.1`)?
fn has_musl(root: &Root) -> Result<bool> {
    for dir in ["lib", "usr/lib"] {
        let found = root.list(dir)?.iter().any(|n| {
            (n.starts_with("ld-musl-") || n.starts_with("libc.musl-")) && n.ends_with(".so.1") && !n.contains('/')
        });
        if found {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The C library of the root. A root can hold both (`gcompat` on alpine
/// ships a shim of glibc): then glibc is named, which makes the crate write
/// an `nsswitch.conf` that is not necessary and not skip one that is.
pub fn probe_libc(root: &Root) -> Result<Libc> {
    Ok(match (has_glibc(root)?, has_musl(root)?) {
        (true, _) => Libc::Glibc,
        (false, true) => Libc::Musl,
        (false, false) => Libc::Unknown,
    })
}

// A package that makes its own service user during an install stops when it
// cannot write `/etc/passwd`, and a tool that calls `getpwuid(0)` with no
// guard stops before it reaches a system call. So the crate makes a file that
// is absent, and makes a file of the image writable; the changes of the
// payload stay.

/// Made only where the file is ABSENT. The file of the image belongs to the
/// image, and the later writes of the payload to it must stay.
const MINIMAL_PASSWD: &str = "root:x:0:0:root:/root:/bin/sh\nnobody:x:65534:65534:nobody:/nonexistent:/sbin/nologin\n";

const MINIMAL_GROUP: &str = "root:x:0:\nnogroup:x:65534:\n";

pub(crate) fn passwd_and_group(root: &Root, r: &mut Report) {
    crate::record(r, "identity", run_identity(root));
}

/// The mode of `etc` where the image closed it for its owner. `useradd`
/// makes its new file and its lock in `etc`, so a writable `etc/passwd`
/// alone does not let a payload with no `CAP_DAC_OVERRIDE` add a user.
fn closed_etc(root: &Root) -> Result<Option<u32>> {
    Ok(root.dir_mode("etc")?.filter(|m| m & 0o300 != 0o300))
}

/// The row of a file that the crate leaves, degraded where `etc` is closed.
fn kept(path: &str, why: String, closed: Option<u32>) -> Fixup {
    let f = Fixup::new("identity", path, Action::Unchanged);
    match closed {
        None => f.why(why),
        Some(m) => f
            .why(format!(
                "the image ships it, but the directory etc has mode {m:04o}: the useradd of a payload with no \
                 CAP_DAC_OVERRIDE cannot make its new file and its lock there. The crate keeps the mode of the image"
            ))
            .degraded(),
    }
}

fn run_identity(root: &Root) -> Result<Vec<Fixup>> {
    let mut out = Vec::new();
    let closed = closed_etc(root)?;
    for (path, body) in [("etc/passwd", MINIMAL_PASSWD), ("etc/group", MINIMAL_GROUP)] {
        match root.kind(path)? {
            Kind::Regular { mode, .. } if mode & 0o200 == 0 => {
                // A file that the image shipped read-only is one that its own
                // `useradd` cannot write. The crate sets the owner write bit.
                let bytes = root.read(path)?.unwrap_or_default();
                let w = root.write(path, &bytes, (mode & 0o7777) | 0o200)?;
                let mut why = format!(
                    "the image shipped it with mode {:04o}, which its own useradd cannot write. The content is the \
                     same; the owner write bit is set, so a package that makes a service user succeeds and the \
                     change stays",
                    mode & 0o7777
                );
                let mut f = Fixup::new("identity", path, crate::act(w));
                if let Some(m) = closed {
                    why.push_str(&format!(
                        ". The directory etc has mode {m:04o}, so the useradd of a payload with no CAP_DAC_OVERRIDE \
                         still cannot make its new file and its lock there"
                    ));
                    f = f.degraded();
                }
                out.push(f.why(why));
            }
            Kind::Regular { .. } => out.push(kept(
                path,
                "the image ships it, and it and its directory are writable, so the useradd of the payload works".into(),
                closed,
            )),
            // A symlink to a file inside the root is the file of the image:
            // the payload reads it through the link.
            Kind::Symlink(t) if root.read_following(path)?.is_some() => out.push(
                Fixup::new("identity", path, Action::Unchanged)
                    .why(format!("the image ships it as a symlink to {t}, which is a file inside the root")),
            ),
            other => {
                let w = root.write(path, body.as_bytes(), 0o644)?;
                out.push(
                    Fixup::new("identity", path, crate::act(w))
                        .why(format!(
                            "the image has no {path} ({}), so a tool that calls getpwuid(0) stops before it reaches \
                             a system call. The crate made a minimal one. Only 0:0 is a real id on this host; each \
                             other id in it is a convention of the image",
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

/// The first service of the `passwd` and `group` lines, for a supplied
/// file to be read at all.
const FILES: &str = "files";

/// What the crate writes where a glibc root has no `nsswitch.conf`: `files`
/// for the databases of this module, and `dns` for hosts, as
/// `/etc/resolv.conf` is for that. Nothing more.
fn minimal_nsswitch(tool: &str) -> String {
    format!(
        "# Written by {tool}: glibc resolves a user through NSS, and only `files`\n# reads /etc/passwd.\n\
         passwd:     files\ngroup:      files\nshadow:     files\nhosts:      files dns\n"
    )
}

pub(crate) fn nsswitch(root: &Root, names: &Names, r: &mut Report) {
    let libc = r.libc;
    crate::record(r, "nsswitch", run_nsswitch(root, libc, names));
}

fn run_nsswitch(root: &Root, libc: Libc, names: &Names) -> Result<Vec<Fixup>> {
    let path = "etc/nsswitch.conf";
    // Through a link inside the root: a symlinked `nsswitch.conf` is the
    // file of the image, not an absent one.
    let present = root.read_following(path)?.and_then(|b| String::from_utf8(b).ok());

    // 1. No nsswitch.conf and no glibc: musl reads /etc/passwd directly.
    if present.is_none() && libc != Libc::Glibc {
        return Ok(vec![Fixup::new("nsswitch", path, Action::Skipped).why(format!(
            "this root is {} and ships no nsswitch.conf. musl has no NSS and reads /etc/passwd directly, so there \
             is nothing to fix",
            libc.word()
        ))]);
    }

    // 2. glibc and no nsswitch.conf: write a minimal one. `reach`, not
    // `Kind::Dir`: an image with NO `/etc` is quiet, and an `/etc` that is a
    // symlink out of the root is a failed row.
    let Some(text) = present else {
        match root.reach("etc")? {
            Reach::Yes => {}
            Reach::Absent => {
                return Ok(vec![Fixup::new("nsswitch", path, Action::Skipped).why("this image has no /etc directory")]);
            }
            Reach::Unreachable(why) => {
                return Ok(vec![Fixup::new("nsswitch", path, Action::Failed).why(why).degraded()]);
            }
        }
        let w = root.write(path, minimal_nsswitch(&names.tool).as_bytes(), 0o644)?;
        return Ok(vec![Fixup::new("nsswitch", path, crate::act(w)).why(
            "this root holds glibc and shipped no nsswitch.conf, so no program reads the supplied /etc/passwd. The \
             crate wrote a minimal one that names files",
        )]);
    };

    match prepend_files(&text) {
        // 3. `passwd` and `group` name `files` first.
        None => Ok(vec![
            Fixup::new("nsswitch", path, Action::Unchanged)
                .why("passwd and group already name files first, so a supplied /etc/passwd is read"),
        ]),
        // 4. They do not. EDIT, never replace: a root that names `sss` or
        // `systemd` can have a reason that the crate cannot see.
        Some(new) => {
            let w = root.write(path, new.as_bytes(), 0o644)?;
            Ok(vec![Fixup::new("nsswitch", path, crate::act(w)).why(
                "passwd or group did not name files FIRST, so the supplied /etc/passwd was not read. The crate put \
                 files first and kept each other service of the line",
            )])
        }
    }
}

/// Put `files` first on the `passwd` and `group` lines, or `None` where both
/// already start with it.
///
/// It must be FIRST: `passwd: sss files` asks sss before the supplied file,
/// and a directory service that answers for another user gives a wrong
/// answer. A `files` later on the line MOVES to the front: glibc accepts a
/// second one, but a line with `files` two times does not show which
/// program wrote which.
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
        // A comment after the services is part of the line, and stays.
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
mod tests;
