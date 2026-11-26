//! Shadowsocks 2022 (SIP022), against a stand-in server
//! (`pod_egress::standin::shadowsocks`, feature `standin`) written from shadowsocks.org's `sip022.html` with the AEAD crates and BLAKE3
//! themselves, not this crate's codec: each stream's key by `derive_key` with
//! "shadowsocks 2022 session subkey" over the key and the salt; the
//! request's fixed header (type 0, a timestamp, the next header's length)
//! and its variable one (the address, then padding); the response's fixed
//! header (type 1, a timestamp, the request's salt, the first payload's
//! length). With each method: `open` names the target and carries bytes
//! each way; a wrong key ends in silence that names the server; a server
//! whose clock is 60 s off, and one that names another request's salt, are
//! refused with their words. The specification gives no test vectors of
//! TCP, so the stand-in is the reader of record. The plant of the Prove (the
//! timestamp in milliseconds) is refused by the stand-in as a replay, and
//! the first test fails.

use std::time::Duration;

use pod_egress::shadowsocks::{self, ChunkError, Method, Node, OpenError};
use pod_egress::standin::shadowsocks::{Answer, RIGHT, Served};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[path = "common/way.rs"]
mod way;

/// A key of `method`'s length, and its base64.
fn key_of(method: Method, seed: u8) -> (Vec<u8>, String) {
    use base64::Engine as _;
    let key: Vec<u8> = (0..method.key_len()).map(|i| seed.wrapping_add(i as u8)).collect();
    let text = base64::engine::general_purpose::STANDARD.encode(&key);
    (key, text)
}

/// A stand-in server of SIP022 for one connection
/// (`pod_egress::standin::shadowsocks::sip022`): with the right key and a
/// timestamp within 30 s it answers `banner` and echoes; a wrong key is
/// silence until the client goes, and a timestamp out of the window a
/// close, as shadowsocks-rust does. The target that it read comes back.
async fn stand_in(method: Method, psk: Vec<u8>, banner: &'static [u8], answer: Answer) -> (u16, Served) {
    pod_egress::standin::shadowsocks::sip022(method, psk, banner, answer).await.unwrap()
}

fn dial(answer: Duration) -> shadowsocks::Dial {
    let (way, trust) = (way::direct(), pod_tls::Trust::Default);
    let timeout = Duration::from_secs(10);
    shadowsocks::Dial { way, trust, timeout, answer: Some(answer), silence_is_ready: false }
}

#[tokio::test]
async fn each_2022_method_names_the_target_reads_the_answer_and_carries_bytes_each_way() {
    for method in Method::SIP022 {
        let (psk, text) = key_of(method, 3);
        let (port, server) = stand_in(method, psk, b"SSH-2.0-stand-in\r\n", RIGHT).await;
        let node = Node::new(method, &text, "127.0.0.1", port).unwrap();
        let opened = shadowsocks::open(&node, "target.test", 2222, &dial(Duration::from_secs(5))).await.unwrap();
        assert!(opened.answered_in.is_some(), "{method:?}");
        let mut stream = opened.carried.stream;
        let mut banner = [0u8; 18];
        stream.read_exact(&mut banner).await.unwrap();
        assert_eq!(&banner, b"SSH-2.0-stand-in\r\n", "{method:?}");
        let sent: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let (mut read, mut write) = tokio::io::split(stream);
        let up = sent.clone();
        let writer = tokio::spawn(async move {
            write.write_all(&up).await.unwrap();
            write.shutdown().await.unwrap();
        });
        let mut back = vec![0u8; sent.len()];
        read.read_exact(&mut back).await.unwrap();
        writer.await.unwrap();
        assert_eq!(back, sent, "{method:?}");
        assert_eq!(server.await.unwrap().as_deref(), Some("target.test:2222"), "{method:?}");
        assert_eq!(opened.carried.status.get(), None, "an ordinary end");
    }
}

#[tokio::test]
async fn a_wrong_key_ends_in_silence_that_names_the_server_and_not_the_key() {
    let method = Method::Blake3Aes256Gcm;
    let (psk, _) = key_of(method, 1);
    let (_, wrong) = key_of(method, 2);
    let (port, _server) = stand_in(method, psk, b"SSH-2.0-x\r\n", RIGHT).await;
    let node = Node::new(method, &wrong, "127.0.0.1", port).unwrap();
    let e = shadowsocks::open(&node, "target.test", 22, &dial(Duration::from_millis(400))).await.unwrap_err();
    assert!(matches!(e, OpenError::Silent { ended: None, .. }), "{e:?}");
    let line = e.line(&node.shown(), "target.test:22");
    assert!(line.contains(&format!("ss://127.0.0.1:{port}")) && line.contains("password or the method"), "{line}");
    assert!(!line.contains(&wrong) && !format!("{node:?}").contains(&wrong), "no key in the words");
}

#[tokio::test]
async fn a_server_60_s_off_and_one_that_names_another_salt_are_refused_with_their_words() {
    let method = Method::Blake3Aes128Gcm;
    for (answer, want) in [
        (Answer { skew: 60, other_salt: false }, "this host's clock, which reads"),
        (Answer { skew: 0, other_salt: true }, "another request's salt"),
    ] {
        let (psk, text) = key_of(method, 7);
        let (port, _server) = stand_in(method, psk, b"SSH-2.0-x\r\n", answer).await;
        let node = Node::new(method, &text, "127.0.0.1", port).unwrap();
        let e = shadowsocks::open(&node, "target.test", 22, &dial(Duration::from_secs(5))).await.unwrap_err();
        let line = e.line(&node.shown(), "target.test:22");
        match (&e, answer.skew) {
            (OpenError::Chunk(ChunkError::Clock { skew, .. }), 60) => assert!((59..=61).contains(skew), "{skew}"),
            (OpenError::Chunk(ChunkError::Salt), 0) => {}
            other => panic!("{other:?}"),
        }
        assert!(line.contains("its answer was refused") && line.contains(want), "{line}");
    }
}

#[test]
fn a_2022_key_is_read_from_an_ss_line_and_one_of_several_users_is_refused() {
    let (_, text) = key_of(Method::Blake3Aes256Gcm, 9);
    // Percent-encoded, as SIP002 asks of the 2022 methods: `+`, `/` and `=`.
    let encoded: String = text.replace('+', "%2B").replace('/', "%2F").replace('=', "%3D");
    let line = ["ss://", "2022-blake3-aes-256-gcm", ":", &encoded, "@127.0.0.1:8388#t"].concat();
    let node = Node::parse(&line).unwrap();
    assert_eq!(node.method, Method::Blake3Aes256Gcm);
    assert_eq!(node.master_key().len(), 32);
    // Kept and read back.
    assert!(Node::parse(&node.to_uri()).unwrap().same_server(&node));
    let users = ["ss://", "2022-blake3-aes-256-gcm", ":", &encoded, "%3A", &encoded, "@127.0.0.1:8388"].concat();
    let e = Node::parse(&users).unwrap_err();
    assert!(e.contains("SIP023") && !e.contains(&text[..8]), "{e}");
    let short = ["ss://", "2022-blake3-aes-128-gcm", ":", &encoded, "@127.0.0.1:8388"].concat();
    let e = Node::parse(&short).unwrap_err();
    assert!(e.contains("base64 of 16 bytes"), "{e}");
}

/// A [`shadowsocks::Road`] is a dialer: a dial names the target in the
/// address and carries the server's answer and an echo. A wrong key is a
/// silence of the relay, a failure of the class timeout, in the line that
/// names the server, with no key in it.
#[tokio::test]
async fn a_shadowsocks_road_is_a_dialer_and_a_silence_is_a_typed_failure() {
    let method = Method::Blake3Aes256Gcm;
    let (psk, text) = key_of(method, 3);
    let (port, server) = stand_in(method, psk, b"SSH-2.0-stand-in\r\n", RIGHT).await;
    let node = Node::new(method, &text, "127.0.0.1", port).unwrap();
    let road = shadowsocks::Road { node, dial: dial(Duration::from_secs(5)) };
    let dialer: &dyn pod_core::Dial = &road;
    assert!(dialer.names_inside());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let target = pod_core::Target::new("target.test", 2222);
    let mut io = dialer.dial(&target, deadline).await.unwrap_or_else(|e| panic!("{e}")).io;
    let (mut banner, mut back) = ([0u8; 18], [0u8; 9]);
    io.read_exact(&mut banner).await.unwrap();
    io.write_all(b"a dialer!").await.unwrap();
    io.read_exact(&mut back).await.unwrap();
    assert_eq!((&banner, &back), (b"SSH-2.0-stand-in\r\n", b"a dialer!"));
    drop(io);
    assert_eq!(server.await.unwrap().as_deref(), Some("target.test:2222"));
    let (psk, _) = key_of(method, 1);
    let (_, wrong) = key_of(method, 2);
    let (port, _server) = stand_in(method, psk, b"SSH-2.0-x\r\n", RIGHT).await;
    let node = Node::new(method, &wrong, "127.0.0.1", port).unwrap();
    let wrong_road = shadowsocks::Road { node, dial: dial(Duration::from_millis(400)) };
    let failure = (&wrong_road as &dyn pod_core::Dial).dial(&target, deadline).await.expect_err("a silence");
    assert_eq!((failure.stage, failure.class), (pod_core::Stage::Relay, pod_core::Class::Timeout), "{failure}");
    let words = format!(
        "the Shadowsocks server ss://127.0.0.1:{port}: no answer within 0.4 s of the salt and target.test:2222"
    );
    assert!(failure.text().starts_with(&words) && !failure.text().contains(&wrong), "{failure}");
}
