//! Three files that the C library of a payload reads before it reaches a
//! socket, and one that it reads before it reaches a file system.
//!
//! - `resolver`, for `/etc/resolv.conf`;
//! - `hosts`, for `/etc/hosts`, with docker's `--add-host`;
//! - `mtab`, for `/etc/mtab`, a symlink through which a write leaves the root.
//!
//! The content of each is a property of the host, not of the image, so the
//! completion owns them. [`crate::write`] keeps each write inside the root.

use std::path::Path;

use crate::write::{Kind, Reach, Root};
use crate::{Action, Fixup, Report, Result, Settings};

// The resolver: ALWAYS, not "when empty". An image can ship an empty
// file, or the unreachable resolver of its build host (`192.168.122.1`, a
// libvirt gateway, in the rocky family). Only a lookup tells the two apart,
// and that is a round trip on each start. docker gives the resolver of the
// host too.

pub(crate) fn resolver(root: &Root, settings: &Settings, r: &mut Report) {
    crate::record(r, "resolv.conf", run_resolver(root, settings));
}

fn run_resolver(root: &Root, settings: &Settings) -> Result<Vec<Fixup>> {
    let from = settings.host_resolv_conf.display();
    let host = match std::fs::read(&settings.host_resolv_conf) {
        Ok(b) => b,
        Err(e) => {
            // A host with no resolver is no failure of this fixup, and a
            // resolver of nothing is worse than the one of the image.
            return Ok(vec![
                Fixup::new("resolv.conf", "etc/resolv.conf", Action::Skipped)
                    .why(format!(
                        "this host has no readable {from}: {e}. The resolver of the image stays, and it can name a \
                         host that does not resolve here"
                    ))
                    .degraded(),
            ]);
        }
    };
    let was = root.kind("etc/resolv.conf")?;
    let w = root.write("etc/resolv.conf", &host, 0o644)?;
    // Not degraded: the payload reads the resolver of this host, byte for
    // byte, as docker's payload does.
    Ok(vec![Fixup::new("resolv.conf", "etc/resolv.conf", crate::act(w)).why(format!(
        "the {from} of this host, {} bytes{}",
        host.len(),
        match was {
            Kind::Symlink(t) =>
                format!(". It was a symlink to {t}, which the crate replaced and did not write through"),
            _ => String::new(),
        }
    ))])
}

// `/etc/hosts`: a file and not an interposed resolver. An interposer does
// not reach a static or a Go payload; the C library of each payload reads
// the file.

pub(crate) fn hosts(root: &Root, settings: &Settings, r: &mut Report) {
    crate::record(r, "hosts", run_hosts(root, settings));
}

/// A word of a line of `/etc/hosts`: no white space, no control character,
/// no `#`. A line end in a name or an address writes a line that the caller
/// did not give.
fn one_word(s: &str) -> bool {
    !s.is_empty() && !s.chars().any(|c| c.is_whitespace() || c.is_control() || c == '#')
}

fn run_hosts(root: &Root, settings: &Settings) -> Result<Vec<Fixup>> {
    let mut s = String::from("127.0.0.1\tlocalhost");
    if let Some(n) = &settings.container_name {
        if !one_word(n) {
            return Err(crate::error::usage(format!("the container name {n:?} is not one word of /etc/hosts")));
        }
        s.push('\t');
        s.push_str(n);
    }
    s.push_str("\n::1\tlocalhost ip6-localhost ip6-loopback\n");
    let mut bad = Vec::new();
    for e in &settings.add_hosts {
        // docker writes `name:address`, and an IPv6 address holds colons:
        // the split is at the FIRST colon.
        match e.split_once(':') {
            Some((name, ip)) if one_word(name) && one_word(ip) => s.push_str(&format!("{ip}\t{name}\n")),
            _ => bad.push(format!("{e:?}")),
        }
    }
    if !bad.is_empty() {
        return Err(crate::error::usage(format!(
            "an added host is name:address, each one word; the crate cannot read {}",
            bad.join(", ")
        )));
    }
    let w = root.write("etc/hosts", s.as_bytes(), 0o644)?;
    Ok(vec![Fixup::new("hosts", "etc/hosts", crate::act(w)).why(format!(
        "localhost{}{}",
        settings.container_name.as_ref().map(|n| format!(", the container name {n}")).unwrap_or_default(),
        if settings.add_hosts.is_empty() {
            String::new()
        } else {
            format!(", and {} added host(s)", settings.add_hosts.len())
        }
    ))])
}

// `/etc/mtab`: `crate::write` is the mechanism. `printf ... > "$R/etc/mtab"`
// follows the link, and with an absolute target the write lands on the
// host. pacman reads `/etc/mtab` only with `CheckSpace`, which Arch ships as
// a comment, so the fixup applies where the file is BROKEN and nowhere else:
// a working regular file stays.

pub(crate) fn mtab(root: &Root, settings: &Settings, r: &mut Report) {
    crate::record(r, "mtab", run_mtab(root, &settings.mounts));
}

fn run_mtab(root: &Root, mounts: &Path) -> Result<Vec<Fixup>> {
    let k = root.kind("etc/mtab")?;
    // A distroless or a scratch image has no `/etc`, and a new `/etc` for a
    // mount table is a directory that nothing asked for.
    match root.reach("etc")? {
        Reach::Yes => {}
        Reach::Absent => {
            return Ok(vec![Fixup::new("mtab", "etc/mtab", Action::Skipped).why("this image has no /etc directory")]);
        }
        Reach::Unreachable(why) => return Ok(vec![Fixup::new("mtab", "etc/mtab", Action::Failed).why(why).degraded()]),
    }
    let reason = match &k {
        Kind::Regular { .. } => {
            return Ok(vec![
                Fixup::new("mtab", "etc/mtab", Action::Unchanged)
                    .why("the image ships a regular file here, and it stays"),
            ]);
        }
        Kind::Missing => "it was absent".to_string(),
        // Two causes, two texts: a link into `/proc`, which a root with no
        // mount does not resolve, and each other link.
        Kind::Symlink(t) if t.contains("/proc/") => format!(
            "it was a symlink to {t}, and /proc is not mounted in the root, so the link does not resolve. The crate \
             REPLACED the link and did not write through it: with an absolute target the write lands on the host"
        ),
        Kind::Symlink(t) => format!(
            "it was a symlink to {t}. The crate REPLACED the link and did not write through it: with an absolute \
             target the write lands outside the root"
        ),
        other => format!("it was {other:?}"),
    };
    let table = std::fs::read_to_string(mounts).unwrap_or_default();
    let w = root.write("etc/mtab", topology(&table, &root.written().to_string_lossy()).as_bytes(), 0o644)?;
    Ok(vec![
        Fixup::new("mtab", "etc/mtab", crate::act(w))
            .why(format!("{reason}. One line, made from the mount table of this host for the file system of the root"))
            .degraded(),
    ])
}

/// The mount point of a line of a mount table: `\040` (space), `\011` (tab),
/// `\012` (line end) and `\134` (backslash) are the escapes of the kernel.
fn unescape(field: &str) -> String {
    let b = field.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        // Three octal digits, the first of them 0 to 3: one byte.
        if b[i] == b'\\'
            && i + 4 <= b.len()
            && (b'0'..=b'3').contains(&b[i + 1])
            && b[i + 2..i + 4].iter().all(|c| (b'0'..=b'7').contains(c))
        {
            out.push((b[i + 1] - b'0') * 64 + (b[i + 2] - b'0') * 8 + (b[i + 3] - b'0'));
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

/// The mount table as it is true inside the root, from the mount table of
/// the host (`table`) and the path of the root.
///
/// MADE, never shipped: the device numbers of another host are numbers that
/// a payload acts on. With no mount namespace the payload sees the mounts of
/// this host, and each one outside the root is out of its reach. What is
/// left: the file system of the root at `/`, and each mount under it. A
/// field of the table keeps its escapes, as the kernel writes them.
pub fn topology(table: &str, rootfs: &str) -> String {
    let rootfs = rootfs.trim_end_matches('/');
    let mut best: Option<(usize, String)> = None;
    let mut under = Vec::new();
    for line in table.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 4 {
            continue;
        }
        let (src, point, fstype, opts) = (f[0], unescape(f[1]), f[2], f[3]);
        if point == "/" || rootfs.starts_with(&format!("{point}/")) || point == rootfs {
            // The longest mount point that holds the root is the file system
            // of the root.
            if best.as_ref().is_none_or(|(n, _)| point.len() > *n) {
                best = Some((point.len(), format!("{src} / {fstype} {opts} 0 0")));
            }
        } else if let Some(rest) = point.strip_prefix(&format!("{rootfs}/")) {
            under.push(format!("{src} /{} {fstype} {opts} 0 0", escape(rest)));
        }
    }
    let mut out = String::new();
    match best {
        Some((_, l)) => out.push_str(&format!("{l}\n")),
        // A dash where the value is not known: no device that nobody
        // measured.
        None => out.push_str("- / unknown rw 0 0\n"),
    }
    for l in under {
        out.push_str(&format!("{l}\n"));
    }
    out
}

/// The escapes of the kernel for a mount point.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' ' => out.push_str("\\040"),
            '\t' => out.push_str("\\011"),
            '\n' => out.push_str("\\012"),
            '\\' => out.push_str("\\134"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests;
