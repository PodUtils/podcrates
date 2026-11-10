//! Package sources of the image that are `http://`, on a host where tcp/80
//! waits for ever.
//!
//! Four rules:
//!
//! 1. **The scheme changes, never the host.** `http://deb.debian.org`
//!    becomes `https://deb.debian.org`. Another mirror is a change of the
//!    supply chain for the payload, and the crate does not make it;
//! 2. **Each changed file is a row**;
//! 3. **`Settings::source_fixup` turns it off**, and then the crate also
//!    **puts back** the original of each file that an earlier run changed:
//!    a root is shared between containers, and a switch whose result
//!    depends on which container started first does not do what it says;
//! 4. **The mirror must speak HTTPS first** ([`crate::reach`]), one question
//!    for each host; a file whose host does not stays.
//!
//! An address and a loopback name are never changed: a mirror at
//! `http://127.0.0.1:8080` is a cache of the caller, and `https://` there is
//! a refused connection.

use std::path::{Path, PathBuf};

use crate::reach::{Probe, Prober, is_name};
use crate::write::{Kind, Root};
use crate::{Action, Fixup, Report, Result, Settings};

/// The files of the package managers that hold the URLs of their sources.
pub const SOURCE_FILES: &[&str] = &["etc/apt/sources.list", "etc/apk/repositories", "etc/pacman.d/mirrorlist"];

/// The directories of the package managers that hold such files, one level.
/// `etc/zypp/repos.d` is made again from the RIS index of zypper
/// ([`crate::pkg::ZYPP_SERVICE_DIRS`]), so the index gets the same change.
pub const SOURCE_DIRS: &[&str] =
    &["etc/apt/sources.list.d", "etc/yum.repos.d", "etc/zypp/repos.d", "etc/xbps.d", "usr/share/xbps.d"];

pub(crate) fn apply(root: &Root, settings: &Settings, probe: &mut dyn Probe, r: &mut Report) {
    crate::record(r, "source-scheme", run(root, settings, probe));
}

/// Each source file of this root.
fn candidates(root: &Root) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let regular = |p: &str| -> Result<bool> { Ok(matches!(root.kind(p)?, Kind::Regular { .. })) };
    for f in SOURCE_FILES {
        if regular(f)? {
            out.push((*f).to_string());
        }
    }
    for d in SOURCE_DIRS {
        for n in root.list(d)? {
            let p = format!("{d}/{n}");
            if regular(&p)? {
                out.push(p);
            }
        }
    }
    // The RIS index: one directory for each service, each with a `repo/` of
    // `.repo` files. Two levels, as the layout of openSUSE is, and no more: a
    // walk of a whole image for URLs is a larger claim than this fixup makes.
    for base in crate::pkg::ZYPP_SERVICE_DIRS {
        for svc in root.list(base)? {
            for dir in [format!("{base}/{svc}/repo"), format!("{base}/{svc}")] {
                for n in root.list(&dir)? {
                    let p = format!("{dir}/{n}");
                    if regular(&p)? {
                        out.push(p);
                    }
                }
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn run(root: &Root, settings: &Settings, probe: &mut dyn Probe) -> Result<Vec<Fixup>> {
    let files = candidates(root)?;
    if files.is_empty() {
        return Ok(vec![
            Fixup::new("source-scheme", "", Action::Skipped)
                .why("this root holds no package source file of a known shape"),
        ]);
    }
    let Some(originals) = &settings.originals else {
        return Ok(vec![Fixup::new("source-scheme", "", Action::Skipped).why(
            "the caller gave no directory for the originals, so the crate changes no package source and puts none \
             back",
        )]);
    };
    let mut out = Vec::new();
    let mut prober = Prober::new(probe);

    for path in files {
        // Rule 3 first: with the fixup off, a change of an earlier run is
        // UNDONE.
        if !settings.source_fixup {
            if restore(root, originals, &path)? {
                out.push(Fixup::new("source-scheme", &path, Action::Restored).why(
                    "the source fixup is off: an earlier run had changed the scheme of this file, and the crate put \
                     the bytes of the image back. A source that is http:// waits where tcp/80 is dropped",
                ));
            }
            continue;
        }
        let Some(text) = root.read_text(&path)? else {
            continue;
        };
        let (new, hosts) = rewrite(&text, &mut prober);
        if hosts.is_empty() {
            continue;
        }
        // The original is kept before the first write and never replaced by
        // a later one: it holds the bytes of the IMAGE.
        keep_original(originals, &path, text.as_bytes())?;
        let w = root.write(&path, new.as_bytes(), 0o644)?;
        out.push(Fixup::new("source-scheme", &path, crate::act(w)).why(format!(
            "http:// -> https:// for {}. The scheme only: the mirror of the image stays",
            hosts.join(", ")
        )));
    }

    // Each host that said no, and why: a file that the crate did NOT change
    // is part of the record too.
    for (host, a) in prober.asked() {
        if !a.ok() {
            out.push(
                Fixup::new("source-reach", host, Action::Skipped)
                    .why(format!(
                        "each http:// source of this host stays: it did not answer over HTTPS ({}). A changed \
                         source that then fails is worse than the wait, as the failure no longer names its cause",
                        a.why()
                    ))
                    .degraded(),
            );
        }
    }
    if out.is_empty() {
        out.push(
            Fixup::new("source-scheme", "", Action::Unchanged)
                .why("each package source of this root is https://, an address or a loopback name"),
        );
    }
    Ok(out)
}

/// The host of the authority of a URL: no user part, no port. `None` for a
/// bracketed IPv6 address, and for an authority with a character that RFC
/// 3986 does not allow there (`\`): two parsers read such a text as two
/// different hosts.
fn host_of(authority: &str) -> Option<&str> {
    let allowed = |c: char| c.is_ascii_alphanumeric() || "-._~!$&'()*+,;=:%@[]".contains(c);
    if !authority.chars().all(allowed) {
        return None;
    }
    let hostport = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if hostport.starts_with('[') {
        return None;
    }
    Some(hostport.split_once(':').map_or(hostport, |(h, _)| h))
}

/// Change each `http://HOST` whose host speaks HTTPS, and name the hosts.
///
/// The scheme and nothing else: the host, the port, the path and each
/// option around them stay. Only a DNS name is asked about
/// ([`is_name`]): an address, a loopback name or a text that is not a name
/// stays `http://`, also behind a user part (`http://user@127.0.0.1/`).
pub(crate) fn rewrite(text: &str, prober: &mut Prober<'_>) -> (String, Vec<String>) {
    let mut out = String::with_capacity(text.len());
    let mut hosts: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("http://") {
        out.push_str(&rest[..at]);
        let after = &rest[at + "http://".len()..];
        let end = after
            .find(|c: char| c.is_whitespace() || matches!(c, '/' | '"' | '\'' | ',' | ')' | ';' | '?' | '#'))
            .unwrap_or(after.len());
        let authority = &after[..end];
        match host_of(authority).filter(|h| is_name(h)) {
            Some(host) if prober.ask(host).ok() => {
                out.push_str("https://");
                if !hosts.iter().any(|h| h == host) {
                    hosts.push(host.to_string());
                }
            }
            _ => out.push_str("http://"),
        }
        out.push_str(authority);
        rest = &after[end..];
    }
    out.push_str(rest);
    (out, hosts)
}

/// The original of `rel`: one flat directory, with `%` and `/` escaped.
fn backup_path(originals: &Path, rel: &str) -> PathBuf {
    originals.join(rel.replace('%', "%25").replace('/', "%2F"))
}

fn keep_original(originals: &Path, rel: &str, bytes: &[u8]) -> Result<()> {
    let p = backup_path(originals, rel);
    if p.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(originals)
        .map_err(|e| crate::error::io(format!("cannot create {}", originals.display()), e))?;
    std::fs::write(&p, bytes).map_err(|e| crate::error::io(format!("cannot write {}", p.display()), e))
}

fn restore(root: &Root, originals: &Path, rel: &str) -> Result<bool> {
    let p = backup_path(originals, rel);
    let Ok(bytes) = std::fs::read(&p) else {
        return Ok(false);
    };
    let w = root.write(rel, &bytes, 0o644)?;
    let _ = std::fs::remove_file(&p);
    Ok(w != crate::write::Wrote::Unchanged)
}

#[cfg(test)]
mod tests;
