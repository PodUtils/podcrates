//! A direct dial over several addresses: an address that never answers does
//! not hold the dial, as the next one starts after the attempt delay and the
//! first that connects wins; and the two families take turns. The pinned
//! addresses are the resolver's that the caller gives. A source whose
//! addresses do not connect hands the dial to the next one when it says
//! that more may follow, and only then.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pod_core::{BoxFuture, Class, Failure, Found, Resolve, Source, Stage};
use pod_proxy::resolve::System;
use pod_proxy::{DialError, Dialer, ProxyChoice, Settings, interleave};
use tokio::net::{TcpListener, TcpStream};

/// The direct dial of podssh, with `resolve` as its resolver.
async fn dial(
    resolve: impl Resolve + 'static,
    host: &str,
    port: u16,
    timeout: Duration,
) -> Result<TcpStream, DialError> {
    Dialer::new(Settings::new("podssh", ProxyChoice::Direct), Arc::new(resolve)).tcp(host, port, timeout).await
}

#[tokio::test]
async fn a_silent_first_address_does_not_hold_the_dial() {
    pod_testkit::need_loopback!();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    // 192.0.2.1 is a documentation address: a SYN to it gets no answer, or
    // an error at once where no route exists. Either way the dial goes on.
    let silent = "192.0.2.1".parse().unwrap();
    let loopback = "127.0.0.1".parse().unwrap();
    let pins = System::with_pins(vec![("silent-first.test".into(), silent), ("silent-first.test".into(), loopback)]);
    let started = Instant::now();
    let stream =
        dial(pins, "silent-first.test", port, Duration::from_secs(10)).await.expect("the second address connects");
    let took = started.elapsed();
    assert!(
        took < pod_testkit::host::scaled(Duration::from_secs(3)),
        "the dial waited for the silent address: {took:?}"
    );
    assert_eq!(stream.peer_addr().unwrap().ip(), loopback);
}

#[test]
fn the_families_take_turns_the_first_family_first() {
    let a = |s: &str| s.parse::<SocketAddr>().unwrap();
    let mixed = vec![a("[2001:db8::1]:1"), a("[2001:db8::2]:1"), a("192.0.2.1:1"), a("192.0.2.2:1"), a("192.0.2.3:1")];
    assert_eq!(
        interleave(mixed),
        vec![a("[2001:db8::1]:1"), a("192.0.2.1:1"), a("[2001:db8::2]:1"), a("192.0.2.2:1"), a("192.0.2.3:1")]
    );
    assert_eq!(interleave(vec![a("192.0.2.1:1"), a("[2001:db8::1]:1")]), vec![a("192.0.2.1:1"), a("[2001:db8::1]:1")]);
    assert_eq!(interleave(Vec::new()), Vec::<SocketAddr>::new());
}

const CACHE: Source = Source { rank: 3, name: "cache" };
const OLD: Source = Source { rank: 5, name: "old cache" };

/// Two sources: a cache whose address does not connect, and an old cache
/// whose address does; `more` is what the cache says. Each source that the
/// dial asks, and each address that it noted, is kept.
struct TwoCaches {
    dead: SocketAddr,
    live: SocketAddr,
    more: bool,
    asked: Mutex<Vec<Option<Source>>>,
    noted: Mutex<Vec<(SocketAddr, Source)>>,
}

impl Resolve for TwoCaches {
    fn label(&self) -> String {
        "two caches".into()
    }

    fn resolve_from<'a>(
        &'a self,
        _host: &'a str,
        _port: u16,
        _deadline: tokio::time::Instant,
        after: Option<Source>,
    ) -> BoxFuture<'a, Result<Found, Failure>> {
        self.asked.lock().unwrap().push(after);
        Box::pin(async move {
            match after {
                None => Ok(Found { addrs: vec![self.dead], source: CACHE, more: self.more }),
                Some(s) if s == CACHE => Ok(Found { addrs: vec![self.live], source: OLD, more: false }),
                Some(_) => Err(Failure::new(Stage::Dns, Class::Unavailable, "no source after the old cache")),
            }
        })
    }

    fn note_dialed(&self, _host: &str, peer: SocketAddr, source: Source) {
        self.noted.lock().unwrap().push((peer, source));
    }
}

/// A cached address that does not connect hands the dial to the next
/// source, and the resolver hears which source carried it; a source that
/// says no more follows ends the dial with its own failure.
#[tokio::test]
async fn a_source_whose_addresses_do_not_connect_hands_the_dial_to_the_next() {
    pod_testkit::need_loopback!();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let live = listener.local_addr().unwrap();
    let dead = {
        let gone = TcpListener::bind("127.0.0.1:0").await.unwrap();
        gone.local_addr().unwrap()
    };
    let caches =
        Arc::new(TwoCaches { dead, live, more: true, asked: Mutex::new(Vec::new()), noted: Mutex::new(Vec::new()) });
    let dialer = Dialer::new(Settings::new("podssh", ProxyChoice::Direct), caches.clone());
    let stream = dialer.tcp("cached.test", live.port(), Duration::from_secs(10)).await.expect("the old address");
    assert_eq!(stream.peer_addr().unwrap(), live);
    assert_eq!(*caches.asked.lock().unwrap(), [None, Some(CACHE)]);
    assert_eq!(*caches.noted.lock().unwrap(), [(live, OLD)]);

    let alone = TwoCaches { more: false, asked: Mutex::new(Vec::new()), noted: Mutex::new(Vec::new()), ..*caches };
    let err = dial(alone, "cached.test", live.port(), Duration::from_secs(10)).await.unwrap_err();
    assert!(
        matches!(&err, DialError::Connect { target, .. } if target == &format!("cached.test:{}", live.port())),
        "{err:?}"
    );
}
