//! The one-POST form of the HTTP road over HTTP/2 (PodUtils/podssh#239,
//! #241), from `Road::tls` to a stand-in of TLS on the loopback whose
//! HTTP/2 is h2's own server, which serves the form as podssh's role does:
//! the head `200` at once, DATA with the mark first, each DATA up to the
//! target, the target's end as `END_STREAM`, a cut as a reset. Where its
//! TLS chooses `http/1.1`, or where a connection offers no ALPN, the
//! stand-in of `tests/http_road_stand_in/` serves the connection. TLS is
//! checked for the relay's name with the test's root.

mod http_road_stand_in;

#[allow(dead_code)]
mod cert {
    include!("../../pod-tls/tests/common/cert_for_test.rs");
}

use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use h2::{Reason, SendStream};
use http_road_stand_in::{Plan, StandIn, pattern};
use pod_relay::http_road::client::{End, Opened, Road, Target, open};
use pod_relay::http_road::{Form, MARK, Tuning};
use pod_relay::relay::Relay;
use pod_ws::{ConnectError, Trust};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const NAME: &str = "relay.one.test";
const LIMIT: Duration = Duration::from_secs(60);
const MIB: usize = 1 << 20;
const TARGET: Target<'static> = Target { host: "target.test", port: 22, family: None };
const REFUSAL: &str = "forward: missing or wrong token";

/// How the stand-in's HTTP/2 answers the form.
#[derive(Debug, Clone, Copy, PartialEq)]
enum H2 {
    /// As podssh's role.
    Serve,
    /// As the role, but the stream is reset after this many bytes down.
    CutAfter(usize),
    /// A role that serves the form on HTTP/1.1 alone: a reset that asks for
    /// HTTP/1.1.
    AskHttp1,
    /// `403` with the words of the role.
    Refuse,
    /// The head `200`, and no mark.
    NoMark,
}

/// A request of HTTP/2 as the stand-in got it.
#[derive(Debug, Clone, PartialEq)]
struct Asked {
    method: String,
    authority: String,
    path: String,
    token: Option<String>,
    chunked: bool,
}

/// What the stand-in saw.
#[derive(Default)]
struct Seen {
    /// The protocol that TLS chose, for each connection.
    alpn: Vec<Option<Vec<u8>>>,
    asked: Vec<Asked>,
    up: Sha256,
    up_bytes: usize,
    up_ended: bool,
}

struct Tls {
    port: u16,
    root: PathBuf,
    seen: Arc<Mutex<Seen>>,
    /// The stand-in of HTTP/1.1.
    h1: Arc<StandIn>,
}

impl Tls {
    /// A stand-in whose TLS offers `alpn`, whose HTTP/2 answers as `h2`, and
    /// whose target sends `down`.
    async fn start(alpn: &[&[u8]], h2: H2, down: Vec<u8>, tag: &str) -> Tls {
        let (acceptor, _) = cert::server_for(NAME).unwrap();
        let mut config = (**acceptor.config()).clone();
        config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen: Arc<Mutex<Seen>> = Arc::default();
        let h1 = StandIn::new(Plan { down: down.clone(), ..Plan::default() });
        let (kept, standin, down) = (seen.clone(), h1.clone(), Arc::new(down));
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let (acceptor, seen, standin, down) = (acceptor.clone(), kept.clone(), standin.clone(), down.clone());
                tokio::spawn(async move {
                    let Ok(mut tls) = acceptor.accept(tcp).await else { return };
                    let alpn = tls.get_ref().1.alpn_protocol().map(<[u8]>::to_vec);
                    seen.lock().unwrap().alpn.push(alpn.clone());
                    if alpn.as_deref() == Some(b"h2") {
                        return serve_h2(tls, h2, seen, down).await;
                    }
                    let (mut ours, theirs) = tokio::io::duplex(64 * 1024);
                    tokio::spawn(standin.serve(theirs));
                    let _ = tokio::io::copy_bidirectional(&mut tls, &mut ours).await;
                });
            }
        });
        let dir = std::env::temp_dir().join(format!("pod-relay-one-h2-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.join("root.pem");
        let der = cert::encoding::self_signed_inner(&cert::encoding::anchor_name(NAME), true).der;
        std::fs::write(&root, cert::encoding::pem(&der)).unwrap();
        Tls { port, root, seen, h1 }
    }

    /// The road of the client to the stand-in, as a caller makes it.
    fn road(&self) -> Road {
        let lo: IpAddr = "127.0.0.1".parse().unwrap();
        let resolve = Arc::new(pod_proxy::resolve::System::with_pins(vec![(NAME.to_string(), lo)]));
        let settings = pod_proxy::Settings::new("pod-relay", pod_proxy::ProxyChoice::Direct);
        let dialer = Arc::new(pod_proxy::Dialer::new(settings, resolve));
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let tls = Arc::new(pod_tls::Context::new(provider, pod_tls::Settings::new(pod_tls::Words::new("pod-relay"))));
        let way = pod_ws::Way::new(tls, dialer, "pod-relay/test");
        let relay = Relay { host: NAME.into(), port: self.port };
        Road::tls(&way, &relay, &Trust::File(self.root.clone()), LIMIT)
    }

    fn seen<T>(&self, f: impl FnOnce(&Seen) -> T) -> T {
        f(&self.seen.lock().unwrap())
    }
}

/// The HTTP/2 of the stand-in on `tls`.
async fn serve_h2<S>(tls: S, plan: H2, seen: Arc<Mutex<Seen>>, down: Arc<Vec<u8>>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let mut builder = h2::server::Builder::new();
    builder.initial_window_size(1 << 20).initial_connection_window_size(16 << 20);
    let Ok(mut connection) = builder.handshake::<_, Bytes>(tls).await else { return };
    while let Some(Ok((request, mut respond))) = connection.accept().await {
        let (seen, down) = (seen.clone(), down.clone());
        tokio::spawn(async move {
            let (parts, mut body) = request.into_parts();
            let header = |name: &str| parts.headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
            seen.lock().unwrap().asked.push(Asked {
                method: parts.method.to_string(),
                authority: parts.uri.authority().map(|a| a.to_string()).unwrap_or_default(),
                path: parts.uri.path_and_query().map(|p| p.to_string()).unwrap_or_default(),
                token: header("x-relay-token"),
                chunked: header("transfer-encoding").is_some(),
            });
            let cut = match plan {
                H2::AskHttp1 => return respond.send_reset(Reason::HTTP_1_1_REQUIRED),
                H2::Refuse => {
                    let head = http::Response::builder().status(403).body(()).unwrap();
                    let Ok(mut send) = respond.send_response(head, false) else { return };
                    let _ = send.send_data(Bytes::from_static(REFUSAL.as_bytes()), true);
                    return;
                }
                H2::NoMark => {
                    let _held = respond.send_response(http::Response::builder().status(200).body(()).unwrap(), false);
                    return tokio::time::sleep(LIMIT).await;
                }
                H2::Serve => None,
                H2::CutAfter(n) => Some(n),
            };
            let head = http::Response::builder().status(200).body(()).unwrap();
            let Ok(mut send) = respond.send_response(head, false) else { return };
            let _ = send.send_data(Bytes::from_static(MARK), false);
            let target = seen.clone();
            let reader = tokio::spawn(async move {
                while let Some(Ok(data)) = body.data().await {
                    let _ = body.flow_control().release_capacity(data.len());
                    let mut seen = target.lock().unwrap();
                    seen.up.update(&data);
                    seen.up_bytes += data.len();
                }
                target.lock().unwrap().up_ended = body.is_end_stream();
            });
            let mut sent = 0;
            for piece in down.chunks(64 * 1024) {
                if cut.is_some_and(|cut| sent >= cut) {
                    reader.abort();
                    return send.send_reset(Reason::CANCEL);
                }
                if give(&mut send, Bytes::copy_from_slice(piece)).await.is_err() {
                    return reader.abort();
                }
                sent += piece.len();
            }
            let _ = reader.await;
            if seen.lock().unwrap().up_ended {
                let _ = send.send_data(Bytes::new(), true);
            }
        });
    }
}

/// `data` as DATA, as the client's window lets it.
async fn give(send: &mut SendStream<Bytes>, mut data: Bytes) -> Result<(), ()> {
    while !data.is_empty() {
        send.reserve_capacity(data.len());
        let room = std::future::poll_fn(|cx| send.poll_capacity(cx)).await.ok_or(())?.map_err(|_| ())?;
        send.send_data(data.split_to(room.min(data.len())), false).map_err(|_| ())?;
    }
    Ok(())
}

async fn opened(road: &Road, form: Option<Form>) -> Result<Opened, ConnectError> {
    open(road, &TARGET, "token", form, Tuning::default()).await
}

/// Send `up` through the session, then its end, and read the bytes down to
/// their end: the digest of the bytes down, their count, and the end.
async fn carry(opened: Opened, up: Vec<u8>) -> ([u8; 32], usize, End) {
    let carried = tokio::time::timeout(LIMIT, async move {
        let (mut from, mut to) = tokio::io::split(opened.stream);
        let writer = tokio::spawn(async move {
            for piece in up.chunks(48 * 1024) {
                if to.write_all(piece).await.is_err() {
                    return;
                }
            }
            let _ = to.shutdown().await;
        });
        let (mut down, mut count) = (Sha256::new(), 0);
        let mut buf = vec![0u8; 64 * 1024];
        while let Ok(n) = from.read(&mut buf).await {
            if n == 0 {
                break;
            }
            down.update(&buf[..n]);
            count += n;
        }
        writer.await.unwrap();
        (down.finalize().into(), count, opened.ended.get().expect("the session's end"))
    });
    carried.await.expect("the session's end in time")
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

const BOTH: &[&[u8]] = &[b"h2", b"http/1.1"];

/// Where TLS chooses `h2`, the form is one stream of HTTP/2: 4 MiB each way
/// with equal digests, the token in its field, no field of HTTP/1.1's
/// chunks, and the target's end as the stream's.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_one_form_carries_4_mib_each_way_on_one_stream_of_h2() {
    pod_testkit::need_loopback!();
    let (up, down) = (pattern(4 * MIB, 3), pattern(4 * MIB, 5));
    let tls = Tls::start(BOTH, H2::Serve, down.clone(), "serve").await;
    let session = opened(&tls.road(), None).await.expect("the open");
    assert_eq!(session.form, Form::One);
    let (got, count, end) = carry(session, up.clone()).await;
    assert_eq!(end, End::Closed { code: 1000, reason: "target closed".into() });
    assert_eq!((got, count), (digest(&down), down.len()), "the bytes down");
    let (alpn, asked, up_digest, up_bytes, ended) = tls.seen(|s| {
        let up_digest: [u8; 32] = s.up.clone().finalize().into();
        (s.alpn.clone(), s.asked.clone(), up_digest, s.up_bytes, s.up_ended)
    });
    assert_eq!(alpn, [Some(b"h2".to_vec())], "one connection, of HTTP/2");
    let want = Asked {
        method: "POST".into(),
        authority: format!("{NAME}:{}", tls.port),
        path: "/v1/http/one/target.test/22".into(),
        token: Some("token".into()),
        chunked: false,
    };
    assert_eq!(asked, [want], "one stream");
    assert_eq!((up_digest, up_bytes), (digest(&up), up.len()), "the bytes up");
    assert!(ended, "the request's END_STREAM came");
    assert_eq!(tls.h1.record(|r| (r.ones, r.sent.len())), (0, 0), "nothing on HTTP/1.1");
}

/// A reset of the stream is a cut, never the target's end.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reset_of_the_stream_is_a_cut() {
    pod_testkit::need_loopback!();
    let tls = Tls::start(BOTH, H2::CutAfter(256 * 1024), pattern(MIB, 9), "cut").await;
    let session = opened(&tls.road(), None).await.expect("the open");
    assert_eq!(session.form, Form::One);
    let (_, count, end) = carry(session, pattern(64 * 1024, 1)).await;
    // A reset drops the DATA that the stand-in's h2 had not sent yet.
    assert!(count > 0 && count <= 256 * 1024, "the bytes before the cut: {count}");
    assert!(matches!(&end, End::Failed(why) if why.contains("cut")), "{end:?}");
}

/// A host whose TLS chooses `http/1.1` carries the form on HTTP/1.1, on the
/// same connection.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_host_whose_tls_chooses_http1_carries_the_form_on_the_same_connection() {
    pod_testkit::need_loopback!();
    let (up, down) = (pattern(MIB, 3), pattern(MIB, 5));
    let tls = Tls::start(&[b"http/1.1"], H2::Serve, down.clone(), "h1").await;
    let session = opened(&tls.road(), None).await.expect("the open");
    assert_eq!(session.form, Form::One);
    let (got, _, end) = carry(session, up.clone()).await;
    assert_eq!((got, end), (digest(&down), End::Closed { code: 1000, reason: "target closed".into() }));
    assert_eq!(tls.seen(|s| (s.alpn.clone(), s.asked.len())), (vec![Some(b"http/1.1".to_vec())], 0));
    assert_eq!(tls.h1.record(|r| (r.ones, r.up_bytes)), (1, up.len() as u64));
}

/// A role that serves the form on HTTP/1.1 alone asks for HTTP/1.1 on the
/// stream: the form goes again on a connection with no ALPN.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_role_that_asks_for_http1_gets_the_form_again_on_http1() {
    pod_testkit::need_loopback!();
    let (up, down) = (pattern(MIB, 3), pattern(MIB, 5));
    let tls = Tls::start(BOTH, H2::AskHttp1, down.clone(), "ask").await;
    let session = opened(&tls.road(), None).await.expect("the open");
    assert_eq!(session.form, Form::One);
    let (got, _, end) = carry(session, up.clone()).await;
    assert_eq!((got, end), (digest(&down), End::Closed { code: 1000, reason: "target closed".into() }));
    assert_eq!(tls.seen(|s| (s.alpn.clone(), s.asked.len())), (vec![Some(b"h2".to_vec()), None], 1));
    assert_eq!(tls.h1.record(|r| (r.ones, r.up_bytes)), (1, up.len() as u64));
}

/// A refusal of the role on HTTP/2 keeps its status and its words, as on
/// HTTP/1.1, and no other form goes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refusal_on_h2_keeps_its_status_and_words() {
    pod_testkit::need_loopback!();
    let tls = Tls::start(BOTH, H2::Refuse, Vec::new(), "refuse").await;
    let refused = opened(&tls.road(), None).await.err().expect("no session");
    assert!(matches!(&refused, ConnectError::Refused { status: 403, body } if body == REFUSAL), "{refused:?}");
    assert_eq!(tls.h1.record(|r| r.sent.len()), 0, "no other form");
}

/// With no mark on the stream in 2 s, the client takes the split form, and
/// the run keeps that.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_no_mark_on_the_stream_the_client_takes_the_split_form_within_the_bound() {
    pod_testkit::need_loopback!();
    let (up, down) = (pattern(MIB, 7), pattern(MIB, 11));
    let tls = Tls::start(BOTH, H2::NoMark, down.clone(), "nomark").await;
    let road = tls.road();
    let started = Instant::now();
    let session = opened(&road, None).await.expect("the open");
    let took = started.elapsed();
    assert_eq!(session.form, Form::Split);
    assert!(took >= Duration::from_secs(2) && took < Duration::from_secs(4), "took the split form after {took:?}");
    assert!(road.learned.held(&format!("{NAME}:{}", tls.port)).is_some_and(|why| why.contains("no mark")));
    let (got, _, end) = carry(session, up.clone()).await;
    assert_eq!((got, end), (digest(&down), End::Closed { code: 1000, reason: "target closed".into() }));
    assert_eq!(tls.h1.record(|r| r.up_bytes), up.len() as u64);
}
