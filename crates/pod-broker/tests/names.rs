//! An upstream that takes names only (T-161): the broker checks the name
//! and its addresses as before, then sends the name, once, in place of the
//! checked address. The canary finds whether the upstream refuses a name
//! of a non-public address. The load flags an allow rule under a suffix
//! where anyone can register a name.

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use pod_broker::{Settings, UpstreamCheck};
use pod_core::{Class, Target};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

/// A copy of the policy of the names-only proxy fixture of podnet and
/// podssh (`scripts/box/proxy.py`), on loopback: a CONNECT to an IP literal
/// gets 403 "not a public host", a port other than 443 gets 403 "not on the
/// egress allowlist", each name in `refuse` gets 403, and each other name
/// gets a tunnel to an echo. It keeps each request head.
async fn names_only_proxy(refuse: &'static [&'static str]) -> (std::net::SocketAddr, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = listener.local_addr().unwrap();
    let heads = Arc::new(Mutex::new(Vec::new()));
    let kept = heads.clone();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let kept = kept.clone();
            tokio::spawn(async move {
                let head = read_head(&mut stream).await;
                kept.lock().unwrap().push(head.clone());
                let target = head.split_whitespace().nth(1).unwrap_or_default().to_string();
                let (host, port) = target.rsplit_once(':').unwrap_or((target.as_str(), ""));
                let host = host.trim_start_matches('[').trim_end_matches(']');
                let why = if host.parse::<std::net::IpAddr>().is_ok() {
                    Some("not a public host")
                } else if port != "443" {
                    Some("not on the egress allowlist")
                } else if refuse.contains(&host) {
                    Some("refused")
                } else {
                    None
                };
                let (status, words) = why.map_or((200, "Connection established"), |w| (403, w));
                let answer = format!("HTTP/1.1 {status} {words}\r\nContent-Length: 0\r\n\r\n");
                let _ = stream.write_all(answer.as_bytes()).await;
                if status == 200 {
                    let (mut r, mut w) = stream.split();
                    let _ = tokio::io::copy(&mut r, &mut w).await;
                }
            });
        }
    });
    (at, heads)
}

/// A broker whose way out is the stand-in at `proxy_at`.
fn chained(
    settings: Settings,
    proxy_at: std::net::SocketAddr,
    answers: Arc<Answers>,
) -> (Arc<pod_broker::Broker>, Arc<pod_broker::Memory>) {
    let proxy = pod_proxy::Proxy::parse(&format!("http://{proxy_at}")).unwrap();
    // The resolver of the way out resolves the name of the proxy only.
    let dial = pod_broker::upstream("pod-broker-test", Some(proxy), Arc::new(pod_proxy::resolve::System::new()));
    let audit = Arc::new(pod_broker::Memory::new());
    let broker = Arc::new(pod_broker::Broker::new(settings, answers, dial).with_audit(audit.clone()));
    (broker, audit)
}

fn names() -> Settings {
    Settings { names_upstream: true, ..Settings::default() }
}

#[tokio::test]
async fn the_canary_tells_an_upstream_that_refuses_a_non_public_name_from_one_that_takes_it() {
    pod_testkit::need_loopback!();
    let (refusing, heads) = names_only_proxy(&["localhost"]).await;
    let (broker, audit) = chained(names(), refusing, Answers::public());
    assert_eq!(broker.check_upstream().await, UpstreamCheck::RefusesNonPublic);
    assert!(heads.lock().unwrap()[0].starts_with("CONNECT localhost:443 HTTP/1.1\r\n"));
    let line = audit.lines().into_iter().find(|l| l.event == "broker.upstream.canary").unwrap();
    assert_eq!((line.level, line.fields["result"].as_str()), ("info", Some("refuses-non-public")));
    assert_eq!(line.fields["canary"], "localhost");

    let (taking, _) = names_only_proxy(&[]).await;
    let (broker, audit) = chained(names(), taking, Answers::public());
    assert_eq!(broker.check_upstream().await, UpstreamCheck::TakesNonPublic);
    let line = audit.lines().into_iter().find(|l| l.event == "broker.upstream.canary").unwrap();
    assert_eq!((line.level, line.fields["result"].as_str()), ("warn", Some("takes-non-public")));
}

#[tokio::test]
async fn a_canary_with_no_answer_is_unknown() {
    let settings = Settings { connect_timeout: std::time::Duration::from_millis(200), ..names() };
    let (broker, _) = broker_with(settings, Far::new(Kind::Hang), Answers::public());
    assert!(matches!(broker.check_upstream().await, UpstreamCheck::Unknown(_)));
    let (broker, _) = broker_with(names(), Far::new(Kind::Fail(Class::Refused)), Answers::public());
    assert_eq!(broker.check_upstream().await, UpstreamCheck::RefusesNonPublic);
}

#[test]
fn an_allow_rule_under_an_open_suffix_is_flagged_at_load() {
    let (flagging, audit) = broker(Far::new(Kind::Echo), Answers::public());
    let policy = pod_broker::Policy::from_json(
        r#"{"version":1,"mode":"allow","allow":[{"host":"*.github.io"},{"host":"github.com"},{"host":"x.pages.dev"}]}"#,
    )
    .unwrap();
    let info = flagging
        .load(pod_broker::Load {
            job: "job1".into(),
            token: pod_broker::Token::new(TOKEN),
            ttl: std::time::Duration::from_secs(600),
            policy,
        })
        .unwrap();
    assert_eq!(info.flagged, ["*.github.io", "x.pages.dev"]);
    let line = audit.lines().into_iter().find(|l| l.event == "broker.job.load").unwrap();
    assert_eq!((line.level, line.fields["flagged"].as_str()), ("warn", Some("*.github.io x.pages.dev")));
    assert_eq!(flagging.list()[0].flagged, info.flagged);

    let policy = serde_json::json!({"version":1,"mode":"allow","allow":[{"host":"*.github.io"}]});
    let line = serde_json::json!({"op":"load","job":"job2","token":TOKEN_TWO,"ttl_s":600,"policy":policy});
    assert_eq!(flagging.control_line(&line.to_string())["flagged"], serde_json::json!(["*.github.io"]));

    let (plain, audit) = broker(Far::new(Kind::Echo), Answers::public());
    load(&plain, "job1", TOKEN, r#"{"version":1,"mode":"allow","allow":[{"host":"github.com"}]}"#);
    let line = audit.lines().into_iter().find(|l| l.event == "broker.job.load").unwrap();
    assert_eq!((line.level, line.fields["flagged"].is_null()), ("info", true));
}

#[test]
fn an_open_suffix_holds_the_suffix_and_each_name_under_it_only() {
    use pod_broker::names::open_suffix;
    assert_eq!(open_suffix("*.github.io"), Some("github.io"));
    assert_eq!(open_suffix("Raw.GitHubUserContent.com."), Some("githubusercontent.com"));
    assert_eq!(open_suffix("github.io"), Some("github.io"));
    assert_eq!(open_suffix("notgithub.io"), None);
    assert_eq!(open_suffix("github.com"), None);
}

/// A resolver that gives a public address, then a private one: the broker
/// checks the first answer and never reads the second, in both modes.
#[tokio::test]
async fn the_broker_sends_the_name_once_and_never_a_second_answer() {
    let answers = Answers::new(&[&[PUBLIC], &["10.0.0.1"]]);
    let far = Far::new(Kind::Echo);
    let (broker, audit) = broker_with(names(), far.clone(), answers.clone());
    load(&broker, "job1", TOKEN, OPEN);
    let (head, mut tunnel) = ask(&broker, &connect("rebind.example:443")).await;
    assert_eq!(status_of(&head), 200, "{head}");
    assert_eq!(round_trip(&mut tunnel, b"by name").await, b"by name");
    assert_eq!(far.dialed(), [Target::Name { host: "rebind.example".into(), port: 443 }]);
    assert_eq!(answers.calls(), 1);
    tunnel.shutdown().await.unwrap();
    let line = audit.connection("allow").await;
    assert_eq!((line.fields["sent"].as_str(), line.fields["addr"].as_str()), (Some("name"), Some("192.0.2.10:443")));

    let answers = Answers::new(&[&[PUBLIC], &["10.0.0.1"]]);
    let far = Far::new(Kind::Echo);
    let (broker, audit) = broker_with(Settings::default(), far.clone(), answers.clone());
    load(&broker, "job1", TOKEN, OPEN);
    let (head, mut tunnel) = ask(&broker, &connect("rebind.example:443")).await;
    assert_eq!(status_of(&head), 200, "{head}");
    assert_eq!(far.dialed(), [Target::Addr(format!("{PUBLIC}:443").parse().unwrap())]);
    assert_eq!(answers.calls(), 1);
    tunnel.shutdown().await.unwrap();
    assert!(audit.connection("allow").await.fields["sent"].is_null());
}

/// The checks still run in names mode: a name of a private address and an
/// address literal are refused before a dial.
#[tokio::test]
async fn names_mode_keeps_each_check() {
    let far = Far::new(Kind::Echo);
    let (broker, _) = broker_with(names(), far.clone(), Answers::new(&[&["10.0.0.1"]]));
    load(&broker, "job1", TOKEN, OPEN);
    assert_eq!(status(&broker, &connect("private.example:443")).await, 403);
    assert_eq!(status(&broker, &connect("10.0.0.1:443")).await, 403);
    assert!(far.dialed().is_empty(), "{:?}", far.dialed());
}

/// Against the copy of the fixture, which refuses a CONNECT to an IP
/// literal: address mode gets 502, names mode gets a tunnel, and a name on
/// a port that the fixture refuses gets 502.
#[tokio::test]
async fn an_upstream_that_takes_names_only_needs_names_mode() {
    pod_testkit::need_loopback!();
    let (proxy_at, heads) = names_only_proxy(&[]).await;
    let (broker, _) = chained(Settings::default(), proxy_at, Answers::public());
    load(&broker, "job1", TOKEN, OPEN);
    assert_eq!(status(&broker, &connect("names.example:443")).await, 502);
    assert!(heads.lock().unwrap()[0].starts_with(&format!("CONNECT {PUBLIC}:443 ")));

    let (broker, _) = chained(names(), proxy_at, Answers::public());
    load(&broker, "job1", TOKEN, OPEN);
    let (head, mut tunnel) = ask(&broker, &connect("names.example:443")).await;
    assert_eq!(status_of(&head), 200, "{head}");
    assert_eq!(round_trip(&mut tunnel, b"through a names-only upstream").await, b"through a names-only upstream");
    let heads = heads.lock().unwrap().clone();
    assert!(heads[1].starts_with("CONNECT names.example:443 HTTP/1.1\r\n"), "{}", heads[1]);
    assert!(!heads[1].contains(TOKEN), "{}", heads[1]);
    assert_eq!(status(&broker, &connect("names.example:8080")).await, 502);
}
