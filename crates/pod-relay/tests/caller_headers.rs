//! The headers of the caller for a host (`pod_ws::Headers`, a credential of
//! a gate in front of the relay such as Cloudflare Access) on the roads of
//! the relay crate, against stand-ins that read each request's head: the
//! HTTP road (its open, a request of its session, the one-POST form), the
//! cleartext road (its mint and its upgrade, only where the caller allows
//! it), and the forward opener on the plain wire. Each stand-in quotes the
//! value that it got back in its refusal: no refusal shows it, and a 401
//! or a 403 keeps its status.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_relay::http_road::client::{Road, Target, open};
use pod_relay::http_road::conn::{BoxIo, Dial, Dialing};
use pod_relay::http_road::{Form, Tuning};
use pod_relay::{Context, Relay};
use pod_ws::headers::{Header, MASK, Zeroizing};
use pod_ws::{ConnectError, Headers};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

mod support;

const ID: &str = "made-up-id.access";
const SECRET: &str = "made-up-secret-of-the-test-0123";
const TOKEN: &str = "testonly-good-token-0001";
const SESSION: &str = "0123456789abcdef0123456789abcdef";
const KEY: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJ-_01234";
const LIMIT: Duration = Duration::from_secs(20);

fn access(_host: &str) -> Vec<Header> {
    vec![("CF-Access-Client-Id", Zeroizing::new(ID.into())), ("CF-Access-Client-Secret", Zeroizing::new(SECRET.into()))]
}

/// The heads that a stand-in got: each request's target, and its headers,
/// names in lowercase, in their order.
type Heads = Arc<Mutex<Vec<(String, Vec<(String, String)>)>>>;

/// The names of `head` from `first` on: three of them.
fn from(head: &(String, Vec<(String, String)>), first: &str) -> Vec<String> {
    let names: Vec<String> = head.1.iter().map(|(n, _)| n.clone()).collect();
    let at = names.iter().position(|n| n == first).unwrap_or(names.len());
    names[at..].iter().take(3).cloned().collect()
}

fn value<'a>(head: &'a (String, Vec<(String, String)>), name: &str) -> Option<&'a str> {
    head.1.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

/// An answer of `status` with `body`.
fn answer(status: u16, body: &str) -> Vec<u8> {
    format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
}

/// The refusal that a target ending in `/401` or `/403` gets: the secret
/// that came, quoted back.
fn refusal(target: &str, headers: &[(String, String)]) -> Option<Vec<u8>> {
    let status = [401u16, 403].into_iter().find(|s| target.contains(&format!("/{s}")))?;
    let secret = headers.iter().find(|(n, _)| n == "cf-access-client-secret").map_or("none", |(_, v)| v.as_str());
    Some(answer(status, &format!("access denied for {secret}")))
}

/// What a stand-in answers to a request's method, target and headers;
/// `None` ends the connection.
type Reply = fn(&str, &str, &[(String, String)]) -> Option<Vec<u8>>;

/// Serve the requests of `io` with `reply`, each head kept in `heads`,
/// until `reply` gives no answer or the connection ends.
async fn serve<S>(mut io: S, heads: Heads, reply: Reply)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut pending = Vec::new();
    loop {
        let Ok((head, rest)) = pod_proxy::http::request::read_request(&mut io, pending).await else { return };
        pending = rest;
        heads.lock().unwrap().push((head.target.clone(), head.headers.clone()));
        let Some(bytes) = reply(&head.method, &head.target, &head.headers) else { return };
        if io.write_all(&bytes).await.is_err() {
            return;
        }
        if head.method == "GET" && head.header("upgrade").is_some() {
            // Held open until the client goes.
            let mut rest = [0u8; 64];
            while matches!(tokio::io::AsyncReadExt::read(&mut io, &mut rest).await, Ok(n) if n > 0) {}
            return;
        }
    }
}

/// The relay role of the HTTP road: the open, then no answer to the next
/// request of the session.
fn http_road_role(method: &str, target: &str, headers: &[(String, String)]) -> Option<Vec<u8>> {
    if let Some(refused) = refusal(target, headers) {
        return Some(refused);
    }
    (method == "POST" && target.starts_with("/v1/http/open/"))
        .then(|| answer(200, &format!("{{\"session\":\"{SESSION}\",\"key\":\"{KEY}\"}}")))
}

/// A road whose each dial is a pipe to [`http_road_role`].
fn road(heads: &Heads, headers: Headers) -> Road {
    let heads = heads.clone();
    let dial: Dial = Arc::new(move || -> Dialing {
        let heads = heads.clone();
        Box::pin(async move {
            let (ours, theirs) = tokio::io::duplex(64 * 1024);
            tokio::spawn(serve(theirs, heads, http_road_role));
            Ok(Box::new(ours) as BoxIo)
        })
    });
    Road {
        dial,
        dial_alpn: None,
        authority: "relay.test".into(),
        user_agent: "pod-relay/test".into(),
        forms: None,
        learned: Arc::default(),
        extra: headers.for_host("relay.test"),
    }
}

async fn wait_for(heads: &Heads, n: usize) {
    let waited = async {
        while heads.lock().unwrap().len() < n {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    };
    tokio::time::timeout(LIMIT, waited).await.expect("the requests in time");
}

/// The open of the HTTP road and each request of its session carry the
/// headers after the token or the key; a refusal of the open keeps its
/// status and shows no value; a bad value is refused before a dial.
#[tokio::test]
async fn the_http_road_carries_the_headers_on_each_request() {
    let heads: Heads = Arc::default();
    let target = Target { host: "target.test", port: 22, family: None };
    let road = road(&heads, Headers::new(access));
    let _opened = open(&road, &target, TOKEN, Some(Form::Serial), Tuning::default()).await.expect("a session");
    wait_for(&heads, 2).await;
    let seen = heads.lock().unwrap().clone();
    assert_eq!(from(&seen[0], "x-relay-token"), ["x-relay-token", "cf-access-client-id", "cf-access-client-secret"]);
    assert!(seen[1].0.starts_with("/v1/http/s/"), "{:?}", seen[1].0);
    assert_eq!(from(&seen[1], "x-session-key"), ["x-session-key", "cf-access-client-id", "cf-access-client-secret"]);
    assert_eq!(value(&seen[1], "cf-access-client-secret"), Some(SECRET));
    for status in [401u16, 403] {
        let denied = Target { host: "deny.test", port: status, family: None };
        for form in [Form::Serial, Form::One] {
            let e = open(&road, &denied, TOKEN, Some(form), Tuning::default()).await.err().expect("a refusal");
            assert!(!e.to_string().contains(SECRET), "{e}");
            if form == Form::Serial {
                assert_eq!(e, ConnectError::Refused { status, body: format!("access denied for {MASK}") });
            }
        }
    }
    // The serial session that is open still polls: take the last head of
    // the one form, not the last head.
    let one = heads.lock().unwrap().iter().rev().find(|h| h.0.starts_with("/v1/http/one/")).cloned();
    let one = one.expect("a head of the one form");
    assert_eq!(from(&one, "x-relay-token"), ["x-relay-token", "cf-access-client-id", "cf-access-client-secret"]);
    // The polls of the open serial session are not requests of these opens.
    let opens = || heads.lock().unwrap().iter().filter(|h| !h.0.starts_with("/v1/http/s/")).count();
    let before = opens();
    let bad = Headers::new(|_| vec![("CF-Access-Client-Secret", Zeroizing::new(format!("{SECRET}\r\nX: 1")))]);
    let e = open(&self::road(&heads, bad), &target, TOKEN, None, Tuning::default()).await.err().expect("refused");
    assert!(matches!(&e, ConnectError::Config(why) if why.contains("CF-Access-Client-Secret")), "{e:?}");
    assert!(!e.to_string().contains(SECRET), "{e}");
    let own = Headers::new(|_| vec![("X-Session-Key", Zeroizing::new("x".into()))]);
    let e = open(&self::road(&heads, own), &target, TOKEN, None, Tuning::default()).await.err().expect("refused");
    assert!(e.to_string().contains("X-Session-Key"), "{e}");
    assert_eq!(opens(), before, "no request");
}

/// The upgrade, where the request asks it: a 101 for its key, or the
/// refusal of a target that names one.
fn upgrade(target: &str, headers: &[(String, String)]) -> Option<Vec<u8>> {
    if let Some(refused) = refusal(target, headers) {
        return Some(refused);
    }
    let key = headers.iter().find(|(n, _)| n == "sec-websocket-key")?;
    Some(pod_ws::server::switching(&key.1))
}

/// A mint of one session's token, then the upgrade.
fn cleartext_relay(method: &str, target: &str, headers: &[(String, String)]) -> Option<Vec<u8>> {
    if method == "POST" {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
        let expires = now as i64 + 120_000;
        return Some(answer(200, &format!("{{\"token\":\"{TOKEN}\",\"expires\":{expires},\"once\":true}}")));
    }
    upgrade(target, headers)
}

/// The cleartext road carries the headers, on its mint and on its upgrade,
/// only where the caller allows them in cleartext.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_cleartext_road_carries_the_headers_only_where_allowed() {
    pod_testkit::need_loopback!();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let heads: Heads = Arc::default();
    let kept = heads.clone();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            tokio::spawn(serve(tcp, kept.clone(), cleartext_relay));
        }
    });
    let relay = Relay { host: "127.0.0.1".into(), port };
    for allowed in [false, true] {
        heads.lock().unwrap().clear();
        let way = support::way().with_headers(Headers::new(access).in_cleartext(allowed));
        let ctx = Context::new(way, pod_relay::cache::Cache::none());
        let _ = pod_relay::cleartext::attempt(&ctx, &relay, "/connect/127.0.0.1/22", LIMIT).await;
        wait_for(&heads, 2).await;
        let seen = heads.lock().unwrap().clone();
        for head in &seen {
            let sent = value(head, "cf-access-client-secret");
            assert_eq!(sent, allowed.then_some(SECRET), "{} allowed: {allowed}", head.0);
        }
        if allowed {
            assert_eq!(
                from(&seen[1], "x-relay-token"),
                ["x-relay-token", "cf-access-client-id", "cf-access-client-secret"]
            );
        }
    }
}

/// The forward opener on the plain wire, to the loopback, carries the
/// headers after the token; the relay's 403 keeps its status and shows no
/// value.
#[cfg(feature = "plain-ws")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_forward_opener_on_the_plain_wire_carries_the_headers() {
    use pod_relay::open::{OpenError, open_on_loopback};
    use pod_relay::relay::{forward_path, select_relays};
    use pod_relay::{Request, Settings};

    pod_testkit::need_loopback!();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let heads: Heads = Arc::default();
    let kept = heads.clone();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            tokio::spawn(serve(tcp, kept.clone(), |_, target, headers| upgrade(target, headers)));
        }
    });
    let way = support::way().with_headers(Headers::new(access));
    let settings = Settings { token: Some(TOKEN.into()), ..Settings::default() };
    let ctx = Context::new(way, pod_relay::cache::Cache::none()).with_settings(settings);
    let relays = select_relays(Some(&format!("http://127.0.0.1:{port}")), None, &[]).unwrap();
    let trust = pod_ws::Trust::Default;
    for (host, status) in [("127.0.0.1", None), ("deny.test", Some(403))] {
        let path = forward_path(host, status.unwrap_or(22)).unwrap();
        let mode = pod_relay::failover::Mode::default();
        let request =
            Request { ctx: &ctx, relays: &relays, path: &path, trust: &trust, target: host, rounds: 1, failover: mode };
        let opened = tokio::time::timeout(LIMIT, open_on_loopback(&request, &mut |_| {})).await.expect("in time");
        let head = heads.lock().unwrap().last().cloned().expect("a request");
        assert_eq!(from(&head, "x-relay-token"), ["x-relay-token", "cf-access-client-id", "cf-access-client-secret"]);
        match (status, opened) {
            (None, opened) => assert!(opened.is_ok(), "a session"),
            (Some(status), Err(failure)) => {
                let OpenError::Connect { error, .. } = failure.last() else { panic!("{failure:?}") };
                assert_eq!(error, &ConnectError::Refused { status, body: format!("access denied for {MASK}") });
            }
            (Some(_), Ok(_)) => panic!("a refusal"),
        }
    }
}

/// The two fetches of `/relays.json` (the pool, and the offer of the layer)
/// carry the headers after `Accept` (T-848): a gate in front of the relay
/// refuses the document without them, and the pool and the offer of its
/// host are then empty.
#[tokio::test]
async fn the_fetches_of_the_relay_document_carry_the_headers() {
    let extra = Headers::new(access).for_host("relay.test").expect("the headers are good");
    let relay = Relay { host: "relay.test".into(), port: 443 };
    for offer in [false, true] {
        let (mut ours, mut theirs) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(async move {
            let (head, _) = pod_proxy::http::request::read_request(&mut theirs, Vec::new()).await.expect("a head");
            let body = br#"{"forward":{"layer":true}}"#;
            let answer = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            theirs.write_all(&[answer.into_bytes(), body.to_vec()].concat()).await.unwrap();
            (head.target, head.headers)
        });
        if offer {
            let got = pod_relay::forward_layer::offer::over_with(&mut ours, "ua", "relay.test", &relay, &extra);
            assert!(got.await.expect("the offer").layer);
        } else {
            let got = pod_relay::pool::body_with(&mut ours, "ua", "relay.test", &extra).await.expect("the body");
            assert!(!got.is_empty());
        }
        let (target, headers) = server.await.unwrap();
        assert_eq!(target, "/relays.json");
        let named = |name: &str| headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone());
        assert_eq!(named("cf-access-client-id").as_deref(), Some(ID), "offer: {offer}");
        assert_eq!(named("cf-access-client-secret").as_deref(), Some(SECRET), "offer: {offer}");
    }
}
