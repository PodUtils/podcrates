//! The traits with loopback implementations: a dialer that connects with
//! TCP, and a resolver with two sources.

use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Duration;

use pod_core::{BoxFuture, Class, Connected, Dial, Failure, Found, Resolve, Source, Stage, Target};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::Instant;

struct Loopback;

impl Dial for Loopback {
    fn label(&self) -> String {
        "loopback".into()
    }

    fn dial<'a>(&'a self, target: &'a Target, deadline: Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move {
            let Target::Addr(addr) = target else {
                return Err(Failure::new(Stage::Dns, Class::Usage, "the loopback dialer takes addresses"));
            };
            let s = tokio::time::timeout_at(deadline, tokio::net::TcpStream::connect(addr))
                .await
                .map_err(|_| Failure::new(Stage::Connect, Class::Timeout, "no connection in time"))?
                .map_err(|e| Failure::io(Stage::Connect, e))?;
            let (peer, local) = (s.peer_addr().ok(), s.local_addr().ok());
            Ok(Connected { io: Box::new(s), peer, local })
        })
    }
}

const SYSTEM: Source = Source { rank: 0, name: "system" };
const DOH: Source = Source { rank: 1, name: "doh" };

struct TwoSources {
    system: Vec<SocketAddr>,
    doh: Vec<SocketAddr>,
    dialed: Mutex<Vec<(SocketAddr, Source)>>,
}

impl Resolve for TwoSources {
    fn label(&self) -> String {
        "two sources".into()
    }

    fn resolve_from<'a>(
        &'a self,
        host: &'a str,
        _port: u16,
        _deadline: Instant,
        after: Option<Source>,
    ) -> BoxFuture<'a, Result<Found, Failure>> {
        Box::pin(async move {
            match after {
                None => Ok(Found { addrs: self.system.clone(), source: SYSTEM, more: true }),
                Some(s) if s == SYSTEM => Ok(Found { addrs: self.doh.clone(), source: DOH, more: false }),
                Some(_) => Err(Failure::new(Stage::Dns, Class::Unavailable, format!("no other source for {host}"))),
            }
        })
    }

    fn note_dialed(&self, _host: &str, peer: SocketAddr, source: Source) {
        self.dialed.lock().unwrap().push((peer, source));
    }
}

/// An address on the loopback where nothing listens.
async fn dead_addr() -> SocketAddr {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let a = l.local_addr().unwrap();
    drop(l);
    a
}

#[tokio::test]
async fn dial_addrs_passes_a_dead_address_and_connects_to_the_next() {
    pod_testkit::need_loopback!();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let live = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        s.write_all(b"hello").await.unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut c = Loopback.dial_addrs("x", &[dead_addr().await, live], deadline).await.expect("the live address");
    assert_eq!(c.peer, Some(live));
    let mut buf = [0u8; 5];
    c.io.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"hello");
}

#[tokio::test]
async fn dial_addrs_with_no_address_fails_at_the_dns_stage() {
    let e = Loopback.dial_addrs("x", &[], Instant::now() + Duration::from_secs(1)).await.unwrap_err();
    assert_eq!((e.stage, e.class), (Stage::Dns, Class::Unavailable));
}

#[tokio::test]
async fn a_dialer_with_no_udp_says_so() {
    let e = Loopback.datagrams().await.unwrap_err();
    assert_eq!(e.text(), "loopback carries no UDP");
    assert!(!Loopback.names_inside());
}

#[tokio::test]
async fn a_resolver_gives_its_sources_in_order_and_hears_which_worked() {
    let a: SocketAddr = "192.0.2.1:443".parse().unwrap();
    let b: SocketAddr = "192.0.2.2:443".parse().unwrap();
    let r = TwoSources { system: vec![a], doh: vec![b], dialed: Mutex::new(Vec::new()) };
    let deadline = Instant::now() + Duration::from_secs(1);
    assert_eq!(r.resolve("relay.example", 443, deadline).await.unwrap(), [a]);
    let next = r.resolve_from("relay.example", 443, deadline, Some(SYSTEM)).await.unwrap();
    assert_eq!((next.addrs, next.source.name), (vec![b], "doh"));
    assert!(r.resolve_from("relay.example", 443, deadline, Some(DOH)).await.is_err());
    assert!(r.pinned("relay.example").is_empty());
    r.note_dialed("relay.example", b, DOH);
    assert_eq!(*r.dialed.lock().unwrap(), [(b, DOH)]);
}
