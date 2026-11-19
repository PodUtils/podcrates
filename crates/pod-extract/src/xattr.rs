//! The extended attributes of an entry: the PAX records `SCHILY.xattr.NAME`
//! that GNU tar, Go's `archive/tar`, containerd and BuildKit write.
//!
//! **Each attribute is set or named.** `security.capability` of `ping` is
//! the measured case (PodUtils/podrunner#317): a layer that loses it gives a
//! `ping` that fails, with no word. The crate tries each attribute. The
//! kernel decides: a process with no `CAP_SETFCAP` cannot set
//! `security.capability`, one with no `CAP_SYS_ADMIN` cannot set `trusted.`,
//! and a file system can refuse `user.`. Each attribute that is not set goes
//! into the stats of its layer, the event of its layer and the sidecar row
//! of its path, with its value, so that a caller can set it later.
//!
//! **The order.** A file gets its attributes through its descriptor after
//! its last write and before its mode: a write by a process clears
//! `security.capability` and `S_ISUID`, and a `user.` attribute needs the
//! owner write bit, which a mode of the image (0555) can remove. The crate
//! makes no `chown`, which clears `security.capability` too.
//!
//! **Bounds.** The limits of Linux: a name of 255 bytes at most
//! (`XATTR_NAME_MAX`), a value of 64 KiB at most (`XATTR_SIZE_MAX`), and the
//! names of one file 64 KiB at most with their NULs (`XATTR_LIST_MAX`). The
//! crate adds a bound on the values of one entry ([`VALUES_MAX`]) and on the
//! records of one entry ([`RECORDS_MAX`]). A record past a bound is named
//! and not kept. [`crate::pax`] bounds the PAX header that holds them.

use pod_hostprobe::sys::{self, CBuf, Sysres};

use crate::safety::Dir;

/// The prefix of the PAX key of an extended attribute.
pub const PAX_PREFIX: &str = "SCHILY.xattr.";
/// The longest name, the namespace included (`XATTR_NAME_MAX`).
pub const NAME_MAX: usize = 255;
/// The longest value (`XATTR_SIZE_MAX`).
pub const VALUE_MAX: usize = 64 * 1024;
/// The names of one entry, each with its NUL (`XATTR_LIST_MAX`).
pub const NAMES_MAX: usize = 64 * 1024;
/// The values of one entry. A file system keeps the attributes of a file
/// in one block or a few: a sum past this is not an image of a real tree.
pub const VALUES_MAX: usize = 1024 * 1024;
/// The attribute records of one entry that the crate reads.
pub const RECORDS_MAX: usize = 1024;
/// The attributes not set that [`Count`] names; the sidecar names each.
pub const FIRST_NAMED: usize = 8;
/// The marks of overlayfs (opaque, redirect, metacopy, whiteout). An OCI
/// layer gives a whiteout as a file; a mark in a layer comes from the
/// snapshotter of its writer, and an overlay mount of the extracted tree
/// (`pod-enter`, `userxattr` for `user.overlay.`) reads it as its own.
pub const OVERLAY: [&str; 2] = ["trusted.overlay.", "user.overlay."];

/// One attribute of an entry, inside the bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Xattr {
    pub name: String,
    pub value: Vec<u8>,
}

/// One attribute that the extraction did not set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unset {
    /// The path of the entry in the tree, as the sidecar row names it.
    pub path: String,
    pub name: String,
    /// `None` for a record past a bound, or one that does not parse: the
    /// crate does not keep it.
    pub value: Option<Vec<u8>>,
    /// The errno name of the refusal (`EPERM`, `EOPNOTSUPP`), or the bound.
    pub reason: String,
}

impl Unset {
    /// The JSON of the sidecar, with no path: the row has it. The value is
    /// hex with `0x`, the form that `getfattr -e hex` gives and that
    /// `setfattr -v` takes.
    pub fn to_json(&self) -> String {
        let mut s = format!(r#"{{"name":{}"#, crate::sidecar::json_string(&self.name));
        if let Some(v) = &self.value {
            s.push_str(r#","value":"0x"#);
            for b in v {
                s.push_str(&format!("{b:02x}"));
            }
            s.push('"');
        }
        s.push_str(&format!(r#","reason":{}}}"#, crate::sidecar::json_string(&self.reason)));
        s
    }
}

/// The attributes of one entry, read before its content.
#[derive(Debug, Default, Clone)]
pub struct Wanted {
    /// The path of the entry in the tree.
    pub path: String,
    pub attrs: Vec<Xattr>,
    /// The records that a bound or the parse refused.
    pub refused: Vec<Unset>,
}

/// What the set of the attributes of one entry gave.
#[derive(Debug, Default, Clone)]
pub struct Applied {
    pub set: u64,
    pub unset: Vec<Unset>,
}

/// The attributes of a layer or of an extraction.
#[derive(Debug, Default, Clone)]
pub struct Count {
    pub set: u64,
    pub unset: u64,
    /// The first [`FIRST_NAMED`] attributes not set.
    pub first_unset: Vec<Unset>,
}

impl Count {
    pub fn add(&mut self, a: &Applied) {
        self.set += a.set;
        self.unset += a.unset.len() as u64;
        self.name_first(&a.unset);
    }

    pub fn merge(&mut self, c: &Count) {
        self.set += c.set;
        self.unset += c.unset;
        self.name_first(&c.first_unset);
    }

    fn name_first(&mut self, unset: &[Unset]) {
        let room = FIRST_NAMED.saturating_sub(self.first_unset.len());
        self.first_unset.extend(unset.iter().take(room).cloned());
    }

    /// The end of the line of a layer: empty for a layer with no attribute.
    pub fn line(&self) -> String {
        match self.first_unset.first() {
            None if self.set == 0 => String::new(),
            None => format!("; {} extended attributes set", self.set),
            Some(u) => format!(
                "; {} extended attributes set, {} not set (the first: {} of {}, {}; the sidecar names each)",
                self.set, self.unset, u.name, u.path, u.reason
            ),
        }
    }
}

/// Read the attribute records of an entry: the `SCHILY.xattr.` records of
/// its PAX header, read by their length ([`crate::pax`]), so a value can
/// hold any byte. `path` is the path of the entry in the tree.
pub fn read(exts: &[(Vec<u8>, Vec<u8>)], path: &str) -> Wanted {
    let mut w = Wanted { path: path.to_string(), ..Wanted::default() };
    let refuse = |w: &mut Wanted, name: String, reason: String| {
        w.refused.push(Unset { path: w.path.clone(), name, value: None, reason })
    };
    let (mut names, mut values, mut records) = (0usize, 0usize, 0usize);
    for (key, value) in exts {
        let Some(raw) = key.strip_prefix(PAX_PREFIX.as_bytes()) else {
            continue;
        };
        records += 1;
        if records > RECORDS_MAX {
            let why = format!("more than {RECORDS_MAX} attribute records in one entry; the rest are not read");
            refuse(&mut w, "(more records)".into(), why);
            break;
        }
        let name = std::str::from_utf8(raw);
        let why = if raw.is_empty() {
            Some("an empty name".to_string())
        } else if raw.len() > NAME_MAX {
            Some(format!("the name is longer than {NAME_MAX} bytes"))
        } else if raw.contains(&0) {
            Some("a NUL in the name".to_string())
        } else if name.is_err() {
            Some("the name is not UTF-8".to_string())
        } else if value.len() > VALUE_MAX {
            Some(format!("the value is longer than {VALUE_MAX} bytes"))
        } else if OVERLAY.iter().any(|p| raw.starts_with(p.as_bytes())) {
            Some("a mark of overlayfs: an overlay mount of the tree reads it, so it is not set".to_string())
        } else if names + raw.len() + 1 > NAMES_MAX || values + value.len() > VALUES_MAX {
            Some(format!("past the bound of one entry: {NAMES_MAX} bytes of names, {VALUES_MAX} bytes of values"))
        } else {
            None
        };
        match (why, name) {
            (None, Ok(n)) => {
                names += raw.len() + 1;
                values += value.len();
                w.attrs.push(Xattr { name: n.to_string(), value: value.to_vec() });
            }
            (why, _) => refuse(&mut w, shown(raw), why.unwrap_or_default()),
        }
    }
    w
}

/// A name for a message: lossy, and cut at [`NAME_MAX`] bytes.
fn shown(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(&raw[..raw.len().min(NAME_MAX)]).into_owned();
    if raw.len() > NAME_MAX { format!("{s}...") } else { s }
}

/// Set each attribute through `fd`, the descriptor of the entry.
///
/// A refused set of a value that the file has already is a set: a hard link
/// carries the attributes of its target again, and a target of mode 0444
/// refuses a second `user.` set to its owner.
pub fn set_fd(fd: i64, w: &Wanted) -> Applied {
    set_each(w, |n, v| {
        sys::fsetxattr(fd, n, v, 0).or_else(|e| {
            let mut buf = vec![0u8; v.len() + 1];
            match sys::fgetxattr(fd, n, &mut buf) {
                Ok(got) if buf.get(..got as usize) == Some(v) => Ok(0),
                _ => Err(e),
            }
        })
    })
}

/// Set each attribute of the entry `name` of `parent`: a directory, a
/// symlink or a hard link.
///
/// Through a descriptor first, opened with `O_NOFOLLOW`. A symlink has no
/// descriptor that takes an attribute (`ELOOP`), and a file with no read
/// bit has no descriptor for a user: the second path is `lsetxattr` of the
/// name below the held parent (`/proc/self/fd/N/NAME`), which does not
/// follow the name. With no `/proc`, each attribute is named as not set.
pub fn set_at(parent: &Dir, name: &str, w: &Wanted) -> Applied {
    if w.attrs.is_empty() {
        return Applied { set: 0, unset: w.refused.clone() };
    }
    let flags = sys::O_RDONLY | sys::O_NOFOLLOW | sys::O_CLOEXEC | sys::O_NONBLOCK;
    if let Some(Ok(fd)) = CBuf::new(name).map(|n| sys::openat(parent.fd(), &n, flags, 0)) {
        let a = set_fd(fd, w);
        let _ = sys::close(fd);
        return a;
    }
    match CBuf::new(&format!("/proc/self/fd/{}/{name}", parent.fd())) {
        Some(p) => set_each(w, |n, v| sys::lsetxattr(&p, n, v, 0)),
        None => set_each(w, |_, _| Err(sys::EINVAL)),
    }
}

fn set_each(w: &Wanted, mut set: impl FnMut(&CBuf, &[u8]) -> Sysres) -> Applied {
    let mut a = Applied { set: 0, unset: w.refused.clone() };
    for x in &w.attrs {
        // `read` refused a name with a NUL: each name here converts.
        let r = CBuf::new(&x.name).map_or(Err(sys::EINVAL), |n| set(&n, &x.value));
        match r {
            Ok(_) => a.set += 1,
            Err(e) => a.unset.push(Unset {
                path: w.path.clone(),
                name: x.name.clone(),
                value: Some(x.value.clone()),
                reason: e.name(),
            }),
        }
    }
    a
}
