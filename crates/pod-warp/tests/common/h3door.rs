//! The in-process edge of the tests of the providers: a QUIC endpoint with
//! the `h3` ALPN, the certificate of a pod-warp identity, and a MASQUE
//! session that echoes each IP packet (a datagram or a capsule). The caller
//! pins the key of the edge, so a session over it needs no client
//! certificate of Cloudflare.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use pod_quic::noq;
use pod_warp::Identity;
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;

/// The certificate of the edge, made for each handshake.
#[derive(Debug)]
struct Shown(Identity);

impl ResolvesServerCert for Shown {
    fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        Some(Arc::new(self.0.certified(now)))
    }
}

/// What the edge saw.
#[derive(Default, Debug)]
pub struct Gate {
    pub sessions: AtomicUsize,
    /// The `:protocol` of each request, in order.
    pub protocols: Mutex<Vec<String>>,
}

/// An edge of HTTP/3 for CONNECT-IP: its address, its pinned key, and its
/// record.
pub struct FrontDoor {
    pub addr: SocketAddr,
    pub pin: Vec<u8>,
    pub gate: Arc<Gate>,
}

/// The edge: each IP packet of a session comes back.
pub async fn ip_echo_door() -> FrontDoor {
    let server_id = Identity::generate().unwrap();
    let pin = server_id.spki_der().to_vec();
    let mut tls = rustls::ServerConfig::builder_with_provider(super::provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(Shown(server_id)));
    tls.alpn_protocols = vec![pod_warp::h3::ALPN.to_vec()];
    let crypto = noq::crypto::rustls::QuicServerConfig::try_from(Arc::new(tls)).unwrap();
    let config = pod_quic::endpoint::server_config(Arc::new(crypto)).unwrap();
    let server = pod_quic::endpoint::server(config, "127.0.0.1:0".parse().unwrap()).unwrap();
    let addr = server.local_addr().unwrap();
    let gate = Arc::new(Gate::default());
    let kept = gate.clone();
    tokio::spawn(async move {
        while let Some(incoming) = server.accept().await {
            let Ok(conn) = incoming.await else { continue };
            kept.sessions.fetch_add(1, Ordering::SeqCst);
            let kept = kept.clone();
            tokio::spawn(async move {
                let Ok((mut send, mut recv)) = conn.accept_bi().await else { return };
                let mut frames = pod_warp::h3::Frames::default();
                let fields = loop {
                    if let Ok(Some((kind, payload))) = frames.next_frame() {
                        if kind == pod_warp::h3::FRAME_HEADERS {
                            break pod_warp::h3::wire::request_fields(&payload).unwrap_or_default();
                        }
                        continue;
                    }
                    match recv.read_chunk(65536).await {
                        Ok(Some(chunk)) => frames.push(&chunk),
                        _ => return,
                    }
                };
                let protocol =
                    fields.iter().find(|(n, _)| n == ":protocol").map(|(_, v)| v.clone()).unwrap_or_default();
                kept.protocols.lock().unwrap_or_else(PoisonError::into_inner).push(protocol);
                let body = [0x00, 0x00, 0xc0 | 25];
                if send.write_all(&pod_warp::h3::frame(pod_warp::h3::FRAME_HEADERS, &body)).await.is_err() {
                    return;
                }
                let echo = conn.clone();
                tokio::spawn(async move {
                    while let Ok(d) = echo.read_datagram().await {
                        let _ = echo.send_datagram(d);
                    }
                });
                let mut capsules = pod_warp::capsule::CapsuleDecoder::new();
                while let Ok(Some(chunk)) = recv.read_chunk(65536).await {
                    frames.push(&chunk);
                    while let Ok(Some((kind, payload))) = frames.next_frame() {
                        if kind != pod_warp::h3::FRAME_DATA {
                            continue;
                        }
                        capsules.push(&payload);
                        while let Some((kind, value)) = capsules.next_capsule() {
                            if kind == pod_warp::capsule::CAPSULE_DATA
                                && send.write_all(&pod_warp::h3::frame(pod_warp::h3::FRAME_DATA, &value)).await.is_err()
                            {
                                return;
                            }
                        }
                    }
                }
            });
        }
    });
    FrontDoor { addr, pin, gate }
}
