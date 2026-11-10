//! The three files a payload's libc reads before it reaches a socket, and the
//! one it reads before it reaches a filesystem.
//!
//! - [`resolver`] -- [`TODO/complete.md`](../../../TODO/complete.md) T-0402,
//!   `/etc/resolv.conf`;
//! - [`hosts`] -- T-0403, `/etc/hosts` and `--add-host`;
//! - [`mtab`] -- T-0405, `/etc/mtab`, which is a symlink a write escapes the
//!   rootfs through.
//!
//! ⚠ They are one module because they share a shape: a file the image ships,
//! whose content is a property of the machine rather than of the image, and
//! which podvm therefore owns. The mechanism that keeps every one of those
//! writes inside the rootfs is [`crate::write`].

use crate::write::{Kind, Root};
use crate::{Action, Fixup, Options, Report, Result};

// ---------------------------------------------------------------- T-0402
//
// ⭐ ALWAYS, not "if empty". Two failures share this fixup: an image that ships
// an empty placeholder, and an image that bakes an unreachable build-host
// resolver -- `192.168.122.1`, a libvirt NAT gateway, in the rocky family. The
// second reads as `Couldn't resolve host mirrors.*` with a `resolv.conf` that
// looks fine, and telling the two apart without resolving something is not
// possible; resolving something is a network round trip on every container
// start. It is also docker's own semantics, which is the stronger argument: an
// agent that knows docker expects the host's resolver.

pub(crate) fn resolver(root: &Root, opts: &Options, r: &mut Report) {
    crate::record(r, "T-0402", "resolv.conf", run_resolver(root, opts));
}

fn run_resolver(root: &Root, opts: &Options) -> Result<Vec<Fixup>> {
    let host = match std::fs::read(&opts.host_resolv_conf) {
        Ok(b) => b,
        Err(e) => {
            // ⛔ A machine with no resolver of its own is not a failure of this
            // fixup, and overwriting the image's file with nothing would be
            // worse than leaving it. Said, not silently skipped.
            return Ok(vec![Fixup::new(
                "T-0402",
                "resolv.conf",
                "etc/resolv.conf",
                Action::Skipped,
            )
            .why(format!(
                "this machine has no readable {}: {e}. The image's own resolver is \
                 left in place and may name a host that does not resolve here",
                opts.host_resolv_conf
            ))
            .degraded()]);
        }
    };
    let was = root.kind("etc/resolv.conf")?;
    let w = root.write("etc/resolv.conf", &host, 0o644)?;
    // ⚠ NOT a degradation when it succeeds. What the payload reads is this
    // machine's resolver, byte for byte, which is what docker's payload reads
    // too; the difference -- podvm writes the file where docker mounts one over
    // it -- is a property of having no mount namespace, and the banner's rung
    // line is where that is stated. ⛔ The `Skipped` arm above IS degraded,
    // because there the payload is left with a resolver that may name a host
    // this machine cannot reach.
    Ok(vec![Fixup::new(
        "T-0402",
        "resolv.conf",
        "etc/resolv.conf",
        crate::act(w),
    )
    .why(format!(
        "this machine's {}, {} bytes{}",
        opts.host_resolv_conf,
        host.len(),
        match was {
            Kind::Symlink(t) => format!(
                ". refused: It was a symlink to {t}, which podvm replaced rather than \
                     wrote through"
            ),
            _ => String::new(),
        }
    ))])
}

// ---------------------------------------------------------------- T-0403
//
// Write the file rather than intercept the resolver: the interposer does not
// reach a static or a Go payload, and `/etc/hosts` is read by the libc of
// whatever is running.

pub(crate) fn hosts(root: &Root, opts: &Options, r: &mut Report) {
    crate::record(r, "T-0403", "hosts", run_hosts(root, opts));
}

fn run_hosts(root: &Root, opts: &Options) -> Result<Vec<Fixup>> {
    let mut s = String::from("127.0.0.1\tlocalhost");
    if let Some(n) = &opts.container_name {
        s.push('\t');
        s.push_str(n);
    }
    s.push_str("\n::1\tlocalhost ip6-localhost ip6-loopback\n");
    let mut bad = Vec::new();
    for e in &opts.add_hosts {
        // ⛔ docker's spelling is `name:ip`, and `ip` may itself contain colons
        // because it may be IPv6. Split on the FIRST colon, not the last.
        match e.split_once(':') {
            Some((name, ip)) if !name.is_empty() && !ip.is_empty() => {
                s.push_str(&format!("{ip}\t{name}\n"));
            }
            _ => bad.push(e.clone()),
        }
    }
    if !bad.is_empty() {
        return Err(crate::Error::Complete(format!(
            "--add-host takes name:ip; podvm cannot read {}",
            bad.join(", ")
        )));
    }
    let w = root.write("etc/hosts", s.as_bytes(), 0o644)?;
    Ok(vec![Fixup::new(
        "T-0403",
        "hosts",
        "etc/hosts",
        crate::act(w),
    )
    .why(format!(
        "localhost{}{}",
        opts.container_name
            .as_ref()
            .map(|n| format!(", the container name {n}"))
            .unwrap_or_default(),
        if opts.add_hosts.is_empty() {
            String::new()
        } else {
            format!(", and {} --add-host entry(s)", opts.add_hosts.len())
        }
    ))])
}

// ---------------------------------------------------------------- T-0405
//
// ⭐ THE MECHANISM IS `crate::write`, and this is its first customer rather than
// its reason. `printf ... > "$R/etc/mtab"` follows the link; if the target is
// absolute the write lands on the host.
//
// ⚠ `paper_final.md` F10 establishes that `/etc/mtab` is not required by pacman
// in general -- it is read only when `CheckSpace` is enabled, which Arch's
// shipped `pacman.conf` leaves commented out -- so this fixup applies where the
// file is BROKEN and nowhere else. An image whose `/etc/mtab` is a working
// regular file keeps it.

pub(crate) fn mtab(root: &Root, r: &mut Report) {
    crate::record(r, "T-0405", "mtab", run_mtab(root));
}

fn run_mtab(root: &Root) -> Result<Vec<Fixup>> {
    let k = root.kind("etc/mtab")?;
    // ⚠ `etc` itself may not exist: a distroless or scratch image has no
    // `/etc`, and creating one just to put a mount table in it is podvm adding
    // a directory nothing asked for.
    // ⛔ `reach` and not `Kind::Dir`; see `identity::nsswitch` for the door
    // sweep that found this and `write::Reach` for the three answers.
    match root.reach("etc")? {
        crate::write::Reach::Yes => {}
        crate::write::Reach::Absent => {
            return Ok(vec![Fixup::new(
                "T-0405",
                "mtab",
                "etc/mtab",
                Action::Skipped,
            )
            .why("this image has no /etc directory")])
        }
        crate::write::Reach::Unreachable(why) => {
            return Ok(vec![Fixup::new(
                "T-0405",
                "mtab",
                "etc/mtab",
                Action::Failed,
            )
            .why(why)
            .degraded()])
        }
    }
    let reason = match &k {
        Kind::Regular { .. } => {
            return Ok(vec![Fixup::new(
                "T-0405",
                "mtab",
                "etc/mtab",
                Action::Unchanged,
            )
            .why("the image ships a regular file here and podvm leaves it")]);
        }
        Kind::Missing => "it was absent".to_string(),
        // ⚠ Two different sentences, because they are two different facts. The
        // ordinary case is a link into `/proc`, which does not resolve because
        // a chroot mounts nothing; any other target is a link podvm is not
        // going to follow whatever it points at. Printing the `/proc`
        // explanation for a link that does not name `/proc` would be podvm
        // asserting a cause it did not check.
        Kind::Symlink(t) if t.starts_with("/proc/") || t.contains("/proc/") => format!(
            "it was a symlink to {t}, and /proc is not mounted inside a chroot, so \
             the link does not resolve. refused: podvm REPLACED the link rather than \
             writing through it: an absolute target would have put this write on \
             the host"
        ),
        Kind::Symlink(t) => format!(
            "it was a symlink to {t}. refused: podvm REPLACED the link rather than \
             writing through it: an absolute target would have put this write \
             outside the rootfs"
        ),
        other => format!("it was {other:?}"),
    };
    let table = topology(root)?;
    let w = root.write("etc/mtab", table.as_bytes(), 0o644)?;
    Ok(vec![Fixup::new(
        "T-0405",
        "mtab",
        "etc/mtab",
        crate::act(w),
    )
    .why(format!(
        "{reason}. One line, generated from this machine's own mount \
                      table for the filesystem the rootfs is on"
    ))
    .degraded()])
}

/// The mount table as it will be true inside the chroot.
///
/// ⛔ **Generated, never shipped.** Another machine's device numbers in a mount
/// table is a fabricated number, and a payload that reads them acts on them.
///
/// ⚠ There is no mount namespace, so the payload sees this machine's mounts;
/// every one of them whose mount point is outside the rootfs is unreachable
/// from inside it and is not in this file. What is left is the filesystem the
/// rootfs itself is on, mounted at `/`, plus anything mounted underneath it.
fn topology(root: &Root) -> Result<String> {
    let mounts = std::fs::read_to_string("/proc/self/mounts").unwrap_or_default();
    let rootfs = root.path();
    let mut best: Option<(usize, String)> = None;
    let mut under = Vec::new();
    for line in mounts.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 4 {
            continue;
        }
        let (src, point, fstype, opts) = (f[0], f[1], f[2], f[3]);
        if point == "/" || rootfs.starts_with(&format!("{point}/")) || point == rootfs {
            // The longest mount point that is a prefix of the rootfs is the
            // filesystem the rootfs is actually on.
            if best.as_ref().map(|(n, _)| point.len() > *n).unwrap_or(true) {
                best = Some((point.len(), format!("{src} / {fstype} {opts} 0 0")));
            }
        } else if point.starts_with(&format!("{rootfs}/")) {
            under.push(format!(
                "{src} {} {fstype} {opts} 0 0",
                &point[rootfs.len()..]
            ));
        }
    }
    let mut out = String::new();
    match best {
        Some((_, l)) => out.push_str(&format!("{l}\n")),
        // ⛔ A dash where the value is unknown, and the file says so rather
        // than naming a device podvm did not measure.
        None => out.push_str("- / unknown rw 0 0\n"),
    }
    for l in under {
        out.push_str(&format!("{l}\n"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> String {
        let p = std::env::temp_dir().join(format!("podvm-net-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("etc")).unwrap();
        p.to_string_lossy().to_string()
    }

    /// ⛔ T-0405's `Prove`, at unit scale. The write must not reach the canary.
    #[test]
    fn mtab_replaces_an_escaping_symlink_and_the_canary_survives() {
        let d = scratch("mtab");
        let canary = std::env::temp_dir().join(format!("podvm-mtab-canary-{}", std::process::id()));
        std::fs::write(&canary, b"host\n").unwrap();
        std::os::unix::fs::symlink(&canary, format!("{d}/etc/mtab")).unwrap();
        let root = Root::open(&d).unwrap();
        let fs = run_mtab(&root).unwrap();
        assert_eq!(fs[0].action, Action::Rewrote);
        assert!(fs[0].detail.contains("REPLACED the link"), "{:?}", fs[0]);
        assert_eq!(std::fs::read(&canary).unwrap(), b"host\n");
        let got = std::fs::read_to_string(format!("{d}/etc/mtab")).unwrap();
        assert!(got.contains(" / "), "{got}");
        let _ = std::fs::remove_file(&canary);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// ⚠ An image that ships a working file keeps it. This fixup is for a
    /// broken one.
    #[test]
    fn mtab_leaves_a_regular_file_alone() {
        let d = scratch("mtabreg");
        std::fs::write(format!("{d}/etc/mtab"), b"mine\n").unwrap();
        let root = Root::open(&d).unwrap();
        let fs = run_mtab(&root).unwrap();
        assert_eq!(fs[0].action, Action::Unchanged);
        assert_eq!(std::fs::read(format!("{d}/etc/mtab")).unwrap(), b"mine\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// ⛔ An IPv6 `--add-host` has colons in the address. Splitting on the last
    /// one loses the address.
    #[test]
    fn add_host_splits_on_the_first_colon() {
        let d = scratch("hosts");
        let root = Root::open(&d).unwrap();
        let opts = Options {
            container_name: Some("c1".into()),
            add_hosts: vec!["six:2001:db8::1".into(), "four:10.0.0.1".into()],
            ..Options::default()
        };
        run_hosts(&root, &opts).unwrap();
        let got = std::fs::read_to_string(format!("{d}/etc/hosts")).unwrap();
        assert!(got.contains("2001:db8::1\tsix\n"), "{got}");
        assert!(got.contains("10.0.0.1\tfour\n"), "{got}");
        assert!(got.contains("127.0.0.1\tlocalhost\tc1\n"), "{got}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_malformed_add_host_is_refused_by_name() {
        let d = scratch("badhost");
        let root = Root::open(&d).unwrap();
        let opts = Options {
            add_hosts: vec!["nocolon".into()],
            ..Options::default()
        };
        let e = run_hosts(&root, &opts).unwrap_err();
        assert!(format!("{e}").contains("nocolon"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// ⛔ T-0402 is unconditional. An image whose resolver names a build host's
    /// libvirt gateway looks exactly like one that is right.
    #[test]
    fn the_resolver_is_overwritten_even_when_the_image_has_one() {
        let d = scratch("resolv");
        std::fs::write(
            format!("{d}/etc/resolv.conf"),
            b"nameserver 192.168.122.1\n",
        )
        .unwrap();
        let host = std::env::temp_dir().join(format!("podvm-hostresolv-{}", std::process::id()));
        std::fs::write(&host, b"nameserver 10.1.2.3\n").unwrap();
        let root = Root::open(&d).unwrap();
        let opts = Options {
            host_resolv_conf: host.to_string_lossy().to_string(),
            ..Options::default()
        };
        let fs = run_resolver(&root, &opts).unwrap();
        assert_eq!(fs[0].action, Action::Rewrote);
        assert_eq!(
            std::fs::read(format!("{d}/etc/resolv.conf")).unwrap(),
            b"nameserver 10.1.2.3\n"
        );
        let _ = std::fs::remove_file(&host);
        let _ = std::fs::remove_dir_all(&d);
    }
}
