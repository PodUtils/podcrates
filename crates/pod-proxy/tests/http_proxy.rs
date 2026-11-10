//! The proxy of a request by the scheme of its URL: a request of plain
//! `http://` takes the proxy of `http_proxy`, and goes directly when only
//! `HTTPS_PROXY` is set; a request of `https://` takes `HTTPS_PROXY`, never
//! `http_proxy`. The client asks an HTTP proxy for a tunnel (`CONNECT`)
//! for each scheme.

mod closed;

use std::ffi::OsString;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_core::Stage;
use pod_proxy::client::Request;
use pod_proxy::resolve::System;
use pod_proxy::{Client, Dialer, Environment, ProxyChoice, Rules, Settings};
use pod_tls::{Context, Words};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const ANSWER: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";

/// A server on the loopback that answers one request with [`ANSWER`], and
/// gives the head of the request.
async fn server() -> (u16, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = tokio::spawn(async move {
        let (s, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(s);
        let mut head = String::new();
        while !head.ends_with("\r\n\r\n") && reader.read_line(&mut head).await.unwrap() > 0 {}
        reader.get_mut().write_all(ANSWER).await.unwrap();
        head
    });
    (port, seen)
}

/// A stand-in HTTP proxy that answers one CONNECT, connects to the port of
/// its target on the loopback, and carries the bytes both ways; it gives
/// the head that it got when it reads it, as the client can keep the tunnel
/// open.
async fn connect_proxy() -> (u16, oneshot::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (told, seen) = oneshot::channel();
    tokio::spawn(async move {
        let (s, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(s);
        let mut head = String::new();
        while !head.ends_with("\r\n\r\n") && reader.read_line(&mut head).await.unwrap() > 0 {}
        let target = head.split(' ').nth(1).unwrap().to_string();
        let far_port = target.rsplit_once(':').and_then(|(_, p)| p.parse::<u16>().ok()).unwrap();
        let mut far = TcpStream::connect(("127.0.0.1", far_port)).await.unwrap();
        let mut near = reader.into_inner();
        near.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await.unwrap();
        let _ = told.send(head);
        let _ = tokio::io::copy_bidirectional(&mut near, &mut far).await;
    });
    (port, seen)
}

/// A client over the proxy of the variables `pairs`, with no loopback rule,
/// and `name.test` pinned to the loopback.
fn client(pairs: Vec<(&'static str, String)>) -> Client {
    let var = move |name: &str| pairs.iter().find(|(n, _)| *n == name).map(|(_, v)| OsString::from(v));
    let rules = Rules { loopback_direct: false, ..Rules::default() };
    let proxy = ProxyChoice::FromEnvironment(Environment::read_with(&var, rules));
    let pins = vec![("name.test".into(), IpAddr::from([127, 0, 0, 1]))];
    let dialer = Dialer::new(Settings::new("podvm", proxy), Arc::new(System::with_pins(pins)));
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = Arc::new(Context::new(provider, pod_tls::Settings::new(Words::new("podvm"))));
    Client::new(context, Arc::new(dialer))
}

fn get(url: String) -> Request {
    Request::new("GET", url).timeout(pod_testkit::host::scaled(Duration::from_secs(10)))
}

#[tokio::test]
async fn a_request_of_http_takes_the_proxy_of_http_proxy() {
    pod_testkit::need_loopback!();
    let (port, served) = server().await;
    let (proxy, seen) = connect_proxy().await;
    let (_held, gone) = closed::port();
    let client =
        client(vec![("http_proxy", format!("http://127.0.0.1:{proxy}")), ("HTTPS_PROXY", format!("http://{gone}"))]);
    let answer = client.send(get(format!("http://name.test:{port}/v2/"))).await.expect("an answer");
    assert_eq!((answer.status, answer.text().as_str()), (200, "ok"));
    let head = tokio::time::timeout(pod_testkit::host::scaled(Duration::from_secs(5)), seen)
        .await
        .expect("the proxy got a request")
        .unwrap();
    assert!(head.starts_with(&format!("CONNECT name.test:{port} HTTP/1.1\r\n")), "{head}");
    assert!(served.await.unwrap().starts_with("GET /v2/ HTTP/1.1\r\n"));

    // `https://` takes HTTPS_PROXY, where nothing listens.
    let failed = client.send(get(format!("https://name.test:{port}/v2/"))).await.unwrap_err();
    assert_eq!(failed.stage, Stage::Proxy, "{failed}");
}

#[tokio::test]
async fn a_request_of_http_takes_https_proxy_when_it_is_the_only_one() {
    pod_testkit::need_loopback!();
    let (_held, gone) = closed::port();
    let client = client(vec![("HTTPS_PROXY", format!("http://{gone}"))]);
    let failed = client.send(get("http://name.test:1/v2/".into())).await.unwrap_err();
    assert_eq!(failed.stage, Stage::Proxy, "the request went to the proxy: {failed}");
}

/// `ALL_PROXY` takes the place of `http_proxy`, and `NO_PROXY` excludes a
/// host for a request of `http://` too.
#[tokio::test]
async fn all_proxy_and_no_proxy_hold_for_a_request_of_http() {
    pod_testkit::need_loopback!();
    let (_held, gone) = closed::port();
    let all = client(vec![("ALL_PROXY", format!("http://{gone}"))]);
    let failed = all.send(get("http://name.test:1/".into())).await.unwrap_err();
    assert_eq!(failed.stage, Stage::Proxy, "{failed}");

    let (port, served) = server().await;
    let excluded = client(vec![("http_proxy", format!("http://{gone}")), ("NO_PROXY", "name.test".into())]);
    assert_eq!(excluded.send(get(format!("http://name.test:{port}/"))).await.expect("a direct answer").status, 200);
    let mut head = served.await.unwrap();
    head.truncate(16);
    assert_eq!(head, "GET / HTTP/1.1\r\n");
}
