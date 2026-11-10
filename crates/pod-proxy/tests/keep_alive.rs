//! The idle connections of the client (keep-alive): requests to one host
//! share one connection; an answer with `Connection: close` or of
//! `HTTP/1.0`, and a body dropped before its end, do not give theirs back;
//! a stale connection is retried once for a `GET`, never for a `POST`;
//! `with_keep_alive(false)` opens one connection for each request; two
//! trusts never share one.

mod kept;

use std::sync::Arc;

use kept::{Act, Kept, NAME, WAIT, mark, ok, pins, plain, tls};
use pod_proxy::client::Request;
use pod_proxy::{Client, Dialer, ProxyChoice, Settings};
use tokio::io::AsyncReadExt;

fn client(kept: &Kept) -> Client {
    let dialer = Dialer::new(Settings::new("podvm", ProxyChoice::Direct), Arc::new(pins()));
    Client::new(kept.context.clone(), Arc::new(dialer)).with_user_agent("podvm/1.2.3")
}

fn url(kept: &Kept, path: &str) -> String {
    format!("http://{NAME}:{}{path}", kept.port)
}

fn get(kept: &Kept, path: &str) -> Request {
    Request::new("GET", url(kept, path)).timeout(pod_testkit::host::scaled(WAIT))
}

/// Each request on a connection that stays open, with its mark.
fn marked(conn: usize, nth: usize, _head: &str) -> Act {
    Act::Answer(ok(&mark(conn, nth)))
}

#[tokio::test]
async fn three_requests_to_one_host_use_one_connection() {
    pod_testkit::need_loopback!();
    let kept = plain(marked);
    let client = client(&kept);
    let first = client.send(get(&kept, "/a")).await.expect("an answer");
    let mut streamed = client.clone().send_streamed(get(&kept, "/b")).await.expect("an answer");
    let mut second = Vec::new();
    streamed.read_to_end(&mut second).await.expect("the body");
    drop(streamed);
    let third = client.send(get(&kept, "/c")).await.expect("an answer");
    assert_eq!((first.text(), second, third.text()), ("c1r1".into(), b"c1r2".to_vec(), "c1r3".into()));
    assert_eq!(kept.accepted(), 1);
    let head = kept.head();
    assert!(head.starts_with("GET /a HTTP/1.1\r\n") && !head.contains("Connection:"), "{head}");
}

/// A redirect to the same origin goes on the connection of the 3xx.
#[tokio::test]
async fn a_redirect_to_the_same_origin_takes_the_same_connection() {
    pod_testkit::need_loopback!();
    fn script(conn: usize, nth: usize, head: &str) -> Act {
        match head.starts_with("GET /from ") {
            true => Act::Answer(b"HTTP/1.1 302 Found\r\nLocation: /to\r\nContent-Length: 0\r\n\r\n".to_vec()),
            false => Act::Answer(ok(&mark(conn, nth))),
        }
    }
    let kept = plain(script);
    let answer = client(&kept).send(get(&kept, "/from").redirects(1)).await.expect("an answer");
    assert_eq!((answer.text().as_str(), kept.accepted()), ("c1r2", 1));
}

/// An answer that says `Connection: close`, and an answer of `HTTP/1.0`,
/// keep no connection, though the server keeps it open.
#[tokio::test]
async fn an_answer_that_closes_makes_the_next_request_open_a_connection() {
    pod_testkit::need_loopback!();
    fn closes(conn: usize, nth: usize, _head: &str) -> Act {
        let text = mark(conn, nth);
        match nth {
            2 => Act::Answer(format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 4\r\n\r\n{text}").into()),
            _ => Act::Answer(ok(&text)),
        }
    }
    fn old(conn: usize, nth: usize, _head: &str) -> Act {
        let text = mark(conn, nth);
        match nth {
            2 => Act::Answer(format!("HTTP/1.0 200 OK\r\nContent-Length: 4\r\n\r\n{text}").into()),
            _ => Act::Answer(ok(&text)),
        }
    }
    for script in [closes as kept::Script, old] {
        let kept = plain(script);
        let client = client(&kept);
        let mut got = Vec::new();
        for _ in 0..3 {
            got.push((client.send(get(&kept, "/")).await.expect("an answer").text(), kept.accepted()));
        }
        let want = [("c1r1", 1), ("c1r2", 1), ("c2r1", 2)];
        assert_eq!(got, want.map(|(text, count)| (text.to_string(), count)));
    }
}

/// A body dropped before its end closes its connection: the next request
/// opens a new one and gets its answer whole.
#[tokio::test]
async fn a_body_dropped_before_its_end_closes_its_connection() {
    pod_testkit::need_loopback!();
    fn script(conn: usize, nth: usize, head: &str) -> Act {
        match head.starts_with("GET /big ") {
            true => Act::Answer(ok(&"x".repeat(1 << 20))),
            false => Act::Answer(ok(&mark(conn, nth))),
        }
    }
    let kept = plain(script);
    let client = client(&kept);
    assert_eq!(client.send(get(&kept, "/")).await.expect("an answer").text(), "c1r1");
    let mut streamed = client.send_streamed(get(&kept, "/big")).await.expect("an answer");
    let mut part = [0u8; 10];
    streamed.read_exact(&mut part).await.expect("a part of the body");
    assert_eq!(kept.accepted(), 1, "the large body comes on the idle connection");
    drop(streamed);
    let answer = client.send(get(&kept, "/")).await.expect("an answer");
    assert_eq!((answer.text().as_str(), kept.accepted()), ("c2r1", 2));
}

/// An idle connection that the server closed is not used.
#[tokio::test]
async fn a_connection_that_the_server_closed_is_not_used() {
    pod_testkit::need_loopback!();
    fn script(conn: usize, nth: usize, _head: &str) -> Act {
        Act::AnswerThenClose(ok(&mark(conn, nth)))
    }
    let kept = plain(script);
    let client = client(&kept);
    assert_eq!(client.send(get(&kept, "/")).await.expect("an answer").text(), "c1r1");
    assert_eq!(kept.closed.recv_timeout(pod_testkit::host::scaled(WAIT)), Ok(1));
    let answer = client.send(get(&kept, "/")).await.expect("an answer");
    assert_eq!((answer.text().as_str(), kept.accepted()), ("c2r1", 2));
}

/// The server closes a connection after the head of its second request,
/// with no answer.
fn second_closes(conn: usize, nth: usize, _head: &str) -> Act {
    match nth {
        1 => Act::Answer(ok(&mark(conn, nth))),
        _ => Act::Close,
    }
}

/// A `GET` on an idle connection that fails before a byte of the answer
/// goes again, once, on a new connection.
#[tokio::test]
async fn a_get_on_a_stale_connection_goes_again_once() {
    pod_testkit::need_loopback!();
    let kept = plain(second_closes);
    let client = client(&kept);
    assert_eq!(client.send(get(&kept, "/")).await.expect("an answer").text(), "c1r1");
    let answer = client.send(get(&kept, "/")).await.expect("an answer");
    assert_eq!((answer.text().as_str(), kept.accepted()), ("c2r1", 2));
    let heads: Vec<String> = (0..3).map(|_| kept.head()).collect();
    assert!(heads.iter().all(|head| head.starts_with("GET / HTTP/1.1\r\n")), "{heads:?}");
}

/// A `POST` does not go again: the server can have done it.
#[tokio::test]
async fn a_post_on_a_stale_connection_does_not_go_again() {
    pod_testkit::need_loopback!();
    let kept = plain(second_closes);
    let client = client(&kept);
    let post = || Request::new("POST", url(&kept, "/p")).json("{}").timeout(pod_testkit::host::scaled(WAIT));
    assert_eq!(client.send(post()).await.expect("an answer").text(), "c1r1");
    let failed = client.send(post()).await.unwrap_err();
    assert!(failed.text().ends_with("the connection closed before the response headers ended"), "{failed}");
    assert_eq!(kept.accepted(), 1);
}

#[tokio::test]
async fn with_keep_alive_off_each_request_opens_a_connection() {
    pod_testkit::need_loopback!();
    let kept = plain(marked);
    let client = client(&kept).with_keep_alive(false);
    for n in 1..=3 {
        let answer = client.send(get(&kept, "/")).await.expect("an answer");
        assert_eq!((answer.text(), kept.accepted()), (mark(n, 1), n));
        assert!(kept.head().contains("\r\nConnection: close\r\n"));
    }
}

/// Two trusts, and two clients, never share a connection; one trust on one
/// client does.
#[tokio::test]
async fn two_trusts_never_share_a_connection() {
    pod_testkit::need_loopback!();
    let kept = tls(marked);
    let (one, other) = (kept.trust(), kept.trust());
    let shared = client(&kept);
    let https = |trust: &pod_tls::Trust| {
        Request::new("GET", format!("https://{NAME}:{}/", kept.port))
            .trust(trust.clone())
            .timeout(pod_testkit::host::scaled(WAIT))
    };
    let mut got = Vec::new();
    for trust in [&one, &other, &one, &other] {
        got.push((shared.send(https(trust)).await.expect("an answer").text(), kept.accepted()));
    }
    let want = [("c1r1", 1), ("c2r1", 2), ("c1r2", 2), ("c2r2", 2)];
    assert_eq!(got, want.map(|(text, count)| (text.to_string(), count)));
    let answer = client(&kept).send(https(&one)).await.expect("an answer");
    assert_eq!((answer.text().as_str(), kept.accepted()), ("c3r1", 3));
}

/// A connection of one runtime is not given to a request on another: the
/// first runtime may shut down, and its connection with it.
#[test]
fn a_connection_stays_with_its_runtime() {
    pod_testkit::need_loopback!();
    let kept = plain(marked);
    let client = client(&kept);
    let runtime = || tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
    let first = runtime().block_on(client.send(get(&kept, "/a"))).expect("an answer");
    // A POST is not tried again: on a connection of the first runtime it
    // would fail.
    let post = Request::new("POST", url(&kept, "/b")).body(b"x".to_vec()).timeout(pod_testkit::host::scaled(WAIT));
    let second = runtime().block_on(client.send(post)).expect("an answer");
    assert_eq!((first.text(), second.text()), ("c1r1".to_string(), "c2r1".to_string()));
    assert_eq!(kept.accepted(), 2);
}
