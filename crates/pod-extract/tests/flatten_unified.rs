//! The unified tarball of incus from `flatten_with`: the entries of the
//! caller before the tree (`metadata.yaml`), the directory of the prefix
//! (`rootfs`), the tree of the image below the prefix, the entries of the
//! caller after the tree, and the end blocks once.
#![cfg(target_os = "linux")]

mod support;

use pod_extract::flatten::{Flattened, Options, flatten_with};
use support::{E, Scratch, entries, extracted, paths, plain, tar};

const META: &[u8] = b"architecture: x86_64\ncreation_date: 0\n";

fn unified(layers: &[pod_extract::Layer], opts: &Options) -> pod_extract::Result<(Vec<u8>, Flattened)> {
    let mut out = Vec::new();
    let f = flatten_with(
        layers,
        &mut out,
        opts,
        |e| e.file("metadata.yaml", 0o644, META),
        |e| {
            e.dir("rootfs/.podvm", 0o755)?;
            e.file("rootfs/.podvm/podvm", 0o755, b"binary")
        },
    )?;
    Ok((out, f))
}

fn rootfs() -> Options {
    Options { prefix: "rootfs/".into(), ..Options::default() }
}

#[test]
fn the_prefix_and_the_entries_of_the_caller_make_a_unified_tarball() {
    let s = Scratch::new("unified");
    let l0 = tar(&[E::Dir("etc", 0o755), E::File("etc/a", b"a", 0o640), E::File("etc/z", b"z", 0o644)]);
    let l1 = tar(&[E::Hardlink("etc/b", "etc/a"), E::Symlink("etc/l", "/etc/a"), E::File("etc/.wh.z", b"", 0)]);
    let layers = plain(s.path(), &[l0, l1]);
    let opts = Options { prefix_mode: 0o711, ..rootfs() };
    let (stream, f) = unified(&layers, &opts).unwrap();
    let list = paths(&stream);
    assert_eq!(list[..2], ["metadata.yaml".to_string(), "rootfs".to_string()], "{list:?}");
    assert_eq!(list[list.len() - 2..], ["rootfs/.podvm".to_string(), "rootfs/.podvm/podvm".to_string()]);
    assert!(list[2..list.len() - 2].iter().all(|p| p.starts_with("rootfs/")), "{list:?}");
    let all = entries(&stream);
    let root = all.iter().find(|e| e.0 == "rootfs").unwrap();
    assert_eq!((root.1, root.2, root.3), (::tar::EntryType::Directory, 0o711, 0));
    let link = all.iter().find(|e| e.0 == "rootfs/etc/b").unwrap();
    assert_eq!(link.5.as_deref(), Some("rootfs/etc/a"), "a hard link names its target with the prefix");
    let sym = all.iter().find(|e| e.0 == "rootfs/etc/l").unwrap();
    assert_eq!(sym.5.as_deref(), Some("/etc/a"), "a symlink keeps its target");
    assert_eq!((f.extra, f.links), (4, 1));
    // The end blocks once, at the end.
    assert!(stream.len() % 512 == 0 && stream[stream.len() - 1024..].iter().all(|b| *b == 0));
    assert!(stream[stream.len() - 1536..stream.len() - 1024].iter().any(|b| *b != 0), "the end blocks two times");
    // The tree below the prefix is the tree of `extract` of the layers.
    let want = extracted(&layers).unwrap();
    let t = Scratch::new("unified-tree");
    let got = extracted(&plain(t.path(), &[stream])).unwrap();
    let below = |m: &std::collections::BTreeMap<String, support::Node>| {
        m.iter()
            .filter_map(|(k, v)| Some((k.strip_prefix("rootfs/")?.to_string(), v.clone())))
            .filter(|(k, _)| !k.starts_with(".podvm"))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    assert_eq!(below(&got.nodes), want.nodes);
    let names: Vec<Vec<String>> =
        got.links.iter().map(|v| v.iter().map(|p| p.replace("rootfs/", "")).collect()).collect();
    assert_eq!(names, want.links.into_iter().collect::<Vec<_>>());
}

#[test]
fn a_caller_that_appends_more_gets_no_end_blocks() {
    let s = Scratch::new("noend");
    let layers = plain(s.path(), &[tar(&[E::File("a", b"a", 0o644)])]);
    let (whole, _) = unified(&layers, &rootfs()).unwrap();
    let (open, f) = unified(&layers, &Options { end: false, ..rootfs() }).unwrap();
    assert_eq!(open[..], whole[..whole.len() - 1024]);
    assert_eq!(f.bytes, open.len() as u64);
}

#[test]
fn an_entry_of_the_caller_at_a_path_of_the_image_is_refused() {
    let s = Scratch::new("collide");
    let l0 = tar(&[E::Dir(".podvm", 0o700), E::Symlink("bin", "usr/bin"), E::File("usr/bin/sh", b"sh", 0o755)]);
    let layers = plain(s.path(), &[l0]);
    // `.podvm` of the image, after the tree.
    let f = unified(&layers, &rootfs()).expect_err("an entry of the caller at a path of the image");
    assert!(f.to_string().contains("rootfs/.podvm"), "{}", f.chain());
    // Below a symlink of the image, before the tree: found after the tree.
    let mut out = Vec::new();
    let r = flatten_with(&layers, &mut out, &rootfs(), |e| e.file("rootfs/bin/x", 0o644, b"x"), |_| Ok(()));
    let f = r.expect_err("an entry of the caller below a symlink of the image");
    assert!(f.to_string().contains("rootfs/bin/x"), "{}", f.chain());
    assert!(out[out.len() - 512..].iter().all(|b| *b == 0xff), "the stream is broken");
    // The directory of the prefix is not the caller's.
    let mut out = Vec::new();
    let r = flatten_with(&layers, &mut out, &rootfs(), |e| e.dir("rootfs", 0o755), |_| Ok(()));
    assert!(r.is_err());
}

#[test]
fn a_prefix_that_is_not_a_path_is_refused_before_the_first_byte() {
    let s = Scratch::new("badprefix");
    let layers = plain(s.path(), &[tar(&[E::File("a", b"a", 0o644)])]);
    for prefix in ["../x", "/abs", "a/../b"] {
        let mut out = Vec::new();
        let opts = Options { prefix: prefix.into(), ..Options::default() };
        assert!(flatten_with(&layers, &mut out, &opts, |_| Ok(()), |_| Ok(())).is_err(), "{prefix}");
        assert!(out.is_empty(), "{prefix}");
    }
}

#[test]
fn the_counts_name_the_attributes_written_and_not_written() {
    let s = Scratch::new("xcounts");
    let big = vec![b'x'; 64 * 1024 + 1];
    let recs: &[(&str, &[u8])] = &[("SCHILY.xattr.user.big", &big), ("SCHILY.xattr.user.ok", b"1")];
    let l0 = tar(&[E::Pax("f", b"f", 0o644, recs), E::Pax("g", b"g", 0o644, &[("SCHILY.xattr.user.g", b"2")])]);
    let layers = plain(s.path(), &[l0]);
    let (_, f) = unified(&layers, &Options::default()).unwrap();
    assert_eq!((f.xattrs.set, f.xattrs.unset), (2, 1));
    assert_eq!(f.xattrs.first_unset[0].name, "user.big");
}

/// GNU tar with the arguments of incusd takes the unified tarball, with
/// the attributes of each file.
#[test]
fn gnu_tar_takes_the_unified_tarball() {
    if let Err(why) = pod_testkit::host::tool("tar") {
        pod_testkit::host::skip_line(&why);
        return;
    }
    let s = Scratch::new("gnuunified");
    let recs: &[(&str, &[u8])] = &[("SCHILY.xattr.user.k", b"v")];
    let l0 = tar(&[E::Dir("bin", 0o755), E::Pax("bin/a", b"a", 0o755, recs), E::Hardlink("bin/b", "bin/a")]);
    let layers = plain(s.path(), &[l0]);
    let (stream, _) = unified(&layers, &rootfs()).unwrap();
    let out = s.path().join("gnu");
    assert!(support::gnu_tar(&stream, &out).unwrap().success(), "GNU tar refused the stream");
    assert_eq!(std::fs::read(out.join("metadata.yaml")).unwrap(), META);
    assert_eq!(std::fs::read(out.join("rootfs/.podvm/podvm")).unwrap(), b"binary");
    let want = extracted(&layers).unwrap();
    assert_eq!(
        support::walk(&out.join("rootfs"))
            .into_iter()
            .filter(|(k, _)| !k.starts_with(".podvm"))
            .collect::<std::collections::BTreeMap<_, _>>(),
        want.nodes
    );
    use std::os::fd::AsRawFd;
    let f = std::fs::File::open(out.join("rootfs/bin/a")).unwrap();
    let mut buf = [0u8; 16];
    let name = pod_hostprobe::sys::CBuf::new("user.k").unwrap();
    let n = pod_hostprobe::sys::fgetxattr(i64::from(f.as_raw_fd()), &name, &mut buf);
    assert_eq!(n.ok().map(|n| buf[..n as usize].to_vec()), Some(b"v".to_vec()), "GNU tar did not set user.k");
}
