//! The registry fixture proves itself before a test uses it: one manifest
//! GET by tag and by digest (with `Docker-Content-Digest` and `ETag`), one
//! configuration and one layer blob GET, one blob HEAD with an empty body, a
//! 404 for an unknown path, and a shutdown that joins the accept loop, so
//! that the test leaves no listener.
//!
//! The client of the first test is a raw loopback socket, not the client of
//! the crate: the test proves that the server speaks HTTP. The second test
//! proves that the client reads it. The digest headers are checked again
//! through `pod_oci::digest`.
#![cfg(target_os = "linux")]

mod common;

use std::io::{Read, Write};

fn request(endpoint: &str, method: &str, path: &str) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let mut stream = std::net::TcpStream::connect(endpoint).expect("connect the fixture");
    stream.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: {endpoint}\r\nConnection: close\r\n\r\n").unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").expect("the fixture ends its headers");
    let head = String::from_utf8_lossy(&raw[..split]);
    let mut lines = head.lines();
    let status: u16 = lines
        .next()
        .expect("a status line")
        .split_whitespace()
        .nth(1)
        .expect("a status code")
        .parse()
        .expect("a numeric status");
    let headers =
        lines.filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))).collect();
    (status, headers, raw[split + 4..].to_vec())
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> &'a str {
    headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str()).unwrap_or("")
}

#[test]
fn the_fixture_serves_manifest_blobs_and_shutdown() {
    pod_testkit::need_loopback!();
    let fx = common::registry::Fixture::start();

    let manifest_path = format!("/v2/{}/manifests/{}", fx.repository, fx.tag);
    let (code, headers, body) = request(&fx.endpoint, "GET", &manifest_path);
    assert_eq!(code, 200, "manifest GET by tag");
    assert_eq!(body, fx.manifest, "the served manifest is the fixed one");
    assert_eq!(
        header(&headers, "Docker-Content-Digest"),
        fx.manifest_digest,
        "the digest header names the served bytes"
    );
    assert_eq!(header(&headers, "ETag"), format!("\"{}\"", fx.manifest_digest), "the ETag quotes the digest");
    let recomputed = pod_oci::digest::Digest::of(&body).to_string();
    assert_eq!(recomputed, fx.manifest_digest, "the digest rechecks");

    let by_digest = format!("/v2/{}/manifests/{}", fx.repository, fx.manifest_digest);
    let (code, _, body) = request(&fx.endpoint, "GET", &by_digest);
    assert_eq!(code, 200, "manifest GET by digest");
    assert_eq!(body, fx.manifest);

    for (digest, bytes) in [(&fx.config_digest, &fx.config), (&fx.layer_digest, &fx.layer)] {
        let blob_path = format!("/v2/{}/blobs/{digest}", fx.repository);
        let (code, headers, body) = request(&fx.endpoint, "GET", &blob_path);
        assert_eq!(code, 200, "blob GET {digest}");
        assert_eq!(&body, bytes, "the served blob is the fixed one");
        assert_eq!(header(&headers, "Docker-Content-Digest"), digest.as_str());
        let (code, _, head_body) = request(&fx.endpoint, "HEAD", &blob_path);
        assert_eq!(code, 200, "blob HEAD {digest}");
        assert!(head_body.is_empty(), "a HEAD carries no body");
    }

    let (code, _, _) = request(&fx.endpoint, "GET", "/v2/no/such/path");
    assert_eq!(code, 404, "unknown paths are refused");

    assert_eq!(
        fx.reference(),
        format!("{}/{}:{}", fx.endpoint, fx.repository, fx.tag),
        "the pullable reference names the endpoint, repository and tag"
    );

    fx.shutdown();
}
