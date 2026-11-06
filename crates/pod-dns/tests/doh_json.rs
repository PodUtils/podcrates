//! podnet's chain on the loopback: the JSON form of DNS over HTTPS over
//! HTTP/1.1, through a way whose far end resolves names and refuses an
//! address (a proxy of names alone), with podnet's settings: the address of
//! each resolver first and its name after it, the A and the AAAA records
//! both, the user agent of the consumer; and podnet's question, whose
//! answer keeps its status.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_core::{BoxFuture, Class, Connected, Dial, Failure, Stage, Target};
use pod_dns::{Doh, Names, Nxdomain, Settings, Way};
use pod_tls::{Context, TlsRoots, Trust, Words};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::Instant;

// The certificates of the tests of pod-tls, minted in pure Rust.
#[allow(dead_code)]
mod cert {
    include!("../../pod-tls/tests/common/cert_for_test.rs");
}

/// The stand-in resolver's name, which its certificate names.
const NAME: &str = "doh.test";
const LIMIT: Duration = Duration::from_secs(10);

/// What the stand-in was asked: the request target, the `Accept` and the
/// `User-Agent` of each request.
type Asked = Arc<Mutex<Vec<(String, String, String)>>>;

/// The JSON answer of the stand-in to the request target `path`.
fn answer_to(path: &str) -> &'static str {
    if path.contains("name=gone.test") || path == "/resolve?name=half.test&type=AAAA" {
        r#"{"Status":3,"Answer":[]}"#
    } else if path.ends_with("type=AAAA") {
        r#"{"Status":0,"Answer":[{"name":"relay.test.","type":28,"TTL":60,"data":"2001:db8::1"}]}"#
    } else if path.ends_with("type=SRV") {
        r#"{"Status":0,"Answer":[{"type":33,"TTL":60,"data":"1 1 7844 region1.v2.argotunnel.com."}]}"#
    } else {
        r#"{"Status":0,"Answer":[{"name":"relay.test.","type":1,"TTL":60,"data":"192.0.2.1"}]}"#
    }
}

/// A resolver of the JSON form over HTTP/1.1 for [`NAME`] on the loopback,
/// for each connection; its port, the trust of its test CA, and what it was
/// asked.
async fn stand_in() -> (u16, Trust, Asked) {
    let (acceptor, roots) = cert::server_for(NAME).expect("a test server");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let asked: Asked = Arc::default();
    let kept = asked.clone();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let (acceptor, kept) = (acceptor.clone(), kept.clone());
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(tcp).await else { return };
                let mut head = Vec::new();
                let mut chunk = [0u8; 1024];
                while !head.ends_with(b"\r\n\r\n") {
                    match tls.read(&mut chunk).await {
                        Ok(n) if n > 0 => head.extend_from_slice(&chunk[..n]),
                        _ => return,
                    }
                }
                let head = String::from_utf8_lossy(&head).into_owned();
                let path = head.split(' ').nth(1).unwrap_or_default().to_string();
                let header = |name: &str| {
                    head.lines()
                        .find_map(|l| l.split_once(": ").filter(|(n, _)| n.eq_ignore_ascii_case(name)))
                        .map_or(String::new(), |(_, v)| v.to_string())
                };
                kept.lock().unwrap().push((path.clone(), header("accept"), header("user-agent")));
                let body = answer_to(&path);
                let answer = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/dns-json\r\nContent-Length: {}\r\n\
                     Connection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = tls.write_all(answer.as_bytes()).await;
                let _ = tls.shutdown().await;
            });
        }
    });
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = Context::new(provider, pod_tls::Settings::new(Words::new("podnet")));
    let roots = TlsRoots { count: roots.len(), roots, source: "the test's own CA".into() };
    (port, Trust::caller(context.client_config(&roots).unwrap()).unwrap(), asked)
}

/// A way out that the far end of resolves names, as a proxy of names
/// alone: it refuses each address, and takes each name to the stand-in. It
/// keeps each target, in order.
struct NamesAlone {
    port: u16,
    targets: Mutex<Vec<String>>,
}

impl Dial for NamesAlone {
    fn label(&self) -> String {
        "a proxy of names alone".into()
    }

    fn dial<'a>(&'a self, target: &'a Target, _deadline: Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move {
            self.targets.lock().unwrap().push(target.to_string());
            if let Target::Addr(_) = target {
                return Err(Failure::new(Stage::Proxy, Class::Refused, "403 not on the allowlist"));
            }
            let tcp = TcpStream::connect(("127.0.0.1", self.port)).await.map_err(|e| Failure::io(Stage::Connect, e))?;
            Ok(Connected::new(Box::new(tcp)))
        })
    }

    fn names_inside(&self) -> bool {
        true
    }
}

/// podnet's settings, with the stand-in as the one resolver: its JSON form
/// at `/resolve`, as Google's.
fn podnet(trust: Trust) -> Settings {
    Settings {
        resolvers: vec![Doh::new([192, 0, 2, 53].into(), NAME).operator("Stand-in").json("/resolve")],
        names: Names::Last,
        system: Some(Duration::from_secs(5)),
        both: true,
        nxdomain: Nxdomain::NextOperator,
        trust,
        user_agent: "podnet/test".into(),
        ..Settings::default()
    }
}

/// The address is refused, the name carries the questions; the A and the
/// AAAA records both come, in the JSON form, with podnet's user agent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn podnet_s_chain_asks_the_json_form_for_both_families_by_the_name_after_the_address() {
    pod_testkit::need_loopback!();
    let (port, trust, asked) = stand_in().await;
    let settings = podnet(trust);
    let context =
        Context::new(Arc::new(rustls::crypto::ring::default_provider()), pod_tls::Settings::new(Words::new("podnet")));
    let dial = NamesAlone { port, targets: Mutex::new(Vec::new()) };
    let way = Way { settings: &settings, context: &context, dial: &dial };
    let found = tokio::time::timeout(LIMIT, way.lookup("relay.test", Instant::now() + LIMIT)).await.expect("in time");
    let want: Vec<std::net::IpAddr> = vec!["192.0.2.1".parse().unwrap(), "2001:db8::1".parse().unwrap()];
    assert_eq!(found, Ok(want));
    let targets = dial.targets.lock().unwrap().clone();
    assert_eq!(targets, ["192.0.2.53:443", "doh.test:443", "doh.test:443"], "the address, then the name");
    let asked = asked.lock().unwrap().clone();
    let each = |kind: &str| (format!("/resolve?name=relay.test&type={kind}"), "application/dns-json".to_string());
    let paths: Vec<(String, String)> = asked.iter().map(|(p, a, _)| (p.clone(), a.clone())).collect();
    assert_eq!(paths, [each("A"), each("AAAA")]);
    assert!(asked.iter().all(|(_, _, agent)| agent == "podnet/test"), "{asked:?}");
}

/// podnet's question: the first answer of the JSON form, whose status says
/// NXDOMAIN, is an answer; the records of an SRV question keep their type,
/// their data and their TTL.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_query_takes_the_first_answer_with_its_status() {
    pod_testkit::need_loopback!();
    let (port, trust, _) = stand_in().await;
    let settings = podnet(trust);
    let context =
        Context::new(Arc::new(rustls::crypto::ring::default_provider()), pod_tls::Settings::new(Words::new("podnet")));
    let dial = NamesAlone { port, targets: Mutex::new(Vec::new()) };
    let way = Way { settings: &settings, context: &context, dial: &dial };
    let gone = way.query("gone.test", "A", Instant::now() + LIMIT).await.expect("an answer");
    assert_eq!((gone.status, gone.records.len()), (Some(3), 0));
    let srv = way.query("_v2-origintunneld._tcp.argotunnel.com", "SRV", Instant::now() + LIMIT).await.unwrap();
    assert_eq!(srv.status, Some(0));
    let record = &srv.records[0];
    assert_eq!((record.kind, record.data.as_str(), record.ttl), (33, "1 1 7844 region1.v2.argotunnel.com.", Some(60)));
    let srv = pod_dns::json::srv_of(&record.data).expect("an SRV record");
    assert_eq!((srv.port, srv.target.as_str()), (7844, "region1.v2.argotunnel.com"));
    // A name that no resolver may be asked for is refused before a question.
    let refused = way.query("bad name", "A", Instant::now() + LIMIT).await.unwrap_err();
    assert!(refused.contains("is not a name to ask a resolver for"), "{refused}");
}

/// With A and AAAA both, an answer of AAAA that says NXDOMAIN after the
/// addresses of A keeps them: podnet's chain took NXDOMAIN only when no
/// question gave an address.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_nxdomain_of_aaaa_after_the_addresses_of_a_keeps_them() {
    pod_testkit::need_loopback!();
    let (port, trust, asked) = stand_in().await;
    let settings = podnet(trust);
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = Context::new(provider, pod_tls::Settings::new(Words::new("podnet")));
    let dial = NamesAlone { port, targets: Mutex::new(Vec::new()) };
    let way = Way { settings: &settings, context: &context, dial: &dial };
    let found = way.lookup("half.test", Instant::now() + LIMIT).await;
    assert_eq!(found, Ok(vec!["192.0.2.1".parse().unwrap()]));
    assert_eq!(asked.lock().unwrap().len(), 2, "A, then AAAA");
}
