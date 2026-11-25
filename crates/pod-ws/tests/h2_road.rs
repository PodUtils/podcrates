//! The WebSocket over HTTP/2's extended CONNECT (RFC 8441) against
//! stand-in relays on the loopback whose HTTP/2 is h2's own server, which
//! this crate did not write: one whose SETTINGS offer extended CONNECT, which
//! takes a CONNECT with `:protocol websocket`, checks its token, answers 200
//! and echoes each message, as a relay role does; one whose SETTINGS
//! offer none; and one whose TLS chooses http/1.1 alone. Each also takes the
//! upgrade of HTTP/1.1, as a relay does, and counts its TLS connections, its
//! upgrades, the CONNECTs that reach it, and each HEADERS frame that the
//! client sends on its HTTP/2, read off the wire before h2 reads it. TLS is
//! checked for the relay's name with the test's root.

mod common;
#[path = "h2_road/support.rs"]
mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use pod_ws::h2_road::{self, Fallback, Offered, Road};
use pod_ws::{ConnectError, Trust};
use support::*;

/// Eight sessions at once are eight streams of one TLS connection, each
/// with its own bytes each way (per_session), and each CONNECT is RFC 8441's: no key of
/// RFC 6455 and no upgrade of HTTP/1.1.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn eight_sessions_share_one_tls_connection_and_carry_4_mib_each_way() {
    pod_testkit::need_loopback!();
    let root = root_file("eight");
    let (port, seen) = stand_in(Offer::Extended).await;
    let mut tasks = Vec::new();
    // Each session opens before bytes flow: under an emulator, the bytes of
    // the first sessions delay the answer of a later CONNECT past the wait
    // of the road, which then retires the connection by design.
    let opened = Arc::new(tokio::sync::Barrier::new(8));
    for seed in 1..=8u64 {
        let config = config(port, &root);
        let opened = opened.clone();
        tasks.push(tokio::spawn(async move {
            let (session, road) = connect(&config, TOKEN).await.expect("a session on the road");
            opened.wait().await;
            each_way(session, per_session(), seed).await;
            road
        }));
    }
    let mut roads = Vec::new();
    for task in tasks {
        roads.push(task.await.unwrap());
    }
    assert_eq!(seen.accepted.load(Ordering::SeqCst), 1, "one TLS connection for the eight sessions: {roads:?}");
    assert_eq!(roads.iter().filter(|r| **r == Road::Stream { shared: false }).count(), 1, "{roads:?}");
    assert_eq!(roads.iter().filter(|r| **r == Road::Stream { shared: true }).count(), 7, "{roads:?}");
    let connects = seen.connects.lock().unwrap();
    assert_eq!(connects.len(), 8);
    let authority = format!("{NAME}:{port}");
    for asked in connects.iter() {
        assert_eq!(asked.protocol.as_deref(), Some("websocket"), "{asked:?}");
        assert_eq!((asked.scheme.as_deref(), asked.path.as_deref()), (Some("https"), Some(PATH)), "{asked:?}");
        assert_eq!(asked.authority.as_deref(), Some(authority.as_str()), "{asked:?}");
        assert_eq!(asked.version.as_deref(), Some("13"), "{asked:?}");
        assert_eq!(asked.key, None, "RFC 8441 drops Sec-WebSocket-Key: {asked:?}");
    }
    assert_eq!(seen.upgrades.load(Ordering::SeqCst), 0, "no upgrade of HTTP/1.1");
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

/// A server that chose `h2` and offered no extended CONNECT gets no request
/// on its HTTP/2, and the upgrade of HTTP/1.1 on a new connection; the
/// process then asks it for no HTTP/2.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_that_offers_no_extended_connect_gets_the_upgrade_on_a_new_connection() {
    pod_testkit::need_loopback!();
    let root = root_file("unoffered");
    let (port, seen) = stand_in(Offer::NoSetting).await;
    let (session, road) = connect(&config(port, &root), TOKEN).await.expect("a session");
    let headers = seen.headers.load(Ordering::SeqCst);
    assert_eq!(headers, 0, "a request went on an HTTP/2 connection whose SETTINGS offered no extended CONNECT");
    assert_eq!(road, Road::Http1(Fallback::NotOffered));
    assert_eq!(seen.accepted.load(Ordering::SeqCst), 2, "the upgrade on a new connection");
    assert_eq!(seen.upgrades.load(Ordering::SeqCst), 1);
    each_way(session, MIB, 9).await;
    let (session, road) = connect(&config(port, &root), TOKEN).await.expect("a second session");
    assert_eq!(road, Road::Http1(Fallback::Remembered(Box::new(Fallback::NotOffered))));
    assert_eq!(seen.accepted.load(Ordering::SeqCst), 3, "one connection for the second session");
    assert_eq!(seen.headers.load(Ordering::SeqCst), 0);
    assert!(seen.connects.lock().unwrap().is_empty());
    each_way(session, MIB, 10).await;
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

/// A server whose TLS chose http/1.1 gets the upgrade on that connection.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_whose_tls_chooses_http1_gets_the_upgrade_on_the_same_connection() {
    pod_testkit::need_loopback!();
    let root = root_file("http1");
    let (port, seen) = stand_in(Offer::Http1Only).await;
    let (session, road) = connect(&config(port, &root), TOKEN).await.expect("a session");
    assert_eq!(road, Road::Http1(Fallback::NotChosen));
    assert_eq!(seen.accepted.load(Ordering::SeqCst), 1, "the upgrade on the connection whose TLS chose it");
    assert_eq!(seen.upgrades.load(Ordering::SeqCst), 1);
    each_way(session, MIB, 11).await;
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

/// A 403 on the CONNECT is a 4xx, so the session takes the upgrade of
/// HTTP/1.1 on a new connection, within a second; that upgrade is refused
/// too, with the relay's words, and the refusal is the session's end. The
/// refusal of the credential is not the host's property: the right token
/// opens a session on HTTP/2 again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_403_on_the_connect_falls_back_to_the_upgrade_within_a_second() {
    pod_testkit::need_loopback!();
    let root = root_file("refused");
    let (port, seen) = stand_in(Offer::Extended).await;
    let started = std::time::Instant::now();
    let Err(refused) = connect(&config(port, &root), "a-wrong-token").await else { panic!("a refusal") };
    let took = started.elapsed();
    assert!(took < pod_testkit::host::scaled(Duration::from_secs(1)), "the fallback took {took:?}");
    assert!(matches!(&refused, ConnectError::Refused { status: 403, body } if body == REFUSAL), "{refused:?}");
    assert_eq!(seen.accepted.load(Ordering::SeqCst), 2, "the CONNECT's connection, and the upgrade's");
    assert_eq!(seen.upgrades.load(Ordering::SeqCst), 0, "the upgrade was refused before it opened");
    let (session, road) = connect(&config(port, &root), TOKEN).await.expect("a session");
    assert_eq!(road, Road::Stream { shared: true }, "the credential's refusal did not retire HTTP/2");
    assert_eq!(seen.accepted.load(Ordering::SeqCst), 2);
    each_way(session, MIB, 12).await;
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

/// CONNECT-2: a proxy that refuses the extended `CONNECT` and the `Upgrade`
/// of HTTP/1.1 in one answer (405) is not the session's end. The upgrade
/// takes the session on a new connection, the fallback is chosen inside a
/// second, and the host is remembered.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_connect_falls_back_to_the_upgrade_and_the_session_completes() {
    pod_testkit::need_loopback!();
    let root = root_file("refuses-connect");
    let (port, seen) = stand_in(Offer::RefusesConnect).await;
    let started = std::time::Instant::now();
    let (session, road) = connect(&config(port, &root), TOKEN).await.expect("a session on the fallback");
    let took = started.elapsed();
    assert_eq!(road, Road::Http1(Fallback::Refused(405)));
    assert!(took < pod_testkit::host::scaled(Duration::from_secs(1)), "the fallback took {took:?}");
    assert_eq!(seen.accepted.load(Ordering::SeqCst), 2, "the CONNECT's connection, and the upgrade's");
    assert_eq!(seen.upgrades.load(Ordering::SeqCst), 1, "the upgrade carried the session");
    assert_eq!(seen.connects.lock().unwrap().len(), 1, "one CONNECT, then no more");
    each_way(session, MIB, 14).await;
    // The host is remembered: the next session asks for no HTTP/2.
    let (session, road) = connect(&config(port, &root), TOKEN).await.expect("a second session");
    assert_eq!(road, Road::Http1(Fallback::Remembered(Box::new(Fallback::Refused(405)))));
    assert_eq!(seen.connects.lock().unwrap().len(), 1, "no CONNECT after the refusal");
    assert_eq!(seen.accepted.load(Ordering::SeqCst), 3);
    each_way(session, MIB, 15).await;
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

/// H2-1: the browser's shape is opt-in, and a session opens on it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_opens_on_the_browser_shape_when_a_caller_asks_for_it() {
    pod_testkit::need_loopback!();
    let root = root_file("browser-shape");
    let (port, seen) = stand_in(Offer::Extended).await;
    let config = config(port, &root);
    let asked =
        tokio::time::timeout(LIMIT * 2, h2_road::connect_with(&POOL, &config, TOKEN, &h2_road::Shape::browser()))
            .await
            .expect("an answer within the limit")
            .expect("a session on the browser's shape");
    assert_eq!(asked.1, Road::Stream { shared: false });
    assert_eq!(seen.accepted.load(Ordering::SeqCst), 1);
    each_way(asked.0, MIB, 16).await;
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

/// H2-1: the request sends its pseudo-fields in the order of its shape, read
/// from the HPACK block of its HEADERS frame on the wire: the fields that
/// the shape names first, then `:protocol`. The order of h2 alone is
/// `:method`, `:scheme`, `:authority`, `:path`, `:protocol`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_request_sends_its_pseudo_fields_in_the_order_of_its_shape() {
    pod_testkit::need_loopback!();
    let root = root_file("pseudo-order");
    let shapes = [
        ("existing", h2_road::Shape::existing(), [":method", ":scheme", ":authority", ":path", ":protocol"]),
        ("chrome", h2_road::Shape::browser(), [":method", ":authority", ":scheme", ":path", ":protocol"]),
        ("firefox", h2_road::Shape::firefox(), [":method", ":path", ":authority", ":scheme", ":protocol"]),
    ];
    for (name, shape, order) in shapes {
        let named: Vec<&str> = shape.headers.iter().copied().filter(|h| h.starts_with(':')).collect();
        assert_eq!(named, order[..named.len()], "{name}: the shape's data is the order");
        let (port, seen) = stand_in(Offer::Extended).await;
        let config = config(port, &root);
        let (session, _) = tokio::time::timeout(LIMIT * 2, h2_road::connect_with(&POOL, &config, TOKEN, &shape))
            .await
            .expect("an answer within the limit")
            .expect("a session");
        let block = seen.blocks.lock().unwrap().first().cloned().expect("the request's header block");
        let fields = pod_ws::h2::decode_header_block(&block).expect("an HPACK block");
        let pseudo: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).filter(|n| n.starts_with(':')).collect();
        assert_eq!(pseudo, order, "{name}: the pseudo-fields on the wire: {fields:?}");
        assert_eq!(fields[..5].iter().filter(|(n, _)| n.starts_with(':')).count(), 5, "the pseudo-fields come first");
        let _ = session.send_close(1000, "").await;
    }
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

/// A lost connection is a lost link for each of its sessions at once, and
/// the next session opens a new connection.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lost_connection_ends_each_of_its_sessions_and_the_next_opens_a_new_one() {
    pod_testkit::need_loopback!();
    let root = root_file("lost");
    let (port, seen) = stand_in(Offer::Extended).await;
    let (first, _) = connect(&config(port, &root), TOKEN).await.expect("a first session");
    let (second, road) = connect(&config(port, &root), TOKEN).await.expect("a second session");
    assert_eq!(road, Road::Stream { shared: true });
    for task in seen.tasks.lock().unwrap().drain(..) {
        task.abort();
    }
    for session in [&first, &second] {
        let ended = async {
            loop {
                if let Err(e) = session.read_frame().await {
                    return e;
                }
            }
        };
        tokio::time::timeout(LIMIT, ended).await.expect("each session ends when its connection is lost");
    }
    let (third, road) = connect(&config(port, &root), TOKEN).await.expect("a session on a new connection");
    assert_eq!(road, Road::Stream { shared: false });
    assert_eq!(seen.accepted.load(Ordering::SeqCst), 2);
    each_way(third, MIB, 13).await;
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}

/// The probe of `doctor` names what each stand-in offers, and sends no
/// request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_probe_names_the_alpn_and_the_setting_and_sends_no_request() {
    pod_testkit::need_loopback!();
    let root = root_file("probe");
    let (trust, way) = (Trust::File(root.clone()), way());
    let probe = |port| h2_road::probe(&way, NAME, port, NAME, &trust, LIMIT);
    let h2 = Some("h2".to_string());
    for (offer, expected) in [
        (Offer::Extended, Offered { alpn: h2.clone(), extended: Some(true), streams: Some(32) }),
        (Offer::NoSetting, Offered { alpn: h2.clone(), extended: Some(false), streams: Some(32) }),
        (Offer::Http1Only, Offered { alpn: Some("http/1.1".into()), extended: None, streams: None }),
    ] {
        let (port, seen) = stand_in(offer).await;
        let offered = tokio::time::timeout(LIMIT, probe(port)).await.expect("within the limit").expect("a probe");
        assert_eq!(offered, expected);
        assert_eq!(seen.headers.load(Ordering::SeqCst), 0, "the probe sends no request");
        assert_eq!(seen.upgrades.load(Ordering::SeqCst), 0, "the probe sends no request");
    }
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}
