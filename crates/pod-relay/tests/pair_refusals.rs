//! The refusals of a pair (PodUtils/podssh#362, ask 2), against a stand-in
//! control host of TLS on the loopback that answers each request with a
//! `403` and its own words: a refused new pair names no pair, a refused
//! token carries the relay's words on one line, and a pair whose connect
//! token takes one upgrade is kept with its mark, so that its `403` names a
//! spent token.
#![cfg(feature = "pair")]

#[allow(dead_code)]
mod cert {
    include!("../../pod-tls/tests/common/cert_for_test.rs");
}

use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pod_relay::Context;
use pod_relay::cache::{self, Cache};
use pod_relay::pair::{self, OperatorPart, PairContext, PairError};
use pod_relay::relay::Relay;
use pod_ws::Trust;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const NAME: &str = "relay.pair-refusals.test";
const LIMIT: Duration = Duration::from_secs(10);
const PAIR: &str = "0123456789abcdef0123456789abcdef";
const TOKEN: &str = "testonlytoken00000000000000000000000000000000000000000000000000";
const NOW_LATER: i64 = 4_000_000_000_000;

/// A stand-in control host that answers each request `403` with `body`:
/// its port and the root of its certificate.
async fn control_host(body: &'static str, tag: &str) -> (u16, PathBuf) {
    let (acceptor, _) = cert::server_for(NAME).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(tcp).await else { return };
                read_request(&mut tls).await;
                let answer = format!(
                    "HTTP/1.1 403 Forbidden\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: \
                     close\r\n\r\n{body}",
                    body.len()
                );
                let _ = tls.write_all(answer.as_bytes()).await;
                let _ = tls.shutdown().await;
            });
        }
    });
    let dir = std::env::temp_dir().join(format!("pod-relay-pair-refusals-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let root = dir.join("root.pem");
    let der = cert::encoding::self_signed_inner(&cert::encoding::anchor_name(NAME), true).der;
    std::fs::write(&root, cert::encoding::pem(&der)).unwrap();
    (port, root)
}

/// The head of a request and its body, by its `Content-Length`: a close with
/// bytes unread would reset the answer.
async fn read_request<S: AsyncReadExt + Unpin>(stream: &mut S) {
    let mut seen = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(end) = seen.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&seen[..end]).to_ascii_lowercase();
            let length = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|n| n.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if seen.len() >= end + 4 + length {
                return;
            }
        }
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => seen.extend_from_slice(&chunk[..n]),
        }
    }
}

fn context() -> Context {
    let lo: IpAddr = "127.0.0.1".parse().unwrap();
    let resolve = Arc::new(pod_proxy::resolve::System::with_pins(vec![(NAME.to_string(), lo)]));
    let settings = pod_proxy::Settings::new("pod-relay", pod_proxy::ProxyChoice::Direct);
    let dialer = Arc::new(pod_proxy::Dialer::new(settings, resolve));
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls = Arc::new(pod_tls::Context::new(provider, pod_tls::Settings::new(pod_tls::Words::new("pod-relay"))));
    let way = pod_ws::Way::new(tls, dialer, "pod-relay/test");
    Context::new(way, Cache::none())
}

/// A pair of the relay's answer, with `"once": true` when `once` is set.
fn made(relay: &Relay, once: bool) -> pair::Pair {
    let once = if once { r#","once":true"# } else { "" };
    let body = format!(
        r#"{{"name":"{PAIR}","node_token":"{TOKEN}","connect_token":"{TOKEN}","stop_token":"{TOKEN}","expires":{NOW_LATER}{once}}}"#
    );
    pair::parse(relay, body.as_bytes(), 0).unwrap()
}

/// A `403` of `/v1/pair` names no pair, as none was made: the relay's words
/// come first, and no stopped or expired pair is said.
#[tokio::test]
async fn a_refused_new_pair_is_said_as_one_and_not_as_a_stopped_pair() {
    let (port, root) = control_host("keys: this key is not on the list", "made").await;
    let (ctx, relay, trust) = (context(), Relay { host: NAME.into(), port }, Trust::File(root));
    let pc = PairContext { ctx: &ctx, relay: &relay, trust: &trust, timeout: LIMIT };
    let error = pair::create(&pc).await.expect_err("a 403");
    assert!(matches!(&error, PairError::NotMade { .. }), "{error:?}");
    let said = error.to_string();
    assert!(said.starts_with("the relay refused to make a pair (403: keys: this key is not on the list)"), "{said}");
    assert!(!said.contains("expired") && !said.contains("stopped"), "{said}");
    assert_eq!(pod_relay::failure::pair(&error).1, pod_core::Class::Refused);
}

/// A `403` of a pair's token carries the relay's own words, and a pair of
/// one upgrade kept in the store and read back names its spent token.
#[tokio::test]
async fn a_refused_token_carries_the_relays_words_and_a_spent_once_token_is_named() {
    let (port, root) = control_host("reverse: forbidden", "token").await;
    let (ctx, relay, trust) = (context(), Relay { host: NAME.into(), port }, Trust::File(root));
    let pc = PairContext { ctx: &ctx, relay: &relay, trust: &trust, timeout: LIMIT };
    let dir = std::env::temp_dir().join(format!("pod-relay-pair-once-{}", std::process::id()));
    cache::private_dir(&dir).unwrap();
    let dirs = [dir.clone()];
    pair::store_in_first(&dirs, "once", &made(&relay, true)).unwrap();
    pair::store_in_first(&dirs, "plain", &made(&relay, false)).unwrap();
    let once = pair::load_from(&dirs, "once").unwrap().unwrap();
    let plain = pair::load_from(&dirs, "plain").unwrap().unwrap();
    assert!(once.once && !plain.once, "the store keeps the mark of one upgrade");
    let _ = std::fs::remove_dir_all(&dir);

    let error = pair::status(&pc, &OperatorPart::of(&once)).await.expect_err("a 403");
    assert!(matches!(&error, PairError::Forbidden { once: true, .. }), "{error:?}");
    let said = error.to_string();
    assert!(said.starts_with("the relay refused the token (403: reverse: forbidden)"), "{said}");
    assert!(said.contains("is for one upgrade, and that upgrade is used"), "{said}");

    let error = pair::status(&pc, &OperatorPart::of(&plain)).await.expect_err("a 403");
    let said = error.to_string();
    assert_eq!(said, "the relay refused the token (403: reverse: forbidden): the pair is stopped or has expired");
    // The stop token takes no upgrade: its refusal never names one.
    let said = pair::stop(&pc, &once).await.expect_err("a 403").to_string();
    assert!(!said.contains("upgrade"), "{said}");
}

/// The relay's words of a `403` are one line and short, whatever it sent.
#[tokio::test]
async fn the_words_of_a_403_are_one_short_line() {
    const LONG: &str = concat!(
        "reverse: forbidden\r\nX-Injected: 1\r\n",
        "0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789",
        "0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789",
        "0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789"
    );
    let (port, root) = control_host(LONG, "long").await;
    let (ctx, relay, trust) = (context(), Relay { host: NAME.into(), port }, Trust::File(root));
    let pc = PairContext { ctx: &ctx, relay: &relay, trust: &trust, timeout: LIMIT };
    let error = pair::status(&pc, &OperatorPart::of(&made(&relay, false))).await.expect_err("a 403");
    let PairError::Forbidden { detail, .. } = &error else { panic!("{error:?}") };
    assert!(detail.starts_with(": reverse: forbidden"), "{detail}");
    assert!(!detail.contains(['\r', '\n']), "{detail:?}");
    assert!(detail.chars().count() <= 203, "{} characters", detail.chars().count());
}

/// The operator's file and the part kept under a label keep the mark too.
#[test]
fn the_operator_s_part_keeps_the_mark_of_one_upgrade() {
    let relay = Relay { host: NAME.into(), port: 443 };
    let dir = std::env::temp_dir().join(format!("pod-relay-pair-part-once-{}", std::process::id()));
    cache::private_dir(&dir).unwrap();
    let file = dir.join("operator.json");
    let _ = std::fs::remove_file(&file);
    pair::write_operator_file(&file, &made(&relay, true)).unwrap();
    assert!(pair::read_operator_file(&file).unwrap().once);
    let dirs = [dir.clone()];
    pair::part::store_in_first(&dirs, "lab", &OperatorPart::of(&made(&relay, true))).unwrap();
    assert!(pair::part::load_from(&dirs, "lab").unwrap().unwrap().once);
    pair::part::store_in_first(&dirs, "lab", &OperatorPart::of(&made(&relay, false))).unwrap();
    assert!(!pair::part::load_from(&dirs, "lab").unwrap().unwrap().once);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A label that is no name is said with "the label" once, in the store of
/// pairs and in the store of operators' parts.
#[test]
fn a_bad_label_is_said_once_in_both_stores() {
    for said in [pair::file_name("").unwrap_err().to_string(), pair::part::file_name("").unwrap_err().to_string()] {
        assert_eq!(said, "the label is empty");
    }
}
