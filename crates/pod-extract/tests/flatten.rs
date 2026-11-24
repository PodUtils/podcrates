//! `flatten` against `extract` of the same layers: the stream of the final
//! tree has the paths, kinds, modes, owners, contents and symlink targets
//! of the tree that `extract` writes (`support::same`).
#![cfg(target_os = "linux")]

mod support;

use pod_extract::flatten::{Options, flatten};
use support::{E, Node, Pack, Scratch, blobs, entries, extracted, flattened, plain, same, tar};

fn refusal(r: pod_extract::Result<impl std::fmt::Debug>) -> pod_extract::Refused {
    match r {
        Err(f) => pod_extract::refused(&f).cloned().unwrap_or_else(|| panic!("not a refusal: {}", f.chain())),
        Ok(v) => panic!("not refused: {v:?}"),
    }
}

fn paths(stream: &[u8]) -> Vec<String> {
    entries(stream).into_iter().map(|e| e.0).collect()
}

#[test]
fn whiteouts_at_the_root_of_a_layer_and_below_it_hide_the_older_paths() {
    let s = Scratch::new("wh");
    let l0 = tar(&[
        E::Dir("etc", 0o755),
        E::File("etc/passwd", b"root\n", 0o644),
        E::File("etc/shadow", b"x\n", 0o600),
        E::Dir("opt", 0o755),
        E::Dir("opt/app", 0o750),
        E::File("opt/app/bin", b"bin", 0o755),
        E::File("top", b"top", 0o644),
    ]);
    let l1 = tar(&[E::File(".wh.top", b"", 0o644), E::File("etc/.wh.shadow", b"", 0o644), E::File(".wh.opt", b"", 0)]);
    let stream = same(&plain(s.path(), &[l0, l1]));
    let p = paths(&stream);
    assert!(p.iter().all(|x| x != "top" && x != "etc/shadow" && !x.starts_with("opt")), "{p:?}");
    assert!(p.iter().all(|x| !x.contains(".wh.")), "a marker went into the stream: {p:?}");
}

#[test]
fn an_opaque_directory_hides_the_older_children_and_keeps_its_own() {
    let s = Scratch::new("opq");
    let l0 = tar(&[E::Dir("var", 0o755), E::Dir("var/cache", 0o755), E::File("var/cache/old", b"old", 0o644)]);
    let l1 = tar(&[
        E::Dir("var/cache", 0o700),
        E::File("var/cache/.wh..wh..opq", b"", 0),
        E::File("var/cache/new", b"new", 0o644),
    ]);
    let stream = same(&plain(s.path(), &[l0, l1]));
    let p = paths(&stream);
    assert!(p.contains(&"var/cache/new".to_string()) && !p.contains(&"var/cache/old".to_string()), "{p:?}");
}

#[test]
fn an_opaque_marker_at_the_root_of_a_layer_hides_each_older_path() {
    let s = Scratch::new("opqroot");
    let l0 = tar(&[E::Dir("a", 0o755), E::File("a/x", b"x", 0o644), E::File("b", b"b", 0o644)]);
    let l1 = tar(&[E::File(".wh..wh..opq", b"", 0), E::File("c", b"c", 0o644)]);
    let stream = same(&plain(s.path(), &[l0, l1]));
    assert_eq!(paths(&stream), vec!["c".to_string()]);
}

#[test]
fn a_file_replaced_by_a_directory_and_back() {
    let s = Scratch::new("flip");
    let l0 = tar(&[E::File("p", b"file one", 0o644), E::Dir("q", 0o755), E::File("q/in", b"in", 0o644)]);
    let l1 = tar(&[E::Dir("p", 0o711), E::File("p/child", b"child", 0o600), E::File("q", b"q is a file", 0o640)]);
    let l2 = tar(&[E::File("p", b"file again", 0o604)]);
    let stream = same(&plain(s.path(), &[l0.clone(), l1.clone(), l2]));
    assert_eq!(paths(&stream), vec!["p".to_string(), "q".to_string()]);
    // The step in the middle: the directory with its child, the file with no
    // child of the directory before it.
    same(&plain(s.path(), &[l0, l1]));
}

#[test]
fn a_directory_replaced_by_a_symlink_hides_its_children() {
    let s = Scratch::new("dirsym");
    let l0 = tar(&[E::Dir("lib", 0o755), E::File("lib/libc.so", b"elf", 0o755)]);
    let l1 = tar(&[E::Dir("usr", 0o755), E::Dir("usr/lib", 0o755), E::Symlink("lib", "usr/lib")]);
    let stream = same(&plain(s.path(), &[l0, l1]));
    assert!(!paths(&stream).contains(&"lib/libc.so".to_string()));
}

#[test]
fn an_entry_through_a_symlink_of_an_older_layer_is_refused_as_extract_refuses_it() {
    let s = Scratch::new("escape");
    let l0 = tar(&[E::Symlink("evil", "/etc")]);
    let l1 = tar(&[E::File("evil/passwd", b"pwned\n", 0o644)]);
    let layers = plain(s.path(), &[l0.clone(), l1.clone()]);
    let want = refusal(extracted(&layers));
    let got = refusal(flattened(&layers));
    assert_eq!(got.entry, want.entry);
    assert_eq!(got.layer, want.layer, "the layer of the entry is named");
    assert!(matches!(got.refusal, pod_extract::safety::Refusal::Escapes(_)), "{:?}", got.refusal);
    // A later layer that removes the symlink does not make the write of
    // the layer before it legal.
    let l2 = tar(&[E::File(".wh.evil", b"", 0)]);
    let layers = plain(s.path(), &[l0.clone(), l1.clone(), l2]);
    refusal(extracted(&layers));
    assert_eq!(refusal(flattened(&layers)).entry, "evil/passwd");
    // A whiteout of the symlink in the layer of the write: legal.
    let l1w = tar(&[E::File("evil/passwd", b"ok\n", 0o644), E::File(".wh.evil", b"", 0)]);
    same(&plain(s.path(), &[l0, l1w]));
}

#[test]
fn an_entry_through_a_symlink_of_the_same_layer_is_refused() {
    let s = Scratch::new("escape1");
    let l = tar(&[E::Symlink("evil", "/etc"), E::File("evil/passwd", b"pwned\n", 0o644)]);
    let layers = plain(s.path(), &[l]);
    refusal(extracted(&layers));
    assert_eq!(refusal(flattened(&layers)).entry, "evil/passwd");
}

#[test]
fn a_path_with_dotdot_and_an_escaping_symlink_are_refused() {
    let s = Scratch::new("dotdot");
    for l in [tar(&[E::Raw("../escaped", b"x")]), tar(&[E::Symlink("up", "../../outside")])] {
        let layers = plain(s.path(), &[tar(&[E::File("ok", b"ok", 0o644)]), l]);
        let want = refusal(extracted(&layers));
        let got = refusal(flattened(&layers));
        assert_eq!((got.entry, got.refusal), (want.entry, want.refusal));
    }
}

#[test]
fn the_same_layer_removes_a_path_and_makes_it_again() {
    let s = Scratch::new("remake");
    let l0 = tar(&[E::Dir("d", 0o755), E::File("d/old", b"old", 0o644)]);
    let l1 = tar(&[E::Dir("d", 0o700), E::File("d/new", b"new", 0o644), E::File(".wh.d", b"", 0)]);
    let stream = same(&plain(s.path(), &[l0, l1]));
    assert_eq!(paths(&stream), vec!["d".to_string(), "d/new".to_string()]);
}

#[test]
fn owners_and_modes_are_those_of_the_image() {
    let s = Scratch::new("owners");
    let l0 = tar(&[E::Dir("home", 0o755), E::Owned("home/u", b"u", 1000, 1000, 0o600), E::Dir("srv", 0o555)]);
    let l1 = tar(&[E::Owned("home/v", b"v", 4_000_000, 42, 0o4755), E::Dir("srv", 0o751)]);
    let stream = same(&plain(s.path(), &[l0, l1]));
    let v = entries(&stream).into_iter().find(|e| e.0 == "home/v").unwrap();
    assert_eq!((v.2, v.3, v.4), (0o4755, 4_000_000, 42));
}

#[test]
fn each_compression_and_two_gzip_members_in_one_blob() {
    let s = Scratch::new("packs");
    let big: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let l0 = tar(&[E::Dir("a", 0o755), E::File("a/big", &big, 0o644), E::File("a/small", b"s", 0o644)]);
    let l1 = tar(&[E::File("a/.wh.small", b"", 0), E::File("b", b"b", 0o644)]);
    let l2 = tar(&[E::File("c", b"c", 0o644)]);
    let l3 = tar(&[E::File("d", b"d", 0o644)]);
    same(&blobs(s.path(), &[(l0, Pack::Gzip2), (l1, Pack::Plain), (l2, Pack::Zstd), (l3, Pack::Gzip)]));
}

#[test]
fn a_blob_that_does_not_agree_with_its_digest_breaks_the_stream() {
    let s = Scratch::new("mismatch");
    let mut layers = plain(s.path(), &[tar(&[E::File("a", b"a", 0o644)]), tar(&[E::File("b", b"b", 0o644)])]);
    layers[0].digest = format!("sha256:{}", "0".repeat(64));
    let mut out = Vec::new();
    let f = flatten(&layers, &mut out, &Options::default()).expect_err("a mismatch fails");
    assert!(pod_extract::mismatch(&f).is_some(), "{}", f.chain());
    // The newest layer went out first. The end is a block that is not a
    // header, so a reader of the upload fails and does not take a short tree.
    assert!(out.len() >= 1024 && out[out.len() - 512..].iter().all(|b| *b == 0xff), "{} bytes", out.len());
    let mut ar = tar::Archive::new(std::io::Cursor::new(out));
    assert!(ar.entries().unwrap().any(|e| e.is_err()), "the tar reader takes the broken stream");
}

#[test]
fn a_deep_tree_and_a_long_name_go_through() {
    let s = Scratch::new("deep");
    let deep: Vec<String> = (0..120).map(|i| format!("d{i}")).collect();
    let mut entries_ = Vec::new();
    let mut dirs = Vec::new();
    for i in 0..deep.len() {
        dirs.push(deep[..=i].join("/"));
    }
    for d in &dirs {
        entries_.push(E::Dir(d, 0o755));
    }
    let leaf = format!("{}/leaf", dirs.last().unwrap());
    entries_.push(E::File(&leaf, b"leaf", 0o644));
    let long = format!("{}/{}", dirs[3], "n".repeat(200));
    entries_.push(E::File(&long, b"long", 0o644));
    let l1 = tar(&[E::File(&format!("{}/.wh.d60", dirs[59]), b"", 0)]);
    let stream = same(&plain(s.path(), &[tar(&entries_), l1]));
    assert!(paths(&stream).iter().all(|p| p.split('/').count() <= 60), "a removed path went out");
}

#[test]
fn a_component_longer_than_a_linux_name_is_refused() {
    let s = Scratch::new("longname");
    let name = format!("a/{}", "x".repeat(256));
    let layers = plain(s.path(), &[tar(&[E::File(&name, b"x", 0o644)])]);
    let r = refusal(flattened(&layers));
    assert!(matches!(r.refusal, pod_extract::safety::Refusal::TooLong(_)), "{:?}", r.refusal);
    assert_eq!(refusal(extracted(&layers)).refusal, r.refusal);
}

#[test]
fn a_sparse_file_of_each_version_is_written_with_its_holes_as_zero_bytes() {
    let s = Scratch::new("sparse");
    let regions: &[(u64, &[u8])] = &[(0, b"head"), (10_000, b"middle"), (70_000, b"tail")];
    for v in ["0.0", "0.1", "1.0"] {
        let l = tar(&[E::Dir("s", 0o755), E::Sparse(v, "s/file", 80_000, regions)]);
        let stream = same(&plain(s.path(), &[l]));
        assert_eq!(paths(&stream), vec!["s".to_string(), "s/file".to_string()], "version {v}");
        let t = extracted(&plain(s.path(), &[stream])).unwrap();
        let Some(Node::File { data, .. }) = t.nodes.get("s/file") else { panic!("version {v}: no file") };
        assert_eq!((data.len(), &data[10_000..10_006], data[9_999]), (80_000, &b"middle"[..], 0));
    }
}

#[test]
fn a_hard_link_to_a_file_of_its_layer_is_kept_and_the_other_links_wait_for_their_entry() {
    let s = Scratch::new("hardlink");
    let l0 = tar(&[E::File("a", b"data", 0o644), E::Hardlink("b", "a")]);
    let stream = same(&plain(s.path(), &[l0.clone()]));
    let link = entries(&stream).into_iter().find(|e| e.0 == "b").unwrap();
    assert_eq!((link.1, link.5), (tar::EntryType::Link, Some("a".to_string())));
    // A link to a file of an older layer: its target comes after it in a
    // stream from the newest layer.
    let layers = plain(s.path(), &[l0, tar(&[E::Hardlink("c", "a")])]);
    let f = flattened(&layers).expect_err("not written yet");
    assert!(f.to_string().contains("hard link"), "{}", f.chain());
}

#[test]
fn a_skipped_kind_is_counted_and_not_written() {
    let s = Scratch::new("fifo");
    let l = tar(&[E::Dir("run", 0o755), E::Fifo("run/fifo"), E::File("run/pid", b"1", 0o644)]);
    let layers = plain(s.path(), &[l]);
    let stream = same(&layers);
    let (_, f) = flattened(&layers).unwrap();
    assert_eq!((f.skipped, f.skipped_kinds.clone(), f.written), (1, vec!["Fifo".to_string()], 2));
    assert_eq!(paths(&stream), vec!["run".to_string(), "run/pid".to_string()]);
}

#[test]
fn the_counts_name_the_written_the_hidden_and_the_markers() {
    let s = Scratch::new("counts");
    let l0 = tar(&[E::Dir("d", 0o755), E::File("d/a", b"a", 0o644), E::File("d/b", b"b", 0o644)]);
    let l1 = tar(&[E::File("d/.wh.a", b"", 0), E::File("d/b", b"B", 0o644)]);
    let layers = plain(s.path(), &[l0, l1]);
    let (stream, f) = flattened(&layers).unwrap();
    assert_eq!((f.layers, f.entries, f.written, f.hidden, f.whiteouts), (2, 5, 2, 2, 1));
    assert_eq!(f.bytes, stream.len() as u64);
}

#[test]
fn a_path_twice_in_one_layer_goes_out_in_the_order_of_the_layer() {
    let s = Scratch::new("twice");
    let l = tar(&[E::File("f", b"one", 0o644), E::File("f", b"two", 0o600), E::Dir("d", 0o700), E::Dir("d", 0o751)]);
    let layers = plain(s.path(), &[l]);
    let (stream, _) = flattened(&layers).unwrap();
    let got = extracted(&plain(s.path(), &[stream])).unwrap();
    assert_eq!(got.nodes, extracted(&layers).unwrap().nodes);
    // A file over a directory that the same layer filled: a reader of the
    // stream cannot replace the full directory, so the layer is refused.
    let l = tar(&[E::Dir("x", 0o755), E::File("x/in", b"in", 0o644), E::File("x", b"x", 0o644)]);
    let r = refusal(flattened(&plain(s.path(), &[l])));
    assert_eq!(r.entry, "x");
}

/// The unpacker of incusd is GNU tar with these arguments
/// (`lxc/incus:shared/archive/archive.go`, `Unpack`), and it takes the
/// order of the stream: a directory after its children, from the newest
/// layer. `-p` keeps the modes as a user that is not root, as root does.
#[test]
fn gnu_tar_with_the_arguments_of_incusd_takes_the_stream() {
    if let Err(why) = pod_testkit::host::tool("tar") {
        pod_testkit::host::skip_line(&why);
        return;
    }
    let s = Scratch::new("gnutar");
    let l0 = tar(&[
        E::Dir("usr", 0o755),
        E::Dir("usr/bin", 0o755),
        E::File("usr/bin/sh", b"sh", 0o755),
        E::Dir("etc", 0o750),
        E::File("etc/old", b"old", 0o644),
    ]);
    let l1 = tar(&[
        E::Dir("usr/bin", 0o555),
        E::File("usr/bin/ls", b"ls", 0o755),
        E::Symlink("usr/bin/dir", "ls"),
        E::File("etc/.wh.old", b"", 0),
        E::Dir("etc", 0o711),
        E::File("etc/new", b"new", 0o600),
    ]);
    let layers = plain(s.path(), &[l0, l1]);
    let want = extracted(&layers).unwrap();
    let (stream, _) = flattened(&layers).unwrap();
    let out = s.path().join("gnu");
    std::fs::create_dir_all(&out).unwrap();
    let mut child = std::process::Command::new("tar")
        .args(["--anchored", "--wildcards", "--exclude=dev/*", "--restrict", "--force-local", "-C"])
        .arg(&out)
        .args(["--numeric-owner", "--xattrs-include=*", "-p", "-xf", "-"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(child.stdin.as_mut().unwrap(), &stream).unwrap();
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success(), "GNU tar refused the stream");
    assert_eq!(support::walk(&out), want.nodes);
}
