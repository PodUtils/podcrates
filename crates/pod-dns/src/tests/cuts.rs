//! How a resolver's connection ended, kept beside the words of its failure:
//! a stand-in on the loopback that resets the ClientHello, closes after it,
//! holds it, or is not there. A doctor counts a reset resolver as it counts
//! a reset relay host.

use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use pod_proxy::{Cut, Dialer, Missed, ProxyChoice};
use pod_tls::{Context, Words};
use tokio::time::Instant;

use crate::{Doh, Settings, Way};

/// How the stand-in ends a connection, once the ClientHello came.
#[derive(Clone, Copy)]
enum End {
    /// Dropped with the hello unread: the system sends a reset.
    Reset,
    /// The hello read, then closed: a FIN with nothing sent.
    Close,
    /// Held, with nothing sent, past the client's bound.
    Hold,
}

/// A stand-in resolver on the loopback that ends its one connection as
/// `end` says; its port.
fn stand_in(end: End) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((conn, _)) = listener.accept() {
            answer(conn, end);
        }
    });
    port
}

fn answer(mut conn: TcpStream, end: End) {
    let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
    let mut first = [0u8; 1];
    if !matches!(conn.peek(&mut first), Ok(n) if n > 0) {
        return;
    }
    // The rest of the ClientHello comes in the same flight.
    std::thread::sleep(Duration::from_millis(50));
    match end {
        End::Reset => drop(conn),
        End::Close => {
            let _ = conn.set_read_timeout(Some(Duration::from_millis(200)));
            let mut hello = [0u8; 4096];
            while matches!(conn.read(&mut hello), Ok(n) if n > 0) {}
            drop(conn);
        }
        End::Hold => std::thread::sleep(Duration::from_secs(6)),
    }
}

fn resolver(http2: bool, port: u16) -> Doh {
    Doh::new([127, 0, 0, 1].into(), "doh.test").json("/dns-query").wire("/dns-query").http2(http2).port(port)
}

/// The failure of asking the resolver at `port` straight at its address,
/// with no proxy, within `bound`.
async fn ask_at(http2: bool, port: u16, bound: Duration) -> Result<(), Missed> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let context = Context::new(provider, pod_tls::Settings::new(Words::new("podssh")));
    let dial = Dialer::new(
        pod_proxy::Settings::new("podssh", ProxyChoice::Direct),
        Arc::new(pod_proxy::resolve::System::new()),
    );
    let settings = Settings::default();
    let way = Way { settings: &settings, context: &context, dial: &dial };
    match way.ask(&resolver(http2, port), "relay.example", Instant::now() + bound).await {
        Ok(found) => panic!("a stand-in that answers nothing gave {found:?}"),
        Err(missed) => Err(missed),
    }
}

/// The failure of asking a stand-in that ends as `end`, within `bound`.
async fn ask(end: End, http2: bool, bound: Duration) -> Missed {
    let port = stand_in(end);
    ask_at(http2, port, bound).await.unwrap_err()
}

/// A reset of the handshake is a reset, over HTTP/1.1 and over HTTP/2 alike,
/// with the words that the lines of a doctor give.
#[tokio::test]
async fn a_resolver_that_resets_the_handshake_is_kept_as_a_reset() {
    pod_testkit::need_loopback!();
    for http2 in [false, true] {
        let missed = ask(End::Reset, http2, Duration::from_secs(4)).await;
        assert_eq!(missed.cut, Some(Cut::Reset), "http2 {http2}: {missed}");
        assert!(missed.why.starts_with("127.0.0.1: TLS: "), "http2 {http2}: {missed}");
    }
}

/// A close with nothing, silence to the bound and a refused connection
/// keep their own kinds.
#[tokio::test]
async fn a_close_a_silence_and_a_refusal_are_kept_apart() {
    pod_testkit::need_loopback!();
    let closed = ask(End::Close, false, Duration::from_secs(4)).await;
    assert_eq!(closed.cut, Some(Cut::Closed), "{closed}");
    let silent = ask(End::Hold, false, Duration::from_millis(800)).await;
    assert_eq!(silent.cut, Some(Cut::TimedOut), "{silent}");
    assert_eq!(silent.why, "127.0.0.1: timed out");
    // A port where nothing listens, held for the test by a socket that does
    // not listen, so that no other test takes it.
    let held = tokio::net::TcpSocket::new_v4().unwrap();
    held.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let port = held.local_addr().unwrap().port();
    let refused = ask_at(false, port, Duration::from_secs(4)).await.unwrap_err();
    assert_eq!(refused.cut, Some(Cut::Refused), "{refused}");
}
