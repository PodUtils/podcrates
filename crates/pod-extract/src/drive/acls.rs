//! The POSIX ACLs of a layer of GNU tar `--acls` (PodUtils/podvm#643): the
//! layer extracts, each ACL in its text form is set in its binary form or
//! named, and the mode of each entry stays.
//!
//! The kernel decides, as for each attribute: a probe of the same ACL on an
//! object of the same kind gives the expected answer.

use super::xattrs::{Kind, can, value_of};
use super::*;
use crate::acl::{ACCESS, DEFAULT};

/// The text of GNU tar `--acls --numeric-owner`, and the value that Linux
/// gave for the same ACL.
const FILE_TEXT: &[u8] =
    b"user::rw-\nuser:1000:rw-\nuser:12345:r--\ngroup::r--\ngroup:0:r-x\ngroup:4:--x\nmask::rw-\nother::r--";
const FILE_VALUE: &str = "0200000001000600ffffffff02000600e8030000020004003930000004000400ffffffff\
                          0800050000000000080001000400000010000600ffffffff20000400ffffffff";
const DIR_TEXT: &[u8] = b"user::rwx\ngroup::r-x\ngroup:1000:rwx\nmask::rwx\nother::r-x\n";
const DIR_VALUE: &str = "0200000001000700ffffffff04000500ffffffff08000700e803000010000700ffffffff20000500ffffffff";
/// The text of GNU tar with no `--numeric-owner`: a name and no id.
const NAMED_TEXT: &[u8] = b"user::rw-\nuser:ubuntu:rw-\ngroup::r--\nmask::rw-\nother::---\n";

fn bytes(hex: &str) -> Vec<u8> {
    (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect()
}

fn acl_layer() -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    let entries: [(&str, Kind, u32, &str, &[u8]); 3] = [
        ("d", Kind::Dir, 0o775, "SCHILY.acl.default", DIR_TEXT),
        ("d/f", Kind::File, 0o664, "SCHILY.acl.access", FILE_TEXT),
        ("named", Kind::File, 0o660, "SCHILY.acl.access", NAMED_TEXT),
    ];
    for (path, kind, mode, key, text) in entries {
        b.append_pax_extensions([("mtime", &b"1795232499.649341102"[..]), (key, text)]).unwrap();
        let mut h = tar::Header::new_ustar();
        h.set_mode(mode);
        if kind == Kind::Dir {
            h.set_entry_type(tar::EntryType::Directory);
            h.set_size(0);
            b.append_data(&mut h, path, std::io::empty()).unwrap();
        } else {
            h.set_entry_type(tar::EntryType::Regular);
            h.set_size(4);
            b.append_data(&mut h, path, Cursor::new(b"data")).unwrap();
        }
    }
    b.into_inner().unwrap()
}

#[test]
fn the_acls_of_gnu_tar_are_set_or_named_and_the_modes_stay() {
    let s = Scratch::new("acl");
    let probes = Scratch::new("acl-probe");
    let root = Dir::open(&s.path().to_string_lossy()).unwrap();
    let mut sc = Sidecar::create(&s.path().join(".meta.jsonl")).unwrap();
    let st = apply_layer(&root, "sha256:acl", &mut Cursor::new(acl_layer()), Ids::current(), &mut sc).unwrap();
    sc.finish().unwrap();
    let rows = meta(s.path());
    let row = |path: &str| rows.lines().rfind(|l| l.contains(&format!(r#""path":"{path}""#))).unwrap_or("").to_string();
    // A name with no id is named, with no value.
    let named = st.xattrs.first_unset.iter().find(|u| u.path == "named" && u.name == ACCESS);
    let named = named.unwrap_or_else(|| panic!("the ACL with a name is not named: {:?}", st.xattrs));
    assert!(named.reason.contains("\"ubuntu\" with no id") && named.value.is_none(), "{named:?}");
    assert!(row("named").contains(&format!(r#"{{"name":"{ACCESS}","reason":"#)), "{}", row("named"));
    for (path, kind, name, value) in [("d", Kind::Dir, DEFAULT, DIR_VALUE), ("d/f", Kind::File, ACCESS, FILE_VALUE)] {
        let value = bytes(value);
        let unset = st.xattrs.first_unset.iter().find(|u| u.path == path);
        if can(probes.path(), kind, name, &value) {
            assert!(unset.is_none(), "{path}: named, but the probe set the ACL: {unset:?}");
            assert_eq!(value_of(&s.path().join(path), name), Some(value), "{path}: {name} is not on disk");
        } else {
            let u = unset.unwrap_or_else(|| panic!("{path}: {name} is neither set nor named"));
            assert_eq!((u.name.as_str(), u.value.as_deref()), (name, Some(&value[..])), "{path}");
            pod_testkit::host::skip_line(&format!(
                "this file system refuses {name}: its report is proven, its set is not"
            ));
        }
    }
    assert_eq!(st.xattrs.set + st.xattrs.unset, 3, "{:?}", st.xattrs);
    use std::os::unix::fs::PermissionsExt;
    for (path, mode) in [("d/f", 0o664), ("named", 0o660)] {
        let m = std::fs::metadata(s.path().join(path)).unwrap().permissions().mode() & 0o7777;
        assert_eq!(m, mode, "{path}: the mode of the image");
    }
}
