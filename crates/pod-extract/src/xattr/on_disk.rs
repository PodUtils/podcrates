//! The extended attributes of a path on disk, read in full or refused with
//! the path: a copy or a pack of a tree (podvm's image of the incus tier and
//! root disk of the machine tier, podrunner's snapshot) loses no attribute
//! without a word.

use std::io::{Error, ErrorKind};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use pod_hostprobe::sys::{self, CBuf, Errno};

use super::{NAMES_MAX, VALUE_MAX};

/// The reads of the list before the reader gives up: each new try follows
/// a list that grew between the ask of its size and its read.
const LIST_TRIES: usize = 4;

/// Each attribute of `path`, as (name, value), in the order of the kernel,
/// with no follow of a symlink at the last component (`llistxattr(2)`,
/// `lgetxattr(2)`).
///
/// * The size of the list comes from the kernel, not from a fixed buffer;
///   a list that grows before its read is read again. A list past 64 KiB
///   (`XATTR_LIST_MAX`) is `InvalidData`.
/// * A file system with no extended attributes (`ENOTSUP`) gives an empty
///   list.
/// * An attribute removed between the list and the read of its value
///   (`ENODATA`) is not in the answer.
/// * A name that is not UTF-8 is `InvalidData`, not a name left out.
/// * Each error names `path` and keeps the kind of the errno (a missing
///   path is `NotFound`).
///
/// The kernel decides which names a process sees: `trusted.` needs
/// `CAP_SYS_ADMIN`.
pub fn of_path(path: &Path) -> std::io::Result<Vec<(String, Vec<u8>)>> {
    let at = |what: &str, e: Error| Error::new(e.kind(), format!("{}: {what}: {e}", path.display()));
    let c = CBuf::from_bytes(path.as_os_str().as_bytes())
        .ok_or_else(|| at("the path", Error::new(ErrorKind::InvalidInput, "a NUL in the path")))?;
    let names = match names(&c) {
        Ok(n) => n,
        Err(e) if e.raw_os_error() == Some(sys::EOPNOTSUPP.0) => return Ok(Vec::new()),
        Err(e) => return Err(at("llistxattr", e)),
    };
    let mut out = Vec::new();
    let mut value = vec![0u8; VALUE_MAX];
    for raw in names.split(|&b| b == 0).filter(|n| !n.is_empty()) {
        let Ok(name) = std::str::from_utf8(raw) else {
            let lossy = String::from_utf8_lossy(raw);
            let e = Error::new(ErrorKind::InvalidData, format!("the attribute name {lossy:?} is not UTF-8"));
            return Err(at("llistxattr", e));
        };
        // A name of the list holds no NUL: the split is at each NUL.
        let cn = CBuf::from_bytes(raw).expect("a name of the list holds no NUL");
        match sys::lgetxattr(&c, &cn, &mut value) {
            Ok(n) => out.push((name.to_string(), value[..n as usize].to_vec())),
            // The attribute went between the list and the read.
            Err(e) if e == sys::ENODATA => continue,
            Err(e) => return Err(at(&format!("lgetxattr {name}"), io(e))),
        }
    }
    Ok(out)
}

/// The NUL-ended names of `c`: the size from `llistxattr(path, NULL, 0)`,
/// a read of that size, and a new try on `ERANGE`.
fn names(c: &CBuf) -> std::io::Result<Vec<u8>> {
    for _ in 0..LIST_TRIES {
        let need = sys::llistxattr(c, &mut []).map_err(io)? as usize;
        if need == 0 {
            return Ok(Vec::new());
        }
        if need > NAMES_MAX {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("the attribute names are {need} bytes, past the limit of the kernel ({NAMES_MAX})"),
            ));
        }
        let mut list = vec![0u8; need];
        match sys::llistxattr(c, &mut list) {
            Ok(n) => {
                list.truncate(n as usize);
                return Ok(list);
            }
            Err(e) if e == sys::ERANGE => continue,
            Err(e) => return Err(io(e)),
        }
    }
    Err(Error::other(format!("the attribute names changed in each of {LIST_TRIES} reads")))
}

fn io(e: Errno) -> Error {
    Error::from_raw_os_error(e.0)
}

#[cfg(test)]
mod tests;
