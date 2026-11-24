//! The records of the stream of `flatten`: the extended attributes and the
//! ACLs that `extract` sets go into the stream as `SCHILY.xattr.` records,
//! in the form that `extract` sets them, and the reader of the stream sets
//! them too. GNU tar with the arguments of incusd (`--xattrs-include=*`, no
//! `--acls`) sets each `SCHILY.xattr.` record, the binary form of an ACL
//! included, and does not read the text form `SCHILY.acl.`.
#![cfg(target_os = "linux")]

mod support;

use support::{E, Scratch, extracted_at, flattened, plain, same, tar, xattrs};

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

/// The value of the attribute `name` of the file `p`, read back.
fn value_of(p: &std::path::Path, name: &str) -> Option<Vec<u8>> {
    use pod_hostprobe::sys::{self, CBuf};
    let fd = sys::open(&CBuf::new(p.to_str()?)?, sys::O_RDONLY | sys::O_CLOEXEC | sys::O_NOFOLLOW, 0).ok()?;
    let mut buf = [0u8; 256];
    let r = sys::fgetxattr(fd, &CBuf::new(name)?, &mut buf);
    let _ = sys::close(fd);
    r.ok().map(|n| buf[..n as usize].to_vec())
}

/// A path with its attributes `user.f` and `user.l`.
type Attrs = (String, Option<Vec<u8>>, Option<Vec<u8>>);

/// The attributes `user.f` and `user.l` of each path under `root`.
fn user_attrs(root: &std::path::Path, paths: &[&str]) -> Vec<Attrs> {
    let each = |p: &&str| (p.to_string(), value_of(&root.join(p), "user.f"), value_of(&root.join(p), "user.l"));
    paths.iter().map(each).collect()
}

/// The records of a hard-link entry go with the data of its inode, where
/// the link comes before the data in the stream: `extract` sets them on
/// the inode, and GNU tar with the arguments of incusd does not set a
/// record of a link entry. A record of the link holds over the record of
/// the same name of the file, and a newer link holds over an older one.
/// Also for a link whose name a newer layer hides.
#[test]
fn the_records_of_a_link_go_with_the_data_of_its_inode() {
    let s = Scratch::new("linkentryrecs");
    let l0 = tar(&[E::Pax("a", b"data", 0o644, &[("SCHILY.xattr.user.f", b"file")])]);
    let l1 = tar(&[E::PaxLink("b", "a", &[("SCHILY.xattr.user.l", b"link"), ("SCHILY.xattr.user.f", b"over")])]);
    let l2 = tar(&[E::PaxLink("c", "a", &[("SCHILY.xattr.user.l", b"newer")])]);
    let gone = tar(&[E::File(".wh.b", b"", 0)]);
    let want = vec![rec("user.f", b"over"), rec("user.l", b"newer")];
    for layers in [vec![l0.clone(), l1.clone(), l2.clone()], vec![l0, l1, l2, gone]] {
        let layers = plain(s.path(), &layers);
        let names: &[&str] = if layers.len() == 3 { &["a", "b", "c"] } else { &["a", "c"] };
        let stream = same(&layers);
        assert_eq!(xattrs(&stream, "a"), want, "the records of the data entry");
        let (_, f) = flattened(&layers).unwrap();
        assert_eq!((f.xattrs.set, f.xattrs.unset), (2, 0));
        let t = Scratch::new("linkentryrecs-tree");
        let from_layers = user_attrs(&extracted_at(t.path(), &layers), names);
        let from_stream = user_attrs(&extracted_at(t.path(), &plain(t.path(), std::slice::from_ref(&stream))), names);
        assert_eq!(from_stream, from_layers);
        assert_eq!(from_layers[0], ("a".into(), Some(b"over".to_vec()), Some(b"newer".to_vec())));
        if pod_testkit::host::tool("tar").is_ok() {
            let out = t.path().join("gnu");
            assert!(support::gnu_tar(&stream, &out).unwrap().success(), "GNU tar refused the stream");
            assert_eq!(user_attrs(&out, names), from_layers, "GNU tar");
        }
    }
}

/// A link after its file in one layer: the data went out before the link
/// was read, so a record of the link that the file does not have is named
/// and not written. A record with the value of the file is no loss.
#[test]
fn a_record_of_a_link_after_the_data_of_its_inode_is_named() {
    let s = Scratch::new("linkafter");
    let l0 = tar(&[
        E::Pax("a", b"data", 0o755, &[("SCHILY.xattr.security.capability", CAP)]),
        E::PaxLink("b", "a", &[("SCHILY.xattr.security.capability", CAP), ("SCHILY.xattr.user.l", b"link")]),
    ]);
    let (_, f) = flattened(&plain(s.path(), &[l0])).unwrap();
    assert_eq!((f.xattrs.set, f.xattrs.unset), (1, 1));
    let u = &f.xattrs.first_unset[0];
    assert_eq!((u.path.as_str(), u.name.as_str(), u.value.as_deref()), ("b", "user.l", Some(&b"link"[..])));
}

/// A hidden file that a link of its layer names after it: the layer is
/// read again, and the data goes out under the name of the link with the
/// records of the file and of the link.
#[test]
fn a_file_read_again_takes_the_records_of_its_link() {
    let s = Scratch::new("rereadrecs");
    let l0 = tar(&[
        E::Pax("a", b"old", 0o644, &[("SCHILY.xattr.user.f", b"file")]),
        E::PaxLink("b", "a", &[("SCHILY.xattr.user.l", b"link")]),
    ]);
    let l1 = tar(&[E::File("a", b"new", 0o644)]);
    let layers = plain(s.path(), &[l0, l1]);
    let stream = same(&layers);
    assert_eq!(xattrs(&stream, "b"), vec![rec("user.f", b"file"), rec("user.l", b"link")]);
    let (_, f) = flattened(&layers).unwrap();
    assert_eq!((f.rereads, f.xattrs.set, f.xattrs.unset), (1, 2, 0));
    let t = Scratch::new("rereadrecs-tree");
    let from_layers = user_attrs(&extracted_at(t.path(), &layers), &["a", "b"]);
    let from_stream = user_attrs(&extracted_at(t.path(), &plain(t.path(), std::slice::from_ref(&stream))), &["a", "b"]);
    assert_eq!(from_stream, from_layers);
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
