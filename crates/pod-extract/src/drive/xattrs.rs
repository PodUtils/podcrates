//! The extended attributes of a layer (PodUtils/podrunner#317): each
//! `SCHILY.xattr.NAME` record is set on its entry, or named in the stats of
//! the layer, its line and the sidecar row of its path, with its value.
//!
//! The kernel decides what this process can set, so a probe of the same
//! name on an object of the same kind, in the same file system, gives the
//! expected answer: set when the probe set it, named when the probe could
//! not. As root, `security.capability` must set; the test binary then runs
//! again as uid 1000, where it must be named.

use super::*;
use pod_hostprobe::sys::{self, CBuf};

/// The variable that makes the worker below run.
const WORKER: &str = "POD_EXTRACT_TEST_XATTR_WORKER";

/// `cap_net_raw+ep`, the capability of `ping`: `vfs_cap_data` revision 2.
const CAP_NET_RAW: &[u8] = &[1, 0, 0, 2, 0, 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Kind {
    File,
    Dir,
    Symlink,
    Hardlink,
}

/// The path, the kind, the mode, the attribute and its value. A mode with
/// no owner write: a `user.` attribute set after the mode is refused.
const CASES: &[(&str, Kind, u32, &str, &[u8])] = &[
    ("bin/ping", Kind::File, 0o755, "security.capability", CAP_NET_RAW),
    ("etc/marked", Kind::File, 0o444, "user.test", b"yes"),
    ("etc/closed", Kind::Dir, 0o555, "user.test", b"dir"),
    ("etc/link", Kind::Symlink, 0o777, "user.test", b"link"),
    // The attributes of its target again, on a target of mode 0444.
    ("etc/again", Kind::Hardlink, 0o444, "user.test", b"yes"),
];

fn xattr_layer() -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    for (path, kind, mode, name, value) in CASES {
        let key = format!("{}{name}", crate::xattr::PAX_PREFIX);
        b.append_pax_extensions([(key.as_str(), *value)]).unwrap();
        let mut h = tar::Header::new_ustar();
        h.set_mode(*mode);
        h.set_size(0);
        match kind {
            Kind::File => {
                h.set_entry_type(tar::EntryType::Regular);
                h.set_size(4);
                b.append_data(&mut h, path, Cursor::new(b"data")).unwrap();
            }
            Kind::Dir => {
                h.set_entry_type(tar::EntryType::Directory);
                b.append_data(&mut h, path, std::io::empty()).unwrap();
            }
            Kind::Symlink => {
                h.set_entry_type(tar::EntryType::Symlink);
                b.append_link(&mut h, path, "marked").unwrap();
            }
            Kind::Hardlink => {
                h.set_entry_type(tar::EntryType::Link);
                b.append_link(&mut h, path, "etc/marked").unwrap();
            }
        }
    }
    b.into_inner().unwrap()
}

/// Can this process set `name` on an object of `kind` in `dir`?
pub(super) fn can(dir: &std::path::Path, kind: Kind, name: &str, value: &[u8]) -> bool {
    let p = dir.join(format!("probe-{}", name.replace('.', "-")));
    let made = match kind {
        Kind::File | Kind::Hardlink => std::fs::write(&p, b"probe"),
        Kind::Dir => std::fs::create_dir(&p),
        Kind::Symlink => std::os::unix::fs::symlink("probe-target", &p),
    };
    made.unwrap();
    let (path, n) = (CBuf::new(p.to_str().unwrap()).unwrap(), CBuf::new(name).unwrap());
    let ok = sys::lsetxattr(&path, &n, value, 0).is_ok();
    let _ = std::fs::remove_file(&p).or_else(|_| std::fs::remove_dir(&p));
    ok
}

/// The value of `name` on the file or directory `p`, read back.
pub(super) fn value_of(p: &std::path::Path, name: &str) -> Option<Vec<u8>> {
    let fd = sys::open(&CBuf::new(p.to_str()?)?, sys::O_RDONLY | sys::O_CLOEXEC | sys::O_NOFOLLOW, 0).ok()?;
    let mut buf = [0u8; 256];
    let r = sys::fgetxattr(fd, &CBuf::new(name)?, &mut buf);
    let _ = sys::close(fd);
    r.ok().map(|n| buf[..n as usize].to_vec())
}

/// Extract the layer in a scratch directory, and check each case against
/// its probe. Returns the names that this process could set.
fn check(who: &str) -> Vec<&'static str> {
    let s = Scratch::new("xattr");
    let probes = Scratch::new("xattr-probe");
    let root = Dir::open(&s.path().to_string_lossy()).unwrap();
    let mut sc = Sidecar::create(&s.path().join(".meta.jsonl")).unwrap();
    let st = apply_layer(&root, "sha256:xattr", &mut Cursor::new(xattr_layer()), Ids::current(), &mut sc).unwrap();
    sc.finish().unwrap();
    let rows = meta(s.path());
    let mut set = Vec::new();
    for (path, kind, _, name, value) in CASES {
        let named = st.xattrs.first_unset.iter().find(|u| u.path == *path && u.name == *name);
        let row = rows.lines().rfind(|l| l.contains(&format!(r#""path":"{path}""#))).unwrap_or("");
        let hex: String = value.iter().map(|b| format!("{b:02x}")).collect();
        let in_row = row.contains(&format!(r#"{{"name":"{name}","value":"0x{hex}","reason":"#));
        if can(probes.path(), *kind, name, value) {
            set.push(*name);
            assert!(named.is_none() && !in_row, "{path}: {name} is named as not set as {who}, but the probe set it");
            if *kind != Kind::Symlink {
                let got = value_of(&s.path().join(path), name);
                assert_eq!(got.as_deref(), Some(*value), "{path}: {name} is not on disk as {who}");
            }
        } else {
            let u = named.unwrap_or_else(|| panic!("{path}: {name} is neither set nor named as {who}"));
            assert!(!u.reason.is_empty() && u.value.as_deref() == Some(*value), "{path}: {u:?}");
            assert!(in_row, "{path}: the sidecar row does not name {name} with its value as {who}:\n{row}");
            assert!(st.xattrs.line().contains(" not set (the first: "), "{}", st.xattrs.line());
            if name.starts_with("user.") && *kind != Kind::Symlink {
                pod_testkit::host::skip_line(&format!(
                    "this file system refuses {name}: its report is proven, its set is not"
                ));
            }
        }
    }
    assert_eq!(st.xattrs.set + st.xattrs.unset, CASES.len() as u64, "as {who}: {:?}", st.xattrs);
    // The mode of the image after the attributes.
    let mode = std::fs::metadata(s.path().join("etc/marked")).unwrap();
    assert_eq!(std::os::unix::fs::PermissionsExt::mode(&mode.permissions()) & 0o7777, 0o444);
    set
}

/// Each attribute is set or named, as the user of the test. As root,
/// `security.capability` is set, and the worker as uid 1000 names it.
#[test]
fn extended_attributes_are_set_or_named() {
    let set = check("the user of the test");
    if Ids::current().uid != 0 {
        pod_testkit::host::skip_line("the test does not run as root, so it cannot start a child as uid 1000");
        return;
    }
    assert!(set.contains(&"security.capability"), "root did not set security.capability: {set:?}");
    let s = Scratch::new("xattr-child");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(s.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    user::as_uid_1000(&s, "drive::xattrs::worker_extracts_as_a_user", WORKER, std::ffi::OsStr::new("1"));
}

/// The half that runs as uid 1000. It does nothing with no [`WORKER`].
#[test]
fn worker_extracts_as_a_user() {
    if std::env::var_os(WORKER).is_none() {
        return;
    }
    assert_ne!(Ids::current().uid, 0, "the worker must not run as root: it then proves nothing");
    let set = check("uid 1000");
    assert!(!set.contains(&"security.capability"), "uid 1000 set security.capability: {set:?}");
}

/// A record past a bound is named and not kept.
#[test]
fn a_record_past_a_bound_is_named_and_not_kept() {
    use crate::xattr::{NAME_MAX, PAX_PREFIX, VALUE_MAX};
    let long_name = format!("{PAX_PREFIX}user.{}", "n".repeat(NAME_MAX));
    let big = vec![b'v'; VALUE_MAX + 1];
    // The records of the entry, the names kept, and the reason of the one refused.
    type Case<'a> = (&'a [(&'a str, &'a [u8])], &'a [&'a str], &'a str);
    let cases: &[Case] = &[
        (&[(&long_name, b"x")], &[], "longer than 255 bytes"),
        (&[(&format!("{PAX_PREFIX}user.big"), &big)], &[], "longer than 65536 bytes"),
        (&[(PAX_PREFIX, b"x"), (&format!("{PAX_PREFIX}user.ok"), b"y")], &["user.ok"], "an empty name"),
        (&[(&format!("{PAX_PREFIX}user.overlay.opaque"), b"y")], &[], "a mark of overlayfs"),
        (&[(&format!("{PAX_PREFIX}trusted.overlay.redirect"), b"/x")], &[], "a mark of overlayfs"),
    ];
    for (records, kept, why) in cases {
        let mut b = tar::Builder::new(Vec::new());
        b.append_pax_extensions(records.iter().copied()).unwrap();
        let mut h = tar::Header::new_ustar();
        h.set_entry_type(tar::EntryType::Regular);
        h.set_mode(0o644);
        h.set_size(1);
        b.append_data(&mut h, "f", Cursor::new(b"1")).unwrap();
        let data = b.into_inner().unwrap();
        let side = crate::pax::Side::default();
        let mut ar = tar::Archive::new(crate::pax::Framed::new(Cursor::new(data), side.clone()));
        ar.entries().unwrap().next().unwrap().unwrap();
        let w = crate::xattr::read(&side.next().xattrs, "f");
        let names: Vec<&str> = w.attrs.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, *kept, "{why}");
        assert_eq!(w.refused.len(), 1, "{why}: {:?}", w.refused);
        assert!(w.refused[0].reason.contains(why) && w.refused[0].value.is_none(), "{:?}", w.refused[0]);
        assert!(w.refused[0].name.len() <= NAME_MAX + 3, "the name in a message is cut");
    }
}
