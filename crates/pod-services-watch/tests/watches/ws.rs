//! `websocket-upgrade` and `forward-banner` (the three forms of a
//! forwarder) against an HTTPS stand-in that upgrades.

use std::time::{Duration, Instant};

use pod_testkit::stand::{Answer, Request, Stand};
use pod_ws::frame::{Frame, OPCODE_BINARY, OPCODE_TEXT, Role, decode, encode};

use crate::common::{NAME, endpoint, fact, file, https, one, text};

/// The frames of the server, written after the head of a 101.
fn frames(list: &[(u8, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (opcode, payload) in list {
        out.extend(encode(&Frame { fin: true, opcode: *opcode, payload: payload.to_vec() }, Role::Server, [0; 4]));
    }
    out
}

/// A stand-in that upgrades each request and then writes `after`; a
/// request with no key gets 400.
fn upgrading(after: Vec<u8>) -> Stand {
    https(move |request: &Request| match request.header("sec-websocket-key") {
        Some(key) => Answer::new(101, after.clone())
            .header("Upgrade", "websocket")
            .header("Connection", "Upgrade")
            .header("Sec-WebSocket-Accept", pod_ws::handshake::accept_key(key)),
        None => Answer::new(400, "no"),
    })
}

/// The frames that the client sent on the first connection that ended, and
/// the head of its request.
fn sent(stand: &Stand) -> (String, Vec<Frame>) {
    let started = Instant::now();
    while stand.seen().is_empty() && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let bytes = stand.seen().into_iter().next().expect("a connection that ended");
    let end = bytes.windows(4).position(|w| w == b"\r\n\r\n").expect("a head") + 4;
    let mut rest = &bytes[end..];
    let mut out = Vec::new();
    while let Ok(Some((frame, used))) = decode(rest, Role::Client) {
        out.push(frame);
        rest = &rest[used..];
    }
    (String::from_utf8_lossy(&bytes[..end]).into_owned(), out)
}

#[tokio::test]
async fn websocket_upgrade_wants_101() {
    pod_testkit::need_loopback!();
    let stand = upgrading(Vec::new());
    let w = "[[watch]]\nkind = \"websocket-upgrade\"\nendpoint = \"ws\"\n";
    let good = endpoint("ws", "wss", NAME, stand.port(), "path = \"/v1\"") + w;
    assert!(one(&file("w1", &good)).await.holds());
    let refusing = https(|_: &Request| Answer::new(403, "no"));
    let bad = endpoint("ws", "wss", NAME, refusing.port(), "path = \"/v1\"") + w;
    let report = one(&file("w2", &bad)).await;
    assert!(text(&report).contains("refused the upgrade: HTTP 403"), "{}", text(&report));
}

const BANNER: &[u8] = b"SSH-2.0-stand\r\n";

fn forwarder(template: &str) -> String {
    fact("template", &format!("\"{template}\""))
        + "[[watch]]\nkind = \"forward-banner\"\ntemplate = \"template\"\ntarget = \"github-ssh\"\n"
}

#[tokio::test]
async fn forward_banner_puts_the_target_in_the_query() {
    pod_testkit::need_loopback!();
    // A Worker's own word comes first, in a text frame: it is no byte of
    // the target.
    let note = br#"{"type":"game.connected"}"#;
    let stand = upgrading(frames(&[(OPCODE_TEXT, note), (OPCODE_BINARY, &BANNER[..9]), (OPCODE_BINARY, &BANNER[9..])]));
    let template = format!("wss://{NAME}:{}/?host={{host}}&port={{port}}", stand.port());
    let report = one(&file("f1", &forwarder(&template))).await;
    assert!(report.holds() && text(&report).contains("SSH-2.0-stand"), "{}", text(&report));
    let (head, _) = sent(&stand);
    assert!(head.starts_with("GET /?host=github.com&port=22 HTTP/1.1"), "{head}");

    let liar = upgrading(frames(&[(OPCODE_BINARY, b"HTTP/1.1 400 Bad Request\r\n")]));
    let template = format!("wss://{NAME}:{}/?host={{host}}&port={{port}}", liar.port());
    let report = one(&file("f2", &forwarder(&template))).await;
    assert!(text(&report).contains("not an SSH banner"), "{}", text(&report));
}

#[tokio::test]
async fn forward_banner_puts_the_target_in_a_first_frame() {
    pod_testkit::need_loopback!();
    // ssheasy answers in a binary frame.
    let stand = upgrading(frames(&[(OPCODE_BINARY, br#"{"status":"ok"}"#), (OPCODE_BINARY, BANNER)]));
    let report = one(&file("f3", &forwarder(&format!("wss://{NAME}:{}/p", stand.port())))).await;
    assert!(report.holds(), "{}", text(&report));
    let (_, got) = sent(&stand);
    assert_eq!(got[0].payload, br#"{"Host":"github.com","Port":22}"#);

    let refusing = upgrading(frames(&[(OPCODE_TEXT, br#"{"status":"denied"}"#)]));
    let report = one(&file("f4", &forwarder(&format!("wss://{NAME}:{}/p", refusing.port())))).await;
    assert!(text(&report).contains("the forwarder refused"), "{}", text(&report));
}

/// A Wisp packet: its type, its stream and its payload.
fn wisp(kind: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![kind];
    out.extend_from_slice(&stream.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

#[tokio::test]
async fn forward_banner_speaks_wisp() {
    pod_testkit::need_loopback!();
    let (go, data) = (wisp(0x03, 0, &128u32.to_le_bytes()), wisp(0x02, 1, BANNER));
    let stand = upgrading(frames(&[(OPCODE_BINARY, &go), (OPCODE_BINARY, &data)]));
    let report = one(&file("f5", &forwarder(&format!("wisp+wss://{NAME}:{}/", stand.port())))).await;
    assert!(report.holds(), "{}", text(&report));
    let (_, got) = sent(&stand);
    let mut connect = wisp(0x01, 1, &[0x01, 22, 0]);
    connect.extend_from_slice(b"github.com");
    assert_eq!(got[0].payload, connect);

    let closed = wisp(0x04, 1, &[0x42]);
    let refusing = upgrading(frames(&[(OPCODE_BINARY, &closed)]));
    let report = one(&file("f6", &forwarder(&format!("wisp+wss://{NAME}:{}/", refusing.port())))).await;
    assert!(text(&report).contains("reason 0x42"), "{}", text(&report));
}
