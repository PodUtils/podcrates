use super::*;

// apt: `_apt` is a user that the runtime does not map, and apt drops its
// privileges to it for each download. A DROP-IN, never an edit of
// `apt.conf`: the fixup is then visible, removable and idempotent.

fn apt_drop_in_body(tool: &str) -> String {
    format!(
        "// Written by {tool}. apt drops its privileges to the `_apt` user for each\n// download, and this runtime \
         does not map that uid.\nAPT::Sandbox::User \"root\";\n"
    )
}

pub(super) fn apt(root: &Root, settings: &Settings) -> Result<Vec<Fixup>> {
    if root.kind("etc/apt")?.is_missing() {
        return Ok(vec![
            Fixup::new("apt", "etc/apt", Action::Skipped).why("no /etc/apt: this is not a root of the Debian family"),
        ]);
    }
    let path = &settings.names.apt_drop_in;
    let w = root.write(path, apt_drop_in_body(&settings.names.tool).as_bytes(), 0o644)?;
    Ok(vec![
        Fixup::new("apt-sandbox", path, crate::act(w))
            .why("APT::Sandbox::User root. This runtime does not map the _apt user"),
    ])
}

/// The CA bundles of the distributions, relative to the root.
pub const BUNDLE_PATHS: &[&str] = &[
    "etc/ssl/certs/ca-certificates.crt",
    "etc/pki/tls/certs/ca-bundle.crt",
    "etc/ssl/ca-bundle.pem",
    "etc/ssl/cert.pem",
    "etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
];

/// The text of a row on a kernel with no `openat2`, or of another refusal
/// of a write through a link.
fn not_written(why: String) -> String {
    format!(
        "the crate cannot write through the trust store while it proves that the write stays inside the root: {why}"
    )
}

/// Make an `https://` package source verify on a host whose TLS is
/// intercepted. Certificate checks are never turned off: with no bundle the
/// row says so, and the change of the scheme does not happen.
///
/// Two cases:
///
/// 1. **the image ships no bundle**: the crate puts the bundle of the host
///    at each usual path whose directory is there, so that an `https://`
///    source has roots;
/// 2. **the image ships one, and A VARIABLE OF THE HOST NAMED the bundle of
///    the host** ([`crate::HostBundle::announced_by`]): the crate appends
///    it. `apk add gcc` inside `alpine:3.20` fails with `certificate verify
///    failed` on a host whose TLS is intercepted, and fails the same under
///    docker; each other tool of such a host reads those variables.
///
/// A bundle found at a default path is the store of a distribution and says
/// nothing of an interception: the crate adds nothing to the trust store of
/// an image for it. `Settings::host_cas` off refuses both cases: the row is
/// `Skipped` and degraded, and no file is written.
pub(crate) fn ca_bundle(root: &Root, settings: &Settings) -> Result<Vec<Fixup>> {
    let marker = &settings.names.host_ca_marker;
    // Read THROUGH a link: `/etc/ssl/ca-bundle.pem` is a symlink to
    // `/var/lib/ca-certificates/ca-bundle.pem` on openSUSE.
    let mut present: Vec<String> = Vec::new();
    for p in BUNDLE_PATHS {
        if root.read_following(p)?.is_some_and(|b| !b.is_empty()) {
            present.push((*p).to_string());
        }
    }
    let host = settings.host_bundle.as_ref();

    if present.is_empty() {
        let Some(h) = host else {
            return Ok(vec![
                Fixup::new("ca-bundle", BUNDLE_PATHS[0], Action::Skipped)
                    .why(
                        "the image has no CA bundle and the host gave none, so an https source in it does not \
                         verify. The crate does not turn the certificate check off",
                    )
                    .degraded(),
            ]);
        };
        // Off means no host root in the trust of the container: a bundle of
        // the host can hold an intercepting CA.
        if !settings.host_cas {
            return Ok(vec![
                Fixup::new("ca-bundle", BUNDLE_PATHS[0], Action::Skipped)
                    .why(format!(
                        "the host CA fixup is off: the image has no CA bundle, and the bundle of the host ({}) is not \
                         written into it. An https source in it does not verify",
                        h.path
                    ))
                    .degraded(),
            ]);
        }
        // EACH usual path whose directory is there, not one: void-musl
        // ships `/etc/ssl/certs/` as a hashed directory with no bundle, and
        // its OpenSSL reads `/etc/ssl/cert.pem`. A directory that is not
        // there is not made: that is a guess of a layout.
        let mut out = Vec::new();
        for dest in BUNDLE_PATHS {
            let Some((dir, _)) = dest.rsplit_once('/') else {
                continue;
            };
            match root.reach(dir)? {
                Reach::Yes => {}
                Reach::Absent => continue,
                // Failed, not Skipped: the crate TRIED and could not.
                Reach::Unreachable(why) => {
                    out.push(Fixup::new("ca-bundle", dest, Action::Failed).why(why).degraded());
                    continue;
                }
            }
            out.push(match root.write_following(dest, &h.bytes, 0o644)? {
                Ok(w) => Fixup::new("ca-bundle", dest, crate::act(w)).why(format!(
                    "the image had no bundle here; this is the bundle of the host, from {}, {} bytes",
                    h.path,
                    h.bytes.len()
                )),
                Err(why) => Fixup::new("ca-bundle", dest, Action::Failed).why(not_written(why)).degraded(),
            });
        }
        if out.is_empty() {
            // Not even `/etc/ssl`: a distroless or a scratch image.
            let w = root.write(BUNDLE_PATHS[0], &h.bytes, 0o644)?;
            out.push(Fixup::new("ca-bundle", BUNDLE_PATHS[0], crate::act(w)).why(format!(
                "this image holds no TLS directory; the bundle of the host is at the usual path, from {}",
                h.path
            )));
        }
        return Ok(out);
    }

    let Some(h) = host else {
        return Ok(vec![
            Fixup::new("ca-bundle", &present.join(", "), Action::Unchanged)
                .why("the image ships its own bundle and the host gave none to add"),
        ]);
    };
    let Some(var) = h.announced_by.clone() else {
        return Ok(vec![Fixup::new("ca-bundle", &present.join(", "), Action::Unchanged).why(
            "the image ships its own bundle, and the bundle of the host is at a default path that no variable \
             named. That tells nothing of an interception, so the crate adds nothing to the trust store of the image",
        )]);
    };
    if !settings.host_cas {
        return Ok(vec![
            Fixup::new("ca-bundle", &present.join(", "), Action::Skipped)
                .why(format!(
                    "the host CA fixup is off: ${var} named the bundle of the host, and the trust store of the image \
                     stays. An https package source does not verify here if this host intercepts TLS"
                ))
                .degraded(),
        ]);
    }

    // EACH usual path, not only those that the image has: void-musl ships
    // `/etc/ssl/certs/ca-certificates.crt`, and its OpenSSL reads
    // `/etc/ssl/cert.pem`. A bundle in the wrong file is a bundle that
    // nothing reads. Only where the DIRECTORY is there.
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
            // A `/etc/ssl/certs` that points OUT of the root: nothing leaves,
            // and a payload that reads the path gets nothing. The row says so.
            Reach::Unreachable(why) => out.push(Fixup::new("ca-bundle", cand, Action::Failed).why(why).degraded()),
        }
    }

    for p in targets {
        // Bytes, not text: a byte of the bundle of the image that is not
        // UTF-8 stays as it is.
        let existing = root.read_following(&p)?.unwrap_or_default();
        if !marker.is_empty() && existing.windows(marker.len()).any(|w| w == marker.as_bytes()) {
            out.push(
                Fixup::new("ca-bundle", &p, Action::Unchanged).why("the bundle of the host is already appended here"),
            );
            continue;
        }
        // APPENDED, never replaced: the roots of the image stay, and the
        // marker says where the block came from.
        let mut merged = existing;
        if !merged.is_empty() && !merged.ends_with(b"\n") {
            merged.push(b'\n');
        }
        merged.extend_from_slice(format!("{marker}, named in ${var} as {}\n", h.path).as_bytes());
        merged.extend_from_slice(&h.bytes);
        out.push(match root.write_following(&p, &merged, 0o644)? {
            Ok(w) => Fixup::new("ca-bundle", &p, crate::act(w))
                .why(format!(
                    "the bundle of the host is APPENDED ({} bytes, named in ${var} as {}). The roots of the image \
                     stay, and a marker line names the added block. The trust of this host is now the trust of the \
                     container too",
                    h.bytes.len(),
                    h.path
                ))
                .degraded(),
            Err(why) => Fixup::new("ca-bundle", &p, Action::Failed).why(not_written(why)).degraded(),
        });
    }
    Ok(out)
}
