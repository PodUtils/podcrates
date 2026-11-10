//! [`TODO/complete.md`](../../../TODO/complete.md) T-0411: a payload whose own
//! package sources are `http://`, on a runtime where tcp/80 hangs.
//!
//! ⛔ **The registry half is [`TODO/image.md`](../../../TODO/image.md) T-0213
//! and it is a different decision.** There the caller names a registry and
//! podvm obeys. Here podvm is changing a file inside somebody else's image,
//! so the default is the conservative one and the disclosure is per file.
//!
//! Four rules, and each is a line below:
//!
//! 1. ⛔ **Rewrite the scheme, never the host.** `http://deb.debian.org`
//!    becomes `https://deb.debian.org`. Substituting a mirror is a supply chain
//!    change made on the payload's behalf and it is not podvm's to make;
//! 2. ⛔ **Say so, per file.** T-0804's honesty rules have no exception for a
//!    helpful edit;
//! 3. **`--no-source-fixup` turns it off**, and then the hang is the caller's
//!    choice and is named as such. ⭐ It also **undoes** a rewrite an earlier
//!    run made, because the rootfs is shared between containers and "off" that
//!    depended on which container started first would be a switch that lies;
//! 4. ⚠ **Verify the mirror speaks HTTPS first** ([`crate::reach`]), once, with
//!    a bounded timeout, and leave the file alone when it does not.
//!
//! ⚠ **`localhost` and a literal address are never rewritten.** A mirror at
//! `http://127.0.0.1:8080` is a caller's own cache, tcp/80 is not in it, and
//! `https://127.0.0.1:8080` is a connection refused.

use crate::write::{Kind, Root};
use crate::{Action, Fixup, Options, Report, Result};

/// Where each package manager keeps the URLs it fetches from.
///
/// ⚠ A directory entry means every file directly inside it; a file entry means
/// that path. ⛔ `usr/share/zypp/local/service` is here as well as
/// `etc/zypp/repos.d` because of T-0408: `refresh-services` regenerates
/// `repos.d` from the RIS index, so a fixup applied only to the generated file
/// reverts on the next refresh.
pub const SOURCE_FILES: &[&str] = &[
    "etc/apt/sources.list",
    "etc/apk/repositories",
    "etc/pacman.d/mirrorlist",
];

pub const SOURCE_DIRS: &[&str] = &[
    "etc/apt/sources.list.d",
    "etc/yum.repos.d",
    "etc/zypp/repos.d",
    "etc/xbps.d",
    "usr/share/xbps.d",
];

/// Where the originals live while a fixup is applied, so rule 3 can put them
/// back byte for byte.
///
/// ⛔ **Beside the rootfs, never inside it.** A backup inside the rootfs is a
/// file the payload can read, edit and be confused by, and `--no-source-fixup`
/// would then restore whatever the payload had left there. Same placement, and
/// the same reason, as the ownership sidecar's.
pub const BACKUP_DIR: &str = ".complete-orig";

pub(crate) fn apply(root: &Root, opts: &Options, r: &mut Report) {
    crate::record(r, "T-0411", "source-scheme", run(root, opts));
}

/// Every source file this rootfs actually has.
fn candidates(root: &Root) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for f in SOURCE_FILES {
        if matches!(root.kind(f)?, Kind::Regular { .. }) {
            out.push((*f).to_string());
        }
    }
    for d in SOURCE_DIRS {
        for n in root.list(d)? {
            let p = format!("{d}/{n}");
            if matches!(root.kind(&p)?, Kind::Regular { .. }) {
                out.push(p);
            }
        }
    }
    // ⚠ The RIS index is a tree rather than a directory of files: one directory
    // per service, each holding a `repo/` of `.repo` files. Two levels, because
    // that is what openSUSE's layout is, and no deeper, because a recursive
    // walk of somebody else's image looking for URLs to edit is a much larger
    // claim than this fixup makes.
    for base in crate::pkg::ZYPP_SERVICE_DIRS {
        for svc in root.list(base)? {
            for sub in ["repo", ""] {
                let dir = if sub.is_empty() {
                    format!("{base}/{svc}")
                } else {
                    format!("{base}/{svc}/{sub}")
                };
                for n in root.list(&dir)? {
                    let p = format!("{dir}/{n}");
                    if matches!(root.kind(&p)?, Kind::Regular { .. }) {
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

fn run(root: &Root, opts: &Options) -> Result<Vec<Fixup>> {
    let files = candidates(root)?;
    if files.is_empty() {
        return Ok(vec![Fixup::new(
            "T-0411",
            "source-scheme",
            "",
            Action::Skipped,
        )
        .why(
            "this rootfs holds no package source file podvm knows the shape of",
        )]);
    }
    let mut out = Vec::new();
    let mut prober = crate::reach::Prober::new();

    for path in files {
        // ⭐ Rule 3, and it comes first: with the fixup off, an earlier run's
        // rewrite is UNDONE, so `--no-source-fixup` means the extracted bytes
        // whether or not another container got here first.
        if !opts.source_fixup {
            if restore(root, &path)? {
                out.push(
                    Fixup::new("T-0411", "source-scheme", &path, Action::Restored).why(
                        "--no-source-fixup: an earlier run had rewritten this file's \
                         scheme, and podvm put the image's own bytes back. note: A \
                         source that is http:// will hang rather than fail where \
                         tcp/80 is black-holed, and that is now the caller's choice",
                    ),
                );
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
        // ⛔ The original is kept before the first write and never overwritten
        // by a later one: the backup is the IMAGE's bytes, not the previous
        // run's.
        keep_original(root, &path, text.as_bytes())?;
        let w = root.write(&path, new.as_bytes(), 0o644)?;
        out.push(
            Fixup::new("T-0411", "source-scheme", &path, crate::act(w)).why(format!(
                "http:// -> https:// for {}. refused: The scheme only: the mirror this \
                 image chose is the mirror podvm uses",
                hosts.join(", ")
            )),
        );
    }

    // ⚠ Every host asked and what it said, including the ones that said no and
    // whose files were therefore left alone. A file podvm did NOT edit is as
    // much of the record as one it did.
    for (host, a) in prober.asked() {
        if !a.ok() {
            out.push(
                Fixup::new("T-0411", "source-reach", host, Action::Skipped)
                    .why(format!(
                        "left every http:// source naming this host alone: it did not \
                         answer over HTTPS ({}). note: Rewriting a source that then fails \
                         is worse than the hang, because the failure stops naming the \
                         cause",
                        a.why()
                    ))
                    .degraded(),
            );
        }
    }
    if out.is_empty() {
        out.push(
            Fixup::new("T-0411", "source-scheme", "", Action::Unchanged).why(
                "every package source in this rootfs is already https:// or a \
                      literal address",
            ),
        );
    }
    Ok(out)
}

/// Rewrite every `http://HOST` whose host speaks HTTPS, and name the hosts.
///
/// ⛔ The scheme and nothing else: the host, the port, the path and every
/// option around it are the image's.
pub fn rewrite(text: &str, prober: &mut crate::reach::Prober) -> (String, Vec<String>) {
    let mut out = String::with_capacity(text.len());
    let mut hosts: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("http://") {
        out.push_str(&rest[..at]);
        let after = &rest[at + "http://".len()..];
        let end = after
            .find(|c: char| c.is_whitespace() || matches!(c, '/' | '"' | '\'' | ',' | ')' | ';'))
            .unwrap_or(after.len());
        let authority = &after[..end];
        let host = authority.split(':').next().unwrap_or(authority);
        if host.is_empty() || is_literal(host) || !prober.ask(host).ok() {
            out.push_str("http://");
        } else {
            out.push_str("https://");
            if !hosts.iter().any(|h| h == host) {
                hosts.push(host.to_string());
            }
        }
        out.push_str(authority);
        rest = &after[end..];
    }
    out.push_str(rest);
    (out, hosts)
}

/// A loopback name, an IPv4 literal or a bracketed IPv6 literal.
///
/// ⚠ Never rewritten. A mirror at `http://127.0.0.1:8080` is the caller's own
/// cache: tcp/80 is not in it, and `https://` there is a refused connection
/// rather than a fix.
fn is_literal(host: &str) -> bool {
    if host == "localhost" || host.starts_with('[') {
        return true;
    }
    let octets: Vec<&str> = host.split('.').collect();
    octets.len() == 4 && octets.iter().all(|o| o.parse::<u8>().is_ok())
}

// ------------------------------------------------------------- the originals

fn backup_path(root: &Root, rel: &str) -> std::path::PathBuf {
    // ⚠ One flat directory with the separator escaped, rather than a mirrored
    // tree: the tree would have to be created component by component with the
    // same care as the rootfs writes, for a directory nothing but podvm reads.
    let name = rel.replace('%', "%25").replace('/', "%2F");
    std::path::Path::new(root.path())
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/"))
        .join(BACKUP_DIR)
        .join(name)
}

fn keep_original(root: &Root, rel: &str, bytes: &[u8]) -> Result<()> {
    let p = backup_path(root, rel);
    if p.exists() {
        return Ok(());
    }
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)
            .map_err(|e| crate::Error::Complete(format!("cannot create {}: {e}", d.display())))?;
    }
    std::fs::write(&p, bytes)
        .map_err(|e| crate::Error::Complete(format!("cannot write {}: {e}", p.display())))
}

fn restore(root: &Root, rel: &str) -> Result<bool> {
    let p = backup_path(root, rel);
    let Ok(bytes) = std::fs::read(&p) else {
        return Ok(false);
    };
    let w = root.write(rel, &bytes, 0o644)?;
    let _ = std::fs::remove_file(&p);
    Ok(w != crate::write::Wrote::Unchanged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reach::Prober;

    /// ⛔ The scheme, and nothing else. The host, the port, the path, the
    /// suite and the components are the image's.
    #[test]
    fn only_the_scheme_changes() {
        let src = "deb http://deb.debian.org/debian bookworm main contrib\n";
        let (got, hosts) = rewrite(src, &mut Prober::always(true));
        assert_eq!(
            got,
            "deb https://deb.debian.org/debian bookworm main contrib\n"
        );
        assert_eq!(hosts, vec!["deb.debian.org"]);
    }

    #[test]
    fn a_port_survives_and_the_host_is_the_key() {
        let (got, hosts) = rewrite(
            "baseurl=http://mirror.example:8080/x\n",
            &mut Prober::always(true),
        );
        assert_eq!(got, "baseurl=https://mirror.example:8080/x\n");
        assert_eq!(hosts, vec!["mirror.example"]);
    }

    /// ⚠ A caller's own cache on loopback is never touched.
    #[test]
    fn a_literal_address_and_localhost_are_left_alone() {
        for s in [
            "deb http://127.0.0.1:8080/debian bookworm main\n",
            "deb http://localhost/debian bookworm main\n",
        ] {
            let (got, hosts) = rewrite(s, &mut Prober::always(true));
            assert_eq!(got, s);
            assert!(hosts.is_empty());
        }
    }

    /// ⛔ Clause 4. A mirror that does not answer over HTTPS keeps its scheme.
    #[test]
    fn a_mirror_that_does_not_speak_https_is_left_alone() {
        let src = "deb http://deb.debian.org/debian bookworm main\n";
        let (got, hosts) = rewrite(src, &mut Prober::always(false));
        assert_eq!(got, src);
        assert!(hosts.is_empty());
    }

    #[test]
    fn an_https_source_is_already_right() {
        let src = "deb https://deb.debian.org/debian bookworm main\n";
        let (got, hosts) = rewrite(src, &mut Prober::always(true));
        assert_eq!(got, src);
        assert!(hosts.is_empty());
    }

    /// ⭐ Rule 3 end to end: rewrite, then run again with the fixup off, and
    /// the file is the image's bytes again.
    #[test]
    fn no_source_fixup_undoes_an_earlier_rewrite() {
        let d = std::env::temp_dir().join(format!("podvm-src-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let rootfs = d.join("rootfs");
        std::fs::create_dir_all(rootfs.join("etc/apt")).unwrap();
        let original = "deb http://deb.debian.org/debian bookworm main\n";
        std::fs::write(rootfs.join("etc/apt/sources.list"), original).unwrap();

        let root = Root::open(rootfs.to_str().unwrap()).unwrap();
        // ⚠ Driven with a fixed prober: this is a test and not a measurement,
        // so it may not depend on a mirror being up.
        let text = root.read_text("etc/apt/sources.list").unwrap().unwrap();
        let (new, hosts) = rewrite(&text, &mut Prober::always(true));
        assert_eq!(hosts.len(), 1);
        keep_original(&root, "etc/apt/sources.list", text.as_bytes()).unwrap();
        root.write("etc/apt/sources.list", new.as_bytes(), 0o644)
            .unwrap();
        assert!(std::fs::read_to_string(rootfs.join("etc/apt/sources.list"))
            .unwrap()
            .contains("https://"));

        let opts = Options {
            source_fixup: false,
            ..Options::default()
        };
        let fs = run(&root, &opts).unwrap();
        assert_eq!(fs[0].action, Action::Restored);
        assert_eq!(
            std::fs::read_to_string(rootfs.join("etc/apt/sources.list")).unwrap(),
            original
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
