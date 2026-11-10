use super::*;

// ---------------------------------------------------------------- T-0412
//
// ⭐ **A CAfile and a hash-indexed CApath are two trust stores, and `zypper`
// reads only the second.** Measured on 2026-09-09 inside podvm, on
// `registry.opensuse.org/opensuse/leap:15.6`:
//
//     curl                               https://download.opensuse.org/  -> 200
//     curl --capath /etc/ssl/certs        https://download.opensuse.org/  -> 60
//
// libzypp hands libcurl a `CURLOPT_CAPATH` of `/etc/ssl/certs`, which on
// openSUSE is a symlink to `/var/lib/ca-certificates/pem`. OpenSSL looks a
// certificate up there by the SHA-1 of its canonical DER subject, so every
// CAfile T-0407 writes is invisible to it and a file dropped in under any other
// name is invisible too.
//
// ⛔ **The link cannot be made from the host, so this fixup has two halves.**
// podvm writes one PEM per root (the write), and asks the caller to run
// `openssl rehash` on the directory (the step). ⚠ `update-ca-certificates` is
// openSUSE's own answer and it CANNOT be the step: measured on the same image,
// it exits 1 at `done < <(find ...)` with `/dev/fd/62: No such file or
// directory`. bash's process substitution needs `/dev/fd`, which is
// `/proc/self/fd`, and `/proc` is not mounted inside a chroot -- the same shape
// as `/etc/mtab -> /proc/self/mounts` in T-0405.
//
// ⚠ ONE CERTIFICATE PER FILE, measured rather than assumed: a file holding
// three of them is refused by `openssl rehash` with `it does not contain
// exactly one certificate or CRL`, and nothing is linked.

/// The conventional hash-indexed CApath directories, rootfs-relative.
///
/// ⛔ **Only where the directory is ALREADY hash-indexed.** podvm writes where
/// the image's TLS stack looks; creating a hash directory in an image that has
/// none would be podvm inventing a trust store nothing consults.
pub const CAPATH_DIRS: &[&str] = &["etc/ssl/certs", "etc/pki/tls/certs"];

/// The prefix of every certificate file this fixup writes. ⛔ A prefix rather
/// than a marker line inside the file: the file has to be readable by
/// `openssl rehash` as one certificate, and the name is what a reader inside
/// the container greps for and what podvm itself removes when the machine's
/// bundle shrinks.
pub const HOST_CA_FILE_PREFIX: &str = "podvm-host-ca-";

/// Split a PEM bundle into one text per `CERTIFICATE` block.
///
/// ⚠ Everything outside a block is dropped, which is deliberate: a bundle
/// carries comments, subject lines and T-0407's own marker, and `openssl
/// rehash` wants a file it can read as exactly one certificate.
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
                out.push(cur.take().expect("just pushed into it"));
            }
        }
    }
    out
}

/// Is `name` a name OpenSSL's hash lookup would find? `01419da9.0`, and the
/// CRL form `01419da9.r0`.
pub(super) fn is_hash_link(name: &str) -> bool {
    let Some((hash, tail)) = name.split_once('.') else {
        return false;
    };
    hash.len() == 8
        && hash.bytes().all(|b| b.is_ascii_hexdigit())
        && !tail.is_empty()
        && tail
            .trim_start_matches('r')
            .bytes()
            .all(|b| b.is_ascii_digit())
}

/// Where two candidates are the same directory, so it is not written and
/// rehashed twice. ⚠ `/etc/ssl/certs -> /etc/pki/tls/certs` is Fedora's own
/// link and both are in [`CAPATH_DIRS`].
fn dir_key(root: &Root, rel: &str) -> Result<String> {
    Ok(match root.kind(rel)? {
        Kind::Symlink(t) if t.starts_with('/') => t.trim_start_matches('/').to_string(),
        _ => rel.to_string(),
    })
}

pub(super) fn ca_hash_dir(root: &Root, opts: &Options) -> Result<(Vec<Fixup>, Vec<Step>)> {
    let entry = "T-0412";
    let id = "ca-hash-dir";
    /// One row and no step, which is every arm of this fixup that does not
    /// write. ⚠ A free function rather than a closure so the `degraded` arms
    /// read the same as the quiet ones.
    fn only(path: &str, a: Action, degraded: bool, why: String) -> (Vec<Fixup>, Vec<Step>) {
        let f = Fixup::new("T-0412", "ca-hash-dir", path, a).why(why);
        (vec![if degraded { f.degraded() } else { f }], Vec::new())
    }

    // ⛔ The ANNOUNCEMENT is the discriminator, exactly as in T-0407's
    // `ca_bundle`: a bundle found at a default path says nothing about
    // interception and is no reason to touch somebody else's trust store.
    let Some(h) = podvm_image::tls::host_bundle_bytes() else {
        return Ok(only(
            CAPATH_DIRS[0],
            Action::Skipped,
            false,
            "this machine has no CA bundle podvm can find, so there is nothing to \
             index into the image's CApath"
                .to_string(),
        ));
    };
    let Some(var) = h.announced_by.clone() else {
        return Ok(only(
            CAPATH_DIRS[0],
            Action::Unchanged,
            false,
            "this machine's CA bundle came from a default path rather than from \
             $SSL_CERT_FILE, $CURL_CA_BUNDLE or $REQUESTS_CA_BUNDLE. refused: That is no \
             announcement, so podvm adds nothing to somebody else's trust store"
                .to_string(),
        ));
    };

    // Which directories are hash-indexed, deduplicated.
    let mut dirs: Vec<(String, Vec<String>)> = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    for d in CAPATH_DIRS {
        // ⚠ The same three answers as the CAfile half, and for the same reason:
        // a CApath that is a symlink is openSUSE's own shape and podvm follows
        // it; one pointing out of the rootfs is door D and podvm does not.
        if !matches!(root.reach(d)?, Reach::Yes) {
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
            "this image carries no hash-indexed CApath directory, so there is none \
             for podvm to index its roots into"
                .to_string(),
        ));
    }
    if !opts.host_cas {
        return Ok(only(
            &dirs[0].0,
            Action::Skipped,
            true,
            format!(
                "--no-host-cas: this machine announced its own CA bundle in ${var} \
                 and podvm left the image's hash-indexed trust store alone. note: A \
                 tool reading a CApath -- libzypp is one -- will fail to verify \
                 here if this machine intercepts TLS"
            ),
        ));
    }

    // ⛔ The program is found BEFORE anything is written. A certificate in a
    // hash directory that nothing has linked is a file no TLS stack reads, and
    // writing 150 of them into somebody else's image for nothing is litter.
    let Some(rehash) = find_program(root, "openssl")? else {
        return Ok(only(
            &dirs[0].0,
            Action::Skipped,
            true,
            "this image has a hash-indexed CApath and no openssl to index it with, \
             so podvm cannot make this machine's roots visible to a tool that \
             reads a CApath rather than a CAfile"
                .to_string(),
        ));
    };
    if !opts.steps {
        return Ok(only(
            &dirs[0].0,
            Action::Skipped,
            true,
            "--no-steps: podvm did not write this machine's roots into the image's \
             hash-indexed CApath, because indexing them needs `openssl rehash` run \
             inside the rootfs. note: A tool reading a CApath will not verify here"
                .to_string(),
        ));
    }

    let announced = split_pem(&h.bytes);
    let mut out = Vec::new();
    let mut steps = Vec::new();
    for (dir, names) in dirs {
        // ⛔ **What the INDEX can reach, and not what the directory contains.**
        // Measured on 2026-09-09 against `ghcr.io/void-linux/void-musl`: T-0407
        // writes the whole announced bundle to
        // `/etc/ssl/certs/ca-certificates.crt`, which is a CAfile sitting inside
        // the CApath, and counting it made every announced root look present
        // while the hash lookup could still find none of them. A CApath is read
        // by NAME, so only a `HASH.N` entry -- and the file a `HASH.N` symlink
        // points at -- is content this directory holds.
        //
        // ⚠ podvm's OWN files are excluded from the comparison, or a second
        // run would find every root already present and delete it as stale.
        let mut reachable: Vec<String> = Vec::new();
        for n in names.iter().filter(|n| is_hash_link(n)) {
            match root.kind(&format!("{dir}/{n}"))? {
                // ⚠ A name and never a path: `openssl rehash` writes a bare
                // name, and a target with a `/` in it is somebody else's
                // convention that podvm does not follow to another directory.
                // ⚠ Many links point at one file, so each name is collected
                // once: `contains` and not `dedup`, which only folds neighbours.
                Kind::Symlink(t) if !t.contains('/') && !reachable.contains(&t) => {
                    reachable.push(t)
                }
                Kind::Regular { .. } if !reachable.contains(n) => reachable.push(n.clone()),
                _ => {}
            }
        }
        let mut held: Vec<String> = Vec::new();
        for t in &reachable {
            if t.starts_with(HOST_CA_FILE_PREFIX) {
                continue;
            }
            if let Some(b) = root.read(&format!("{dir}/{t}"))? {
                held.extend(split_pem(&b));
            }
        }
        let adding: Vec<&String> = announced.iter().filter(|c| !held.contains(c)).collect();

        let mut wrote = 0usize;
        for (i, pem) in adding.iter().enumerate() {
            let rel = format!("{dir}/{HOST_CA_FILE_PREFIX}{i:03}.pem");
            let body = format!(
                "# podvm ({entry}): one root of this machine's own CA bundle, \
                 announced in ${var} as {}, that this image did not already \
                 carry. refused: podvm added it to somebody else's trust store; \
                 --no-host-cas refuses it\n{pem}",
                h.path
            );
            if root.write(&rel, body.as_bytes(), 0o644)? != crate::write::Wrote::Unchanged {
                wrote += 1;
            }
        }
        // ⚠ A bundle that SHRANK leaves files nothing wrote this run, and a
        // stale root in a trust store is the one kind of leftover that matters.
        let mut removed = 0usize;
        for n in &names {
            let Some(rest) = n.strip_prefix(HOST_CA_FILE_PREFIX) else {
                continue;
            };
            let keep = rest
                .strip_suffix(".pem")
                .and_then(|d| d.parse::<usize>().ok())
                .is_some_and(|i| i < adding.len());
            if !keep && root.unlink(&format!("{dir}/{n}"))? {
                removed += 1;
            }
        }
        // ⭐ The step is asked for only where the LINKS are missing, so a second
        // container on the same shared rootfs does not pay for a rehash that has
        // already happened. ⛔ Read off the index rather than off "podvm wrote
        // something this run": the rootfs is content-addressed and shared, and a
        // payload that deleted the links must get them back.
        let linked = reachable.iter().any(|t| t.starts_with(HOST_CA_FILE_PREFIX));
        if adding.is_empty() {
            out.push(Fixup::new(entry, id, &dir, Action::Unchanged).why(format!(
                "this image's CApath already holds every one of this machine's {} \
                 announced roots, so there is nothing to add to it",
                announced.len()
            )));
            continue;
        }
        let action = if wrote > 0 || removed > 0 {
            Action::Rewrote
        } else {
            Action::Unchanged
        };
        out.push(
            Fixup::new(entry, id, &dir, action)
                .why(format!(
                    "{} of this machine's {} announced roots are not in this image's \
                     CApath; written as one certificate per file ({}*.pem), {wrote} \
                     changed, {removed} stale one(s) removed. note: A hash-indexed \
                     CApath is read by NAME, so they stay invisible until `openssl \
                     rehash` links them; this machine's trust is then the \
                     container's too",
                    adding.len(),
                    announced.len(),
                    HOST_CA_FILE_PREFIX
                ))
                .degraded(),
        );
        if !linked {
            steps.push(Step {
                entry,
                id,
                argv: vec![rehash.clone(), "rehash".to_string(), format!("/{dir}")],
                why: format!(
                    "/{dir} is a hash-indexed CApath and OpenSSL looks a certificate \
                     up there by the hash of its subject, so the roots podvm just \
                     wrote are invisible until they are linked. libzypp reads this \
                     directory and no CAfile at all"
                ),
            });
        }
    }
    Ok((out, steps))
}
