//! One kept connection for each resolver: the questions of a lookup, and of
//! the next one, go over one connection; a connection that the server
//! closed is replaced by one reconnect; no kept connection with no
//! [`Kept`].

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use pod_core::{BoxFuture, Connected, Dial, Failure, Stage, Target};
use pod_dns::{Doh, Kept, Names, Settings, Way};
use pod_tls::{Context, TlsRoots, Trust, Words};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::Instant;

// The certificates of the tests of pod-tls, minted in pure Rust.
#[allow(dead_code)]
mod cert {
    include!("../../pod-tls/tests/common/cert_for_test.rs");
}

const NAME: &str = "doh.test";
const LIMIT: Duration = Duration::from_secs(10);

/// What the stand-in does after an answer.
#[derive(Clone, Copy, PartialEq)]
enum After {
    /// Waits for the next request.
    Wait,
    /// Closes the connection with no word of it, as an idle timeout does.
    Hangs,
}

/// A resolver of the JSON form for [`NAME`]; its port, the trust of its CA,
/// the connections it accepted and the requests it read.
async fn stand_in(after: After) -> (u16, Trust, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let (acceptor, roots) = cert::server_for(NAME).expect("a test server");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (accepted, requests) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (seen_connections, seen_requests) = (accepted.clone(), requests.clone());
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            seen_connections.fetch_add(1, Ordering::SeqCst);
            let (acceptor, requests) = (acceptor.clone(), seen_requests.clone());
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(tcp).await else { return };
                let mut head = Vec::new();
                let mut chunk = [0u8; 1024];
                loop {
                    while !head.ends_with(b"\r\n\r\n") {
                        match tls.read(&mut chunk).await {
                            Ok(n) if n > 0 => head.extend_from_slice(&chunk[..n]),
                            _ => return,
                        }
                    }
                    requests.fetch_add(1, Ordering::SeqCst);
                    let text = String::from_utf8_lossy(&head).into_owned();
                    head.clear();
                    let body = if text.contains("type=AAAA") {
                        r#"{"Status":0,"Answer":[{"name":"relay.test.","type":28,"TTL":60,"data":"2001:db8::1"}]}"#
                    } else {
                        r#"{"Status":0,"Answer":[{"name":"relay.test.","type":1,"TTL":60,"data":"192.0.2.1"}]}"#
                    };
                    let answer = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
                    if tls.write_all(answer.as_bytes()).await.is_err() || after == After::Hangs {
                        let _ = tls.shutdown().await;
                        return;
                    }
                }
            });
        }
    });
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = Context::new(provider, pod_tls::Settings::new(Words::new("podnet")));
    let roots = TlsRoots { count: roots.len(), roots, source: "the test's own CA".into() };
    (port, Trust::caller(context.client_config(&roots).unwrap()).unwrap(), accepted, requests)
}

/// A way out to the stand-in that counts its dials.
struct Counting {
    port: u16,
    dials: AtomicUsize,
}

impl Dial for Counting {
    fn label(&self) -> String {
        "the stand-in".into()
    }

    fn dial<'a>(&'a self, _target: &'a Target, _deadline: Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move {
            self.dials.fetch_add(1, Ordering::SeqCst);
            let tcp = TcpStream::connect(("127.0.0.1", self.port)).await.map_err(|e| Failure::io(Stage::Connect, e))?;
            Ok(Connected::new(Box::new(tcp)))
        })
    }

    fn names_inside(&self) -> bool {
        true
    }
}

fn settings(trust: Trust) -> Settings {
    Settings {
        resolvers: vec![Doh::new([192, 0, 2, 53].into(), NAME).json("/resolve")],
        names: Names::First,
        both: true,
        trust,
        ..Settings::default()
    }
}

fn context() -> Context {
    Context::new(Arc::new(rustls::crypto::ring::default_provider()), pod_tls::Settings::new(Words::new("podnet")))
}

/// Four questions (A and AAAA, twice), one connection.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn many_questions_use_one_connection() {
    pod_testkit::need_loopback!();
    let (port, trust, accepted, requests) = stand_in(After::Wait).await;
    let (settings, context, kept) = (settings(trust), context(), Kept::new());
    let dial = Counting { port, dials: AtomicUsize::new(0) };
    let way = Way { settings: &settings, context: &context, dial: &dial, kept: Some(&kept) };
    for _ in 0..2 {
        let found = way.lookup("relay.test", Instant::now() + LIMIT).await.unwrap();
        assert_eq!(found.len(), 2);
    }
    assert_eq!(requests.load(Ordering::SeqCst), 4);
    assert_eq!((accepted.load(Ordering::SeqCst), dial.dials.load(Ordering::SeqCst)), (1, 1));
    assert_eq!(kept.len(), 1, "the connection waits for the next question");
    kept.clear();
    assert!(kept.is_empty());
}

/// A server that closed the idle connection: the next question reconnects
/// once and gets its answer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_closed_connection_is_replaced_by_one_reconnect() {
    pod_testkit::need_loopback!();
    let (port, trust, accepted, _) = stand_in(After::Hangs).await;
    let (settings, context, kept) = (settings(trust), context(), Kept::new());
    let dial = Counting { port, dials: AtomicUsize::new(0) };
    let way = Way { settings: &settings, context: &context, dial: &dial, kept: Some(&kept) };
    let found = way.lookup("relay.test", Instant::now() + LIMIT).await.unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(accepted.load(Ordering::SeqCst), 2, "one connection for each question: the server hangs up");
}

/// With no [`Kept`], each question opens its own connection.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_kept_connection_opens_one_for_each_question() {
    pod_testkit::need_loopback!();
    let (port, trust, accepted, _) = stand_in(After::Wait).await;
    let (settings, context) = (settings(trust), context());
    let dial = Counting { port, dials: AtomicUsize::new(0) };
    let way = Way { settings: &settings, context: &context, dial: &dial, kept: None };
    way.lookup("relay.test", Instant::now() + LIMIT).await.unwrap();
    assert_eq!((accepted.load(Ordering::SeqCst), dial.dials.load(Ordering::SeqCst)), (2, 2));
}
