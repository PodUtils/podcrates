//! The package managers, and the three walls each of them hits that are not
//! about the network.
//!
//! - [`pacman`] -- [`TODO/complete.md`](../../../TODO/complete.md) T-0406,
//!   `DownloadUser` and the keyring;
//! - [`apt`] -- T-0407, the sandbox user and the CA bundle;
//! - [`zypper`] -- T-0408, ⭐ **the RIS index and not `repos.d`**;
//! - [`ca_hash_dir`] -- T-0412, ⭐ **the CApath libzypp actually reads**;
//! - T-0409 is a rule rather than a fixup and it is enforced elsewhere: podvm
//!   never translates a package manager's exit status. See
//!   [`crate`]'s third rule and [`TODO/cli.md`](../../../TODO/cli.md) T-0802.
//!
//! ⚠ The `http://` half of every one of these is [`crate::sources`], T-0411.
//! Splitting them is deliberate: the protocol fixup is one rule applied to
//! every distribution, and these are three distributions' own walls.

use crate::write::{Kind, Reach, Root};
use crate::{Action, Fixup, Options, Report, Result, Step};

pub(crate) fn apply(root: &Root, opts: &Options, r: &mut Report) {
    crate::record_steps(r, "T-0406", "pacman", pacman(root, opts));
    crate::record(r, "T-0407", "apt", apt(root, opts));
    crate::record(r, "T-0408", "zypper", zypper(root, opts));
    // ⭐ Every rootfs, not only a Debian- or SUSE-family one: an https package
    // source is not a property of the package manager, and alpine, arch, fedora
    // and void all hit the same wall.
    crate::record(r, "T-0407", "ca-bundle", ca_bundle(root, "T-0407", opts));
    // ⭐ T-0412, and it is a SECOND trust store rather than a second copy of the
    // first: a CAfile and a hash-indexed CApath are read by different calls, and
    // `zypper` reads only the second.
    crate::record_steps(r, "T-0412", "ca-hash-dir", ca_hash_dir(root, opts));
}

/// Where a program is, inside the rootfs, or `None`.
///
/// ⛔ **Found rather than assumed.** A step names an absolute path this crate
/// has already `stat`ed, so a command announced on the banner cannot turn out
/// not to be there. ⚠ The list is the conventional `bin` directories and not the
/// image's own `PATH`: `PATH` is the payload's environment, and a step is
/// podvm's own command rather than the payload's.
fn find_program(root: &Root, name: &str) -> Result<Option<String>> {
    for dir in ["usr/bin", "usr/sbin", "bin", "sbin", "usr/local/bin"] {
        let rel = format!("{dir}/{name}");
        // ⚠ A symlink counts. `/usr/bin/openssl -> openssl-1.1` is one image's
        // packaging and podvm does not follow it: if it dangles the step fails
        // and says so, which is a better answer than podvm deciding the
        // command is absent because it could not read a link.
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

// ---------------------------------------------------------------- T-0406
//
// `pacman -Sy` fails with `failed to chown temporary download directory ...:
// Invalid argument`. `DownloadUser = alpm` names a uid this runtime cannot map
// and the errno is the section 9.1 wall again.
//
// ⛔ Creating the `alpm` user does not help: the id still has no mapping, so
// the chown still returns EINVAL. The config line is what has to go.
//
// ⚠ CheckSpace is deliberately NOT enabled. It is what makes `/etc/mtab`
// load-bearing (T-0405), and Arch ships it commented out.

fn pacman(root: &Root, opts: &Options) -> Result<(Vec<Fixup>, Vec<Step>)> {
    let path = "etc/pacman.conf";
    let Some(text) = root.read_text(path)? else {
        return Ok((
            vec![Fixup::new("T-0406", "pacman", path, Action::Skipped)
                .why("no pacman.conf: this is not an Arch-family rootfs")],
            Vec::new(),
        ));
    };
    let mut out = Vec::new();
    let (new, hits) = comment_out(&text, "DownloadUser");
    if hits == 0 {
        out.push(
            Fixup::new("T-0406", "pacman", path, Action::Unchanged)
                .why("no active DownloadUser line, so pacman already downloads as root"),
        );
    } else {
        let w = root.write(path, new.as_bytes(), 0o644)?;
        out.push(
            Fixup::new("T-0406", "pacman", path, crate::act(w)).why(format!(
                "commented out {hits} DownloadUser line(s). It names a uid this runtime \
             cannot map, so pacman's chown of its download directory returns EINVAL \
             and the whole transaction fails"
            )),
        );
    }

    // ⭐ The keyring half needs a COMMAND: `pacman-key --init` and `--populate`
    // run gpg INSIDE the rootfs, which the completion layer cannot do from the
    // host. T-0412 gave it a step, so podvm asks the caller to run it rather
    // than only printing the two commands a reader would have to type.
    // ⚠ Measured on the pinned `archlinux` image: it ships an initialized
    // keyring, so this is for a rootfs that does not.
    let mut steps = Vec::new();
    let keyring = "etc/pacman.d/gnupg";
    let initialized = !root.kind("etc/pacman.d/gnupg/trustdb.gpg")?.is_missing();
    let key = find_program(root, "pacman-key")?;
    out.push(match (initialized, opts.steps, key) {
        (true, _, _) => Fixup::new("T-0406", "pacman-keyring", keyring, Action::Unchanged)
            .why("the image ships an initialized keyring, so signed packages verify"),
        (false, true, Some(prog)) => {
            for arg in ["--init", "--populate"] {
                steps.push(Step {
                    entry: "T-0406",
                    id: "pacman-keyring",
                    argv: vec![prog.clone(), arg.to_string()],
                    why: format!(
                        "this rootfs ships no initialized keyring, so pacman cannot \
                         verify a signed package. `{prog} {arg}` builds one, and it \
                         runs gpg inside the rootfs, which podvm cannot do from \
                         the host"
                    ),
                });
            }
            Fixup::new("T-0406", "pacman-keyring", keyring, Action::Skipped).why(
                "no initialized keyring here; podvm asks the caller to run \
                 pacman-key --init and --populate inside the rootfs (T-0412's step \
                 mechanism), because gpg cannot be run from outside the chroot",
            )
        }
        (false, true, None) => Fixup::new("T-0406", "pacman-keyring", keyring, Action::Skipped)
            .why(
                "this rootfs has no initialized keyring and no pacman-key to build \
                 one with. Signed packages will not verify",
            )
            .degraded(),
        (false, false, _) => Fixup::new("T-0406", "pacman-keyring", keyring, Action::Skipped)
            .why(
                "--no-steps: this rootfs has no initialized keyring and podvm did \
                 not run pacman-key inside it. Run `pacman-key --init && pacman-key \
                 --populate` in the container before installing signed packages",
            )
            .degraded(),
    });
    Ok((out, steps))
}

/// Comment out every uncommented line whose first word is `key`, and say how
/// many. ⚠ The first word, not a substring: `#DownloadUser` is already
/// commented and `SomeDownloadUserThing` is a different key.
fn comment_out(text: &str, key: &str) -> (String, usize) {
    let mut hits = 0;
    let mut out = String::with_capacity(text.len() + 32);
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        let first = t.split(|c: char| c.is_whitespace() || c == '=').next();
        if first == Some(key) {
            hits += 1;
            out.push_str("# podvm (T-0406): ");
            out.push_str(line);
        } else {
            out.push_str(line);
        }
    }
    (out, hits)
}

mod bundle;
pub use bundle::*;
mod hash_dir;
pub use hash_dir::*;
// ---------------------------------------------------------------- T-0408
//
// ⭐ A naive edit to `/etc/zypp/repos.d/` REVERTS. `refresh-services`
// regenerates that directory from the RIS index and overwrites whatever was
// written there, so the fixup edits the source of the generated file.
//
// ⚠ The rewrite itself is T-0411's, applied to both directories. What is here
// is the discovery of the index and the report that says which of the two
// podvm actually reached, because "edited repos.d" and "edited the service
// definitions" are different claims.

pub const ZYPP_SERVICE_DIRS: &[&str] = &["usr/share/zypp/local/service", "etc/zypp/services.d"];

fn zypper(root: &Root, _opts: &Options) -> Result<Vec<Fixup>> {
    if root.kind("etc/zypp")?.is_missing() {
        return Ok(vec![Fixup::new(
            "T-0408",
            "zypper",
            "etc/zypp",
            Action::Skipped,
        )
        .why("no /etc/zypp: this is not a SUSE-family rootfs")]);
    }
    let mut found = Vec::new();
    for d in ZYPP_SERVICE_DIRS {
        if !root.list(d)?.is_empty() {
            found.push(*d);
        }
    }
    let out = vec![if found.is_empty() {
        Fixup::new(
            "T-0408",
            "zypper-index",
            "etc/zypp/repos.d",
            Action::Skipped,
        )
        .why(
            "this rootfs carries no RIS service index, so repos.d is not \
             regenerated and editing it in place holds",
        )
    } else {
        Fixup::new(
            "T-0408",
            "zypper-index",
            &found.join(", "),
            Action::Unchanged,
        )
        .why(
            "a RIS service index is present. note: Any scheme fixup (T-0411) is \
             applied HERE as well as to repos.d, or `zypper refresh-services` \
             regenerates repos.d from the uncorrected index and reverts it",
        )
    }];
    Ok(out)
}

#[cfg(test)]
mod tests;
