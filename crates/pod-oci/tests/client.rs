//! The client of the crate against the registry fixture: the resolve of a
//! reference, the blobs into memory with their digests, the classes of a
//! missing manifest and of a missing blob, the new try of a short body,
//! and the line of the move to plain HTTP.
#![cfg(target_os = "linux")]

mod common;

use std::sync::{Arc, Mutex};

use pod_core::{Class, Event, Stage};
use pod_oci::{Digest, Platform, Reference, pull};

#[test]
fn the_client_resolves_and_fetches_from_the_fixture() {
    pod_testkit::need_loopback!();
    let fx = common::registry::Fixture::start();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let keep = seen.clone();
    let mut client = common::client::client(common::client::environment(&[]), &[&fx.endpoint])
        .with_events(Arc::new(move |e: Event| keep.lock().unwrap().push(e)));
    let reference = Reference::parse(&fx.reference()).unwrap();
    let resolved = pull::resolve(&mut client, &reference, &Platform::host()).unwrap();
    assert_eq!(resolved.top.digest.to_string(), fx.manifest_digest);
    assert_eq!(resolved.manifest_bytes, fx.manifest);
    for (descriptor, published) in
        resolved.manifest.layers.iter().chain(std::iter::once(&resolved.manifest.config)).zip([&fx.layer, &fx.config])
    {
        let digest = descriptor.parsed_digest().unwrap();
        let mut out = Vec::new();
        let got = client
            .blob(reference.endpoint(), &reference.repository, &digest, Some(descriptor.size), Vec::new(), &mut out)
            .unwrap();
        assert_eq!(&got, published, "{digest}");
        assert!(out.is_empty(), "a whole transfer needs no new try: {}", String::from_utf8_lossy(&out));
    }
    // One line, when the endpoint went to plain HTTP, and no more: the
    // client keeps the choice.
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].kind, "oci.plain-http");
    assert!(seen[0].text.contains(&fx.endpoint) && seen[0].text.contains("http://"), "{}", seen[0].text);
    drop(seen);
    fx.shutdown();
}

#[test]
fn a_missing_manifest_and_a_missing_blob_are_not_found() {
    pod_testkit::need_loopback!();
    let fx = common::registry::Fixture::start();
    let mut client = common::client::client(common::client::environment(&[]), &[&fx.endpoint]);
    let e = match client.manifest(&fx.endpoint, &fx.repository, "no-such-tag") {
        Ok(_) => panic!("a missing tag gave a manifest"),
        Err(e) => e,
    };
    assert_eq!((e.stage, e.class), (Stage::Registry, Class::NotFound), "{e}");
    assert!(e.to_string().contains("HTTP 404"), "{e}");
    let missing = Digest::of(b"no such blob");
    let e = client.blob(&fx.endpoint, &fx.repository, &missing, None, Vec::new(), &mut Vec::new()).unwrap_err();
    assert_eq!(e.class, Class::NotFound, "{e}");
    fx.shutdown();
}

/// A body shorter than its descriptor is a cut transfer: the client tries
/// it again, says so, and then refuses it with `Class::Protocol`.
#[test]
fn a_body_shorter_than_its_descriptor_is_tried_again_and_refused() {
    pod_testkit::need_loopback!();
    let fx = common::registry::Fixture::start();
    let mut client = common::client::client(common::client::environment(&[]), &[&fx.endpoint]);
    let config = Digest::parse(&fx.config_digest).unwrap();
    let mut out = Vec::new();
    let e = client.blob(&fx.endpoint, &fx.repository, &config, Some(fx.config.len() as u64 + 1), Vec::new(), &mut out);
    let e = e.expect_err("a short body is refused");
    assert!(e.to_string().contains("declares"), "{e}");
    assert_eq!(e.class, Class::Protocol, "{e}");
    assert!(String::from_utf8_lossy(&out).contains("attempt 2 of 3"), "{}", String::from_utf8_lossy(&out));
    fx.shutdown();
}
