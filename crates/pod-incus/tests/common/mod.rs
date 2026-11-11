//! A stand-in incus on a Unix socket in the temporary directory, with no
//! `incusd`: an HTTP/1.1 server that answers the documented JSON of each
//! call of the crate (`doc/rest-api.md`, `doc/rest-api.yaml` and
//! `shared/api/*.go` of lxc/incus at the commit of the README), with the
//! model of a background operation (202, then its wait), the websockets of
//! exec, and an error answer on demand. Each request is kept for the
//! asserts of a test.
#![allow(dead_code)]

pub mod model;
pub mod store;
pub mod ws;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pod_incus::{Client, Settings};
use pod_proxy::http::{Body, Framing, Response};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

#[allow(unused_imports)]
pub use model::{Plan, State};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A socket path of this test process that no other test takes.
pub fn socket_path(name: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let path = pod_testkit::host::socket_path(&format!("pod-incus-{name}-{}-{n}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    path
}

/// One request as the stand-in read it.
#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub query: BTreeMap<String, String>,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers.iter().find(|(n, _)| *n == name).map(|(_, v)| v.as_str())
    }
}

/// The shared parts of the stand-in.
pub struct Shared {
    pub state: Mutex<State>,
    /// Each change of an operation.
    pub changed: Notify,
    /// The websockets of each operation of exec.
    pub sockets: Mutex<ws::Sockets>,
    /// Each request that came.
    pub arrived: Notify,
}

/// A stand-in incus that serves until it is dropped.
pub struct Stand {
    pub socket: PathBuf,
    pub shared: Arc<Shared>,
    task: JoinHandle<()>,
}

impl Stand {
    pub fn start() -> Stand {
        let socket = socket_path("stand");
        let listener = UnixListener::bind(&socket).expect("bind the socket of the stand-in");
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            changed: Notify::new(),
            sockets: Mutex::new(ws::Sockets::default()),
            arrived: Notify::new(),
        });
        let serving = shared.clone();
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(serve(serving.clone(), stream));
            }
        });
        Stand { socket, shared, task }
    }

    pub fn settings(&self) -> Settings {
        let mut settings = Settings::new(&self.socket);
        settings.user_agent = "pod-incus-test".into();
        settings
    }

    pub fn client(&self) -> Client {
        Client::new(self.settings())
    }

    /// Wait until a request of `method` whose path starts with `prefix`
    /// came, 10 s at most.
    pub async fn arrival(&self, method: &str, prefix: &str) {
        let wait = async {
            loop {
                let arrived = self.shared.arrived.notified();
                if !self.seen(method, prefix).is_empty() {
                    return;
                }
                arrived.await;
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(10), wait).await.expect("the request came");
    }

    /// End the operation `id` with `metadata`, and tell its waiters.
    pub fn end_op(&self, id: &str, metadata: serde_json::Value) {
        self.state().end_op(id, metadata);
        self.shared.changed.notify_waiters();
    }

    pub fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.shared.state.lock().unwrap()
    }

    /// The requests of `method` whose path starts with `prefix`.
    pub fn seen(&self, method: &str, prefix: &str) -> Vec<Seen> {
        self.state().requests.iter().filter(|s| s.method == method && s.path.starts_with(prefix)).cloned().collect()
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        self.task.abort();
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// One connection: one request, one answer, or the websocket of an
/// operation.
async fn serve(shared: Arc<Shared>, mut stream: UnixStream) {
    let Ok((request, rest)) = pod_proxy::http::read_request(&mut stream, Vec::new()).await else { return };
    let (path, query) = split_target(&request.target);
    if request.method == "GET" && path.starts_with("/1.0/operations/") && path.ends_with("/websocket") {
        ws::serve(shared, stream, request, rest, path, query).await;
        return;
    }
    let framing = if request.header("transfer-encoding").is_some_and(|v| v.contains("chunked")) {
        Framing::Chunked
    } else {
        Framing::Length(request.header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0))
    };
    let Ok(reader) = Body::new(&mut stream, rest, framing, Some(1 << 30)) else { return };
    let Ok(body) = reader.to_end().await else { return };
    let seen = Seen { method: request.method.clone(), path, query, headers: request.headers.clone(), body };
    let answer = model::answer(&shared, seen).await;
    let _ = stream.write_all(&answer).await;
    let _ = stream.shutdown().await;
    // Read to the end so that the close is not a reset.
    let mut sink = [0u8; 64];
    let _ = stream.read(&mut sink).await;
}

/// The path and the decoded query of a request target.
pub fn split_target(target: &str) -> (String, BTreeMap<String, String>) {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let query = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(k), decode(v))
        })
        .collect();
    (path.to_string(), query)
}

/// A percent-decoded text (`+` stays).
pub fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok());
        if bytes[i] == b'%'
            && let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// An answer: a status, the headers, a body.
pub fn http(status: u16, headers: &[(&str, String)], body: &[u8]) -> Vec<u8> {
    let mut head = format!("HTTP/1.1 {status} {}\r\nContent-Length: {}\r\n", reason(status), body.len());
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    let mut out = head.into_bytes();
    out.extend_from_slice(body);
    out
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Other",
    }
}

/// The body of a sync answer with `metadata`.
pub fn sync_body(metadata: serde_json::Value) -> String {
    serde_json::json!({"type": "sync", "status": "Success", "status_code": 200, "operation": "",
        "error_code": 0, "error": "", "metadata": metadata})
    .to_string()
}

/// A sync answer with `metadata`.
pub fn sync(metadata: serde_json::Value) -> Vec<u8> {
    http(200, &[("Content-Type", "application/json".into())], sync_body(metadata).as_bytes())
}

/// An error answer of `status` with `error`.
pub fn error(status: u16, error: &str) -> Vec<u8> {
    let body = serde_json::json!({"type": "error", "status": "", "status_code": 0, "operation": "",
        "error_code": status, "error": error, "metadata": null});
    http(status, &[("Content-Type", "application/json".into())], body.to_string().as_bytes())
}

/// The answer of a response for a test that reads it apart.
pub fn parse(bytes: &[u8]) -> Response {
    let text = String::from_utf8_lossy(bytes);
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    let (status, reason, headers) = pod_proxy::http::parse_head(head).unwrap();
    Response { status, reason, headers, body: body.as_bytes().to_vec(), url: String::new() }
}
