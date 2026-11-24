//! The cgroup directories of a process from the text of
//! `/proc/self/mountinfo`: the mounts that the host or the container
//! manager made, for a process that cannot mount cgroup itself (an
//! AppArmor profile that allows a mount of cgroup only under
//! `/sys/fs/cgroup`, PodUtils/podvm#659).

use std::ffi::OsString;
use std::io::Read;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use pod_core::{Class, Failure, Stage};

use super::{MAX_FILE, Meter};
use crate::cgroup::{CGROUP_SUPER_MAGIC, CGROUP2_SUPER_MAGIC};
use crate::sys::{self, CBuf};

/// The mount table of this process.
pub const MOUNTINFO: &str = "/proc/self/mountinfo";
/// The most lines of a mount table that [`Mounts::of`] reads, so that a
/// hostile table costs a bounded work.
pub const MAX_LINES: usize = 4096;
/// The most bytes of a line that counts. A longer line does not count.
pub const MAX_LINE: usize = 8 * 1024;

/// The three directories of [`Meter::open`]. A field is `None` where no
/// mount of its kind counts.
///
/// A line of mountinfo (`proc(5)`) is `ID PARENT MAJ:MIN ROOT MOUNTPOINT
/// OPTIONS [OPTIONAL...] - FSTYPE SOURCE SUPEROPTIONS`. A line counts where
/// `ROOT` is `/` (in a cgroup namespace, the root of the namespace; a
/// sub-tree is another cgroup), `MOUNTPOINT` is an absolute path after its
/// octal escapes are undone, and the check of the mount point agrees.
/// `FSTYPE` `cgroup2` gives `v2`; `FSTYPE` `cgroup` gives `v1_memory` for
/// the item `memory` of `SUPEROPTIONS` and `v1_cpuacct` for the item
/// `cpuacct`. Of the lines of a kind that count, the last one wins: a later
/// mount at a point covers an earlier one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mounts {
    pub v2: Option<PathBuf>,
    pub v1_memory: Option<PathBuf>,
    pub v1_cpuacct: Option<PathBuf>,
}

impl Mounts {
    /// The directories of the text `mountinfo`, with no check of the mount
    /// points.
    pub fn of(mountinfo: &str) -> Mounts {
        scan(mountinfo.as_bytes(), &|_, _| true)
    }

    /// The directories of the text `mountinfo`. `magic` gives the `f_type`
    /// of `statfs(2)` of a mount point, or `None` where `statfs` fails. A
    /// line counts only where it gives `CGROUP2_SUPER_MAGIC` for `cgroup2`
    /// and `CGROUP_SUPER_MAGIC` for `cgroup`: a mount over the point (a
    /// tmpfs) hides the line, and the line before of its kind stays.
    pub fn of_with(mountinfo: &str, magic: &dyn Fn(&Path) -> Option<u64>) -> Mounts {
        scan(mountinfo.as_bytes(), &|point, want| magic(point) == Some(want))
    }

    /// The directories of [`MOUNTINFO`] of this process, each checked with
    /// `statfs(2)`. A failure where the table cannot be read: the caller
    /// gives its chain to [`Meter::note`], so that the reason of each
    /// `None` of the usage is not lost.
    pub fn of_host() -> Result<Mounts, Failure> {
        let mut text = Vec::new();
        std::fs::File::open(MOUNTINFO)
            .and_then(|f| f.take(MAX_FILE * 16).read_to_end(&mut text))
            .map_err(|e| Failure::caused(Stage::Host, Class::of_io(e.kind()), format!("read {MOUNTINFO}"), e))?;
        Ok(scan(&text, &|point, want| magic_of(point) == Some(want)))
    }

    /// The meter of the three directories ([`Meter::open`]).
    pub fn meter(&self) -> Meter {
        Meter::open(self.v2.as_deref(), self.v1_memory.as_deref(), self.v1_cpuacct.as_deref())
    }
}

/// The `f_type` of `statfs(2)` of `point`, or `None` where it fails.
fn magic_of(point: &Path) -> Option<u64> {
    let path = CBuf::from_bytes(point.as_os_str().as_encoded_bytes())?;
    sys::statfs(&path).ok().map(|s| s.f_type)
}

/// The mounts of the bytes of a table. `accept(point, magic)` is the check
/// of a mount point against the magic of its kind.
fn scan(text: &[u8], accept: &dyn Fn(&Path, u64) -> bool) -> Mounts {
    let mut m = Mounts::default();
    for line in text.split(|&b| b == b'\n').take(MAX_LINES) {
        let Some((point, kind)) = parse(line) else { continue };
        let point = PathBuf::from(point);
        match kind {
            Kind::V2 if accept(&point, CGROUP2_SUPER_MAGIC) => m.v2 = Some(point),
            Kind::V1 { memory, cpuacct } if accept(&point, CGROUP_SUPER_MAGIC) => {
                if memory {
                    m.v1_memory = Some(point.clone());
                }
                if cpuacct {
                    m.v1_cpuacct = Some(point);
                }
            }
            _ => {}
        }
    }
    m
}

/// The kind of a cgroup mount.
enum Kind {
    V2,
    V1 { memory: bool, cpuacct: bool },
}

/// The mount point and the kind of a line that counts, before the check.
fn parse(line: &[u8]) -> Option<(OsString, Kind)> {
    if line.len() > MAX_LINE {
        return None;
    }
    let fields: Vec<&[u8]> = line.split(|&b| b == b' ').collect();
    // The separator is a field by itself after the six fixed fields; the
    // optional fields end at it, and the field of the options can be `-`.
    let sep = 6 + fields.get(6..)?.iter().position(|f| *f == b"-")?;
    let (fstype, superopts) = match &fields[sep + 1..] {
        [fstype, _source, superopts, ..] => (*fstype, *superopts),
        _ => return None,
    };
    // A root that holds an escape is not `/`, so the root is not unescaped.
    if fields[3] != b"/" {
        return None;
    }
    let kind = match fstype {
        b"cgroup2" => Kind::V2,
        b"cgroup" => {
            let has = |item: &[u8]| superopts.split(|&b| b == b',').any(|o| o == item);
            let (memory, cpuacct) = (has(b"memory"), has(b"cpuacct"));
            if !memory && !cpuacct {
                return None;
            }
            Kind::V1 { memory, cpuacct }
        }
        _ => return None,
    };
    Some((unescape(fields[4])?, kind))
}

/// The mount point of its escaped form: the kernel writes a space, a tab, a
/// newline and a backslash as `\NNN`. A bad or cut escape, a NUL or a path
/// that is not absolute gives `None`. The bytes are kept as they are, also
/// where they are not UTF-8: a path on Linux is bytes, and a lossy text of
/// it names another file.
fn unescape(field: &[u8]) -> Option<OsString> {
    let mut out = Vec::with_capacity(field.len());
    let mut rest = field;
    while let Some((&b, tail)) = rest.split_first() {
        rest = tail;
        if b != b'\\' {
            out.push(b);
            continue;
        }
        let [d0 @ b'0'..=b'3', d1 @ b'0'..=b'7', d2 @ b'0'..=b'7', tail @ ..] = rest else { return None };
        out.push((d0 - b'0') << 6 | (d1 - b'0') << 3 | (d2 - b'0'));
        rest = tail;
    }
    (out.first() == Some(&b'/') && !out.contains(&0)).then(|| OsString::from_vec(out))
}

#[cfg(test)]
mod tests;
