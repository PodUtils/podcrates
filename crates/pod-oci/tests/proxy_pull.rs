//! A pull through the proxy of the environment, and its `NO_PROXY` bypass,
//! against the registry fixture (`tests/common/registry.rs`).
//!
//! The proxy is a stand-in on loopback: an HTTP CONNECT proxy in `std` that
//! connects each tunnel to the fixture, whatever host the tunnel names, and
//! keeps the target of each CONNECT. `HTTPS_PROXY` names it.
//!
//! The caller reads its environment and gives it to `pod-proxy`: each test
//! gives its own variables, and no test changes the environment of the
//! process.
#![cfg(target_os = "linux")]

mod common;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use pod_oci::{Platform, Reference, pull};

/// The stand-in CONNECT proxy: its port, and the target of each CONNECT.
fn connect_proxy(fixture_port: u16) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback binds where tests run");
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for client in listener.incoming() {
            let Ok(client) = client else { return };
            let log = log.clone();
            std::thread::spawn(move || tunnel(client, fixture_port, &log));
        }
    });
    (port, seen)
}

/// One CONNECT: the head, the target kept, a tunnel to the fixture.
fn tunnel(mut client: TcpStream, fixture_port: u16, log: &Mutex<Vec<String>>) {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match client.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => return,
        }
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let mut words = head.split_whitespace();
    if words.next() != Some("CONNECT") {
        let _ = client.write_all(b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\n\r\n");
        return;
    }
    log.lock().unwrap().push(words.next().unwrap_or("").to_string());
    let Ok(upstream) = TcpStream::connect(("127.0.0.1", fixture_port)) else {
        let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n");
        return;
    };
    let _ = client.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
    let (mut up_read, mut up_write) = (upstream.try_clone().unwrap(), upstream);
    let (mut down_read, mut down_write) = (client.try_clone().unwrap(), client);
    let up = std::thread::spawn(move || {
        let _ = std::io::copy(&mut down_read, &mut up_write);
        let _ = up_write.shutdown(std::net::Shutdown::Write);
    });
    let _ = std::io::copy(&mut up_read, &mut down_write);
    let _ = down_write.shutdown(std::net::Shutdown::Write);
    let _ = up.join();
}

/// Pull the fixture `fx` by `endpoint` with `proxy_env`: the manifest and
/// each blob. Check that the bytes are the bytes that the fixture
/// published.
fn pull_through(fx: &common::registry::Fixture, endpoint: &str, proxy_env: &[(&str, &str)]) {
    let mut client = common::client::client(common::client::environment(proxy_env), &[endpoint]);
    let want = format!("http://{}", fx.reference().replacen(&fx.endpoint, endpoint, 1));
    assert!(want.ends_with(&format!("/{}:{}", fx.repository, fx.tag)), "{want}");
    let reference = Reference::parse(&want).unwrap();
    assert!(reference.plain_http && client.policy.permits_explicit_http(reference.endpoint()), "{want}");
    let resolved =
        pull::resolve(&mut client, &reference, &Platform::host()).unwrap_or_else(|e| panic!("pull {want}: {e}"));
    assert!(resolved.manifest_bytes == fx.manifest, "the manifest is not the published one");
    let mut fetched = 0;
    for (descriptor, published) in
        resolved.manifest.layers.iter().chain(std::iter::once(&resolved.manifest.config)).zip([&fx.layer, &fx.config])
    {
        let digest = descriptor.parsed_digest().unwrap();
        let got = client
            .blob(
                reference.endpoint(),
                &reference.repository,
                &digest,
                Some(descriptor.size),
                Vec::new(),
                &mut Vec::new(),
            )
            .unwrap_or_else(|e| panic!("blob {digest}: {e}"));
        assert!(got == *published, "{digest}: the fetched bytes are not the published ones");
        fetched += 1;
    }
    assert_eq!(fetched, 2);
}

/// `HTTPS_PROXY` names the stand-in: the manifest and each blob go through
/// it, by a name that only the proxy resolves.
#[test]
fn a_pull_goes_through_the_proxy_that_https_proxy_names() {
    pod_testkit::need_loopback!();
    let fx = common::registry::Fixture::start();
    let port = fx.endpoint.rsplit_once(':').unwrap().1.parse::<u16>().unwrap();
    let (proxy, seen) = connect_proxy(port);
    let endpoint = format!("registry.pod-oci.test:{port}");
    pull_through(&fx, &endpoint, &[("HTTPS_PROXY", &format!("http://127.0.0.1:{proxy}"))]);
    let seen = seen.lock().unwrap().clone();
    // The manifest and two blobs, each over HTTPS first, then over HTTP.
    assert!(seen.len() >= 3, "the proxy carried {seen:?}");
    assert!(seen.iter().all(|t| *t == endpoint), "the proxy carried {seen:?}");
    fx.shutdown();
}

/// `NO_PROXY` names the registry: the pull goes directly, and the proxy of
/// `HTTPS_PROXY` carries nothing. `0.0.0.0` gets to the fixture on this host
/// and is not a loopback name, so only `NO_PROXY` keeps it off the proxy.
#[test]
fn no_proxy_takes_a_pull_around_the_proxy() {
    pod_testkit::need_loopback!();
    let fx = common::registry::Fixture::start();
    let port = fx.endpoint.rsplit_once(':').unwrap().1.parse::<u16>().unwrap();
    let (proxy, seen) = connect_proxy(port);
    pull_through(
        &fx,
        &format!("0.0.0.0:{port}"),
        &[("HTTPS_PROXY", &format!("http://127.0.0.1:{proxy}")), ("NO_PROXY", "registry.internal,0.0.0.0")],
    );
    let seen = seen.lock().unwrap().clone();
    assert!(seen.is_empty(), "the proxy carried {seen:?}");
    fx.shutdown();
}
