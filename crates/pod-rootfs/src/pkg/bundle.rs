use super::*;

// ---------------------------------------------------------------- T-0407
//
// Two failures. `_apt` is an unmapped user and apt drops privileges to it for
// downloads. And the image may have no CA bundle, which the https rewrite in
// T-0411 then needs.
//
// ⛔ A DROP-IN, never an edit of `apt.conf`: the fixup is then visible,
// removable and idempotent, and an image that already sets the key is not
// clobbered.

pub(super) const APT_DROP_IN: &str = "etc/apt/apt.conf.d/99podvm";

const APT_DROP_IN_BODY: &str = "\
// Written by podvm (TODO/complete.md T-0407). apt drops privileges to the
// `_apt` user for downloads, and that uid is not mapped on this runtime, so
// every fetch fails with a permission error that names the method rather than
// the id. podvm runs as uid 0 and cannot become anything else.
APT::Sandbox::User \"root\";
";

pub(super) fn apt(root: &Root, _opts: &Options) -> Result<Vec<Fixup>> {
    if root.kind("etc/apt")?.is_missing() {
        return Ok(vec![Fixup::new(
            "T-0407",
            "apt",
            "etc/apt",
            Action::Skipped,
        )
        .why("no /etc/apt: this is not a Debian-family rootfs")]);
    }
    let mut out = Vec::new();
    let w = root.write(APT_DROP_IN, APT_DROP_IN_BODY.as_bytes(), 0o644)?;
    out.push(
        Fixup::new("T-0407", "apt-sandbox", APT_DROP_IN, crate::act(w))
            .why("APT::Sandbox::User root. The _apt user is not mapped on this runtime"),
    );
    Ok(out)
}

/// Make an `https://` package source verify, on a machine whose egress is
/// intercepted.
///
/// ⛔ **Never disable certificate verification to make https work.** If no
/// bundle can be found that is a named refusal, and the scheme rewrite in
/// T-0411 is what then does not happen.
///
/// ⭐ **Two cases, and the second is what the runtimes podvm targets are.**
///
/// 1. **the image ships no bundle**: install this machine's, so an `https://`
///    source has something to verify against;
/// 2. **the image ships one and THIS MACHINE ANNOUNCED ITS OWN** through
///    `$SSL_CERT_FILE`, `$CURL_CA_BUNDLE` or `$REQUESTS_CA_BUNDLE`: append it.
///    ⚠ Measured on 2026-09-09 on a host whose egress is intercepted:
///    `apk add gcc` inside `alpine:3.20` fails with `certificate verify
///    failed`, and it fails **identically under docker**, so this is a property
///    of the machine rather than of either runtime. Every other tool on such a
///    machine -- `curl`, `python`, `node`, `cargo` -- already reads those
///    variables; a container that does not is the anomaly.
///
/// ⛔ **A machine that did NOT announce one gets nothing appended.** A bundle
/// found only at a default path is that distribution's own trust store and says
/// nothing about interception, and adding a root to somebody else's image on
/// that basis would be a trust change nobody asked for. `--no-host-cas` refuses
/// it in the announced case too.
///
/// ⚠ The candidate list is [`podvm_image::tls`]'s, because podvm's own
/// transport already had to answer this question and two lists would drift.
const BUNDLE_PATHS: &[&str] = &[
    "etc/ssl/certs/ca-certificates.crt",
    "etc/pki/tls/certs/ca-bundle.crt",
    "etc/ssl/ca-bundle.pem",
    "etc/ssl/cert.pem",
    "etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
];

/// The line that makes the append idempotent and auditable. ⛔ A marker rather
/// than a byte comparison: the appended block has to be findable by a human
/// reading the file inside the container, and re-appending it on every run
/// would grow the bundle without bound.
pub const HOST_CA_MARKER: &str = "# podvm (TODO/complete.md T-0407): this machine's own CA bundle";

pub(super) fn ca_bundle(root: &Root, entry: &'static str, opts: &Options) -> Result<Vec<Fixup>> {
    // Where the image already keeps one, and the default for an image with none.
    // ⚠ Read THROUGH a link: `/etc/ssl/ca-bundle.pem` is a symlink to
    // `/var/lib/ca-certificates/ca-bundle.pem` on openSUSE, and a non-following
    // read reports "no bundle here" for a store the image very much has.
    let mut present: Vec<String> = Vec::new();
    for p in BUNDLE_PATHS {
        if root
            .read_following(p)?
            .map(|b| !b.is_empty())
            .unwrap_or(false)
        {
            present.push((*p).to_string());
        }
    }
    let host = podvm_image::tls::host_bundle_bytes();

    if present.is_empty() {
        let Some(h) = host else {
            return Ok(vec![Fixup::new(
                entry,
                "ca-bundle",
                BUNDLE_PATHS[0],
                Action::Skipped,
            )
            .why(
                "the image has no CA bundle and this machine has none podvm can \
                 find, so an https source in it will not verify. refused: podvm does \
                 not disable verification to make one work",
            )
            .degraded()]);
        };
        // ⛔ **EVERY conventional location whose directory is already there, not
        // one.** Measured on 2026-09-09 against `ghcr.io/void-linux/void-musl`:
        // it ships `/etc/ssl/certs/` as a HASHED DIRECTORY with no bundle file
        // in it, and OpenSSL's default `CAfile` is `$OPENSSLDIR/cert.pem` =
        // `/etc/ssl/cert.pem`. Writing only `ca-certificates.crt` put the roots
        // somewhere `xbps`'s libfetch never looks, and every fetch failed with
        // `SSL_connect returned 1`.
        //
        // ⚠ The directory has to exist already: podvm writes where the image's
        // TLS stack looks, and inventing `/etc/pki/...` in an image that has no
        // such tree is podvm guessing.
        let mut out = Vec::new();
        for dest in BUNDLE_PATHS {
            let Some((dir, _)) = dest.rsplit_once('/') else {
                continue;
            };
            match root.reach(dir)? {
                Reach::Yes => {}
                Reach::Absent => continue,
                // ⛔ `Failed` and not `Skipped`. podvm TRIED and could not,
                // which is a different sentence, and only one of the two reaches
                // the banner: `Report::banner` prints a row that changed
                // something or failed, so a `Skipped` row here would be the
                // silent skip this arm exists to end.
                Reach::Unreachable(why) => {
                    out.push(
                        Fixup::new(entry, "ca-bundle", dest, Action::Failed)
                            .why(why)
                            .degraded(),
                    );
                    continue;
                }
            }
            let Some(w) = root.write_following(dest, &h.bytes, 0o644)? else {
                out.push(
                    Fixup::new(entry, "ca-bundle", dest, Action::Failed)
                        .why(
                            "this kernel has no openat2(2), so podvm cannot write \
                             through a symlinked trust store while proving it stays \
                             inside the rootfs",
                        )
                        .degraded(),
                );
                continue;
            };
            out.push(
                Fixup::new(entry, "ca-bundle", dest, crate::act(w)).why(format!(
                    "the image had no bundle here; installed this machine's, from \
                     {}, {} bytes",
                    h.path,
                    h.bytes.len()
                )),
            );
        }
        if out.is_empty() {
            // Not even `/etc/ssl` exists: a distroless or scratch image.
            let w = root.write(BUNDLE_PATHS[0], &h.bytes, 0o644)?;
            out.push(
                Fixup::new(entry, "ca-bundle", BUNDLE_PATHS[0], crate::act(w)).why(format!(
                    "this image carries no TLS directory at all; installed this \
                     machine's bundle at the conventional path, from {}",
                    h.path
                )),
            );
        }
        return Ok(out);
    }

    let Some(h) = host else {
        return Ok(vec![Fixup::new(
            entry,
            "ca-bundle",
            &present.join(", "),
            Action::Unchanged,
        )
        .why(
            "the image ships its own bundle and this machine has none to add",
        )]);
    };
    let Some(var) = h.announced_by.clone() else {
        return Ok(vec![Fixup::new(
            entry,
            "ca-bundle",
            &present.join(", "),
            Action::Unchanged,
        )
        .why(
            "the image ships its own bundle, and this machine's came from a \
                 default path rather than from $SSL_CERT_FILE, $CURL_CA_BUNDLE or \
                 $REQUESTS_CA_BUNDLE. refused: That is no announcement, so podvm adds \
                 nothing to somebody else's trust store",
        )]);
    };
    if !opts.host_cas {
        return Ok(vec![Fixup::new(
            entry,
            "ca-bundle",
            &present.join(", "),
            Action::Skipped,
        )
        .why(format!(
            "--no-host-cas: this machine announced its own CA bundle in ${var} \
                 and podvm left the image's trust store alone. note: An https package \
                 source will fail to verify here if this machine intercepts TLS"
        ))
        .degraded()]);
    }

    // ⛔ **EVERY conventional path, not only the ones the image already has.**
    // Measured on 2026-09-09 against `ghcr.io/void-linux/void-musl`: it ships
    // `/etc/ssl/certs/ca-certificates.crt`, so the append landed there and
    // `xbps` still failed with `SSL_connect returned 1`, because libfetch's
    // OpenSSL reads its default `CAfile`, `$OPENSSLDIR/cert.pem` =
    // `/etc/ssl/cert.pem`, which the image does not ship. A bundle in the wrong
    // file is a bundle nothing reads.
    //
    // ⚠ Only where the DIRECTORY is already there: inventing `/etc/pki/...` in an
    // image with no such tree is podvm guessing at a layout.
    let mut targets = present.clone();
    let mut out = Vec::new();
    for cand in BUNDLE_PATHS {
        if targets.iter().any(|p| p == cand) {
            continue;
        }
        let Some((dir, _)) = cand.rsplit_once('/') else {
            continue;
        };
        match root.reach(dir)? {
            Reach::Yes => targets.push((*cand).to_string()),
            Reach::Absent => {}
            // ⛔ REPORTED rather than skipped. `experiments/85-completion-symlink-escape.sh`
            // door D is a `/etc/ssl/certs` pointing OUT of the rootfs: nothing
            // escapes, and a payload reading that path still gets nothing, so a
            // silent skip is a fixup that quietly did not happen.
            Reach::Unreachable(why) => out.push(
                Fixup::new(entry, "ca-bundle", cand, Action::Failed)
                    .why(why)
                    .degraded(),
            ),
        }
    }

    for p in targets {
        let existing = root
            .read_following(&p)?
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .unwrap_or_default();
        if existing.contains(HOST_CA_MARKER) {
            out.push(
                Fixup::new(entry, "ca-bundle", &p, Action::Unchanged)
                    .why("this machine's bundle is already appended here"),
            );
            continue;
        }
        // ⛔ APPENDED, never replaced: the image's own roots stay, and the file
        // says where the extra block came from so a reader inside the container
        // can see it and remove it.
        let mut merged = existing.into_bytes();
        if !merged.is_empty() && !merged.ends_with(b"\n") {
            merged.push(b'\n');
        }
        merged.extend_from_slice(
            format!("{HOST_CA_MARKER}, announced in ${var} as {}\n", h.path).as_bytes(),
        );
        merged.extend_from_slice(&h.bytes);
        let Some(w) = root.write_following(&p, &merged, 0o644)? else {
            out.push(
                Fixup::new(entry, "ca-bundle", &p, Action::Failed)
                    .why(
                        "this kernel has no openat2(2), so podvm cannot write through \
                         a symlinked trust store while proving it stays inside the \
                         rootfs",
                    )
                    .degraded(),
            );
            continue;
        };
        out.push(
            Fixup::new(entry, "ca-bundle", &p, crate::act(w))
                .why(format!(
                    "APPENDED this machine's CA bundle ({} bytes, announced in ${var} \
                     as {}). The image's own roots are kept and the added block is \
                     marked. note: This machine's trust is now the container's too; \
                     --no-host-cas refuses it",
                    h.bytes.len(),
                    h.path
                ))
                .degraded(),
        );
    }
    Ok(out)
}
