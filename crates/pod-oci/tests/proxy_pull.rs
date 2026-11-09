//! A pull through the proxy of the environment, and its `NO_PROXY` bypass,
//! against the T-1531 fixture (`tests/common/registry.rs`).
//!
//! ⭐ The proxy is a stand-in on loopback: an HTTP CONNECT proxy in `std`
//! that connects each tunnel to the fixture, whatever host the tunnel names,
//! and keeps the target of each CONNECT. `HTTPS_PROXY` names it.
//!
//! ⚠ Each pull runs in a child process of this test binary, with the proxy
//! variables set for the child alone: the pull reads the environment of its
//! process, as the CLI does, and a test does not change the environment of
//! the process that runs the other tests. A `child_*` test does nothing
//! unless `PODVM_PROXY_CHILD` is set.

mod common;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use podvm_image::{platform::Platform, pull::pull, store::Store, transport::Policy};

const CHILD: &str = "PODVM_PROXY_CHILD";
const PROXY_VARS: [&str; 6] = [
    "https_proxy",
    "HTTPS_PROXY",
    "all_proxy",
    "ALL_PROXY",
    "no_proxy",
    "NO_PROXY",
];

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
    log.lock()
        .unwrap()
        .push(words.next().unwrap_or("").to_string());
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

/// Run the child test `name` with `env`, and the proxy variables of this
/// process removed; panic with its output when it fails.
fn run_child(name: &str, env: &[(&str, String)]) {
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", name, "--nocapture", "--test-threads=1"]);
    for var in PROXY_VARS {
        cmd.env_remove(var);
    }
    cmd.env(CHILD, "1");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("the test binary runs again");
    assert!(
        out.status.success(),
        "{name} failed: {}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// In the child: pull `PODVM_PROXY_REFERENCE` into the store at
/// `PODVM_PROXY_STORE`, with its endpoint named insecure.
fn child_pull() {
    let want = std::env::var("PODVM_PROXY_REFERENCE").unwrap();
    let store = Store::open(std::env::var("PODVM_PROXY_STORE").unwrap()).expect("open the store");
    let endpoint = want
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap();
    let policy = Policy::with_insecure(&[endpoint]);
    let mut out = Vec::new();
    let pulled = pull(&store, &want, &Platform::host(), &policy, &mut out)
        .unwrap_or_else(|e| panic!("pull {want}: {e}"));
    assert_eq!(pulled.fetched.len(), 2, "fetched {:?}", pulled.fetched);
}

/// Pull the fixture `fx` by `endpoint` in a child with `proxy_env`; check
/// that the store holds the bytes that the fixture published.
fn pull_in_child(fx: &common::registry::Fixture, endpoint: &str, proxy_env: &[(&str, String)]) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("podvm-image-proxy-{n}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let want = format!(
        "http://{}",
        fx.reference().replacen(&fx.endpoint, endpoint, 1)
    );
    assert!(
        want.ends_with(&format!("/{}:{}", fx.repository, fx.tag)),
        "{want}"
    );
    let mut env = proxy_env.to_vec();
    env.push(("PODVM_PROXY_REFERENCE", want));
    env.push(("PODVM_PROXY_STORE", dir.display().to_string()));
    run_child("child_pull_through_the_environment", &env);
    let store = Store::open(&dir).expect("open the store");
    for (digest, published) in [
        (&fx.manifest_digest, &fx.manifest),
        (&fx.config_digest, &fx.config),
        (&fx.layer_digest, &fx.layer),
    ] {
        let path = store.blob_path(&podvm_image::digest::Digest::parse(digest).unwrap());
        let stored = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(
            stored == *published,
            "{digest}: the stored bytes are not the published ones"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn child_pull_through_the_environment() {
    if std::env::var_os(CHILD).is_some() {
        child_pull();
    }
}

/// `HTTPS_PROXY` names the stand-in: the manifest and each blob go through
/// it, by a name that only the proxy resolves.
#[test]
fn a_pull_goes_through_the_proxy_that_https_proxy_names() {
    if std::env::var_os(CHILD).is_some() {
        return;
    }
    let fx = common::registry::Fixture::start();
    let port = fx
        .endpoint
        .rsplit_once(':')
        .unwrap()
        .1
        .parse::<u16>()
        .unwrap();
    let (proxy, seen) = connect_proxy(port);
    let endpoint = format!("registry.podvm.test:{port}");
    pull_in_child(
        &fx,
        &endpoint,
        &[("HTTPS_PROXY", format!("http://127.0.0.1:{proxy}"))],
    );
    let seen = seen.lock().unwrap().clone();
    // The manifest and two blobs, each over HTTPS first and then over HTTP.
    assert!(seen.len() >= 3, "the proxy carried {seen:?}");
    assert!(
        seen.iter().all(|t| *t == endpoint),
        "the proxy carried {seen:?}"
    );
    fx.shutdown();
}

/// `NO_PROXY` names the registry: the pull goes directly, and the proxy of
/// `HTTPS_PROXY` carries nothing. ⚠ `0.0.0.0` reaches the fixture on this
/// host and is not a loopback name, so only `NO_PROXY` keeps it off the proxy.
#[test]
fn no_proxy_takes_a_pull_around_the_proxy() {
    if std::env::var_os(CHILD).is_some() {
        return;
    }
    let fx = common::registry::Fixture::start();
    let port = fx
        .endpoint
        .rsplit_once(':')
        .unwrap()
        .1
        .parse::<u16>()
        .unwrap();
    let (proxy, seen) = connect_proxy(port);
    pull_in_child(
        &fx,
        &format!("0.0.0.0:{port}"),
        &[
            ("HTTPS_PROXY", format!("http://127.0.0.1:{proxy}")),
            ("NO_PROXY", "registry.internal,0.0.0.0".to_string()),
        ],
    );
    let seen = seen.lock().unwrap().clone();
    assert!(seen.is_empty(), "the proxy carried {seen:?}");
    fx.shutdown();
}
