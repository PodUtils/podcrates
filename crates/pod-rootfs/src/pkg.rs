//! The package managers, and the walls of each that are not about the
//! network.
//!
//! - `pacman`, for `DownloadUser` and the keyring;
//! - `apt`, for the sandbox user;
//! - `ca_bundle`, for the CA bundle, for each root;
//! - `zypper`, for the RIS index, not `repos.d`;
//! - `ca_hash_dir`, for the hash-indexed CA directory that libzypp reads.
//!
//! The `http://` part of each is [`crate::sources`]: one rule for each
//! distribution, where these are the walls of three distributions. The
//! crate never changes the exit status of a package manager.

use crate::write::{Kind, Reach, Root};
use crate::{Action, Fixup, Report, Result, Settings, Step};

pub(crate) fn apply(root: &Root, settings: &Settings, r: &mut Report) {
    crate::record_steps(r, "pacman", pacman(root, settings));
    crate::record(r, "apt", apt(root, settings));
    crate::record(r, "zypper", zypper(root));
    // Each root, not only one of the Debian or SUSE family: an https
    // source is not a property of the package manager.
    crate::record(r, "ca-bundle", ca_bundle(root, settings));
    // A SECOND trust store, not a second copy of the first: a CA file and a
    // hash-indexed CA directory are read by different calls, and zypper
    // reads only the second.
    crate::record_steps(r, "ca-hash-dir", ca_hash_dir(root, settings));
}

/// Where a program is, inside the root, or `None`.
///
/// The crate FINDS it: a step names an absolute path that the crate saw. The
/// list is the usual `bin` directories, not the `PATH` of the image: `PATH`
/// is the environment of the payload, and a step is not the command of the
/// payload. A symlink counts: `/usr/bin/openssl -> openssl-1.1` is a packaging
/// of one image, and a dangling link makes the step fail with its reason.
fn find_program(root: &Root, name: &str) -> Result<Option<String>> {
    for dir in ["usr/bin", "usr/sbin", "bin", "sbin", "usr/local/bin"] {
        let rel = format!("{dir}/{name}");
        let found = match root.kind(&rel)? {
            Kind::Regular { mode, .. } => mode & 0o111 != 0,
            Kind::Symlink(_) => true,
            _ => false,
        };
        if found {
            return Ok(Some(format!("/{rel}")));
        }
    }
    Ok(None)
}

// pacman: `pacman -Sy` fails with `failed to chown temporary download
// directory ...: Invalid argument`: `DownloadUser = alpm` names a uid that
// the runtime cannot map. A new `alpm` user does not help, as the id still
// has no mapping; the line must go. `CheckSpace` stays off: it makes
// `/etc/mtab` necessary, and Arch ships it as a comment.

fn pacman(root: &Root, settings: &Settings) -> Result<(Vec<Fixup>, Vec<Step>)> {
    let path = "etc/pacman.conf";
    let Some(text) = root.read_text(path)? else {
        return Ok((
            vec![
                Fixup::new("pacman", path, Action::Skipped)
                    .why("no pacman.conf: this is not a root of the Arch family"),
            ],
            Vec::new(),
        ));
    };
    let mut out = Vec::new();
    let (new, hits) = comment_out(&text, "DownloadUser", &settings.names.comment);
    if hits == 0 {
        out.push(
            Fixup::new("pacman", path, Action::Unchanged)
                .why("no active DownloadUser line, so pacman already downloads as root"),
        );
    } else {
        let w = root.write(path, new.as_bytes(), 0o644)?;
        out.push(Fixup::new("pacman", path, crate::act(w)).why(format!(
            "{hits} DownloadUser line(s) are now comments. The line names a uid that this runtime cannot map, so the \
             chown of the download directory of pacman gives EINVAL and the transaction fails"
        )));
    }

    // The keyring needs a COMMAND: `pacman-key --init` and `--populate` run
    // gpg INSIDE the root, which the crate cannot do from the host. The
    // `archlinux` image ships a keyring, so this is for a root that does not.
    let mut steps = Vec::new();
    let keyring = "etc/pacman.d/gnupg";
    let initialized = !root.kind("etc/pacman.d/gnupg/trustdb.gpg")?.is_missing();
    let key = find_program(root, "pacman-key")?;
    out.push(match (initialized, settings.steps, key) {
        (true, _, _) => Fixup::new("pacman-keyring", keyring, Action::Unchanged)
            .why("the image ships an initialized keyring, so signed packages verify"),
        (false, true, Some(prog)) => {
            for arg in ["--init", "--populate"] {
                steps.push(Step {
                    id: "pacman-keyring",
                    argv: vec![prog.clone(), arg.to_string()],
                    why: format!(
                        "this root ships no initialized keyring, so pacman cannot verify a signed package. `{prog} \
                         {arg}` makes one, and it runs gpg inside the root"
                    ),
                });
            }
            Fixup::new("pacman-keyring", keyring, Action::Skipped).why(
                "no initialized keyring here; the caller runs pacman-key --init and --populate inside the root, \
                 as gpg cannot run from outside it",
            )
        }
        (false, true, None) => Fixup::new("pacman-keyring", keyring, Action::Skipped)
            .why("this root has no initialized keyring and no pacman-key to make one. Signed packages do not verify")
            .degraded(),
        (false, false, _) => Fixup::new("pacman-keyring", keyring, Action::Skipped)
            .why(
                "the steps are off: this root has no initialized keyring, and pacman-key did not run inside it. \
                 Run `pacman-key --init && pacman-key --populate` in the container before a signed package",
            )
            .degraded(),
    });
    Ok((out, steps))
}

/// Put `comment` before each line whose first word is `key`, and give the
/// count. The first word, not a part of a word: `#DownloadUser` is already a
/// comment, and `DownloadUserAgent` is another key.
fn comment_out(text: &str, key: &str, comment: &str) -> (String, usize) {
    let mut hits = 0;
    let mut out = String::with_capacity(text.len() + 32);
    for line in text.split_inclusive('\n') {
        let first = line.trim_start().split(|c: char| c.is_whitespace() || c == '=').next();
        if first == Some(key) {
            hits += 1;
            out.push_str(comment);
        }
        out.push_str(line);
    }
    (out, hits)
}

mod bundle;
pub use bundle::*;
mod hash_dir;
pub use hash_dir::*;

// zypper: an edit of `/etc/zypp/repos.d/` GOES BACK. `refresh-services`
// makes that directory again from the RIS index, so the source of the
// made file gets the change. The change itself is of `crate::sources`; this
// row says which of the two the crate reached.

/// The directories of the RIS index of zypper.
pub const ZYPP_SERVICE_DIRS: &[&str] = &["usr/share/zypp/local/service", "etc/zypp/services.d"];

fn zypper(root: &Root) -> Result<Vec<Fixup>> {
    if root.kind("etc/zypp")?.is_missing() {
        return Ok(vec![
            Fixup::new("zypper", "etc/zypp", Action::Skipped)
                .why("no /etc/zypp: this is not a root of the SUSE family"),
        ]);
    }
    let mut found = Vec::new();
    for d in ZYPP_SERVICE_DIRS {
        if !root.list(d)?.is_empty() {
            found.push(*d);
        }
    }
    Ok(vec![if found.is_empty() {
        Fixup::new("zypper-index", "etc/zypp/repos.d", Action::Skipped)
            .why("this root holds no RIS service index, so repos.d is not made again and an edit of it stays")
    } else {
        Fixup::new("zypper-index", &found.join(", "), Action::Unchanged).why(
            "a RIS service index is here. A change of the scheme of a source applies HERE as well as to repos.d, \
             or `zypper refresh-services` makes repos.d again from the index and undoes it",
        )
    }])
}

#[cfg(test)]
mod tests;
