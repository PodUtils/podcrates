//! The caller's side of the tests against the stand-in CA: a TLS listener
//! that answers TLS-ALPN-01 through `pod_tls::server::Served`, as podnet's
//! and podssh's do; the places of the state; the events.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use pod_tls::server::{ACME_TLS_1, HTTP1, Served};
use rustls::crypto::CryptoProvider;

/// The name of each order.
pub const NAME: &str = "files.podacme.test";

pub fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// A TLS listener on the loopback that presents what `served` holds,
/// offering `alpn`. A handshake ends the connection.
pub async fn listener_with(served: Arc<Served>, alpn: &[&[u8]]) -> SocketAddr {
    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a listener on the loopback");
    let at = tcp.local_addr().unwrap();
    let acceptor = pod_tls::server::acceptor_for(provider(), served, alpn).expect("an acceptor");
    tokio::spawn(async move {
        while let Ok((stream, _)) = tcp.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let _ = acceptor.accept(stream).await;
            });
        }
    });
    at
}

/// The listener of a server with ACME: `http/1.1` for a visitor,
/// `acme-tls/1` for the CA.
pub async fn listener(served: Arc<Served>) -> SocketAddr {
    listener_with(served, &[HTTP1, ACME_TLS_1]).await
}

/// A new empty directory for the state of one test.
pub fn places(test: &str) -> Vec<PathBuf> {
    static N: AtomicUsize = AtomicUsize::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("pod-acme-{test}-{}-{nanos}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    vec![dir]
}

/// The events of a client, kept.
pub type Lines = Arc<Mutex<Vec<String>>>;

pub fn events() -> (Arc<dyn pod_core::Events>, Lines) {
    let lines: Lines = Arc::default();
    let kept = lines.clone();
    let sink = move |event: pod_core::Event| kept.lock().unwrap().push(format!("{}: {}", event.kind, event.text));
    (Arc::new(sink), lines)
}
