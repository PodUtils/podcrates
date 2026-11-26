//! Shadowsocks' AEAD form, checked three ways. The vectors of
//! `pod-testkit::vectors` (SIP004's construction, whose bytes OpenSSL
//! computed), run against this crate's codec; the master key and the stream
//! key that Python's `hashlib`, `hmac` and RFC 5869's HKDF-SHA1 give. A codec
//! written here from the specification
//! (shadowsocks.org's `aead.html`), with the AEAD crates themselves and not
//! this crate's wrapper of them: it opens this crate's stream, and this
//! crate opens its own. And `open` against a stand-in server of that codec on the loopback,
//! where a wrong password ends in silence that names the server (the
//! stand-in of `pod_egress::standin::shadowsocks`, feature `standin`). Then the
//! `ss://` URIs of SIP002, and the forms of a subscription's list.

use std::time::Duration;

use pod_egress::shadowsocks::{self, Decoder, Encoder, Method, Node, OpenError};
use pod_egress::standin::shadowsocks::{Served, Spec, evp};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[path = "common/way.rs"]
mod way;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A whole stream as the specification writes it: the salt, then for each
/// payload of 0x3FFF bytes at most its sealed length and itself sealed.
fn spec_stream(method: Method, master: &[u8], salt: &[u8], data: &[u8]) -> Vec<u8> {
    let spec = Spec::aead(method, master, salt).unwrap();
    let mut out = salt.to_vec();
    for (i, payload) in data.chunks(0x3FFF).enumerate() {
        let n = 2 * i as u64;
        out.extend(spec.seal(n, &(payload.len() as u16).to_be_bytes()));
        out.extend(spec.seal(n + 1, payload));
    }
    out
}

/// A whole stream as the specification reads it: its payloads, joined, or
/// `None` at the first piece that does not open.
fn spec_read(method: Method, master: &[u8], wire: &[u8]) -> Option<Vec<u8>> {
    let salt_len = method.salt_len();
    let spec = Spec::aead(method, master, wire.get(..salt_len)?)?;
    let (mut at, mut n, mut out) = (salt_len, 0u64, Vec::new());
    while at < wire.len() {
        let len = spec.open(n, wire.get(at..at + 18)?)?;
        let len = usize::from(u16::from_be_bytes([len[0], len[1]]));
        assert!(len <= 0x3FFF, "the reserved bits are zero");
        out.extend(spec.open(n + 1, wire.get(at + 18..at + 18 + len + 16)?)?);
        at += 18 + len + 16;
        n += 2;
    }
    Some(out)
}

/// An `ss://` URI whose user part is percent-encoded text, built from its
/// parts: a secret scanner takes a URI with a user and a password, written
/// whole in a source, for a credential.
fn plain(method: &str, password: &str, rest: &str) -> String {
    ["ss://", method, ":", password, "@", rest].concat()
}

fn data(len: usize) -> Vec<u8> {
    (0..len as u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 11) as u8).collect()
}

#[test]
fn the_master_key_is_evp_bytes_to_key_with_md5() {
    for password in [&b"foobar"[..], &b"a"[..], &b"a password of more than one block of MD5 and some"[..]] {
        for len in [16, 32] {
            assert_eq!(*shadowsocks::master_key(password, len), evp(password, len), "{len}");
        }
    }
    // Python's hashlib, from the same definition.
    let foobar = "3858f62230ac3c915f300c664312c63f568378529614d22ddb49237d2f60bfdf";
    assert_eq!(hex(&shadowsocks::master_key(b"foobar", 32)), foobar);
    assert_eq!(hex(&shadowsocks::master_key(b"foobar", 16)), foobar[..32]);
}

#[test]
fn a_stream_s_key_is_hkdf_sha1_of_the_salt_as_openssl_computes_it() {
    let salt32: Vec<u8> = (0..32).collect();
    let key = shadowsocks::subkey(&evp(b"foobar", 32), &salt32);
    assert_eq!(hex(&key), "c4f0e9818348b2f30188d82b37a4cddc9f5ea531070ec67225160209faff573c");
    let salt16: Vec<u8> = (0..16).collect();
    assert_eq!(hex(&shadowsocks::subkey(&evp(b"foobar", 16), &salt16)), "e59e945699e8699144c332b9e641ef65");
}

/// The first chunk of each method, with a fixed salt (0, 1, 2, ...), the
/// password `foobar`, and the payload `example.com:22` then `hello`: the
/// SIP004 construction, whose bytes OpenSSL sealed. The vector is data of
/// `pod-testkit` ([`pod_testkit::vectors::SHADOWSOCKS`]), and this codec
/// answers it. A round trip proves only that this client and this server
/// agree; the vector says that the bytes are the protocol's.
#[test]
fn the_published_sip004_answers_are_the_ones_that_this_codec_makes() {
    use pod_testkit::vectors::{ShadowsocksCodec, check_shadowsocks};
    struct Ours;
    impl ShadowsocksCodec for Ours {
        fn subkey(&self, master: &[u8], salt: &[u8]) -> Vec<u8> {
            shadowsocks::subkey(master, salt).to_vec()
        }

        fn seal(&self, method: &str, master: &[u8], salt: &[u8], plain: &[u8]) -> Vec<u8> {
            let method = Method::parse(method).expect("a method of the vector");
            let mut wire = Vec::new();
            Encoder::with_salt(method, master, salt).expect("a key of the method").seal(plain, &mut wire).unwrap();
            wire
        }

        fn open(&self, method: &str, master: &[u8], wire: &[u8]) -> Option<Vec<u8>> {
            let method = Method::parse(method).expect("a method of the vector");
            let mut decoder = Decoder::new(method, master);
            decoder.feed(wire);
            let mut plain = Vec::new();
            loop {
                match decoder.chunk() {
                    Ok(Some(piece)) => plain.extend_from_slice(&piece),
                    Ok(None) => return Some(plain),
                    Err(_) => return None,
                }
            }
        }
    }
    assert_eq!(check_shadowsocks(&Ours).unwrap(), 3 * pod_testkit::vectors::SHADOWSOCKS.len());
}

/// This crate's stream (podssh's codec) opens with the specification's
/// reading, chunk by chunk, and the specification's stream opens with it, across
/// several chunks of the largest size.
#[test]
fn podssh_and_the_specification_read_each_other_s_streams() {
    for method in Method::ALL {
        let master = evp(b"round trip", method.key_len());
        let sent = data(3 * 0x3FFF + 1234);
        let mut wire = Vec::new();
        let mut encoder = Encoder::new(method, &master).unwrap();
        encoder.seal(&sent[..5000], &mut wire).unwrap();
        encoder.seal(&sent[5000..], &mut wire).unwrap();
        assert_eq!(spec_read(method, &master, &wire).as_deref(), Some(&sent[..]), "{method:?}: the spec reads ours");
        let salt = vec![0x42u8; method.salt_len()];
        let theirs = spec_stream(method, &master, &salt, &sent);
        let mut decoder = Decoder::new(method, &master);
        let mut back = Vec::new();
        for piece in theirs.chunks(1000) {
            decoder.feed(piece);
            while let Some(plain) = decoder.chunk().unwrap() {
                back.extend_from_slice(&plain);
            }
        }
        assert_eq!(back, sent, "{method:?}: ours reads the spec");
        // The nonce of the length and of the payload are two: one stream
        // with one count for both does not open.
        let skewed = Spec::aead(method, &master, &salt).unwrap();
        let mut wrong = salt.clone();
        wrong.extend(skewed.seal(0, &5u16.to_be_bytes()));
        wrong.extend(skewed.seal(0, b"hello"));
        let mut decoder = Decoder::new(method, &master);
        decoder.feed(&wrong);
        assert!(decoder.chunk().is_err(), "{method:?}: a nonce used twice");
    }
}

/// A stand-in server of the specification's codec on the loopback
/// (`pod_egress::standin::shadowsocks::aead`): it reads the salt and the
/// address; with the right key it answers with `banner` and then echoes,
/// and with another it stays silent until the client goes. The address that
/// it read comes back.
async fn stand_in(method: Method, password: &[u8], banner: &'static [u8]) -> (u16, Served) {
    pod_egress::standin::shadowsocks::aead(method, password, banner).await.unwrap()
}

fn dial(answer: Duration) -> shadowsocks::Dial {
    let (way, trust) = (way::direct(), pod_tls::Trust::Default);
    let timeout = Duration::from_secs(10);
    shadowsocks::Dial { way, trust, timeout, answer: Some(answer), silence_is_ready: false }
}

#[tokio::test]
async fn open_names_the_target_reads_the_answer_and_carries_bytes_each_way() {
    for method in Method::ALL {
        let (port, server) = stand_in(method, b"stand-in", b"SSH-2.0-stand-in\r\n").await;
        let node = Node::new(method, "stand-in", "127.0.0.1", port).unwrap();
        let opened = shadowsocks::open(&node, "target.test", 2222, &dial(Duration::from_secs(5))).await.unwrap();
        assert!(opened.answered_in.is_some(), "{method:?}");
        let mut stream = opened.carried.stream;
        let mut banner = [0u8; 18];
        stream.read_exact(&mut banner).await.unwrap();
        assert_eq!(&banner, b"SSH-2.0-stand-in\r\n");
        let sent = data(100_000);
        let (mut read, mut write) = tokio::io::split(stream);
        let writer = tokio::spawn(async move {
            write.write_all(&sent).await.unwrap();
            write.shutdown().await.unwrap();
            sent
        });
        let mut back = vec![0u8; 100_000];
        read.read_exact(&mut back).await.unwrap();
        let sent = writer.await.unwrap();
        assert_eq!(back, sent, "{method:?}");
        let mut end = Vec::new();
        read.read_to_end(&mut end).await.unwrap();
        assert!(end.is_empty());
        assert_eq!(server.await.unwrap().as_deref(), Some("target.test:2222"));
        assert_eq!(opened.carried.status.get(), None, "an ordinary end");
    }
}

/// The end of this side's input is TCP's half-close: 1 MiB goes and
/// this side's input ends at once, while the echo is still on its way; the
/// server reads to the end of its input and the echo comes whole.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_end_of_input_is_a_half_close_and_the_echo_of_a_mebibyte_comes_whole() {
    let method = Method::Chacha20IetfPoly1305;
    let (port, server) = stand_in(method, b"stand-in", b"SSH-2.0-stand-in\r\n").await;
    let node = Node::new(method, "stand-in", "127.0.0.1", port).unwrap();
    let opened = shadowsocks::open(&node, "target.test", 2222, &dial(Duration::from_secs(5))).await.unwrap();
    let (mut read, mut write) = tokio::io::split(opened.carried.stream);
    let sent = data(1 << 20);
    let kept = sent.clone();
    let writer = tokio::spawn(async move {
        write.write_all(&kept).await.unwrap();
        write.shutdown().await.unwrap();
    });
    let mut all = Vec::new();
    let read_all = tokio::time::timeout(Duration::from_secs(30), read.read_to_end(&mut all));
    read_all.await.expect("the end within the limit").unwrap();
    writer.await.unwrap();
    let banner: &[u8] = b"SSH-2.0-stand-in\r\n";
    assert!(all.starts_with(banner), "the answer first");
    let echo = &all[banner.len()..];
    assert_eq!(echo.len(), sent.len(), "the echo after the end of this side's input");
    assert!(echo == sent.as_slice(), "the echo came changed");
    assert_eq!(server.await.unwrap().as_deref(), Some("target.test:2222"));
    assert_eq!(opened.carried.status.get(), None, "an ordinary end");
}

#[tokio::test]
async fn a_wrong_password_ends_in_silence_that_names_the_server_and_not_the_password() {
    let method = Method::Chacha20IetfPoly1305;
    let (port, _server) = stand_in(method, b"the server's", b"SSH-2.0-x\r\n").await;
    let node = Node::new(method, "not-the-password", "127.0.0.1", port).unwrap();
    let e = shadowsocks::open(&node, "target.test", 22, &dial(Duration::from_millis(400))).await.unwrap_err();
    assert!(matches!(e, OpenError::Silent { ended: None, proxy: None, .. }), "{e:?}");
    let line = e.line(&node.shown(), "target.test:22");
    let named = format!("the Shadowsocks server ss://127.0.0.1:{port}: no answer within 0.4 s");
    assert!(line.starts_with(&named) && line.contains("password or the method"), "{line}");
    assert!(!line.contains("not-the-password") && !format!("{node:?}").contains("not-the-password"));
}

/// A pipe takes a silent server (its client may speak first); a server
/// that then closes with no byte of its own, as outline-ss-server ends a
/// stream that it cannot open, leaves `Unanswered`, which a pipe says with
/// the password as a cause, never a quiet end.
#[tokio::test]
async fn a_pipe_that_took_the_silence_learns_at_the_end_that_nothing_answered() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        let _ = tcp.read(&mut buf).await;
        tokio::time::sleep(Duration::from_millis(600)).await;
    });
    let method = Method::Chacha20IetfPoly1305;
    let node = Node::new(method, "not-the-password", "127.0.0.1", port).unwrap();
    let pipe = shadowsocks::Dial { silence_is_ready: true, ..dial(Duration::from_millis(200)) };
    let mut opened = shadowsocks::open(&node, "target.test", 22, &pipe).await.expect("the silence is taken");
    let mut got = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), opened.carried.stream.read_to_end(&mut got)).await.unwrap().unwrap();
    assert!(got.is_empty());
    server.await.unwrap();
    assert_eq!(opened.carried.status.get(), Some(shadowsocks::Ended::Unanswered));
}

#[test]
fn sip002_uris_are_read_and_their_password_never_shown() {
    // The examples of SIP002: base64url user parts, with and without a plugin.
    let one = Node::parse("ss://YWVzLTEyOC1nY206dGVzdA@192.168.100.1:8888#Example1").unwrap();
    assert_eq!((one.method, one.host.as_str(), one.port), (Method::Aes128Gcm, "192.168.100.1", 8888));
    assert_eq!(one.shown(), "ss://192.168.100.1:8888 (Example1)");
    assert_eq!(*one.master_key(), evp(b"test", 16));
    let plugin = Node::parse("ss://cmM0LW1kNTpwYXNzd2Q@192.168.100.1:8888/?plugin=obfs-local%3Bobfs%3Dhttp#Example2");
    assert!(plugin.unwrap_err().contains("the plugin obfs-local, another program"));
    // Percent-encoded, padded base64 in the standard alphabet, the older
    // form, an IPv6 address.
    let percent = Node::parse(&plain("aes-256-gcm", "p%40ss%3Aword", "[2001:db8::1]:443/#a%20name")).unwrap();
    assert_eq!((percent.host.as_str(), percent.tag.as_deref()), ("2001:db8::1", Some("a name")));
    assert_eq!(*percent.master_key(), evp(b"p@ss:word", 32));
    assert_eq!(percent.shown(), "ss://[2001:db8::1]:443 (a name)");
    let padded = Node::parse("ss://Y2hhY2hhMjAtaWV0Zi1wb2x5MTMwNTo/YQ==@h.example:8388").unwrap();
    assert_eq!((padded.method, padded.master_key().to_vec()), (Method::Chacha20IetfPoly1305, evp(b"?a", 32)));
    let older = Node::parse("ss://YWVzLTI1Ni1nY206c2VjcmV0QGguZXhhbXBsZTo4Mzg4").unwrap();
    assert_eq!((older.method, older.host.as_str(), older.port), (Method::Aes256Gcm, "h.example", 8388));
    assert_eq!(Node::parse(&older.to_uri()).unwrap(), older, "a node written out reads back");
    // Each refusal says why, never the password.
    for (line, why) in [
        // A 2022 method's key is base64 of its length (24 bytes here, not 32).
        (
            plain("2022-blake3-aes-256-gcm", "c2VjcmV0c2VjcmV0c2VjcmV0c2VjcmV0", "h.example:443"),
            "not the base64 of 32 bytes",
        ),
        (plain("aes-256-gcm", "hunter2", "h.example"), "no port"),
        (plain("aes-256-gcm", "hunter2", "h.example:0"), "not a port"),
        (plain("aes-256-gcm", "", "h.example:443"), "password is empty"),
        (plain("rc4-md5", "hunter2", "h.example:443"), "rc4-md5 is not one"),
        ("ss://aGVsbG8gaHVudGVyMg@h.example:443".to_string(), "no method:password"),
        ("ss://hunter2!".to_string(), "neither"),
        ("http://hunter2@h.example:443".to_string(), "not an ss://"),
    ] {
        let e = Node::parse(&line).unwrap_err();
        assert!(e.contains(why), "{line}: {e}");
        assert!(!e.contains("hunter2") && !e.contains("c2VjcmV0"), "{e}");
    }
    assert!(!format!("{one:?}").contains("test"), "Debug hides the password");
}

#[test]
fn a_subscription_s_list_comes_as_lines_as_base64_or_as_sip008() {
    use base64::Engine as _;
    let lines = format!(
        "ss://YWVzLTEyOC1nY206dGVzdA@192.168.100.1:8888#one\nvmess://eyJhZGQiOiJ4In0\n{}\n\
         ss://YWVzLTEyOC1nY206dGVzdA@192.168.100.1:8888\n",
        plain("aes-256-gcm", "pw", "h.example:443/?plugin=v2ray-plugin")
    );
    let listed = shadowsocks::subscription(lines.as_bytes()).unwrap();
    assert_eq!(listed.nodes.len(), 1, "each node once: {listed:?}");
    assert_eq!((listed.others, listed.refused.len()), (1, 1));
    assert!(listed.refused[0].starts_with("line 3: it names the plugin v2ray-plugin"), "{:?}", listed.refused);
    let encoded = base64::engine::general_purpose::STANDARD.encode(lines);
    let pieces: Vec<&str> = encoded.as_bytes().chunks(76).map(|c| std::str::from_utf8(c).unwrap()).collect();
    let wrapped = pieces.join("\n");
    assert_eq!(shadowsocks::subscription(wrapped.as_bytes()).unwrap(), listed, "base64 in lines of 76");
    let json = r#"{"version":1,"servers":[
        {"server":"h.example","server_port":8388,"password":"pw","method":"chacha20-ietf-poly1305","remarks":"tag"},
        {"server":"g.example","server_port":8388,"password":"pw","method":"2022-blake3-aes-128-gcm"}]}"#;
    let sip008 = shadowsocks::subscription(json.as_bytes()).unwrap();
    assert_eq!(sip008.nodes[0].shown(), "ss://h.example:8388 (tag)");
    // "pw" is no key of 16 bytes in base64, as 2022-blake3-aes-128-gcm takes.
    assert!(sip008.refused[0].starts_with("server 2: its key is not the base64 of 16 bytes"), "{:?}", sip008.refused);
    for bad in ["", "   ", "<html>not a list</html>", "{\"servers\":3}"] {
        assert!(shadowsocks::subscription(bad.as_bytes()).is_err(), "{bad}");
    }
}
