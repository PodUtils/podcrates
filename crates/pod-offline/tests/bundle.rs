//! The tests of the offline bundle: a signed manifest, a delta from a named
//! base, and a refusal of a wrong base.

mod common;

use pod_identity::Identity;
use pod_offline::bundle::{self, Op};

fn files(list: &[(&str, &[u8])]) -> Vec<(String, Vec<u8>)> {
    list.iter().map(|(path, body)| ((*path).to_string(), body.to_vec())).collect()
}

#[test]
fn a_bundle_is_self_describing_and_names_its_key() {
    let scratch = common::Scratch::new("bundle-read");
    let identity = Identity::from_seed(&[31; 32]);
    let dir = scratch.join("bundle");
    let written = bundle::write_bundle(
        &dir,
        &identity,
        "field-kit",
        &files(&[("docs/readme.txt", b"hello"), ("bin/run", b"#!/bin/sh\n")]),
    )
    .expect("a bundle");
    let read = bundle::read_bundle(&dir).expect("the bundle back");
    assert_eq!(read.name, "field-kit");
    assert_eq!(read.key, identity.public());
    assert_eq!(read.id, written.id);
    assert_eq!(read.files.len(), 2);
    assert_eq!(std::fs::read(dir.join("data/docs/readme.txt")).unwrap(), b"hello");
}

#[test]
fn a_delta_applies_to_its_base_and_a_wrong_base_is_refused() {
    let scratch = common::Scratch::new("bundle-delta");
    let identity = Identity::from_seed(&[32; 32]);
    let one = scratch.join("one");
    let base = bundle::write_bundle(
        &one,
        &identity,
        "kit-one",
        &files(&[("a.txt", b"one"), ("b.txt", b"two"), ("c.txt", b"three")]),
    )
    .expect("the first bundle");

    let next_files = files(&[("a.txt", b"ONE"), ("c.txt", b"three"), ("d.txt", b"four")]);
    let delta_dir = scratch.join("delta");
    let delta = bundle::write_delta(&delta_dir, &identity, &base, "kit-two", &next_files).expect("a delta");
    assert_eq!(delta.base, base.id);
    assert_eq!(delta.ops.get("a.txt"), Some(&Op::Change));
    assert_eq!(delta.ops.get("b.txt"), Some(&Op::Delete));
    assert_eq!(delta.ops.get("c.txt"), None);
    assert_eq!(delta.ops.get("d.txt"), Some(&Op::Add));

    // A host with the new bundle already is not the base of this delta.
    let wrong = scratch.join("wrong");
    let wrong_manifest = bundle::write_bundle(&wrong, &identity, "kit-two", &next_files).expect("the new bundle");
    let failed = bundle::apply_delta(&wrong, &delta_dir).expect_err("a wrong base");
    assert_eq!(failed.name(), "wrong-base");
    assert!(failed.to_string().contains(&delta.base));
    assert_eq!(bundle::read_bundle(&wrong).unwrap().id, wrong_manifest.id);

    // A host with the previous bundle takes the delta.
    let applied = bundle::apply_delta(&one, &delta_dir).expect("the delta");
    assert_eq!(applied.id, delta.manifest.id);
    assert_eq!(std::fs::read(one.join("data/a.txt")).unwrap(), b"ONE");
    assert!(!one.join("data/b.txt").exists());
    assert_eq!(std::fs::read(one.join("data/c.txt")).unwrap(), b"three");
    assert_eq!(std::fs::read(one.join("data/d.txt")).unwrap(), b"four");
    let after = bundle::read_bundle(&one).expect("the new bundle");
    assert_eq!(after.id, delta.manifest.id);
    assert_eq!(after.name, "kit-two");
    assert_eq!(after.key, identity.public());
}

#[test]
fn a_changed_file_and_a_changed_manifest_are_refused() {
    let scratch = common::Scratch::new("bundle-tamper");
    let identity = Identity::from_seed(&[33; 32]);
    let dir = scratch.join("bundle");
    bundle::write_bundle(&dir, &identity, "kit", &files(&[("a.txt", b"one")])).expect("a bundle");

    std::fs::write(dir.join("data/a.txt"), b"two").expect("a change");
    let failed = bundle::read_bundle(&dir).expect_err("a changed file");
    assert_eq!(failed.name(), "hash-mismatch");
    std::fs::write(dir.join("data/a.txt"), b"one").expect("the bytes back");

    let manifest = std::fs::read_to_string(dir.join("manifest")).expect("the manifest");
    std::fs::write(dir.join("manifest"), manifest.replace("name: kit", "name: other")).expect("a change");
    let failed = bundle::read_bundle(&dir).expect_err("a changed manifest");
    assert_eq!(failed.name(), "proof");
}
