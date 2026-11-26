//! Outline's keys: a private key file builds the whole node, with the
//! `wss://` URL whose path is the server's secret, and no line, `Display` or
//! `Debug` shows that path; a salt prefix (in the YAML of a dynamic key, or
//! as `prefix=` of an `ss://` link) goes first in each stream's salt, and
//! the stand-in server of `pod_egress::standin::shadowsocks` reads the
//! stream as any other (planted: the prefix not sent).

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use pod_egress::shadowsocks::{self, Method, Node};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[path = "common/way.rs"]
mod way;

/// A key of Outline's YAML: a WebSocket with a secret path, and a prefix.
const KEY: &str = "\
transport:
  $type: tcpudp
  tcp:
    <<: &shared
      $type: shadowsocks
      endpoint:
        $type: websocket
        url: wss://ss.example/made-up-secret-path/tcp
      cipher: chacha20-ietf-poly1305
      secret: made-up-outline-secret
    prefix: \"\\u0016\\u0003\\u0001\\u0000\\u00a8\\u0001\\u0001\"
  udp: *shared
";

/// A new private file of `text` in the temporary directory.
fn private_file(text: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let name = format!("pod-egress-outline-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst));
    let path = std::env::temp_dir().join(name);
    let mut file = pod_core::files::create_new_private(&path).unwrap();
    file.write_all(text.as_bytes()).unwrap();
    path
}

fn dial(answer: Option<Duration>) -> shadowsocks::Dial {
    let (way, trust) = (way::direct(), pod_tls::Trust::Default);
    shadowsocks::Dial { way, trust, timeout: Duration::from_secs(10), answer, silence_is_ready: false }
}

#[test]
fn a_key_file_builds_the_whole_node_and_no_line_shows_its_path_or_secret() {
    let path = private_file(KEY);
    let node = shadowsocks::key_file(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(node.ws.as_ref().map(|u| u.target.as_str()), Some("/made-up-secret-path/tcp"));
    assert_eq!((node.method, node.host.as_str(), node.port), (Method::Chacha20IetfPoly1305, "ss.example", 443));
    assert_eq!(node.prefix(), Some(&[0x16, 0x03, 0x01, 0x00, 0xa8, 0x01, 0x01][..]));
    assert_eq!(node.to_string(), "wss://ss.example/...");
    for shown in [node.to_string(), format!("{node:?}"), node.shown()] {
        assert!(!shown.contains("made-up-secret-path") && !shown.contains("made-up-outline-secret"), "{shown}");
    }
    // A key that cannot build a node: the file is named, its text is not.
    let bad = private_file(&KEY.replace("cipher: chacha20-ietf-poly1305", "cipher: rc4-md5"));
    let why = shadowsocks::key_file(&bad).unwrap_err();
    std::fs::remove_file(&bad).unwrap();
    assert!(why.starts_with(&bad.display().to_string()) && why.contains("rc4-md5 is not one"), "{why}");
    assert!(!why.contains("made-up-secret-path") && !why.contains("made-up-outline-secret"), "{why}");
}

#[cfg(unix)]
#[test]
fn a_key_file_that_others_can_read_is_refused() {
    use std::os::unix::fs::PermissionsExt as _;
    let path = private_file(KEY);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let why = shadowsocks::key_file(&path).unwrap_err();
    std::fs::remove_file(&path).unwrap();
    assert!(why.contains(&path.display().to_string()), "{why}");
    assert!(!why.contains("made-up-outline-secret"), "{why}");
}

/// Planted: the prefix not sent, and the salt's first bytes are random.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_prefix_goes_first_in_the_salt_and_the_server_reads_the_stream() {
    // The bytes on the wire: a server that keeps what came first.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let first = tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.unwrap();
        let mut salt = [0u8; 32];
        tcp.read_exact(&mut salt).await.unwrap();
        salt
    });
    let link = |port: u16| {
        let (method, password) = ("chacha20-ietf-poly1305", "stand-in");
        format!("ss://{method}:{password}@127.0.0.1:{port}/?outline=1&prefix=POST%20")
    };
    let node = Node::parse(&link(port)).unwrap();
    assert_eq!(node.prefix(), Some(&b"POST "[..]));
    assert_eq!(Node::parse(&node.to_uri()).unwrap().prefix(), Some(&b"POST "[..]), "written out, read back");
    let _opened = shadowsocks::open(&node, "target.test", 22, &dial(None)).await.unwrap();
    let salt = tokio::time::timeout(Duration::from_secs(10), first).await.unwrap().unwrap();
    assert_eq!(&salt[..5], b"POST ", "the prefix first");
    // The server reads the salt with its prefix as any salt.
    let banner: &'static [u8] = b"SSH-2.0-stand-in\r\n";
    let (port, served) = shadowsocks_standin(banner).await;
    let node = Node::parse(&link(port)).unwrap();
    let opened = shadowsocks::open(&node, "target.test", 2222, &dial(Some(Duration::from_secs(5)))).await.unwrap();
    let mut stream = opened.carried.stream;
    let mut got = vec![0u8; banner.len()];
    stream.read_exact(&mut got).await.unwrap();
    assert_eq!(got, banner);
    stream.write_all(b"echo").await.unwrap();
    let mut back = [0u8; 4];
    stream.read_exact(&mut back).await.unwrap();
    assert_eq!(&back, b"echo");
    drop(stream);
    assert_eq!(served.await.unwrap().as_deref(), Some("target.test:2222"));
}

#[test]
fn a_prefix_that_leaves_too_few_random_bytes_is_refused() {
    // aes-128-gcm has a salt of 16 bytes: 8 of a prefix at most.
    let link = "ss://aes-128-gcm:pw@h.example:443/?prefix=HTTP%2F1.1%20";
    let why = Node::parse(link).unwrap_err();
    assert!(why.contains("9 bytes; with aes-128-gcm it has 8 at most"), "{why}");
    let wide = Node::parse("ss://aes-256-gcm:pw@h.example:443/?prefix=%C4%80").unwrap_err();
    assert!(wide.contains("past U+00FF"), "{wide}");
}

async fn shadowsocks_standin(banner: &'static [u8]) -> (u16, pod_egress::standin::shadowsocks::Served) {
    pod_egress::standin::shadowsocks::aead(Method::Chacha20IetfPoly1305, b"stand-in", banner).await.unwrap()
}
