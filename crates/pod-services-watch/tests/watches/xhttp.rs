//! `proxy-banner` over XHTTP: stand-in nodes of the mode `packet-up`, over
//! HTTP/1.1 and over HTTP/2, both over TLS. A stand-in checks each request
//! as Xray's server does (`transport/internet/splithttp/hub.go`): the host,
//! the path and its query, the padding in the query of the `Referer`, the
//! session id, and a sequence number on each upload. The download waits for
//! the uploads of its session from sequence 0 in order, checks that they
//! hold the VLESS request, and then sends the VLESS answer and a banner.

use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use bytes::Bytes;
use pod_testkit::stand::{Io, Stand};

use crate::common::{NAME, file, one, server_config, text, wrap_with};
use crate::ssh::{BANNER, node};

/// The UUID of the links.
const UUID: &str = "00000000-0000-4000-8000-000000000003";
/// The path that the stand-ins serve.
const PATH: &str = "/xh/";
/// The longest wait of a download for its uploads.
const WAIT: Duration = Duration::from_secs(10);

/// What a request asks.
enum Ask {
    Download(String),
    Upload(String, u64),
}

/// What the download of a session sends next.
enum Next {
    Wait,
    Refused,
    Send(Vec<u8>),
}

/// One stand-in node: the query that each request carries, and what the
/// node saw.
struct Node {
    query: Option<&'static str>,
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Default)]
struct State {
    /// Each upload, by its session and its sequence number.
    uploads: BTreeMap<(String, u64), Vec<u8>>,
    /// Why the node refused a request.
    fault: Option<String>,
}

impl Node {
    fn new(query: Option<&'static str>) -> Arc<Node> {
        Arc::new(Node { query, state: Mutex::default(), changed: Condvar::new() })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The check of one request; a refusal is kept, and ends the download.
    fn ask(
        &self,
        method: &str,
        path: &str,
        query: Option<&str>,
        host: Option<&str>,
        referer: Option<&str>,
    ) -> Option<Ask> {
        match check(method, path, query, self.query, host, referer) {
            Ok(ask) => Some(ask),
            Err(why) => {
                eprintln!("the stand-in refused {method} {path}: {why}");
                self.lock().fault.get_or_insert(why);
                self.changed.notify_all();
                None
            }
        }
    }

    /// Keep an upload; false for a sequence number that came before.
    fn upload(&self, session: String, seq: u64, bytes: Vec<u8>) -> bool {
        let mut state = self.lock();
        let again = state.uploads.insert((session, seq), bytes).is_some();
        if again {
            state.fault.get_or_insert(format!("the sequence number {seq} again"));
        }
        drop(state);
        self.changed.notify_all();
        !again
    }

    /// The VLESS answer and the banner, once the uploads of `session` from
    /// sequence 0, in order, hold the VLESS request to github.com:22.
    fn next(&self, session: &str) -> Next {
        let state = self.lock();
        if state.fault.is_some() {
            return Next::Refused;
        }
        let mut bytes = Vec::new();
        let mut seq = 0;
        while let Some(upload) = state.uploads.get(&(session.to_string(), seq)) {
            bytes.extend_from_slice(upload);
            seq += 1;
        }
        let want = vless_request();
        if bytes.len() < want.len() {
            return Next::Wait;
        }
        if bytes != want {
            eprintln!("the stand-in got the uploads {bytes:?}");
            return Next::Refused;
        }
        Next::Send([&[0u8, 0][..], BANNER].concat())
    }

    /// [`Node::next`] that waits on a thread, at most [`WAIT`].
    fn wait(&self, session: &str) -> Next {
        let end = Instant::now() + WAIT;
        loop {
            let next = self.next(session);
            if !matches!(next, Next::Wait) || Instant::now() > end {
                return next;
            }
            let state = self.lock();
            let _ = self.changed.wait_timeout(state, Duration::from_millis(50));
        }
    }
}

/// The check of Xray's server of one request: the host, the query of the
/// path, the padding (`x_padding` in the query of the `Referer`, 100 to
/// 1000 bytes), the session id after the path, and the sequence number of
/// an upload after the session id.
fn check(
    method: &str,
    path: &str,
    query: Option<&str>,
    want: Option<&str>,
    host: Option<&str>,
    referer: Option<&str>,
) -> Result<Ask, String> {
    if host != Some(NAME) {
        return Err(format!("the host {host:?}"));
    }
    if query != want {
        return Err(format!("the query {query:?}"));
    }
    let referer = referer.ok_or("no Referer")?;
    let padding =
        referer.strip_prefix(&format!("https://{NAME}{PATH}?x_padding=")).ok_or(format!("the Referer {referer:?}"))?;
    if !(100..=1000).contains(&padding.len()) || padding.bytes().any(|b| b != b'X') {
        return Err(format!("a padding of {} bytes", padding.len()));
    }
    let rest = path.strip_prefix(PATH).ok_or(format!("the path {path:?}"))?;
    let (session, seq) = match rest.split_once('/') {
        Some((session, seq)) => (session, Some(seq)),
        None => (rest, None),
    };
    if !is_uuid(session) {
        return Err(format!("the session id {session:?}"));
    }
    match (method, seq) {
        ("GET", None) => Ok(Ask::Download(session.to_string())),
        ("POST", Some(seq)) => {
            seq.parse().map(|seq| Ask::Upload(session.to_string(), seq)).map_err(|_| format!("the sequence {seq:?}"))
        }
        ("POST", None) => Err("an upload with no sequence number".into()),
        _ => Err("another method".into()),
    }
}

/// A UUID of version 4 in its text form (`GenerateSessionID` of Xray).
fn is_uuid(id: &str) -> bool {
    let parts: Vec<&str> = id.split('-').collect();
    parts.iter().map(|p| p.len()).eq([8, 4, 4, 4, 12])
        && id.bytes().all(|b| b == b'-' || b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && parts[2].starts_with('4')
}

/// The VLESS request of the links: version 0, the UUID, no addon, a
/// CONNECT of TCP to github.com:22.
fn vless_request() -> Vec<u8> {
    let hex = UUID.replace('-', "");
    let mut out = vec![0u8];
    out.extend((0..16).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex")));
    out.extend_from_slice(&[0, 1, 0, 22, 2, 10]);
    out.extend_from_slice(b"github.com");
    out
}

/// An XHTTP node on HTTP/1.1 over TLS, with one request on each connection.
fn h1_node(node: Arc<Node>) -> Stand {
    Stand::raw_over(move |io| serve_h1(io, &node), wrap_with(server_config(&[b"http/1.1"]))).expect("a stand-in")
}

fn serve_h1(io: &mut dyn Io, node: &Node) {
    let Some((head, body)) = read_request(io) else { return };
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split(' ');
    let (method, target) = (first.next().unwrap_or_default(), first.next().unwrap_or_default());
    let headers: Vec<(String, &str)> =
        lines.filter_map(|l| l.split_once(':')).map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim())).collect();
    let header = |name: &str| headers.iter().find(|(n, _)| n == name).map(|(_, v)| *v);
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (target, None),
    };
    match node.ask(method, path, query, header("host"), header("referer")) {
        None => {
            let _ = io.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n");
        }
        Some(Ask::Upload(session, seq)) => {
            let status = if node.upload(session, seq, body) { "200 OK" } else { "400 Bad Request" };
            let _ = io.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\n\r\n").as_bytes());
        }
        Some(Ask::Download(session)) => {
            let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n";
            if io.write_all(head.as_bytes()).and_then(|_| io.flush()).is_err() {
                return;
            }
            if let Next::Send(bytes) = node.wait(&session) {
                let mut chunk = format!("{:x}\r\n", bytes.len()).into_bytes();
                chunk.extend_from_slice(&bytes);
                chunk.extend_from_slice(b"\r\n");
                let _ = io.write_all(&chunk).and_then(|_| io.flush());
            }
            let _ = io.write_all(b"0\r\n\r\n");
        }
    }
}

/// The head of one request, and its body by its `Content-Length`.
fn read_request(io: &mut dyn Io) -> Option<(String, Vec<u8>)> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        match io.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&bytes[..end]).into_owned();
    let length: usize = head
        .split("\r\n")
        .filter_map(|l| l.split_once(':'))
        .find(|(n, _)| n.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0);
    let mut body = bytes.split_off(end + 4);
    while body.len() < length {
        match io.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    Some((head, body))
}

/// An XHTTP node on HTTP/2 over TLS, on a thread of its own: its port.
fn h2_node(node: Arc<Node>) -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    let port = listener.local_addr().expect("an address").port();
    listener.set_nonblocking(true).expect("a listener that does not block");
    let acceptor = tokio_rustls::TlsAcceptor::from(server_config(&[b"h2"]));
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("a listener");
            while let Ok((tcp, _)) = listener.accept().await {
                let (acceptor, node) = (acceptor.clone(), node.clone());
                tokio::spawn(async move {
                    let Ok(tls) = acceptor.accept(tcp).await else { return };
                    let Ok(mut connection) = h2::server::handshake(tls).await else { return };
                    while let Some(Ok((request, respond))) = connection.accept().await {
                        tokio::spawn(serve_h2(request, respond, node.clone()));
                    }
                });
            }
        });
    });
    port
}

async fn serve_h2(
    request: http::Request<h2::RecvStream>,
    mut respond: h2::server::SendResponse<Bytes>,
    node: Arc<Node>,
) {
    let uri = request.uri().clone();
    let referer = request.headers().get("referer").and_then(|v| v.to_str().ok()).map(String::from);
    let asked = node.ask(
        request.method().as_str(),
        uri.path(),
        uri.query(),
        uri.authority().map(|a| a.as_str()),
        referer.as_deref(),
    );
    let answer = |status: u16| http::Response::builder().status(status).body(()).expect("an answer");
    match asked {
        None => {
            let _ = respond.send_response(answer(400), true);
        }
        Some(Ask::Upload(session, seq)) => {
            let mut body = request.into_body();
            let mut bytes = Vec::new();
            while let Some(Ok(chunk)) = body.data().await {
                let _ = body.flow_control().release_capacity(chunk.len());
                bytes.extend_from_slice(&chunk);
            }
            let status = if node.upload(session, seq, bytes) { 200 } else { 400 };
            let _ = respond.send_response(answer(status), true);
        }
        Some(Ask::Download(session)) => {
            let Ok(mut send) = respond.send_response(answer(200), false) else { return };
            let end = tokio::time::Instant::now() + WAIT;
            loop {
                match node.next(&session) {
                    Next::Wait if tokio::time::Instant::now() < end => {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                    Next::Send(bytes) => {
                        let _ = send.send_data(Bytes::from(bytes), false);
                        break;
                    }
                    _ => break,
                }
            }
            let _ = send.send_data(Bytes::new(), true);
        }
    }
}

/// A VLESS link over XHTTP with TLS to the stand-in at `port`, with `more`.
fn link(port: u16, more: &str) -> String {
    format!("vless://{UUID}@{NAME}:{port}?type=xhttp&security=tls&sni={NAME}&encryption=none&{more}")
}

#[tokio::test]
async fn proxy_banner_speaks_xhttp_packet_up_over_http1() {
    pod_testkit::need_loopback!();
    // The path of the link has a query of its own and no `/` at its end,
    // and the list writes it percent-encoded.
    let stand = h1_node(Node::new(Some("ed=2048")));
    let report =
        one(&file("x1", &node(&link(stand.port(), "mode=packet-up&alpn=http%2F1.1&path=%2Fxh%3Fed%3D2048")))).await;
    let seen = text(&report);
    assert!(report.holds() && seen.contains("over XHTTP packet-up on HTTP/1.1"), "{seen}");
    assert!(seen.contains("SSH-2.0-stand"), "{seen}");
    // A path that the node does not serve: the node refuses each request.
    let stand = h1_node(Node::new(None));
    let report = one(&file("x2", &node(&link(stand.port(), "mode=packet-up&alpn=http/1.1&path=/other/")))).await;
    assert!(text(&report).contains("answered HTTP 400"), "{}", text(&report));
}

#[tokio::test]
async fn proxy_banner_speaks_xhttp_packet_up_over_http2() {
    pod_testkit::need_loopback!();
    // `auto` and no ALPN, as the link of node-xhttp-193-160-208-240; then
    // `packet-up` and two ALPN protocols, as a link of a public list.
    for (id, more) in [("x3", "mode=auto&path=/xh/"), ("x4", "mode=packet-up&alpn=h2,http/1.1&path=%2Fxh%2F")] {
        let port = h2_node(Node::new(None));
        let report = one(&file(id, &node(&link(port, more)))).await;
        let seen = text(&report);
        assert!(report.holds() && seen.contains("over XHTTP packet-up on HTTP/2"), "{seen}");
        assert!(seen.contains("SSH-2.0-stand"), "{seen}");
    }
}

#[tokio::test]
async fn proxy_banner_skips_an_xhttp_form_that_it_does_not_speak() {
    pod_testkit::need_loopback!();
    for (id, more, why) in [
        ("x5", "mode=stream-one", "XHTTP mode stream-one"),
        ("x6", "mode=stream-up", "XHTTP mode stream-up"),
        ("x7", "mode=auto&alpn=h3", "XHTTP over HTTP/3"),
        ("x8", "mode=auto&extra=%7B%7D", "XHTTP with extra settings"),
        ("x9", "mode=auto&flow=xtls-rprx-vision", "VLESS flow xtls-rprx-vision"),
    ] {
        let report = one(&file(id, &node(&link(1, more)))).await;
        let seen = text(&report);
        assert!(report.holds() && seen.starts_with(&format!("skipped {id} proxy-banner")), "{seen}");
        assert!(seen.contains(why), "{seen}");
    }
}
