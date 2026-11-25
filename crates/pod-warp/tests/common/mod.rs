//! The helpers of the loopback tests: the HTTP client of a caller, a dial
//! that moves the doors of a record to the loopback, the settings, a
//! scratch directory, a port that refuses, and an accept that does not end.

#![allow(dead_code)]

pub mod edge;
pub mod h3door;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use pod_core::{BoxFuture, Class, Connected, Dial, Event, Events, Failure, Observer, Report, Stage, Target};
use pod_proxy::resolve::System;
use pod_proxy::{Client, Dialer, ProxyChoice};
use pod_tls::{Context, Words};
use pod_warp::{Api, Settings, Way};

/// The TLS provider of the tests.
pub fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// The HTTP client of a caller: a direct dial, ring.
pub fn client() -> Client {
    let dialer = Dialer::new(pod_proxy::Settings::new("podnet", ProxyChoice::Direct), Arc::new(System::new()));
    let context = Arc::new(Context::new(provider(), pod_tls::Settings::new(Words::new("podnet"))));
    Client::new(context, Arc::new(dialer)).with_user_agent("podnet/0.0.0-test")
}

/// The API of the stand-in at `base`.
pub fn api(base: &str) -> Api {
    Api::new(client()).with_bases(vec![base.to_string()])
}

/// A dial that sends each address of `map` to its loopback address, and
/// each other target to the loopback address of the same port; it counts
/// each dial. The first dial of an address of `first` goes to its loopback
/// address in `first`, and each later dial to the one in `map`.
pub struct Moved {
    pub map: Vec<(SocketAddr, SocketAddr)>,
    pub first: Mutex<Vec<(SocketAddr, SocketAddr)>>,
    pub dials: AtomicUsize,
}

impl Moved {
    pub fn new(map: Vec<(SocketAddr, SocketAddr)>) -> Arc<Moved> {
        Moved::with_first(map, Vec::new())
    }

    /// [`Moved::new`] with the first dial of each address of `first`
    /// moved to another place.
    pub fn with_first(map: Vec<(SocketAddr, SocketAddr)>, first: Vec<(SocketAddr, SocketAddr)>) -> Arc<Moved> {
        Arc::new(Moved { map, first: Mutex::new(first), dials: AtomicUsize::new(0) })
    }

    fn to(&self, addr: SocketAddr) -> Option<SocketAddr> {
        let mut first = self.first.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(i) = first.iter().position(|(from, _)| *from == addr) {
            return Some(first.remove(i).1);
        }
        self.map.iter().find(|(from, _)| *from == addr).map(|(_, to)| *to)
    }
}

impl Dial for Moved {
    fn label(&self) -> String {
        "the moved dial of the test".into()
    }

    fn dial<'a>(
        &'a self,
        target: &'a Target,
        _deadline: tokio::time::Instant,
    ) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move {
            self.dials.fetch_add(1, Ordering::SeqCst);
            let to = match target {
                Target::Addr(addr) => self.to(*addr),
                Target::Name { .. } => None,
            };
            let to = to.unwrap_or_else(|| SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), target.port()));
            let tcp = tokio::net::TcpStream::connect(to)
                .await
                .map_err(|e| Failure::io(Stage::Connect, "the connection", e))?;
            Ok(Connected { io: Box::new(tcp), peer: Some(to), local: None })
        })
    }
}

/// The events of a test, kept.
#[derive(Default)]
pub struct Kept {
    pub events: Mutex<Vec<Event>>,
    pub decisions: Mutex<Vec<(String, String)>>,
}

impl Events for Kept {
    fn event(&self, event: Event) {
        self.events.lock().unwrap().push(event);
    }
}

impl Observer for Kept {
    fn decision(&self, decision: &'static str, outcome: &str) {
        self.decisions.lock().unwrap().push((decision.into(), outcome.into()));
    }
}

impl Kept {
    pub fn kinds(&self) -> Vec<&'static str> {
        self.events.lock().unwrap().iter().map(|e| e.kind).collect()
    }
}

/// The settings of a test: the API at `base`, the dial `dial`, and the
/// events kept in the answer.
pub fn settings(base: &str, dial: Arc<dyn Dial>) -> (Settings, Arc<Kept>) {
    let kept = Arc::new(Kept::default());
    let mut settings = Settings::new(api(base), Way::new(dial, provider()));
    settings.report = Report { events: kept.clone(), observer: kept.clone() };
    settings.way.timeout = std::time::Duration::from_secs(10);
    (settings, kept)
}

/// A new and empty scratch directory of a test.
pub fn scratch(tag: &str) -> std::path::PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("pod-warp-it-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The class of a failure, for an assertion.
pub fn class(f: &Failure) -> Class {
    f.class
}

/// A port of the loopback where nothing listens. A socket bound with no
/// listen holds the port until the test binary ends: a connection to it is
/// refused, and no listener of another test takes the port. A port freed
/// at once can go to the edge of another test, whose other key sends the
/// session into the read of the record again.
///
/// macOS gives no refusal for a port that a socket holds with no listen:
/// the connection waits for its time limit. There the port is 1: a bind to
/// port 0 never gives it, so no listener of a test can take it.
pub fn dead() -> SocketAddr {
    if cfg!(target_os = "macos") {
        return SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1);
    }
    static HELD: Mutex<Vec<tokio::net::TcpSocket>> = Mutex::new(Vec::new());
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
    let addr = socket.local_addr().unwrap();
    HELD.lock().unwrap_or_else(PoisonError::into_inner).push(socket);
    addr
}

/// The next connection of `listener`. A failed accept (a connection reset
/// before the accept, no free descriptor for a moment) does not end the
/// listener: a later dial to it would be refused.
pub async fn accept(listener: &tokio::net::TcpListener) -> tokio::net::TcpStream {
    loop {
        match listener.accept().await {
            Ok((tcp, _)) => return tcp,
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(10)).await,
        }
    }
}
