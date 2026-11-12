//! The content of the test and its push: an index of linux/amd64 and
//! linux/arm64, each image one layer of one file, a config and a manifest.
//! The push is the OCI distribution API in plain requests of HTTP/1.1: a
//! POST and a PUT for each blob, a PUT for each manifest and for the index.
//! It is a second client beside the one under test, so it shares no code
//! with `pod-oci` but the digest of fixed bytes.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

pub const INDEX: &str = "application/vnd.oci.image.index.v1+json";
pub const MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
const CONFIG: &str = "application/vnd.oci.image.config.v1+json";
const LAYER: &str = "application/vnd.oci.image.layer.v1.tar";

fn digest(bytes: &[u8]) -> String {
    pod_oci::Digest::of(bytes).to_string()
}

/// One image of one platform.
pub struct Image {
    pub arch: &'static str,
    pub layer: Vec<u8>,
    pub layer_digest: String,
    pub config: Vec<u8>,
    pub config_digest: String,
    pub manifest: Vec<u8>,
    pub manifest_digest: String,
}

/// The index of the test and its images.
pub struct Content {
    pub repository: String,
    pub tag: String,
    pub index: Vec<u8>,
    pub index_digest: String,
    pub images: Vec<Image>,
}

/// A tar of one file `name` with `data`: a ustar header, the data in
/// blocks of 512 bytes, and two blocks of zeros.
fn tar(name: &str, data: &[u8]) -> Vec<u8> {
    let mut head = [0u8; 512];
    let put = |head: &mut [u8; 512], at: usize, field: &[u8]| head[at..at + field.len()].copy_from_slice(field);
    put(&mut head, 0, name.as_bytes());
    put(&mut head, 100, b"0000644\0");
    put(&mut head, 108, b"0000000\0");
    put(&mut head, 116, b"0000000\0");
    put(&mut head, 124, format!("{:011o}\0", data.len()).as_bytes());
    put(&mut head, 136, b"00000000000\0");
    put(&mut head, 148, b"        ");
    head[156] = b'0';
    put(&mut head, 257, b"ustar\0");
    put(&mut head, 263, b"00");
    let sum: u32 = head.iter().map(|b| u32::from(*b)).sum();
    put(&mut head, 148, format!("{sum:06o}\0 ").as_bytes());
    let mut out = head.to_vec();
    out.extend_from_slice(data);
    out.resize(out.len().div_ceil(512) * 512 + 1024, 0);
    out
}

fn image(arch: &'static str) -> Image {
    let layer = tar("etc/pod-oci-test", format!("pod-oci test image for linux/{arch}\n").as_bytes());
    let layer_digest = digest(&layer);
    let config = serde_json::json!({
        "architecture": arch,
        "os": "linux",
        "config": {},
        "rootfs": { "type": "layers", "diff_ids": [layer_digest] },
    });
    let config = serde_json::to_vec(&config).unwrap();
    let config_digest = digest(&config);
    let manifest = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": MANIFEST,
        "config": { "mediaType": CONFIG, "digest": config_digest, "size": config.len() },
        "layers": [{ "mediaType": LAYER, "digest": layer_digest, "size": layer.len() }],
    });
    let manifest = serde_json::to_vec(&manifest).unwrap();
    let manifest_digest = digest(&manifest);
    Image { arch, layer, layer_digest, config, config_digest, manifest, manifest_digest }
}

impl Content {
    pub fn new(repository: &str, tag: &str) -> Content {
        let images = vec![image("amd64"), image("arm64")];
        let entries: Vec<_> = images
            .iter()
            .map(|i| {
                serde_json::json!({
                    "mediaType": MANIFEST,
                    "digest": i.manifest_digest,
                    "size": i.manifest.len(),
                    "platform": { "architecture": i.arch, "os": "linux" },
                })
            })
            .collect();
        let index = serde_json::json!({ "schemaVersion": 2, "mediaType": INDEX, "manifests": entries });
        let index = serde_json::to_vec(&index).unwrap();
        let index_digest = digest(&index);
        Content { repository: repository.into(), tag: tag.into(), index, index_digest, images }
    }

    /// `127.0.0.1:PORT/REPOSITORY:TAG`.
    pub fn reference(&self, endpoint: &str) -> String {
        format!("{endpoint}/{}:{}", self.repository, self.tag)
    }

    /// Push each blob, each manifest and then the index under the tag.
    pub fn push(&self, to: &Pusher) {
        for image in &self.images {
            to.blob(&self.repository, &image.layer, &image.layer_digest);
            to.blob(&self.repository, &image.config, &image.config_digest);
            to.manifest(&self.repository, &image.manifest_digest, MANIFEST, &image.manifest);
        }
        to.manifest(&self.repository, &self.tag, INDEX, &self.index);
    }
}

/// One answer of the registry.
pub struct Answer {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Answer {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// The second client: one request for each connection, over TLS when
/// `tls` is given, with `authorization` on each request.
pub struct Pusher {
    pub port: u16,
    pub tls: Option<Arc<rustls::ClientConfig>>,
    pub authorization: Option<String>,
}

impl Pusher {
    /// One request. The answer is read to its `Content-Length`: the end of
    /// TLS with no close_notify is not a fault here.
    pub fn send(&self, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Answer {
        let tcp = TcpStream::connect((super::AT, self.port)).expect("the registry accepts");
        tcp.set_read_timeout(Some(pod_testkit::host::scaled(Duration::from_secs(20)))).unwrap();
        let mut head =
            format!("{method} {path} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\n", super::AT, self.port);
        for (k, v) in headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        if let Some(a) = &self.authorization {
            head.push_str(&format!("Authorization: {a}\r\n"));
        }
        head.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
        let mut stream: Box<dyn ReadWrite> = match &self.tls {
            Some(config) => {
                let name =
                    rustls_pki_types::ServerName::IpAddress(super::AT.parse::<std::net::IpAddr>().unwrap().into());
                let conn = rustls::ClientConnection::new(config.clone(), name).expect("a TLS client");
                Box::new(rustls::StreamOwned::new(conn, tcp))
            }
            None => Box::new(tcp),
        };
        stream.write_all(head.as_bytes()).expect("the request is sent");
        stream.write_all(body).expect("the body is sent");
        stream.flush().unwrap();
        read_answer(&mut *stream, &format!("{method} {path}"))
    }

    fn blob(&self, repository: &str, bytes: &[u8], digest: &str) {
        let start = self.send("POST", &format!("/v2/{repository}/blobs/uploads/"), &[], &[]);
        assert_eq!(start.status, 202, "POST of an upload: {}", String::from_utf8_lossy(&start.body));
        let location = start.header("Location").expect("an upload has a Location");
        // A Location can be a URL or a path; the request takes the path.
        let path = match location.split_once("://") {
            Some((_, rest)) => rest.find('/').map_or("/", |i| &rest[i..]).to_string(),
            None => location.to_string(),
        };
        let sep = if path.contains('?') { '&' } else { '?' };
        let path = format!("{path}{sep}digest={}", digest.replace(':', "%3A"));
        let headers = [("Content-Type", "application/octet-stream")];
        let done = self.send("PUT", &path, &headers, bytes);
        assert_eq!(done.status, 201, "PUT of {digest}: {}", String::from_utf8_lossy(&done.body));
        assert_eq!(done.header("Docker-Content-Digest"), Some(digest), "the registry stored {digest}");
    }

    fn manifest(&self, repository: &str, reference: &str, media_type: &str, bytes: &[u8]) {
        let path = format!("/v2/{repository}/manifests/{reference}");
        let done = self.send("PUT", &path, &[("Content-Type", media_type)], bytes);
        assert_eq!(done.status, 201, "PUT of {path}: {}", String::from_utf8_lossy(&done.body));
    }
}

trait ReadWrite: Read + Write {}
impl<T: Read + Write> ReadWrite for T {}

/// The head of an answer: where the body starts, the status, the
/// headers and the length of the body.
struct Head {
    start: usize,
    status: u16,
    headers: Vec<(String, String)>,
    length: usize,
}

fn read_answer(stream: &mut dyn ReadWrite, what: &str) -> Answer {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 16384];
    let mut head: Option<Head> = None;
    loop {
        if let Some(h) = &head
            && buf.len() >= h.start.saturating_add(h.length)
        {
            break;
        }
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => panic!("{what}: reading the answer: {e}"),
        }
        if head.is_none()
            && let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n")
        {
            let text = String::from_utf8_lossy(&buf[..end]).into_owned();
            let mut lines = text.split("\r\n");
            let status = lines.next().and_then(|l| l.split_whitespace().nth(1)).and_then(|s| s.parse().ok());
            let headers: Vec<(String, String)> = lines
                .filter_map(|l| l.split_once(':'))
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                .collect();
            // With no length, the body ends at the close.
            let length = headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("Content-Length"))
                .and_then(|(_, v)| v.parse().ok())
                .unwrap_or(usize::MAX);
            let status = status.unwrap_or_else(|| panic!("{what}: no status in {text:?}"));
            head = Some(Head { start: end + 4, status, headers, length });
        }
    }
    let h = head.unwrap_or_else(|| panic!("{what}: no answer"));
    let end = buf.len().min(h.start.saturating_add(h.length));
    Answer { status: h.status, headers: h.headers, body: buf[h.start..end].to_vec() }
}
