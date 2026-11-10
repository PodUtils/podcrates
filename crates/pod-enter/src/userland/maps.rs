//! The table of path maps of a run with no chroot: the root pair and the
//! volume pairs.
//!
//! The caller puts the table into the variable `names.maps`, and the
//! interposer changes each absolute guest path through the pair with the
//! longest guest prefix. The entry resolves the working directory and the
//! first program through the same table ([`crate::abi::GuestRoot`]), so a
//! `-w` or a program in a volume is the file of the volume, as the payload
//! sees it.

use std::path::{Path, PathBuf};

use crate::Names;
use crate::abi::GuestRoot;

/// The longest side of a pair of the table that the interposer reads. A
/// longer side is malformed there and the interposer skips its pair,
/// which on this rung would stop each change of a path and leave host
/// paths bare. The builders below refuse it.
pub const MAX_MAP_SIDE: usize = 4096;

/// The most pairs that the interposer reads from one table. It ignores a
/// pair past this bound, so the builders refuse a table that holds one.
pub const MAX_MAP_PAIRS: usize = 32;

/// One volume: the guest path and the host path that serves it
/// (`-v HOST:GUEST`, the pair `GUEST:HOST` of the table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    pub guest: String,
    pub host: String,
}

/// The root file system and the volumes of a run with no chroot.
///
/// [`Maps::of_root`] is the table of the root pair alone: each guest path
/// resolves inside the root file system. [`Maps::new`] adds the volumes.
/// The guest side of a volume is a normal absolute path that is not `/`,
/// and one guest path has one volume, so the longest prefix is one pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Maps {
    rootfs: String,
    volumes: Vec<Volume>,
    /// The components of the guest side of each volume, in the order of
    /// `volumes`.
    parts: Vec<Vec<String>>,
}

impl Maps {
    /// The table of the root pair alone.
    pub fn of_root(rootfs: &str) -> Maps {
        Maps { rootfs: rootfs.to_string(), volumes: Vec::new(), parts: Vec::new() }
    }

    /// The root pair and the volume pairs, in the order given.
    ///
    /// A volume is refused by name where its guest side has a NUL, is not
    /// absolute, is `/`, has a `.`, `..` or empty component, or repeats the
    /// guest side of another volume; and where its host side has a NUL or
    /// is not absolute. The guest side loses a trailing `/`, as the host
    /// side does unless it is `/`. The spelling of each side in the
    /// variable is checked when the table is spelled
    /// ([`userland_maps_table`]).
    pub fn new(rootfs: &str, volumes: &[Volume]) -> Result<Maps, String> {
        let mut maps = Maps::of_root(rootfs);
        for v in volumes {
            let at = format!("the volume {}", label(v));
            if v.guest.contains('\0') || v.host.contains('\0') {
                return Err(format!("{at}: a side contains a NUL byte"));
            }
            if !v.guest.starts_with('/') {
                return Err(format!("{at}: the guest side must be an absolute path"));
            }
            let guest = v.guest.trim_end_matches('/');
            if guest.is_empty() {
                return Err(format!("{at}: a volume cannot replace the guest root `/`"));
            }
            let parts: Vec<String> = guest[1..].split('/').map(str::to_string).collect();
            if parts.iter().any(|c| c.is_empty() || c == "." || c == "..") {
                return Err(format!(
                    "{at}: the guest side must be a normal path, with no `.`, `..` or empty \
                     component, because the interposer matches the normal form"
                ));
            }
            if maps.parts.contains(&parts) {
                return Err(format!("{at}: a second volume at the guest path {guest}"));
            }
            if !v.host.starts_with('/') {
                return Err(format!("{at}: the host side must be an absolute path"));
            }
            let host = match v.host.trim_end_matches('/') {
                "" => "/",
                h => h,
            };
            maps.volumes.push(Volume { guest: guest.to_string(), host: host.to_string() });
            maps.parts.push(parts);
        }
        Ok(maps)
    }

    /// The host path of the root file system, as given.
    pub fn rootfs(&self) -> &str {
        &self.rootfs
    }

    /// The volumes, each side in its normal form.
    pub fn volumes(&self) -> &[Volume] {
        &self.volumes
    }

    /// The volume that serves the guest path `guest` (the longest guest
    /// prefix at a component boundary), or `None` where the root file
    /// system serves it. Lexical: no link is followed, as the interposer
    /// matches.
    pub fn volume_at(&self, guest: &str) -> Option<&Volume> {
        let parts: Vec<String> = guest.split('/').filter(|c| !c.is_empty()).map(str::to_string).collect();
        self.pick(&parts).map(|i| &self.volumes[i])
    }

    /// The index of the volume with the longest guest prefix of `parts`.
    fn pick(&self, parts: &[String]) -> Option<usize> {
        self.parts.iter().enumerate().filter(|(_, g)| parts.starts_with(g)).max_by_key(|(_, g)| g.len()).map(|(i, _)| i)
    }

    /// True where a volume serves the resolved guest path `parts`.
    pub(crate) fn in_volume(&self, parts: &[String]) -> bool {
        self.pick(parts).is_some()
    }
}

impl GuestRoot for Maps {
    fn root(&self) -> &Path {
        Path::new(&self.rootfs)
    }

    fn host_of(&self, parts: &[String]) -> (PathBuf, bool) {
        match self.pick(parts) {
            Some(i) => {
                let rest = &parts[self.parts[i].len()..];
                let host = rest.iter().fold(PathBuf::from(&self.volumes[i].host), |a, c| a.join(c));
                (host, rest.is_empty())
            }
            None => (parts.iter().fold(PathBuf::from(&self.rootfs), |a, c| a.join(c)), false),
        }
    }

    fn above_mount(&self, parts: &[String]) -> bool {
        self.parts.iter().any(|g| g.len() > parts.len() && g.starts_with(parts))
    }
}

/// A volume as `HOST:GUEST` for a message, each side cut to 64 bytes.
fn label(v: &Volume) -> String {
    format!("{}:{}", cut(&v.host), cut(&v.guest))
}

fn cut(s: &str) -> String {
    if s.len() <= 64 {
        return s.to_string();
    }
    let mut end = 64;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}

/// Check one side for the variable: no `:` or `,`, at most
/// [`MAX_MAP_SIDE`] bytes. `what` names the side in the refusal.
fn side(names: &Names, what: &str, path: &str) -> Result<(), String> {
    if path.contains(':') || path.contains(',') {
        return Err(format!(
            "{what} {:?} cannot be spelled in a {} table: `:` and `,` separate pairs, so a table \
             carrying it would parse as the wrong mapping",
            cut(path),
            names.maps
        ));
    }
    if path.len() > MAX_MAP_SIDE {
        return Err(format!(
            "{what} is {} bytes, over the {MAX_MAP_SIDE}-byte table side the interposer reads",
            path.len()
        ));
    }
    Ok(())
}

/// The root pair as the variable spells it.
fn root_pair(names: &Names, rootfs: &str) -> Result<String, String> {
    if rootfs.contains('\0') {
        return Err("the rootfs path contains a NUL byte".to_string());
    }
    let base = rootfs.trim_end_matches('/');
    if base.is_empty() {
        return Err("the rootfs path is empty".to_string());
    }
    if !base.starts_with('/') {
        return Err(format!("{rootfs:?} is not an absolute rootfs path"));
    }
    side(names, "the rootfs path", base)?;
    Ok(format!("/:{base}"))
}

/// The volume pairs as the variable spells them, each side checked with
/// its own name.
fn volume_pairs(names: &Names, maps: &Maps) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(maps.volumes.len());
    for v in &maps.volumes {
        let at = format!("the volume {}", label(v));
        side(names, &format!("{at}: the guest side"), &v.guest)?;
        side(names, &format!("{at}: the host side"), &v.host)?;
        out.push(format!("{}:{}", v.guest, v.host));
    }
    Ok(out)
}

fn bounded(names: &Names, pairs: Vec<String>) -> Result<String, String> {
    if pairs.len() > MAX_MAP_PAIRS {
        return Err(format!(
            "the {} table holds {} pairs, over the {MAX_MAP_PAIRS} that the interposer reads",
            names.maps,
            pairs.len()
        ));
    }
    Ok(pairs.join(","))
}

/// Build the table of path maps (`names.maps`) of a run with no chroot:
/// the root pair, then one pair for each volume.
///
/// The `/` of the guest maps to the host path of the root file system, so
/// an absolute guest path resolves inside the image and not on the tree
/// of the host. A volume pair maps its guest side to its host side, and
/// the interposer takes the longest guest prefix, so the volume wins over
/// the root pair. The paths that the interposer never changes (its own
/// guard directory, `/proc`, the list of the user) are not in the table:
/// the interposer enforces them, and skips a pair of a table that tries
/// to move them.
///
/// A side that the table cannot spell is refused with its own name: the
/// rootfs path relative, with a NUL, empty after the trim; a side with
/// `:` or `,` or over [`MAX_MAP_SIDE`] bytes; more than
/// [`MAX_MAP_PAIRS`] pairs. An empty table changes nothing, which on this
/// rung is a silent write to the host, so a malformed side is a refusal,
/// never a table.
///
/// The caller sets the variable to the returned table before the payload
/// starts, after it removes each value of the caller, and names the table
/// on the banner: the change of paths must be visible, never silent.
pub fn userland_maps_table(names: &Names, maps: &Maps) -> Result<String, String> {
    let mut pairs = vec![root_pair(names, &maps.rootfs)?];
    pairs.extend(volume_pairs(names, maps)?);
    bounded(names, pairs)
}

/// The volume pairs of [`userland_maps_table`] with no root pair, for a
/// run whose caller keeps the root of the host (an opt-in of the caller).
/// Empty where the table has no volume.
pub fn userland_volume_table(names: &Names, maps: &Maps) -> Result<String, String> {
    bounded(names, volume_pairs(names, maps)?)
}

#[cfg(test)]
mod tests;
