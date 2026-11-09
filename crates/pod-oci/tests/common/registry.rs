//! A minimal OCI registry for integration tests: what podvm's client
//! actually requests and nothing more (T-R003: do not build a full OCI
//! registry).
//!
//! Serves, over plain HTTP on a loopback ephemeral port: one manifest GET
//! (with `Docker-Content-Digest` and `ETag`), one config and one layer blob
//! GET, and blob HEAD. Anything else is 404. Plain HTTP only: callers name
//! the endpoint insecure through `Policy::with_insecure`, the same deliberate
//! act the CLI's `--insecure-registry` is.
//!
//! `std` only, no new dependency: the bloat rulings hold the dependency set,
//! and a test helper is the wrong place to widen it. Connections are handled
//! on one thread each, because pulls fetch blobs concurrently. Shutdown
//! stops the accept loop and joins it, so no test leaks a listener.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// One served repository: a single tag over one config and one layer.
pub struct Fixture {
    /// `127.0.0.1:PORT`, for `Reference` and `Policy::with_insecure`.
    pub endpoint: String,
    pub repository: String,
    pub tag: String,
    pub manifest: Vec<u8>,
    /// `sha256:<hex>` of the manifest bytes, as served and as computed.
    pub manifest_digest: String,
    pub config: Vec<u8>,
    pub config_digest: String,
    pub layer: Vec<u8>,
    pub layer_digest: String,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

struct Data {
    repository: String,
    tag: String,
    manifest: Vec<u8>,
    manifest_digest: String,
    blobs: HashMap<String, Vec<u8>>,
}

/// The digest of fixed bytes, through the crate under test. A second
/// SHA-256 here would be a second implementation of a digest the tree
/// already owns, and shelling out is not hermetic. The bytes stay fixed,
/// so a broken `Digest::of` still yields a stable mismatch downstream.
fn fixture_digest(data: &[u8]) -> String {
    podvm_image::digest::Digest::of(data).to_string()
}

impl Fixture {
    /// Build the blobs and start serving them on loopback.
    pub fn start() -> Fixture {
        let layer: Vec<u8> = (0u32..2048).map(|i| (i % 251) as u8).collect();
        let layer_digest = fixture_digest(&layer);
        let config = br#"{"architecture":"amd64","os":"linux","rootfs":{"type":"layers","diff_ids":[]},"history":[{}]}"#;
        let config_digest = fixture_digest(config);
        let manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {
                "mediaType": "application/vnd.oci.image.config.v1+json",
                "digest": config_digest,
                "size": config.len(),
            },
            "layers": [{
                "mediaType": "application/vnd.oci.image.layer.v1.tar",
                "digest": layer_digest,
                "size": layer.len(),
            }],
        });
        let manifest = serde_json::to_vec(&manifest).unwrap();
        let manifest_digest = fixture_digest(&manifest);
        let repository = "library/fixture".to_string();
        let tag = "test".to_string();
        let mut blobs = HashMap::new();
        blobs.insert(config_digest.clone(), config.to_vec());
        blobs.insert(layer_digest.clone(), layer.clone());
        let data = Arc::new(Data {
            repository: repository.clone(),
            tag: tag.clone(),
            manifest: manifest.clone(),
            manifest_digest: manifest_digest.clone(),
            blobs,
        });
        let listener = TcpListener::bind("127.0.0.1:0").expect("the fixture binds loopback");
        listener
            .set_nonblocking(true)
            .expect("the accept loop polls");
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread = std::thread::spawn(move || {
            accept_loop(listener, data, thread_stop);
        });
        Fixture {
            endpoint: format!("127.0.0.1:{port}"),
            repository,
            tag,
            manifest,
            manifest_digest,
            config: config.to_vec(),
            config_digest,
            layer,
            layer_digest,
            stop,
            thread: Some(thread),
        }
    }

    /// The pullable reference, `127.0.0.1:PORT/library/fixture:test`.
    pub fn reference(&self) -> String {
        format!("{}/{}:{}", self.endpoint, self.repository, self.tag)
    }

    /// Stop accepting and join the accept loop. In-flight handlers are
    /// short-lived and detached; the joined loop is what held the port.
    pub fn shutdown(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn accept_loop(listener: TcpListener, data: Arc<Data>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let data = data.clone();
                std::thread::spawn(move || handle(stream, &data));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(_) => break,
        }
    }
}

fn handle(mut stream: std::net::TcpStream, data: &Data) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
    let mut buf = [0u8; 16384];
    let mut head = 0usize;
    while head < buf.len() {
        match stream.read(&mut buf[head..]) {
            Ok(0) => return,
            Ok(n) => {
                head += n;
                if buf[..head].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => return,
        }
    }
    let text = String::from_utf8_lossy(&buf[..head]);
    let mut parts = text.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    let (code, reason, headers, body) = route(method, path, data);
    let mut out = format!("HTTP/1.1 {code} {reason}\r\nConnection: close\r\n");
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    let _ = stream.write_all(out.as_bytes());
    if method != "HEAD" {
        let _ = stream.write_all(&body);
    }
}

fn route(
    method: &str,
    path: &str,
    data: &Data,
) -> (u16, &'static str, Vec<(&'static str, String)>, Vec<u8>) {
    let not_found = || {
        (
            404u16,
            "Not Found",
            Vec::new(),
            b"no such path in the fixture".to_vec(),
        )
    };
    let manifest_path = format!("/v2/{}/manifests/", data.repository);
    if method == "GET" && path.starts_with(&manifest_path) {
        let selector = &path[manifest_path.len()..];
        if selector == data.tag || selector == data.manifest_digest {
            return (
                200,
                "OK",
                vec![
                    (
                        "Content-Type",
                        "application/vnd.oci.image.manifest.v1+json".to_string(),
                    ),
                    ("Docker-Content-Digest", data.manifest_digest.clone()),
                    ("ETag", format!("\"{}\"", data.manifest_digest)),
                ],
                data.manifest.clone(),
            );
        }
        return not_found();
    }
    let blob_path = format!("/v2/{}/blobs/", data.repository);
    if (method == "GET" || method == "HEAD") && path.starts_with(&blob_path) {
        let digest = &path[blob_path.len()..];
        if let Some(bytes) = data.blobs.get(digest) {
            return (
                200,
                "OK",
                vec![
                    ("Content-Type", "application/octet-stream".to_string()),
                    ("Docker-Content-Digest", digest.to_string()),
                    ("ETag", format!("\"{digest}\"")),
                ],
                bytes.clone(),
            );
        }
        return not_found();
    }
    not_found()
}
