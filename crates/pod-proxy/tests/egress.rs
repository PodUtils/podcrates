//! The ways of podnet's egress, with stand-ins on the loopback: a proxy that
//! asks for Digest, a proxy on a Unix socket, a proxy at a command, a
//! refusal with its words in the body, the decisions of an observer, the
//! resolver of the proxy's own name, and a silent address in a race.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pod_core::{BoxFuture, Class, Failure, Found, Observer, Resolve, Source, Stage};
use pod_proxy::resolve::System;
use pod_proxy::{DialError, Dialer, Environment, Proxy, ProxyChoice, Settings, Stream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const LIMIT: Duration = Duration::from_secs(5);

fn dialer(choice: ProxyChoice) -> Dialer {
    Dialer::new(Settings::new("podnet", choice), Arc::new(System::new()))
}

/// The stream to `host:port` through `proxy`.
async fn through(proxy: &Proxy, host: &str, port: u16) -> Result<Stream, DialError> {
    dialer(ProxyChoice::Via(proxy.clone())).stream(host, port, LIMIT).await.map(|(stream, _)| stream)
}

/// A request head, read to its blank line.
async fn read_head<S: tokio::io::AsyncRead + Unpin>(s: &mut S) -> Option<String> {
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        s.read_exact(&mut b).await.ok()?;
        head.push(b[0]);
    }
    String::from_utf8(head).ok()
}

/// A proxy that asks for Digest. The first CONNECT (Basic) gets 407 with a
/// challenge; the second connection carries the Digest answer, and the
/// proxy checks it.
#[tokio::test]
async fn a_digest_proxy_takes_the_answer() {
    pod_testkit::need_loopback!();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut first, _) = listener.accept().await.unwrap();
        let head = read_head(&mut first).await.unwrap();
        assert!(head.contains("Proxy-Authorization: Basic "), "{head}");
        first
            .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Digest realm=\"podnet-test\", nonce=\"abc123\", qop=\"auth\", algorithm=MD5\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
        drop(first);
        let (mut second, _) = listener.accept().await.unwrap();
        let head = read_head(&mut second).await.unwrap();
        let line =
            head.lines().find(|l| l.starts_with("Proxy-Authorization: Digest ")).expect("a Digest answer").to_string();
        let field = |name: &str| {
            let start = line.find(&format!("{name}=")).unwrap() + name.len() + 1;
            let rest = &line[start..];
            let rest = rest.trim_start_matches('"');
            rest[..rest.find(['"', ',']).unwrap_or(rest.len())].to_string()
        };
        let challenge = pod_proxy::digest::parse_challenge(
            "Digest realm=\"podnet-test\", nonce=\"abc123\", qop=\"auth\", algorithm=MD5",
        )
        .unwrap();
        let want = pod_proxy::digest::authorization(
            &challenge,
            "user",
            "pa:ss",
            "CONNECT",
            "example.test:443",
            &field("cnonce"),
            1,
        );
        assert_eq!(&line["Proxy-Authorization: ".len()..], want);
        second.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n").await.unwrap();
        second.write_all(b"through").await.unwrap();
    });
    let proxy = Proxy::parse(&format!("http://user:pa%3Ass@{addr}")).unwrap();
    let mut io = through(&proxy, "example.test", 443).await.unwrap();
    let mut got = [0u8; 7];
    io.read_exact(&mut got).await.unwrap();
    assert_eq!(&got, b"through");
    server.await.unwrap();
}

/// A Digest answer that the proxy refuses is a 407 that says so, and names
/// the user and the password.
#[tokio::test]
async fn a_refused_digest_answer_names_the_user_and_the_password() {
    pod_testkit::need_loopback!();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let heads = tokio::spawn(async move {
        let mut heads = Vec::new();
        for _ in 0..2 {
            let (mut s, _) = listener.accept().await.unwrap();
            heads.push(read_head(&mut s).await.unwrap());
            let _ = s
                .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Digest realm=\"r\", nonce=\"n\"\r\n\r\n")
                .await;
        }
        heads
    });
    let proxy = Proxy::parse(&format!("http://user:wrong@{addr}")).unwrap();
    let err = through(&proxy, "example.test", 443).await.unwrap_err();
    assert!(matches!(&err, DialError::ProxyRefused { status: 407, .. }), "{err:?}");
    assert!(err.to_string().contains("sent a Digest answer"), "{err}");
    assert!(err.to_string().contains("check the user and the password"), "{err}");
    let heads = heads.await.unwrap();
    assert!(heads[0].contains("Basic ") && heads[1].contains("Digest username=\"user\""), "{heads:?}");
}

/// An HTTP CONNECT proxy on a Unix socket that answers 200 and echoes.
#[cfg(unix)]
async fn unix_connect_echo(path: std::path::PathBuf) {
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            tokio::spawn(async move {
                let Some(head) = read_head(&mut s).await else { return };
                assert!(head.starts_with("CONNECT echo.test:7 HTTP/1.1"));
                s.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await.unwrap();
                let (mut r, mut w) = s.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
}

async fn round_trip(proxy: &Proxy) -> Vec<u8> {
    let mut io = through(proxy, "echo.test", 7).await.unwrap();
    io.write_all(b"ping").await.unwrap();
    let mut back = [0u8; 4];
    tokio::time::timeout(LIMIT, io.read_exact(&mut back)).await.unwrap().unwrap();
    back.to_vec()
}

#[cfg(unix)]
#[tokio::test]
async fn a_proxy_on_a_unix_socket_carries_a_connection() {
    let dir = std::env::temp_dir().join(format!("pod-proxy-unix-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("p.sock");
    unix_connect_echo(path.clone()).await;
    let proxy = Proxy::parse(&format!("http+unix:{}", path.display())).unwrap();
    assert_eq!(round_trip(&proxy).await, b"ping");
    let _ = std::fs::remove_dir_all(&dir);
}

// The proxy is at the stdin and the stdout of a command: a shell script
// reads the CONNECT, answers 200, then echoes.
#[cfg(unix)]
#[tokio::test]
async fn a_proxy_command_carries_a_connection() {
    let script = r#"http+exec:while IFS= read -r l; do [ "$l" = "$(printf '\r')" ] && break; done; printf 'HTTP/1.1 200 OK\r\n\r\n'; exec cat"#;
    let proxy = Proxy::parse_trusted(script).unwrap();
    assert_eq!(round_trip(&proxy).await, b"ping");
    let proxy = Proxy::parse_trusted("http+exec:exit 3").unwrap();
    let Err(err) = through(&proxy, "echo.test", 7).await else {
        panic!("a command that exits is no proxy");
    };
    assert_eq!(err.stage_and_class().0, Stage::Proxy, "{err}");
}

/// A refusal keeps the reason phrase and the first line of the body.
#[tokio::test]
async fn a_refusal_keeps_the_first_line_of_its_body() {
    pod_testkit::need_loopback!();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        read_head(&mut s).await;
        let body = "\r\nnot on the egress\x1b[2J allowlist\nsecond line\n";
        let answer = format!("HTTP/1.1 403 Forbidden\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        s.write_all(answer.as_bytes()).await.unwrap();
        // The connection stays open: the length ends the read.
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    let started = Instant::now();
    let err = through(&Proxy::parse(&format!("http://{addr}")).unwrap(), "api.example", 443).await.unwrap_err();
    match &err {
        DialError::ProxyRefused { status: 403, reason, .. } => {
            assert_eq!(reason, "Forbidden: not on the egress[2J allowlist")
        }
        other => panic!("{other:?}"),
    }
    assert!(started.elapsed() < Duration::from_secs(2), "the read waited for the close");
}

/// The decisions of a dial reach the observer of the dialer.
#[tokio::test]
async fn each_dial_is_one_decision_of_the_observer() {
    pod_testkit::need_loopback!();
    pod_testkit::need_localhost!();
    #[derive(Default)]
    struct Seen(Mutex<Vec<String>>);
    impl Observer for Seen {
        fn decision(&self, decision: &'static str, outcome: &str) {
            self.0.lock().unwrap().push(format!("{decision}.{outcome}"));
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { while listener.accept().await.is_ok() {} });
    let var = |name: &str| (name == "HTTPS_PROXY").then(|| std::ffi::OsString::from("http://127.0.0.1:1"));
    let env = Environment::read(&var);
    let seen = Arc::new(Seen::default());
    let observed = |choice| dialer(choice).with_observer(seen.clone());
    observed(ProxyChoice::Direct).tcp("127.0.0.1", addr.port(), LIMIT).await.unwrap();
    observed(ProxyChoice::FromEnvironment(env.clone())).tcp("localhost", addr.port(), LIMIT).await.ok();
    let _ = observed(ProxyChoice::FromEnvironment(env)).tcp("relay.invalid", 443, Duration::from_millis(300)).await;
    let _ = observed(ProxyChoice::FromEnvironment(Environment::default())).tcp("127.0.0.1", addr.port(), LIMIT).await;
    assert_eq!(*seen.0.lock().unwrap(), ["egress.direct", "egress.loopback", "egress.http", "egress.none"]);
}

/// A resolver that knows one name and counts its questions.
struct OneName(&'static str, Mutex<usize>);

impl Resolve for OneName {
    fn label(&self) -> String {
        "one name".into()
    }

    fn resolve_from<'a>(
        &'a self,
        host: &'a str,
        port: u16,
        _deadline: tokio::time::Instant,
        _after: Option<Source>,
    ) -> BoxFuture<'a, Result<Found, Failure>> {
        *self.1.lock().unwrap() += 1;
        Box::pin(async move {
            if host != self.0 {
                return Err(Failure::new(Stage::Dns, Class::Unavailable, format!("{host} does not resolve")));
            }
            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            Ok(Found { addrs: vec![addr], source: Source { rank: 0, name: "one" }, more: false })
        })
    }
}

/// The host of the proxy is resolved by the resolver of the proxy, not by
/// the dialer's; a host that does not resolve is named as the proxy host.
#[tokio::test]
async fn the_proxy_host_takes_the_resolver_of_the_proxy() {
    pod_testkit::need_loopback!();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        read_head(&mut s).await;
        s.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await.unwrap();
    });
    let proxy = Proxy::parse(&format!("http://proxy.test:{port}")).unwrap();
    let main = Arc::new(OneName("other.test", Mutex::new(0)));
    let own = Arc::new(OneName("proxy.test", Mutex::new(0)));
    let settings = Settings::new("podnet", ProxyChoice::Via(proxy.clone()));
    let dialer = Dialer::new(settings.clone(), main.clone()).with_proxy_resolver(own.clone());
    dialer.tcp("relay.invalid", 443, LIMIT).await.expect("a tunnel");
    assert_eq!((*main.1.lock().unwrap(), *own.1.lock().unwrap()), (0, 1));
    let err = Dialer::new(settings, main).tcp("relay.invalid", 443, LIMIT).await.unwrap_err();
    assert!(err.to_string().contains("the proxy host proxy.test does not resolve"), "{err}");
    assert_eq!(err.stage_and_class(), (Stage::Proxy, Class::Unavailable));
}

/// An address that does not answer delays the dial by one attempt delay,
/// not by its own time-out.
#[tokio::test]
async fn a_silent_address_does_not_hold_the_dial() {
    pod_testkit::need_loopback!();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let good = listener.local_addr().unwrap();
    // TEST-NET-1: no host answers there.
    let silent: std::net::SocketAddr = "192.0.2.1:9".parse().unwrap();
    let start = Instant::now();
    let (index, _stream) = pod_proxy::dial_addrs(&[silent, good], Duration::from_secs(10)).await.unwrap();
    let took = start.elapsed();
    assert_eq!(index, 1);
    assert!(took < Duration::from_millis(1000), "the dial took {took:?}");
}

/// A dial that no address takes keeps the kind of the last error of I/O
/// whole, beside its cut: podnet's doctor reads a refusal of this host (an
/// unreachable network or host, its rules) by the kind.
#[tokio::test]
async fn a_failed_dial_keeps_the_kind_of_its_last_error() {
    pod_testkit::need_loopback!();
    let closed = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap();
    let err = pod_proxy::dial_addrs(&[closed], Duration::from_secs(5)).await.unwrap_err();
    match err {
        DialError::Connect { kind, cut, .. } => {
            assert_eq!(kind, Some(std::io::ErrorKind::ConnectionRefused));
            assert_eq!(cut, Some(pod_proxy::Cut::Refused));
        }
        other => panic!("{other:?}"),
    }
    let direct = dialer(ProxyChoice::Direct).tcp("127.0.0.1", closed.port(), Duration::from_secs(5)).await;
    assert!(
        matches!(direct, Err(DialError::Connect { kind: Some(std::io::ErrorKind::ConnectionRefused), .. })),
        "{direct:?}"
    );
}
