//! The redirects of the client (`Request::redirects`): 301, 302, 303, 307
//! and 308 of a `GET` or a `HEAD`, the `Location` resolved against the URL
//! that gave it; the credentials stay with the first origin; no step from
//! `https://` to `http://`; a bound of the count; another method and a
//! request that asks for none get the 3xx as it is.

#[allow(dead_code)]
mod served;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use pod_proxy::client::Request;
use pod_proxy::resolve::System;
use pod_proxy::{Client, Dialer, ProxyChoice, Settings};
use pod_tls::{Context, Words};
use served::{Answer, NAME, body, pins, serve};
use tokio::io::AsyncReadExt;

const OK: &str = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";

/// A plain HTTP server on the loopback that answers each connection with
/// the next of `answers`, and gives the head of each request.
fn origin(answers: Vec<String>) -> (u16, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("its address").port();
    let (told, heads) = mpsc::channel();
    thread::spawn(move || {
        for answer in answers {
            let Ok((mut sock, _)) = listener.accept() else { return };
            let mut head = Vec::new();
            let mut chunk = [0u8; 1024];
            while !head.ends_with(b"\r\n\r\n") {
                match sock.read(&mut chunk) {
                    Ok(n) if n > 0 => head.extend_from_slice(&chunk[..n]),
                    _ => break,
                }
            }
            let _ = told.send(String::from_utf8_lossy(&head).into_owned());
            let _ = sock.write_all(answer.as_bytes());
        }
    });
    (port, heads)
}

fn to(status: u16, location: &str) -> String {
    format!("HTTP/1.1 {status} Moved\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n")
}

fn client(resolve: System) -> Client {
    let dialer = Dialer::new(Settings::new("podvm", ProxyChoice::Direct), Arc::new(resolve));
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = Arc::new(Context::new(provider, pod_tls::Settings::new(Words::new("podvm"))));
    Client::new(context, Arc::new(dialer)).with_user_agent("podvm/1.2.3")
}

fn get(url: String) -> Request {
    Request::new("GET", url).timeout(Duration::from_secs(10))
}

fn head(heads: &mpsc::Receiver<String>) -> String {
    heads.recv_timeout(Duration::from_secs(5)).expect("a request")
}

#[tokio::test]
async fn a_302_to_another_path_of_the_host_is_followed() {
    pod_testkit::need_loopback!();
    let (port, heads) = origin(vec![to(302, "/moved"), OK.into()]);
    let url = format!("http://127.0.0.1:{port}/v2/x");
    let answer = client(System::new()).send(get(url).redirects(5)).await.expect("an answer");
    assert_eq!((answer.status, answer.text().as_str()), (200, "ok"));
    assert_eq!(answer.url, format!("http://127.0.0.1:{port}/moved"));
    assert!(head(&heads).starts_with("GET /v2/x HTTP/1.1\r\n"));
    assert!(head(&heads).starts_with("GET /moved HTTP/1.1\r\n"));
}

/// The same with the streamed answer, for each status of a redirect, and
/// for a `HEAD`.
#[tokio::test]
async fn each_status_of_a_redirect_is_followed() {
    pod_testkit::need_loopback!();
    for (status, method) in [(301, "GET"), (302, "HEAD"), (303, "GET"), (307, "HEAD"), (308, "GET")] {
        let (port, heads) = origin(vec![to(status, "/next"), OK.into()]);
        let request = Request::new(method, format!("http://127.0.0.1:{port}/")).redirects(1);
        let streamed = client(System::new()).send_streamed(request).await.expect("an answer");
        assert_eq!(streamed.status, 200, "{status}");
        assert!(streamed.url.ends_with("/next"), "{status}: {}", streamed.url);
        head(&heads);
        assert!(head(&heads).starts_with(&format!("{method} /next HTTP/1.1\r\n")), "{status}");
    }
}

/// `Authorization` and `Cookie` go to the same origin after a redirect,
/// and not to another port.
#[tokio::test]
async fn the_credentials_stay_with_the_first_origin() {
    pod_testkit::need_loopback!();
    let (far, far_heads) = origin(vec![OK.into()]);
    let elsewhere = format!("http://127.0.0.1:{far}/b?sig=1");
    let (port, heads) = origin(vec![to(307, "/a"), to(302, &elsewhere)]);
    let request = get(format!("http://127.0.0.1:{port}/"))
        .header("Authorization", "Bearer made-up")
        .header("Cookie", "made=up")
        .header("X-Kept", "1")
        .redirects(5);
    let answer = client(System::new()).send(request).await.expect("an answer");
    assert_eq!((answer.status, answer.url.as_str()), (200, elsewhere.as_str()));
    for _ in 0..2 {
        let near = head(&heads);
        assert!(near.contains("\r\nAuthorization: Bearer made-up\r\n"), "{near}");
        assert!(near.contains("\r\nCookie: made=up\r\n"), "{near}");
    }
    let far = head(&far_heads);
    assert!(far.starts_with("GET /b?sig=1 HTTP/1.1\r\n"), "{far}");
    assert!(!far.contains("Authorization") && !far.contains("Cookie"), "{far}");
    assert!(far.contains("\r\nX-Kept: 1\r\n") && far.contains("\r\nUser-Agent: podvm/1.2.3\r\n"), "{far}");
}

/// A redirect between paths of an `https://` origin takes the trust of the
/// request.
#[tokio::test]
async fn a_redirect_over_https_keeps_the_trust() {
    pod_testkit::need_loopback!();
    let sent = body(1 << 16);
    let served = serve(Answer::Redirect("/v2/moved"), sent.clone());
    let url = format!("https://{NAME}:{}/v2/x", served.port);
    let request = get(url).trust(served.trust.clone()).redirects(1);
    let dialer = Dialer::new(Settings::new("podvm", ProxyChoice::Direct), Arc::new(pins()));
    let client = Client::new(served.context.clone(), Arc::new(dialer));
    let mut streamed = client.send_streamed(request).await.expect("an answer");
    let mut got = Vec::new();
    streamed.read_to_end(&mut got).await.expect("the body");
    assert_eq!((streamed.status, got.as_slice()), (200, sent.as_slice()));
    assert_eq!(streamed.url, format!("https://{NAME}:{}/v2/moved", served.port));
}

#[tokio::test]
async fn a_redirect_from_https_to_http_is_refused() {
    pod_testkit::need_loopback!();
    let served = serve(Answer::Redirect("http://relay.test:1/x?sig=secret"), body(16));
    let url = format!("https://{NAME}:{}/v2/x", served.port);
    let request = get(url).trust(served.trust.clone()).redirects(5);
    let dialer = Dialer::new(Settings::new("podvm", ProxyChoice::Direct), Arc::new(pins()));
    let failed = Client::new(served.context.clone(), Arc::new(dialer)).send(request).await.unwrap_err();
    let text = failed.text();
    assert!(text.contains("a redirect from https:// to http:// is refused"), "{text}");
    assert!(text.ends_with("to http://relay.test:1/x"), "{text}");
}

#[tokio::test]
async fn more_redirects_than_the_bound_is_an_error() {
    pod_testkit::need_loopback!();
    let (port, _heads) = origin(vec![to(302, "/1"), to(302, "/2"), to(302, "/3?t=x"), OK.into()]);
    let request = get(format!("http://127.0.0.1:{port}/")).redirects(2);
    let failed = client(System::new()).send(request).await.unwrap_err();
    let text = failed.text();
    assert!(text.contains("more than 2 redirects; the last one to http://127.0.0.1:"), "{text}");
    assert!(text.ends_with("/3"), "{text}");
}

/// A `POST` gets the 302 as it is, and so does a request that asks for no
/// redirect (the default).
#[tokio::test]
async fn a_post_and_the_default_get_the_redirect_as_it_is() {
    pod_testkit::need_loopback!();
    let (port, heads) = origin(vec![to(302, "/x"), to(302, "/x")]);
    let url = format!("http://127.0.0.1:{port}/");
    let client = client(System::new());
    let posted = client.send(Request::new("POST", url.clone()).redirects(5)).await.expect("an answer");
    assert_eq!((posted.status, posted.header("location"), posted.url.as_str()), (302, Some("/x"), url.as_str()));
    let got = client.send(get(url.clone())).await.expect("an answer");
    assert_eq!((got.status, got.url.as_str()), (302, url.as_str()));
    head(&heads);
    head(&heads);
    assert!(heads.recv_timeout(Duration::from_millis(200)).is_err(), "no request follows");
}

#[tokio::test]
async fn a_relative_location_resolves_against_the_url() {
    pod_testkit::need_loopback!();
    let (port, heads) = origin(vec![to(301, "c?d=1#frag"), to(302, "../e/./f"), OK.into()]);
    let request = get(format!("http://127.0.0.1:{port}/v2/a/b?q=0")).redirects(2);
    let answer = client(System::new()).send(request).await.expect("an answer");
    assert_eq!((answer.status, answer.url), (200, format!("http://127.0.0.1:{port}/v2/e/f")));
    head(&heads);
    assert!(head(&heads).starts_with("GET /v2/a/c?d=1 HTTP/1.1\r\n"));
    assert!(head(&heads).starts_with("GET /v2/e/f HTTP/1.1\r\n"));
}
