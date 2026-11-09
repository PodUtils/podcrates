//! `--device HOST[:GUEST[:PERMS]]` by a descriptor.
//!
//! The rungs, the cheapest first. `mknod(2)` inside the payload is
//! denied, so no node can be made at the guest path. `mount(2)` is denied,
//! so no host path can be bound over it. The descriptor holds: the entry
//! opens it before the root changes, gives it to the child at a fixed
//! number, and the interposer serves it where an open names the exact
//! guest path. A payload that opens the guest path gets a duplicate of the
//! open file description of the host: reads and writes reach the device
//! of the host, and `stat` gives the host file, not a node that was never
//! made. Where the interposer does not hold (a static payload, a Go
//! payload, a foreign architecture, or no object for the C library of the
//! image) the open fails with the `ENOENT` of the kernel, as an absent path
//! does, and the banner said so before the payload ran.
//!
//! `m` in `PERMS` does nothing: there is no `mknod` to grant. It parses,
//! and the banner says so on each run that has one.
//!
//! The wire format: two variables, one for each job. The device list
//! (`Names::device_spec`) is in the container record, so a `create` and a
//! later `start` open the devices again and do not inherit them. The serve
//! table (`Names::device_fds`) is built beside the descriptors that it
//! names. Entries join with `\x1e` (the record separator, never in a real
//! path); fields join with `:`. A field with either byte is refused at the
//! parse, with its name, because the split would put the end of the field
//! in the wrong place.
//! - device list: `host:guest:perms` for each entry, canonical perms.
//! - serve table: `guest:childfd:perms` for each entry, which the open hook
//!   of the interposer reads.

use pod_core::Class;
use pod_hostprobe::sys::{self, CBuf};

use crate::names::var_name;
use crate::{Error, Names, errno_failure};

/// Record separator between entries. Never in a real path.
pub const SEP_ENTRY: char = '\x1e';
/// The first descriptor of the child for a device. Above the standard
/// streams (0, 1, 2) and a memo of the caller at 17, below nothing that
/// the entry makes later: the readiness pipe is `O_CLOEXEC`, and an image
/// lock goes by inheritance, never by number.
pub const CHILD_FD_BASE: i64 = 41;
/// Host descriptors below this are relocated clear of the child range
/// before the hand-over, so two mappings can never clobber each other in
/// the child's `dup2` loop.
pub const HOST_FD_FLOOR: i64 = 64;

/// One parsed `--device` mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapping {
    pub host: String,
    pub guest: String,
    pub read: bool,
    pub write: bool,
}

/// Parse `HOST[:GUEST[:PERMS]]`. The guest defaults to the host spelling;
/// perms default to `rwm`. `r` and `w` open the host path for reading and
/// writing; `m` parses and grants nothing (`mknod` is denied on this
/// runtime, and the banner says so where one rides).
pub fn parse(spec: &str) -> Result<Mapping, String> {
    let parts: Vec<&str> = spec.split(':').collect();
    if parts.len() > 3 {
        return Err(format!("{spec:?} has more than two colons: --device takes HOST[:GUEST[:PERMS]]"));
    }
    let (host, guest, perms) = match parts.len() {
        1 => (parts[0], parts[0], "rwm"),
        2 => (parts[0], parts[1], "rwm"),
        _ => (parts[0], parts[1], parts[2]),
    };
    if host.is_empty() {
        return Err("--device needs a host path, not an empty one".into());
    }
    if !host.starts_with('/') {
        return Err(format!(
            "{host:?} is not absolute: the entry opens it before the chroot, \
             and a caller-relative path would name this process's directory, \
             not the payload's"
        ));
    }
    if guest.is_empty() || !guest.starts_with('/') {
        return Err(format!("{guest:?} is not a guest-absolute path: --device takes HOST[:GUEST[:PERMS]]"));
    }
    if guest == "/" {
        return Err("--device cannot map over /: the guest path must name a file inside it".into());
    }
    if host.contains(SEP_ENTRY) || guest.contains(SEP_ENTRY) {
        return Err(format!("{spec:?} holds a record separator: the device table cannot carry it"));
    }
    if host.contains(':') || guest.contains(':') {
        // A guard for callers that pass pre-split fields rather than a raw
        // spelling: a colon inside a field would lie about where the field
        // ends on the wire. Today's callers split first, so this is the
        // invariant stated where it is enforced.
        return Err(format!("{spec:?} holds a colon inside a path: --device splits on colons"));
    }
    let mut read = false;
    let mut write = false;
    for c in perms.chars() {
        match c {
            'r' => read = true,
            'w' => write = true,
            'm' => {}
            _ => {
                return Err(format!("{perms:?} is not device permissions: --device perms take r, w and m only"));
            }
        }
    }
    if !read && !write {
        return Err(format!("{perms:?} grants neither read nor write: a device with only `m` cannot be opened"));
    }
    Ok(Mapping { host: host.into(), guest: guest.into(), read, write })
}

/// Canonical perms for the wire: `r` then `w`. The `m` is dropped here and
/// re-stated by the banner, because the table the interposer reads only
/// answers reads and writes.
pub fn canon(m: &Mapping) -> &'static str {
    match (m.read, m.write) {
        (true, true) => "rw",
        (true, false) => "r",
        (false, true) => "w",
        (false, false) => "r",
    }
}

/// One entry of the device list.
pub fn spec_entry(m: &Mapping) -> String {
    format!("{}:{}:{}", m.host, m.guest, canon(m))
}

/// One entry of the serve table.
pub fn serve_entry(guest: &str, child_fd: i64, perms: &str) -> String {
    format!("{guest}:{child_fd}:{perms}")
}

/// The banner line naming one mapping, shared by every verb that enters
/// with devices: what the guest spelling serves, where, and what `m`
/// does, before anything of the payload's runs.
pub fn banner_line(names: &Names, m: &Mapping) -> String {
    format!(
        "{}: --device {} is reachable inside as {} ({}): a duplicate \
         of a host descriptor opened before the chroot, not a device \
         node, served where the interposer holds and only where the \
         open fails without it (a creating open the image satisfies \
         lands in the image, not the device); `m` grants nothing\n",
        names.product,
        m.host,
        m.guest,
        canon(m)
    )
}

/// The opened devices: what the child's `dup2` loop is handed, and the
/// serve table beside it.
pub struct Opened {
    /// `(child_fd, host_fd)` pairs for [`crate::Fds`].
    pub pass: Vec<(i64, i64)>,
    /// `(guest, child_fd, perms)` rows of the serve table.
    pub serve: Vec<(String, i64, String)>,
}

/// Open every host path and stage the hand-over. Everything here runs
/// before the fork, in the process whose root is still the host's: a
/// descriptor opened now keeps working after the child's `chroot`.
///
/// A host path that does not open is refused with its name, because a mapping that silently serves nothing would
/// read as a node that exists and fails later for no stated reason.
pub fn open_all(specs: &[Mapping]) -> Result<Opened, Error> {
    let mut pass = Vec::with_capacity(specs.len());
    let mut serve = Vec::with_capacity(specs.len());
    // The child numbers this call hands out, so a host descriptor sitting
    // on one is moved clear before the hand-over.
    let count = i64::try_from(specs.len())
        .map_err(|_| Error::runtime(Class::Usage, "--device: too many devices".to_string()))?;
    let child_top = CHILD_FD_BASE + count;
    // Relocated descriptors land past the child range, so no move can ever
    // collide with a number the child loop `dup2`s onto.
    let clear_floor = HOST_FD_FLOOR.max(child_top) as u64;
    for (i, m) in specs.iter().enumerate() {
        // `i` is below `count`, which fits an i64.
        let child_fd = CHILD_FD_BASE + i as i64;
        let flags = match (m.read, m.write) {
            (true, true) => sys::O_RDWR,
            (true, false) => sys::O_RDONLY,
            (false, true) => sys::O_WRONLY,
            (false, false) => sys::O_RDONLY,
        } | sys::O_CLOEXEC;
        let Some(c) = CBuf::new(&m.host) else {
            return Err(Error::runtime(
                Class::Usage,
                format!("--device {:?}: the host path contains a NUL byte", m.host),
            ));
        };
        // Opened by name here, before the fork, and never again inside:
        // after the `chroot` this path is gone, and the child sees only the
        // descriptor.
        let mut host_fd = sys::open(&c, flags, 0)
            .map_err(|e| errno_failure(&format!("--device {:?}: opening the host path failed", m.host), e))?;
        if host_fd < HOST_FD_FLOOR || (host_fd >= CHILD_FD_BASE && host_fd < child_top) {
            // Clear of the child range before the hand-over: the child's
            // loop `dup2`s in order and closes as it goes, so a host
            // descriptor sitting on a later child's number would be closed
            // before it is duplicated.
            match sys::fcntl(host_fd, sys::F_DUPFD_CLOEXEC, clear_floor) {
                Ok(moved) => {
                    let _ = sys::close(host_fd);
                    host_fd = moved;
                }
                Err(e) => {
                    let _ = sys::close(host_fd);
                    return Err(errno_failure(
                        &format!("--device {:?}: moving the host descriptor clear failed", m.host),
                        e,
                    ));
                }
            }
        }
        pass.push((child_fd, host_fd));
        serve.push((m.guest.clone(), child_fd, canon(m).into()));
    }
    Ok(Opened { pass, serve })
}

/// Read the device list back from an environment that the caller built.
/// A value of the user never gets here (the caller removes both variables
/// before it adds its own), so a malformed row is a defect of the caller,
/// and it is refused as one, not skipped.
pub fn specs_of(names: &Names, env: &[String]) -> Result<Vec<Mapping>, Error> {
    let Some(raw) = env.iter().rev().find_map(|e| {
        let (k, v) = e.split_once('=')?;
        (k == names.device_spec).then(|| v.to_string())
    }) else {
        return Ok(Vec::new());
    };
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    raw.split(SEP_ENTRY)
        .map(|row| {
            let parts: Vec<&str> = row.split(':').collect();
            if parts.len() != 3 {
                return Err(Error::runtime(
                    Class::Usage,
                    format!("{} holds a malformed row {row:?}: want host:guest:perms", names.device_spec),
                ));
            }
            parse(&format!("{}:{}:{}", parts[0], parts[1], parts[2])).map_err(|why| {
                Error::runtime(Class::Usage, format!("{} holds a bad row {row:?}: {why}", names.device_spec))
            })
        })
        .collect()
}

/// Remove the device variables of the user, then add the device list
/// that the caller parsed. An old or foreign table must not get into the
/// entry.
pub fn push_spec(names: &Names, env: &mut Vec<String>, specs: &[Mapping]) {
    env.retain(|e| {
        let name = var_name(e);
        name != names.device_spec && name != names.device_fds
    });
    if specs.is_empty() {
        return;
    }
    let rows: Vec<String> = specs.iter().map(spec_entry).collect();
    env.push(format!("{}={}", names.device_spec, rows.join(&SEP_ENTRY.to_string())));
}

/// Add the serve table beside the descriptors that [`open_all`] staged.
/// The entering process calls it after the opens, before the fork.
pub fn push_serve(names: &Names, env: &mut Vec<String>, opened: &Opened) {
    env.retain(|e| var_name(e) != names.device_fds);
    if opened.serve.is_empty() {
        return;
    }
    let rows: Vec<String> = opened.serve.iter().map(|(g, fd, p)| serve_entry(g, *fd, p)).collect();
    env.push(format!("{}={}", names.device_fds, rows.join(&SEP_ENTRY.to_string())));
}

#[cfg(test)]
mod tests;
