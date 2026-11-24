//! The records of the stream of `flatten`: the extended attributes and the
//! ACLs that `extract` sets go into the stream as `SCHILY.xattr.` records,
//! in the form that `extract` sets them, and the reader of the stream sets
//! them too. GNU tar with the arguments of incusd (`--xattrs-include=*`, no
//! `--acls`) sets each `SCHILY.xattr.` record, the binary form of an ACL
//! included, and does not read the text form `SCHILY.acl.`.
#![cfg(target_os = "linux")]

mod support;

use support::{E, Scratch, flattened, plain, same, tar, xattrs};

/// `security.capability` of version 2: `cap_net_raw` permitted.
const CAP: &[u8] = &[1, 0, 0, 2, 0, 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

fn rec(name: &str, value: &[u8]) -> (String, Vec<u8>) {
    (name.to_string(), value.to_vec())
}

#[test]
fn a_capability_and_a_user_attribute_go_into_the_stream() {
    let s = Scratch::new("caps");
    let recs: &[(&str, &[u8])] = &[("SCHILY.xattr.security.capability", CAP), ("SCHILY.xattr.user.k", b"v")];
    let l0 = tar(&[E::Dir("bin", 0o755), E::Pax("bin/ping", b"elf", 0o4755, recs)]);
    let stream = same(&plain(s.path(), &[l0]));
    assert_eq!(xattrs(&stream, "bin/ping"), vec![rec("security.capability", CAP), rec("user.k", b"v")]);
}

#[test]
fn an_acl_in_each_form_goes_into_the_stream_in_its_binary_form() {
    let s = Scratch::new("acls");
    let text = "user::rwx,user:1000:r-x,group::r-x,mask::r-x,other::r--";
    let binary = pod_extract::acl::to_xattr(text.as_bytes()).unwrap();
    let l0 = tar(&[
        E::Pax("text", b"t", 0o754, &[("SCHILY.acl.access", text.as_bytes())]),
        E::Pax("binary", b"b", 0o754, &[("SCHILY.xattr.system.posix_acl_access", &binary)]),
    ]);
    let stream = same(&plain(s.path(), &[l0]));
    for path in ["text", "binary"] {
        assert_eq!(xattrs(&stream, path), vec![rec("system.posix_acl_access", &binary)], "{path}");
    }
}

#[test]
fn a_record_of_libarchive_alone_is_decoded() {
    let s = Scratch::new("libarchive");
    // `val` in base64; `%2E` is a `.` with a percent escape.
    let l0 = tar(&[E::Pax("f", b"f", 0o644, &[("LIBARCHIVE.xattr.user%2Elib", b"dmFs")])]);
    let stream = same(&plain(s.path(), &[l0]));
    assert_eq!(xattrs(&stream, "f"), vec![rec("user.lib", b"val")]);
}

#[test]
fn an_attribute_past_a_bound_and_a_mark_of_overlayfs_are_not_written() {
    let s = Scratch::new("bounds");
    let big = vec![b'x'; 64 * 1024 + 1];
    let recs: &[(&str, &[u8])] = &[
        ("SCHILY.xattr.user.big", &big),
        ("SCHILY.xattr.trusted.overlay.opaque", b"y"),
        ("SCHILY.xattr.user.ok", b"1"),
    ];
    let l0 = tar(&[E::Pax("f", b"f", 0o644, recs)]);
    let stream = same(&plain(s.path(), &[l0]));
    assert_eq!(xattrs(&stream, "f"), vec![rec("user.ok", b"1")]);
}

/// A file that a newer layer hides goes out under the name of the hard
/// link that names it, with its own records.
#[test]
fn the_name_of_a_link_gets_the_records_of_its_file() {
    let s = Scratch::new("linkrecs");
    let l0 = tar(&[E::Pax("a", b"old", 0o755, &[("SCHILY.xattr.security.capability", CAP)])]);
    let l1 = tar(&[E::Hardlink("b", "a")]);
    let l2 = tar(&[E::File("a", b"new", 0o644)]);
    let stream = same(&plain(s.path(), &[l0, l1, l2]));
    assert_eq!(xattrs(&stream, "b"), vec![rec("security.capability", CAP)]);
    assert_eq!(xattrs(&stream, "a"), vec![]);
}

#[test]
fn a_name_past_100_bytes_and_a_name_that_is_not_ascii_go_through() {
    let s = Scratch::new("names");
    let long = format!("d\u{e9}/{}", "\u{fc}".repeat(60));
    let l0 = tar(&[E::Dir("d\u{e9}", 0o755), E::File(&long, b"x", 0o644), E::Symlink("l\u{e9}", &long)]);
    let stream = same(&plain(s.path(), &[l0]));
    assert!(support::paths(&stream).contains(&long));
}

/// The stream depends on the layers alone: two runs give the same bytes,
/// and a directory with no entry of its own has the time 0.
#[test]
fn the_same_layers_give_the_same_bytes() {
    let s = Scratch::new("same");
    let l0 = tar(&[E::File("usr/bin/a", b"a", 0o755), E::Pax("f", b"f", 0o644, &[("SCHILY.xattr.user.k", b"v")])]);
    let layers = plain(s.path(), &[l0]);
    let (one, _) = flattened(&layers).unwrap();
    let (two, _) = flattened(&layers).unwrap();
    assert_eq!(one, two);
    let mut ar = ::tar::Archive::new(std::io::Cursor::new(&one));
    for e in ar.entries().unwrap() {
        let e = e.unwrap();
        if e.header().entry_type().is_dir() {
            assert_eq!(e.header().mtime().unwrap(), 0, "{:?}", e.path().unwrap());
        }
    }
}
