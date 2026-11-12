//! The dispatch of a client of the proxy, with a dial of the test: SOCKS5,
//! HTTP CONNECT, an `http://` URL, the credentials, the refusals and the
//! limit (from podnet's `src/warp/proxy.rs`).

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_core::{BoxFuture, BoxedIo, Class, Connected, Dial, Failure, Report, Stage, Target};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWriteExt as _};

use super::udp::UdpClient;
use super::*;
use crate::rules::Refused;

fn opening(report: &Report) -> Opening<'_> {
    Opening { credentials: None, limit: REQUEST_LIMIT, report, control: None }
}

/// A dial that answers with an echo server and records the target.
fn echo_dial(seen: Arc<Mutex<Option<String>>>) -> impl FnOnce(Target) -> std::future::Ready<Result<BoxedIo, Failure>> {
    move |target| {
        *seen.lock().unwrap() = Some(target.to_string());
        let (local, mut remote) = tokio::io::duplex(1024);
        tokio::spawn(async move {
            let mut buf = [0u8; 64];
            while let Ok(n) = remote.read(&mut buf).await {
                if n == 0 || remote.write_all(&buf[..n]).await.is_err() {
                    break;
                }
            }
        });
        std::future::ready(Ok(Box::new(local) as BoxedIo))
    }
}

async fn serve<S>(stream: S, seen: Arc<Mutex<Option<String>>>, credentials: Option<Credentials>) -> Result<(), Failure>
where
    S: AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let report = Report::default();
    let opening = Opening { credentials: credentials.as_ref(), ..opening(&report) };
    serve_client(stream, echo_dial(seen), None, &opening).await
}

async fn read_head<R: AsyncRead + Unpin>(r: &mut R) -> String {
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if r.read_exact(&mut b).await.is_err() {
            break;
        }
        head.push(b[0]);
    }
    String::from_utf8_lossy(&head).into_owned()
}

/// PodUtils/podnet issue 178: a SOCKS4 client is refused at once.
#[tokio::test]
async fn a_socks4_client_is_refused_at_once() {
    let (mut client, server) = tokio::io::duplex(1024);
    let task = tokio::spawn(serve(server, Arc::default(), None));
    // SOCKS4 CONNECT to 1.2.3.4:80, user "".
    client.write_all(&[0x04, 0x01, 0x00, 0x50, 1, 2, 3, 4, 0x00]).await.unwrap();
    let mut reply = [0u8; 8];
    tokio::time::timeout(Duration::from_secs(2), client.read_exact(&mut reply)).await.expect("an answer").unwrap();
    assert_eq!(reply[1], 0x5b);
    let err = tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap().unwrap_err();
    assert!(err.chain().contains("SOCKS4"), "{}", err.chain());
}

/// PodUtils/podnet#278: the first sender (or the address of the request)
/// is the client; another sender gets no reply and sends nothing.
#[test]
fn the_udp_client_is_fixed_by_the_first_sender() {
    let a: SocketAddr = "127.0.0.1:4000".parse().unwrap();
    let b: SocketAddr = "127.0.0.1:5000".parse().unwrap();
    let mut any = UdpClient::default();
    assert!(any.accept(a));
    assert!(!any.accept(b), "another sender is dropped");
    assert!(any.accept(a));
    let named = |host: &str, port| Target::Addr(SocketAddr::new(host.parse().unwrap(), port));
    let mut by_port = UdpClient::of(&named("0.0.0.0", 5000));
    assert!(!by_port.accept(a), "the port of the request");
    assert!(by_port.accept(b));
    let mut by_ip = UdpClient::of(&named("127.0.0.2", 0));
    assert!(!by_ip.accept(a));
    assert!(by_ip.accept("127.0.0.2:9".parse().unwrap()));
}

/// PodUtils/podnet#278: the relay binds the local address of the control
/// connection.
#[test]
fn the_udp_relay_binds_the_address_of_the_control_connection() {
    assert_eq!(relay_bind_ip(None), IpAddr::from([127, 0, 0, 1]));
    assert_eq!(relay_bind_ip(Some("0.0.0.0".parse().unwrap())), IpAddr::from([127, 0, 0, 1]));
    assert_eq!(relay_bind_ip(Some("10.1.2.3".parse().unwrap())), "10.1.2.3".parse::<IpAddr>().unwrap());
}

/// A dial that reaches nothing.
struct Nowhere;

impl Dial for Nowhere {
    fn label(&self) -> String {
        "nowhere".into()
    }

    fn dial<'a>(&'a self, _t: &'a Target, _d: tokio::time::Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async { Err(Failure::new(Stage::Connect, Class::Unavailable, "nowhere")) })
    }
}

/// PodUtils/podnet issue 117: the limit refuses the client over it, and
/// frees the place of a client that ends.
#[test]
fn the_connection_limit() {
    let mut proxy = Proxy::new(Arc::new(Fixed(Arc::new(Nowhere))), Arc::new(Nowhere));
    proxy.max_connections = 2;
    let a = proxy.admit();
    let b = proxy.admit();
    assert!(a.is_some() && b.is_some());
    assert!(proxy.admit().is_none());
    assert_eq!(proxy.health().refused, 1);
    assert_eq!(proxy.health().clients, 2);
    drop(a);
    assert!(proxy.admit().is_some());
    assert_eq!(proxy.health().since_start, 3);
}

#[tokio::test]
async fn a_socks5_client_reaches_the_dialed_target() {
    let seen = Arc::new(Mutex::new(None));
    let (mut client, server) = tokio::io::duplex(1024);
    let task = tokio::spawn(serve(server, seen.clone(), None));
    // The greeting, then CONNECT example.com:443 by name.
    client.write_all(&[5, 1, 0]).await.unwrap();
    let mut reply = [0u8; 2];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply, [5, 0]);
    let mut request = vec![5, 1, 0, 3, 11];
    request.extend_from_slice(b"example.com");
    request.extend_from_slice(&443u16.to_be_bytes());
    client.write_all(&request).await.unwrap();
    let mut head = [0u8; 10];
    client.read_exact(&mut head).await.unwrap();
    assert_eq!(head[1], 0, "SOCKS5 success");
    client.write_all(b"ping").await.unwrap();
    let mut echo = [0u8; 4];
    client.read_exact(&mut echo).await.unwrap();
    assert_eq!(&echo, b"ping");
    assert_eq!(seen.lock().unwrap().as_deref(), Some("example.com:443"));
    drop(client);
    let _ = task.await;
}

/// PodUtils/podnet issues 132 and 136: a destination that a rule refuses
/// gets the SOCKS5 reply 2 (RFC 1928: not allowed by the ruleset), or the
/// HTTP status 403.
#[tokio::test]
async fn a_refused_destination_gets_its_own_reply() {
    let refuse = |target: Target| async move { Err::<BoxedIo, _>(Refused(target.to_string()).failure()) };
    let report = Report::default();
    let (mut client, server) = tokio::io::duplex(1024);
    let socks = async { serve_client(server, refuse, None, &opening(&report)).await };
    let peer = async {
        client.write_all(&[5, 1, 0]).await.unwrap();
        let mut reply = [0u8; 2];
        client.read_exact(&mut reply).await.unwrap();
        let mut request = vec![5, 1, 0, 1, 127, 0, 0, 1];
        request.extend_from_slice(&22u16.to_be_bytes());
        client.write_all(&request).await.unwrap();
        let mut head = [0u8; 2];
        client.read_exact(&mut head).await.unwrap();
        head
    };
    let (result, head) = tokio::join!(socks, peer);
    assert_eq!(head, [5, 2]);
    assert_eq!(result.unwrap_err().class, Class::Policy);
    let (mut client, server) = tokio::io::duplex(1024);
    let http = async { serve_client(server, refuse, None, &opening(&report)).await };
    let peer = async {
        client.write_all(b"CONNECT 127.0.0.1:22 HTTP/1.1\r\nHost: 127.0.0.1:22\r\n\r\n").await.unwrap();
        read_head(&mut client).await
    };
    let (result, head) = tokio::join!(http, peer);
    assert!(head.starts_with("HTTP/1.1 403"), "{head}");
    assert!(result.is_err());
}

/// PodUtils/podnet issue 146: with credentials, a SOCKS5 client needs the
/// right user and password; a client that offers no acceptable method gets
/// 0xff.
#[tokio::test]
async fn a_socks5_client_gives_its_credentials() {
    let creds = pod_proxy::Credentials::new("alice", "pw").unwrap();
    for (user, pass, ok) in [("alice", "pw", true), ("alice", "no", false)] {
        let (mut client, server) = tokio::io::duplex(1024);
        let task = tokio::spawn(serve(server, Arc::default(), Some(creds.clone())));
        client.write_all(&[5, 2, 0, 2]).await.unwrap();
        let mut reply = [0u8; 2];
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply, [5, 2]);
        let mut auth = vec![1, user.len() as u8];
        auth.extend_from_slice(user.as_bytes());
        auth.push(pass.len() as u8);
        auth.extend_from_slice(pass.as_bytes());
        client.write_all(&auth).await.unwrap();
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply, [1, if ok { 0 } else { 1 }], "{user}:{pass}");
        if ok {
            let mut request = vec![5, 1, 0, 1, 10, 0, 0, 1];
            request.extend_from_slice(&22u16.to_be_bytes());
            client.write_all(&request).await.unwrap();
            let mut head = [0u8; 10];
            client.read_exact(&mut head).await.unwrap();
            assert_eq!(head[1], 0);
        }
        drop(client);
        let result = task.await.unwrap();
        assert_eq!(result.is_ok(), ok, "{result:?}");
    }
    // No credentials offered to a server that needs them: 0xff.
    let (mut client, server) = tokio::io::duplex(1024);
    let task = tokio::spawn(serve(server, Arc::default(), Some(creds.clone())));
    client.write_all(&[5, 1, 0]).await.unwrap();
    let mut reply = [0u8; 2];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply, [5, 0xff]);
    assert!(task.await.unwrap().is_err());
    // Only user and password offered to a server with no credentials: 0xff.
    let (mut client, server) = tokio::io::duplex(1024);
    let task = tokio::spawn(serve(server, Arc::default(), None));
    client.write_all(&[5, 1, 2]).await.unwrap();
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply, [5, 0xff]);
    assert!(task.await.unwrap().is_err());
}

/// PodUtils/podnet issue 146: an HTTP client with no credentials gets 407;
/// with Basic it gets 200.
#[tokio::test]
async fn an_http_client_gives_its_credentials() {
    let creds = pod_proxy::Credentials::new("alice", "pw").unwrap();
    for (header, want) in [("", "407"), ("Proxy-Authorization: Basic YWxpY2U6cHc=\r\n", "200")] {
        let (mut client, server) = tokio::io::duplex(1024);
        let task = tokio::spawn(serve(server, Arc::default(), Some(creds.clone())));
        let request = format!("CONNECT 10.0.0.1:22 HTTP/1.1\r\nHost: 10.0.0.1:22\r\n{header}\r\n");
        client.write_all(request.as_bytes()).await.unwrap();
        let head = read_head(&mut client).await;
        assert!(head.starts_with(&format!("HTTP/1.1 {want}")), "{head}");
        if want == "407" {
            assert!(head.contains("Proxy-Authenticate: Basic"), "{head}");
        }
        drop(client);
        let _ = task.await;
    }
}

/// PodUtils/podnet issue 146: a client that sends nothing is closed after
/// the limit.
#[tokio::test(start_paused = true)]
async fn a_silent_client_is_closed() {
    let (_client, server) = tokio::io::duplex(1024);
    let err = serve(server, Arc::default(), None).await.unwrap_err();
    assert!(err.chain().contains("no request"), "{}", err.chain());
    assert_eq!(err.class, Class::Timeout);
}

#[tokio::test]
async fn an_http_connect_client_reaches_the_dialed_target() {
    let seen = Arc::new(Mutex::new(None));
    let (mut client, server) = tokio::io::duplex(1024);
    let task = tokio::spawn(serve(server, seen.clone(), None));
    client.write_all(b"CONNECT 10.1.2.3:22 HTTP/1.1\r\nHost: 10.1.2.3:22\r\n\r\nearly").await.unwrap();
    let head = read_head(&mut client).await;
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    // The bytes that came with the head go first.
    let mut echo = [0u8; 5];
    client.read_exact(&mut echo).await.unwrap();
    assert_eq!(&echo, b"early");
    client.write_all(b"x").await.unwrap();
    let mut echo = [0u8; 1];
    client.read_exact(&mut echo).await.unwrap();
    assert_eq!(&echo, b"x");
    assert_eq!(seen.lock().unwrap().as_deref(), Some("10.1.2.3:22"));
    drop(client);
    let _ = task.await;
}

/// PodUtils/podnet issue 78: a request for an http:// URL goes to the
/// origin in origin form.
#[tokio::test]
async fn an_http_url_request_reaches_the_origin_in_origin_form() {
    let seen = Arc::new(Mutex::new(None));
    let (mut client, server) = tokio::io::duplex(4096);
    let task = tokio::spawn(serve(server, seen.clone(), None));
    let request = "GET http://deb.example/dists HTTP/1.1\r\nHost: deb.example\r\nProxy-Connection: keep-alive\r\n\r\n";
    client.write_all(request.as_bytes()).await.unwrap();
    // The echo gives back what the origin got.
    let head = read_head(&mut client).await;
    assert!(head.starts_with("GET /dists HTTP/1.1\r\n"), "{head}");
    assert!(!head.to_ascii_lowercase().contains("proxy-"), "{head}");
    assert_eq!(seen.lock().unwrap().as_deref(), Some("deb.example:80"));
    drop(client);
    let _ = task.await;
}
