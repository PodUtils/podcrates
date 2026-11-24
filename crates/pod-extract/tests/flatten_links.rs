//! The hard links of `flatten` against `extract` of the same layers: the
//! tree of the stream has the names of each inode of the tree of `extract`
//! (`support::same` compares the inodes), and each link entry follows the
//! entry of its target.
#![cfg(target_os = "linux")]

mod support;

use pod_extract::safety::Refusal;
use support::{E, Scratch, entries, extracted, flattened, paths, plain, refusal, same, tar};

/// The kind and the target of the entry of `path` in a stream.
fn kind_of(stream: &[u8], path: &str) -> (tar::EntryType, Option<String>) {
    let e = entries(stream).into_iter().find(|e| e.0 == path).unwrap_or_else(|| panic!("no {path}"));
    (e.1, e.5)
}

/// Each link entry of a stream comes after the entry that it names.
fn links_follow_their_targets(stream: &[u8]) {
    let listed = entries(stream);
    for (i, e) in listed.iter().enumerate() {
        if e.1 == tar::EntryType::Link {
            let t = e.5.as_deref().unwrap();
            assert!(listed[..i].iter().any(|x| x.0 == t), "the link {} comes before {t}: {listed:?}", e.0);
        }
    }
}

#[test]
fn a_link_to_a_file_of_an_older_layer_follows_its_target() {
    let s = Scratch::new("older");
    let l0 = tar(&[E::Dir("usr", 0o755), E::File("usr/a", b"data", 0o755)]);
    let l1 = tar(&[E::Hardlink("usr/b", "usr/a")]);
    let stream = same(&plain(s.path(), &[l0, l1]));
    assert_eq!(kind_of(&stream, "usr/b"), (tar::EntryType::Link, Some("usr/a".into())));
    links_follow_their_targets(&stream);
}

#[test]
fn a_link_keeps_the_bytes_of_a_target_that_a_later_layer_replaced_or_removed() {
    let s = Scratch::new("kept");
    let l0 = tar(&[E::File("a", b"old", 0o640)]);
    let l1 = tar(&[E::Hardlink("b", "a"), E::Hardlink("c", "a")]);
    let later = [
        tar(&[E::File("a", b"new", 0o644)]),
        tar(&[E::Dir("a", 0o755), E::File("a/x", b"x", 0o644)]),
        tar(&[E::File(".wh.a", b"", 0)]),
        tar(&[E::Symlink("a", "b")]),
    ];
    for l2 in later {
        let stream = same(&plain(s.path(), &[l0.clone(), l1.clone(), l2]));
        assert_eq!(kind_of(&stream, "b").0, tar::EntryType::Regular, "the first name holds the data");
        assert_eq!(kind_of(&stream, "c"), (tar::EntryType::Link, Some("b".into())));
        links_follow_their_targets(&stream);
    }
}

#[test]
fn a_chain_of_links_is_one_inode() {
    let s = Scratch::new("chain");
    let l0 = tar(&[E::File("a", b"x", 0o644)]);
    let l1 = tar(&[E::Hardlink("b", "a")]);
    let l2 = tar(&[E::Hardlink("c", "b")]);
    let l3 = tar(&[E::File(".wh.a", b"", 0)]);
    let stream = same(&plain(s.path(), &[l0.clone(), l1.clone(), l2.clone(), l3]));
    links_follow_their_targets(&stream);
    same(&plain(s.path(), &[l0, l1, l2]));
    // A chain in one layer, and a link of a newer layer to a hidden link.
    let l0 = tar(&[E::File("p", b"p", 0o600), E::Hardlink("q", "p"), E::Hardlink("r", "q")]);
    let l1 = tar(&[E::Hardlink("s", "q"), E::File(".wh.p", b"", 0), E::File("q", b"new q", 0o644)]);
    links_follow_their_targets(&same(&plain(s.path(), &[l0, l1])));
}

#[test]
fn a_link_after_its_target_in_one_layer_with_the_target_hidden_later() {
    let s = Scratch::new("reread");
    let l0 = tar(&[E::Dir("d", 0o755), E::File("d/a", b"first", 0o751), E::Hardlink("d/b", "d/a")]);
    let l1 = tar(&[E::File("d/a", b"second", 0o644)]);
    let layers = plain(s.path(), &[l0, l1]);
    let stream = same(&layers);
    assert_eq!(kind_of(&stream, "d/b").0, tar::EntryType::Regular);
    let (_, f) = flattened(&layers).unwrap();
    assert_eq!(f.rereads, 1, "the data of the hidden target went by: its layer is read again");
}

#[test]
fn a_link_before_its_target_in_its_layer() {
    let s = Scratch::new("before");
    let l0 = tar(&[E::Hardlink("b", "a"), E::File("a", b"x", 0o644)]);
    let stream = same(&plain(s.path(), std::slice::from_ref(&l0)));
    links_follow_their_targets(&stream);
    // The target hidden by a later layer: the link holds the data.
    let stream = same(&plain(s.path(), &[l0, tar(&[E::File(".wh.a", b"", 0)])]));
    assert_eq!(paths(&stream), vec!["b".to_string()]);
    // A target of an older layer is there at the time of the link: the link
    // keeps it, and the later entry of the layer is a new inode.
    let l0 = tar(&[E::File("a", b"old", 0o644)]);
    let l1 = tar(&[E::Hardlink("b", "a"), E::File("a", b"new", 0o644)]);
    same(&plain(s.path(), &[l0, l1]));
    // The directory of the target comes after the link too.
    let l0 = tar(&[E::Hardlink("b", "d/a"), E::Dir("d", 0o750), E::File("d/a", b"x", 0o644)]);
    links_follow_their_targets(&same(&plain(s.path(), &[l0])));
}

/// An entry below a link that waits for its target is an entry through a
/// file of the same layer.
#[test]
fn an_entry_below_a_link_that_waits_is_refused() {
    let s = Scratch::new("below");
    let l0 = tar(&[E::Hardlink("b", "a"), E::File("b/x", b"x", 0o644), E::File("a", b"a", 0o644)]);
    let layers = plain(s.path(), &[l0]);
    let want = refusal(extracted(&layers));
    let got = refusal(flattened(&layers));
    assert!(matches!(want.refusal, Refusal::Escapes(_)), "{want:?}");
    assert_eq!((got.entry, std::mem::discriminant(&got.refusal)), (want.entry, std::mem::discriminant(&want.refusal)));
}

#[test]
fn two_links_to_one_hidden_target_are_one_inode() {
    let s = Scratch::new("two");
    let l0 = tar(&[E::File("a", b"x", 0o644)]);
    let l1 = tar(&[E::Hardlink("b", "a")]);
    let l2 = tar(&[E::Hardlink("c", "a")]);
    let l3 = tar(&[E::File("a", b"y", 0o644)]);
    let layers = plain(s.path(), &[l0, l1, l2, l3]);
    let stream = same(&layers);
    links_follow_their_targets(&stream);
    let want = extracted(&layers).unwrap();
    assert_eq!(want.links.into_iter().collect::<Vec<_>>(), vec![vec!["b".to_string(), "c".to_string()]]);
}

#[test]
fn a_link_to_a_missing_target_or_a_directory_is_refused_as_extract_refuses_it() {
    let s = Scratch::new("missing");
    let cases = [
        vec![tar(&[E::Hardlink("b", "never")])],
        vec![tar(&[E::Hardlink("b", "d/never")]), tar(&[E::Dir("d", 0o755)])],
        vec![tar(&[E::Dir("d", 0o755)]), tar(&[E::Hardlink("b", "d")])],
        vec![tar(&[E::File("a", b"a", 0o644)]), tar(&[E::File(".wh.a", b"", 0), E::Hardlink("b", "a")])],
        vec![tar(&[E::Hardlink("b", "b")])],
    ];
    for layers in cases {
        let layers = plain(s.path(), &layers);
        let want = refusal(extracted(&layers));
        let got = refusal(flattened(&layers));
        assert!(matches!(want.refusal, Refusal::Link(_)), "{want:?}");
        assert_eq!((got.entry, got.refusal.clone()), (want.entry, want.refusal), "{:?}", got.refusal);
    }
}

#[test]
fn a_link_out_of_the_tree_or_through_a_symlink_is_refused() {
    let s = Scratch::new("out");
    let cases = [
        vec![tar(&[E::Hardlink("b", "../x")])],
        vec![tar(&[E::Hardlink("b", "/etc/passwd")])],
        vec![tar(&[E::Symlink("s", "/etc")]), tar(&[E::Hardlink("b", "s/passwd")])],
        vec![tar(&[E::Dir("d", 0o755), E::File("d/a", b"a", 0o644)]), tar(&[E::File("d", b"f", 0o644)]), {
            tar(&[E::Hardlink("b", "d/a")])
        }],
    ];
    for layers in cases {
        let layers = plain(s.path(), &layers);
        let want = refusal(extracted(&layers));
        let got = refusal(flattened(&layers));
        assert_eq!(std::mem::discriminant(&got.refusal), std::mem::discriminant(&want.refusal), "{got:?} {want:?}");
        assert_eq!(got.entry, want.entry);
    }
}

/// GNU tar with the arguments of incusd takes each name of an inode: the
/// data at the first, a link entry after it for each other.
#[test]
fn gnu_tar_takes_the_links_of_the_stream() {
    if let Err(why) = pod_testkit::host::tool("tar") {
        pod_testkit::host::skip_line(&why);
        return;
    }
    let s = Scratch::new("gnulinks");
    let l0 = tar(&[E::Dir("bin", 0o755), E::File("bin/a", b"old", 0o755), E::File("bin/z", b"z", 0o644)]);
    let l1 = tar(&[E::Hardlink("bin/b", "bin/a"), E::Hardlink("bin/c", "bin/a"), E::Hardlink("bin/y", "bin/z")]);
    let l2 = tar(&[E::File("bin/a", b"new", 0o755)]);
    let layers = plain(s.path(), &[l0, l1, l2]);
    let want = extracted(&layers).unwrap();
    let (stream, _) = flattened(&layers).unwrap();
    let out = s.path().join("gnu");
    assert!(support::gnu_tar(&stream, &out).unwrap().success(), "GNU tar refused the stream");
    assert_eq!((support::walk(&out), support::inodes(&out)), (want.nodes, want.links));
}
