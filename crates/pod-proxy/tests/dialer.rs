//! The dialer as a `pod_core::Dial`: its label and its names for each
//! choice of the settings, the stream and its addresses, a failure with the
//! dial's error as its cause, the race of the addresses that a caller
//! resolved, the user agent of a CONNECT, and the write limit.

use std::ffi::OsString;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pod_core::{Class, Dial, Failure, Stage, Target};
use pod_proxy::resolve::System;
use pod_proxy::{Cut, DialError, Dialer, Environment, Proxy, ProxyChoice, Settings};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn dialer(proxy: ProxyChoice) -> Dialer {
    Dialer::new(Settings::new("podssh", proxy), Arc::new(System::new()))
}

fn deadline() -> tokio::time::Instant {
    tokio::time::Instant::now() + Duration::from_secs(10)
}

#[test]
fn each_choice_has_its_label_and_says_who_resolves_names() {
    let direct = dialer(ProxyChoice::Direct);
    assert_eq!((direct.label(), direct.names_inside()), ("direct".to_string(), false));
    let socks = dialer(ProxyChoice::Via(Proxy::parse("socks5://user:secret@proxy.example").unwrap()));
    assert_eq!((socks.label(), socks.names_inside()), ("socks5://proxy.example:1080".to_string(), true));
    let var = |name: &str| (name == "HTTPS_PROXY").then(|| OsString::from("http://proxy.example:3128"));
    let env = dialer(ProxyChoice::FromEnvironment(Environment::read(&var)));
    assert_eq!(env.label(), "http://proxy.example:3128 of HTTPS_PROXY");
    assert!(env.names_inside());
    let bad = |name: &str| (name == "all_proxy").then(|| OsString::from("socks4://proxy.example"));
    let bad = dialer(ProxyChoice::FromEnvironment(Environment::read(&bad)));
    assert_eq!((bad.label().as_str(), bad.names_inside()), ("the proxy of all_proxy, which cannot be used", false));
    let none = dialer(ProxyChoice::FromEnvironment(Environment::default()));
    assert_eq!((none.label(), none.names_inside()), ("direct".to_string(), false));
}

/// An echo server on the loopback for one connection.
async fn echo() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 64];
        let n = sock.read(&mut buf).await.unwrap();
        sock.write_all(&buf[..n]).await.unwrap();
    });
    addr
}

#[tokio::test]
async fn a_dial_gives_the_stream_and_its_addresses() {
    pod_testkit::need_loopback!();
    let addr = echo().await;
    let settings =
        Settings { write_limit: Some(Duration::from_secs(5)), ..Settings::new("podssh", ProxyChoice::Direct) };
    let dialer = Dialer::new(settings, Arc::new(System::new()));
    let mut connected = dialer.dial(&Target::Addr(addr), deadline()).await.expect("a connection");
    assert_eq!(connected.peer, Some(addr));
    assert!(connected.local.is_some_and(|l| l.ip().is_loopback()), "{connected:?}");
    connected.io.write_all(b"ping").await.unwrap();
    let mut back = [0u8; 4];
    connected.io.read_exact(&mut back).await.unwrap();
    assert_eq!(&back, b"ping");
}

/// The failure of the trait has the stage and the class of the dial's error,
/// and that error as its cause.
#[tokio::test]
async fn a_failure_keeps_the_dial_error_as_its_cause() {
    pod_testkit::need_loopback!();
    let port = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
    let via = dialer(ProxyChoice::Via(Proxy::parse(&format!("http://127.0.0.1:{port}")).unwrap()));
    let failed: Failure = via.dial(&Target::new("relay.invalid", 443), deadline()).await.unwrap_err();
    assert_eq!((failed.stage, failed.class), (Stage::Proxy, Class::Unavailable), "{failed}");
    let cause = std::error::Error::source(&failed).and_then(|e| e.downcast_ref::<DialError>());
    assert!(matches!(cause, Some(DialError::ProxyUnreachable { .. })), "{cause:?}");
    let direct = dialer(ProxyChoice::Direct);
    let refused = direct.dial(&Target::new("127.0.0.1", port), deadline()).await.unwrap_err();
    assert_eq!((refused.stage, Cut::of_failure(&refused)), (Stage::Connect, Some(Cut::Refused)), "{refused}");
    let usage = direct.dial(&Target::new("two words", 443), deadline()).await.unwrap_err();
    assert_eq!((usage.stage, usage.class), (Stage::Connect, Class::Usage));
}

/// The addresses that a caller resolved race as a name's do: a silent first
/// address does not hold the dial.
#[tokio::test]
async fn dial_addrs_races_the_addresses_of_a_direct_dial() {
    pod_testkit::need_loopback!();
    let live = echo().await;
    let silent: SocketAddr = format!("192.0.2.1:{}", live.port()).parse().unwrap();
    let started = Instant::now();
    let connected = dialer(ProxyChoice::Direct).dial_addrs("relay.test", &[silent, live], deadline()).await;
    assert_eq!(connected.expect("the second address").peer, Some(live));
    assert!(started.elapsed() < Duration::from_secs(3), "the dial waited for the silent address");
    let none = dialer(ProxyChoice::Direct).dial_addrs("relay.test", &[], deadline()).await.unwrap_err();
    assert_eq!((none.stage, none.class), (Stage::Dns, Class::Unavailable));
}

/// A stand-in HTTP proxy that answers 200 to one CONNECT: the head that it
/// got.
async fn proxy_that_sees() -> (Proxy, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") && sock.read(&mut byte).await.unwrap() == 1 {
            head.push(byte[0]);
        }
        sock.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await.unwrap();
        String::from_utf8(head).unwrap()
    });
    (Proxy::parse(&format!("http://127.0.0.1:{port}")).unwrap(), seen)
}

/// The user agent of the settings goes with a CONNECT; with none, no
/// `User-Agent` goes.
#[tokio::test]
async fn the_user_agent_of_the_settings_goes_with_connect() {
    pod_testkit::need_loopback!();
    for agent in [Some("podssh/1.2.3"), None] {
        let (proxy, seen) = proxy_that_sees().await;
        let settings =
            Settings { user_agent: agent.map(str::to_string), ..Settings::new("podssh", ProxyChoice::Via(proxy)) };
        let dialer = Dialer::new(settings, Arc::new(System::new()));
        dialer.tcp("relay.invalid", 443, Duration::from_secs(5)).await.expect("a tunnel");
        let head = seen.await.unwrap();
        match agent {
            Some(agent) => assert!(head.contains(&format!("\r\nUser-Agent: {agent}\r\n")), "{head}"),
            None => assert!(!head.contains("User-Agent"), "{head}"),
        }
    }
}

/// The proxy of the environment goes for a name that its `NO_PROXY` does
/// not name, and not for one that it names.
#[tokio::test]
async fn the_proxy_of_the_environment_goes_by_its_no_proxy() {
    pod_testkit::need_loopback!();
    let (proxy, seen) = proxy_that_sees().await;
    let url = format!("http://{proxy}");
    let var = move |name: &str| match name {
        "https_proxy" => Some(OsString::from(url.clone())),
        "NO_PROXY" => Some(OsString::from("direct.test")),
        _ => None,
    };
    let dialer = dialer(ProxyChoice::FromEnvironment(Environment::read(&var)));
    assert_eq!(dialer.proxy_for("relay.direct.test"), Ok(None));
    dialer.tcp("relay.invalid", 443, Duration::from_secs(5)).await.expect("a tunnel");
    assert!(seen.await.unwrap().starts_with("CONNECT relay.invalid:443 HTTP/1.1\r\n"));
}
