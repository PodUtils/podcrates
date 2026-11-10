use super::*;

// A CA file and a hash-indexed CA directory are two trust stores, and
// zypper reads only the second. In `opensuse/leap:15.6`, `curl
// https://download.opensuse.org/` gives 200 and `curl --capath
// /etc/ssl/certs` the same URL gives 60: libzypp gives libcurl a
// `CURLOPT_CAPATH` of `/etc/ssl/certs` (a symlink to
// `/var/lib/ca-certificates/pem`), and OpenSSL finds a certificate there by
// the SHA-1 of its subject. A CA file is invisible to it, and so is a file
// under each other name.
//
// The crate cannot make the link from the host, so the fixup has two
// parts: the crate writes one PEM for each root, and asks the caller to run
// `openssl rehash` on the directory (the step). `update-ca-certificates` of
// openSUSE cannot be the step: it exits 1 at `done < <(find ...)` with
// `/dev/fd/62: No such file or directory`, as the process substitution of
// bash needs `/dev/fd`, and `/proc` is not mounted in the root.
//
// ONE certificate for each file: `openssl rehash` refuses a file with three
// (`it does not contain exactly one certificate or CRL`) and links nothing.

/// The usual hash-indexed CA directories, relative to the root. Only a
/// directory that is ALREADY hash-indexed: a new hash directory in an image
/// with none is a trust store that nothing reads.
pub const CAPATH_DIRS: &[&str] = &["etc/ssl/certs", "etc/pki/tls/certs"];

/// One text for each `CERTIFICATE` block of a PEM bundle. Each text outside
/// a block goes: `openssl rehash` wants a file that it reads as one
/// certificate.
pub(super) fn split_pem(bundle: &[u8]) -> Vec<String> {
    const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
    const END: &str = "-----END CERTIFICATE-----";
    let text = String::from_utf8_lossy(bundle);
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        let t = line.trim();
        if t == BEGIN {
            cur = Some(String::new());
        }
        if let Some(c) = cur.as_mut() {
            c.push_str(t);
            c.push('\n');
            if t == END {
                out.push(std::mem::take(c));
                cur = None;
            }
        }
    }
    out
}

/// Is `name` a name that the hash lookup of OpenSSL finds? `01419da9.0`,
/// and the CRL form `01419da9.r0`.
pub(super) fn is_hash_link(name: &str) -> bool {
    let Some((hash, tail)) = name.split_once('.') else {
        return false;
    };
    hash.len() == 8
        && hash.bytes().all(|b| b.is_ascii_hexdigit())
        && !tail.is_empty()
        && tail.trim_start_matches('r').bytes().all(|b| b.is_ascii_digit())
}

/// The key of a directory, so that two candidates that are one directory
/// are not written two times: `/etc/ssl/certs -> /etc/pki/tls/certs` is the
/// own link of Fedora.
fn dir_key(root: &Root, rel: &str) -> Result<String> {
    Ok(match root.kind(rel)? {
        Kind::Symlink(t) if t.starts_with('/') => t.trim_start_matches('/').to_string(),
        _ => rel.to_string(),
    })
}

/// One row and no step.
fn only(path: &str, a: Action, degraded: bool, why: String) -> (Vec<Fixup>, Vec<Step>) {
    let f = Fixup::new("ca-hash-dir", path, a).why(why);
    (vec![if degraded { f.degraded() } else { f }], Vec::new())
}

pub(crate) fn ca_hash_dir(root: &Root, settings: &Settings) -> Result<(Vec<Fixup>, Vec<Step>)> {
    let id = "ca-hash-dir";
    let prefix = &settings.names.host_ca_prefix;
    // A variable that NAMED the bundle decides, as for `ca_bundle`.
    let Some(h) = &settings.host_bundle else {
        return Ok(only(
            CAPATH_DIRS[0],
            Action::Skipped,
            false,
            "the host gave no CA bundle, so there is nothing to index into the CA directory of the image".into(),
        ));
    };
    let Some(var) = h.announced_by.clone() else {
        return Ok(only(
            CAPATH_DIRS[0],
            Action::Unchanged,
            false,
            "the bundle of the host is at a default path that no variable named. That tells nothing of an \
             interception, so the crate adds nothing to the trust store of the image"
                .into(),
        ));
    };

    // The hash-indexed directories, each one time.
    let mut dirs: Vec<(String, Vec<String>)> = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    for d in CAPATH_DIRS {
        // A symlinked CA directory is the shape of openSUSE and the crate
        // follows it; one that points out of the root is not reached.
        if root.reach(d)? != Reach::Yes {
            continue;
        }
        let names = root.list(d)?;
        if !names.iter().any(|n| is_hash_link(n)) {
            continue;
        }
        let key = dir_key(root, d)?;
        if keys.contains(&key) {
            continue;
        }
        keys.push(key);
        dirs.push(((*d).to_string(), names));
    }
    if dirs.is_empty() {
        return Ok(only(
            CAPATH_DIRS[0],
            Action::Skipped,
            false,
            "this image holds no hash-indexed CA directory to index the roots of the host into".into(),
        ));
    }
    if !settings.host_cas {
        return Ok(only(
            &dirs[0].0,
            Action::Skipped,
            true,
            format!(
                "the host CA fixup is off: ${var} named the bundle of the host, and the hash-indexed trust store of \
                 the image stays. A tool that reads a CA directory (libzypp) does not verify here if this host \
                 intercepts TLS"
            ),
        ));
    }
    // The program is found BEFORE a write: a certificate in a hash directory
    // with no link is a file that no TLS stack reads.
    let Some(rehash) = find_program(root, "openssl")? else {
        return Ok(only(
            &dirs[0].0,
            Action::Skipped,
            true,
            "this image has a hash-indexed CA directory and no openssl to index it, so a tool that reads a CA \
             directory does not see the roots of the host"
                .into(),
        ));
    };
    if !settings.steps {
        return Ok(only(
            &dirs[0].0,
            Action::Skipped,
            true,
            "the steps are off: the roots of the host are not written into the hash-indexed CA directory of the \
             image, as their index needs `openssl rehash` inside the root. A tool that reads a CA directory does \
             not verify here"
                .into(),
        ));
    }

    let announced = split_pem(&h.bytes);
    let mut out = Vec::new();
    let mut steps = Vec::new();
    for (dir, names) in dirs {
        // What the INDEX reaches, not what the directory holds: a CA file in
        // the directory (`ca-certificates.crt`) is not found by name. Only a
        // `HASH.N` entry, and the file that a `HASH.N` symlink names, count.
        // The files of the crate are not compared: a second run must not find
        // each root present and remove it as old.
        let mut reachable: Vec<String> = Vec::new();
        for n in names.iter().filter(|n| is_hash_link(n)) {
            match root.kind(&format!("{dir}/{n}"))? {
                // A name, never a path: `openssl rehash` writes a bare name.
                Kind::Symlink(t) if !t.contains('/') && !reachable.contains(&t) => reachable.push(t),
                Kind::Regular { .. } if !reachable.contains(n) => reachable.push(n.clone()),
                _ => {}
            }
        }
        let mut held: Vec<String> = Vec::new();
        for t in reachable.iter().filter(|t| !t.starts_with(prefix.as_str())) {
            if let Some(b) = root.read(&format!("{dir}/{t}"))? {
                held.extend(split_pem(&b));
            }
        }
        let adding: Vec<&String> = announced.iter().filter(|c| !held.contains(c)).collect();

        let mut wrote = 0usize;
        let mut ours = Vec::new();
        for (i, pem) in adding.iter().enumerate() {
            let name = format!("{prefix}{i:03}.pem");
            let body = format!(
                "# Written by {}: one root of the CA bundle of the host, named in ${var} as {}, that this image \
                 did not hold\n{pem}",
                settings.names.tool, h.path
            );
            if root.write(&format!("{dir}/{name}"), body.as_bytes(), 0o644)? != crate::write::Wrote::Unchanged {
                wrote += 1;
            }
            ours.push(name);
        }
        // A bundle that SHRANK leaves files that no write of this run made:
        // an old root in a trust store is the leftover that matters.
        let mut removed = 0usize;
        for n in &names {
            let Some(rest) = n.strip_prefix(prefix.as_str()) else {
                continue;
            };
            let keep =
                rest.strip_suffix(".pem").and_then(|d| d.parse::<usize>().ok()).is_some_and(|i| i < adding.len());
            if !keep && root.unlink(&format!("{dir}/{n}"))? {
                removed += 1;
            }
        }
        if adding.is_empty() {
            out.push(Fixup::new(id, &dir, Action::Unchanged).why(format!(
                "the CA directory of this image already holds each of the {} roots that the host named",
                announced.len()
            )));
            continue;
        }
        out.push(
            Fixup::new(id, &dir, if wrote > 0 || removed > 0 { Action::Rewrote } else { Action::Unchanged })
                .why(format!(
                    "{} of the {} roots that the host named are not in the CA directory of this image; one \
                     certificate for each file ({prefix}*.pem), {wrote} changed, {removed} old one(s) removed. A \
                     hash-indexed CA directory is read by NAME, so they are invisible until `openssl rehash` \
                     links them. The trust of this host is then the trust of the container too",
                    adding.len(),
                    announced.len(),
                ))
                .degraded(),
        );
        // The step is asked for where the INDEX does not reach each file of
        // the crate, or where a file changed (its hash link can name another
        // subject): a second container of a shared root does not pay for a
        // rehash that happened, and a root added to a grown bundle gets its
        // link.
        if wrote > 0 || removed > 0 || ours.iter().any(|n| !reachable.contains(n)) {
            steps.push(Step {
                id,
                argv: vec![rehash.clone(), "rehash".to_string(), format!("/{dir}")],
                why: format!(
                    "/{dir} is a hash-indexed CA directory, and OpenSSL finds a certificate there by the hash of its \
                     subject, so the roots that the crate wrote are invisible until they are linked. libzypp reads \
                     this directory and no CA file"
                ),
            });
        }
    }
    Ok((out, steps))
}
