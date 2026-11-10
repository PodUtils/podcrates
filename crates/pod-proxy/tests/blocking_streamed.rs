//! The streamed answer of the blocking client (feature `blocking`), with no
//! runtime of the caller: a body larger than the bound of a buffered one,
//! by length and by chunks; a body cut before its length; a peer that stops
//! sending; through a CONNECT proxy, and directly where `NO_PROXY` names the
//! host; a redirect that the request asks to follow.

mod closed;
#[allow(dead_code)]
mod served;

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use pod_core::Stage;
use pod_proxy::blocking::{Client, Dialer, Streamed};
use pod_proxy::client::Request;
use pod_proxy::resolve::System;
use pod_proxy::{Environment, Proxy, ProxyChoice, Rules, Settings};
use served::{Answer, LARGE, NAME, Served, body, pins, serve, sha256};

fn client(served: &Served, proxy: ProxyChoice, resolve: System) -> Client {
    let dialer = Dialer::new(Settings::new("podvm", proxy), Arc::new(resolve)).expect("a runtime");
    Client::new(served.context.clone(), &dialer).with_user_agent("podvm/1.2.3")
}

fn request(served: &Served) -> Request {
    Request::new("GET", format!("https://{NAME}:{}/v2/blobs/x", served.port)).trust(served.trust.clone())
}

/// Read each byte of `streamed` in reads of an odd size.
fn read_all(streamed: &mut Streamed) -> std::io::Result<Vec<u8>> {
    let mut got = Vec::new();
    let mut chunk = vec![0u8; 7919];
    loop {
        let n = streamed.read(&mut chunk)?;
        if n == 0 {
            return Ok(got);
        }
        got.extend_from_slice(&chunk[..n]);
    }
}

#[test]
fn a_large_body_streams_by_length_and_by_chunks() {
    pod_testkit::need_loopback!();
    let sent = body(LARGE);
    for answer in [Answer::Length, Answer::Chunked] {
        let served = serve(answer, sent.clone());
        let client = client(&served, ProxyChoice::Direct, pins());
        let mut streamed = client.send_streamed(request(&served)).expect("an answer");
        assert_eq!((streamed.status(), streamed.is_success()), (200, true), "{answer:?}");
        let got = read_all(&mut streamed).expect("the body");
        assert_eq!((got.len(), sha256(&got)), (LARGE, sha256(&sent)), "{answer:?}");
        let head = served.heads.recv().expect("the request");
        assert!(head.contains("\r\nUser-Agent: podvm/1.2.3\r\n"), "{head}");
    }
}

#[test]
fn a_body_cut_before_its_length_is_an_error() {
    pod_testkit::need_loopback!();
    let served = serve(Answer::Cut, body(1 << 20));
    let mut streamed = client(&served, ProxyChoice::Direct, pins()).send_streamed(request(&served)).expect("an answer");
    assert_eq!(streamed.header("content-length"), Some("1048576"));
    let cut = read_all(&mut streamed).unwrap_err();
    assert_eq!(cut.kind(), std::io::ErrorKind::UnexpectedEof, "{cut}");
}

#[test]
fn a_body_that_stops_ends_at_the_idle_limit() {
    pod_testkit::need_loopback!();
    let served = serve(Answer::Stall, body(1 << 20));
    let request = request(&served).body_idle(Duration::from_millis(300));
    let mut streamed = client(&served, ProxyChoice::Direct, pins()).send_streamed(request).expect("an answer");
    let start = Instant::now();
    let stopped = read_all(&mut streamed).unwrap_err();
    assert_eq!(stopped.kind(), std::io::ErrorKind::TimedOut, "{stopped}");
    assert!(start.elapsed() < ended_within(), "the read waited {:?}", start.elapsed());
}

/// The bound of a read that the idle limit of 300 ms ends: 5 s on a kernel,
/// five times that under an emulator. It tells an end by the limit from a hang.
fn ended_within() -> Duration {
    pod_testkit::host::scaled(Duration::from_secs(5))
}

/// The blocking face follows a redirect through the async one, and gives
/// the URL of the answer.
#[test]
fn a_redirect_is_followed() {
    pod_testkit::need_loopback!();
    let sent = body(1 << 16);
    let served = serve(Answer::Redirect("/v2/moved"), sent.clone());
    let client = client(&served, ProxyChoice::Direct, pins());
    let mut streamed = client.send_streamed(request(&served).redirects(1)).expect("an answer");
    assert_eq!(streamed.status(), 200);
    assert_eq!(streamed.url(), format!("https://{NAME}:{}/v2/moved", served.port));
    assert_eq!(sha256(&read_all(&mut streamed).expect("the body")), sha256(&sent));
    let followed = client.send(request(&served).redirects(1)).expect("an answer");
    assert_eq!((followed.status, followed.url.ends_with("/v2/moved")), (200, true));
    assert_eq!(client.send(request(&served)).expect("an answer").status, 302);
}

/// A stand-in HTTP proxy that answers one CONNECT, connects to the port of
/// its target on the loopback, and carries the bytes both ways; it gives
/// the head that it got.
fn connect_proxy() -> (Proxy, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("its address").port();
    let seen = thread::spawn(move || {
        let (sock, _) = listener.accept().expect("a connection");
        let mut reader = BufReader::new(sock.try_clone().expect("a clone"));
        let mut head = String::new();
        while !head.ends_with("\r\n\r\n") && reader.read_line(&mut head).expect("the head") > 0 {}
        let target = head.split(' ').nth(1).expect("a target");
        let port = target.rsplit_once(':').and_then(|(_, p)| p.parse::<u16>().ok()).expect("a port");
        let mut far = TcpStream::connect(("127.0.0.1", port)).expect("the target");
        let mut sock = sock;
        sock.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").expect("the answer");
        let (mut back, mut near) = (far.try_clone().expect("a clone"), sock.try_clone().expect("a clone"));
        thread::spawn(move || std::io::copy(&mut back, &mut near));
        let _ = std::io::copy(&mut reader, &mut far);
        head
    });
    (Proxy::parse(&format!("http://127.0.0.1:{port}")).expect("a proxy"), seen)
}

#[test]
fn a_body_streams_through_a_connect_proxy() {
    pod_testkit::need_loopback!();
    let sent = body(2 << 20);
    let served = serve(Answer::Chunked, sent.clone());
    let (proxy, seen) = connect_proxy();
    let client = client(&served, ProxyChoice::Via(proxy), System::new());
    let mut streamed = client.send_streamed(request(&served)).expect("an answer");
    assert_eq!(sha256(&read_all(&mut streamed).expect("the body")), sha256(&sent));
    // The client keeps the idle connection: the proxy ends with the client.
    drop((streamed, client));
    let head = seen.join().expect("the proxy");
    assert!(head.starts_with(&format!("CONNECT {NAME}:{} HTTP/1.1\r\n", served.port)), "{head}");
}

/// With the rules of podnet (no loopback rule), the proxy of the
/// environment takes the host unless `NO_PROXY` names it.
#[test]
fn a_host_that_no_proxy_names_goes_directly() {
    pod_testkit::need_loopback!();
    let sent = body(1 << 20);
    let served = serve(Answer::Length, sent.clone());
    let (_held, gone) = closed::port();
    let environment = |no_proxy: Option<&str>| {
        let proxy = format!("http://{gone}");
        let var = move |name: &str| match name {
            "HTTPS_PROXY" => Some(OsString::from(&proxy)),
            "NO_PROXY" => no_proxy.map(OsString::from),
            _ => None,
        };
        let rules = Rules { loopback_direct: false, ..Rules::default() };
        ProxyChoice::FromEnvironment(Environment::read_with(&var, rules))
    };
    let failed = client(&served, environment(None), pins()).send_streamed(request(&served)).unwrap_err();
    assert_eq!(failed.stage, Stage::Proxy, "{failed}");
    let client = client(&served, environment(Some(NAME)), pins());
    let mut streamed = client.send_streamed(request(&served)).expect("an answer");
    assert_eq!(sha256(&read_all(&mut streamed).expect("the body")), sha256(&sent));
}

/// A request of plain `http://` takes the proxy of `http_proxy` on the
/// blocking face too, by a CONNECT; `HTTPS_PROXY` is for `https://`.
#[test]
fn a_request_of_http_takes_the_proxy_of_http_proxy() {
    pod_testkit::need_loopback!();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("its address").port();
    let target = thread::spawn(move || {
        let (mut sock, _) = listener.accept().expect("a connection");
        let mut head = String::new();
        let mut reader = BufReader::new(sock.try_clone().expect("a clone"));
        while !head.ends_with("\r\n\r\n") && reader.read_line(&mut head).expect("the head") > 0 {}
        sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").expect("the answer");
        head
    });
    let (proxy, seen) = connect_proxy();
    let (_held, gone) = closed::port();
    let var = move |name: &str| match name {
        "http_proxy" => Some(OsString::from(format!("http://{}:{}", proxy.host, proxy.port))),
        "HTTPS_PROXY" => Some(OsString::from(format!("http://{gone}"))),
        _ => None,
    };
    let rules = Rules { loopback_direct: false, ..Rules::default() };
    let choice = ProxyChoice::FromEnvironment(Environment::read_with(&var, rules));
    let dialer = Dialer::new(Settings::new("podvm", choice), Arc::new(pins())).expect("a runtime");
    let http = dialer.proxy_for_scheme(pod_proxy::Scheme::Http, NAME, Some(port)).expect("usable");
    assert_eq!(http.map(|p| p.port), Some(proxy.port));
    let context = Arc::new(pod_tls::Context::new(
        Arc::new(rustls::crypto::ring::default_provider()),
        pod_tls::Settings::new(pod_tls::Words::new("podvm")),
    ));
    let client = Client::new(context, &dialer);
    let mut streamed =
        client.send_streamed(Request::new("GET", format!("http://{NAME}:{port}/v2/"))).expect("an answer");
    assert_eq!(read_all(&mut streamed).expect("the body"), b"ok");
    drop((streamed, client));
    assert!(target.join().expect("the target").starts_with("GET /v2/ HTTP/1.1\r\n"));
    let head = seen.join().expect("the proxy");
    assert!(head.starts_with(&format!("CONNECT {NAME}:{port} HTTP/1.1\r\n")), "{head}");
}
