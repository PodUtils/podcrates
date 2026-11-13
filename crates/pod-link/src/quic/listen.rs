//! The accept side of the quic carrier. One task takes the connections of
//! the endpoint and runs the handshake of each in a task of its own, at
//! most `max_handshakes` at once: a peer that stops in its handshake holds
//! only its own task, until its time limit. The result of each handshake
//! waits in a queue for [`QuicListener::accept_key`].

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use pod_core::Datagrams;
use pod_identity::PublicKey;
use pod_quic::noq::crypto::rustls::QuicServerConfig;
use pod_quic::noq::{Endpoint, Incoming};
use pod_tls::peer;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::{JoinHandle, JoinSet};

use super::io::{QuicIo, check, connection_error, proved};
use super::{ACCEPTED, NAME, Quic, err};
use crate::carrier::{Accepted, BoxIo, CarrierError, CarrierErrorKind, Listener, Pending};

/// The result of one handshake.
type Shaken = Result<(BoxIo, PublicKey), CarrierError>;

/// The results of handshakes that wait for an accept. A handshake that
/// ended waits with its permit, so the streams that wait are at most
/// `max_handshakes`.
const QUEUE: usize = 8;

/// Where a listener takes its datagrams.
pub(super) enum Socket {
    /// A UDP socket of the system at this address.
    System(SocketAddr),
    /// The datagrams of the caller.
    Over(Arc<dyn Datagrams>),
}

/// A listener of the quic carrier. Its drop ends the task of its accepts
/// and each handshake under way, and the endpoint takes no new connection;
/// the connections that it gave live on.
pub struct QuicListener {
    local: SocketAddr,
    endpoint: Endpoint,
    results: mpsc::Receiver<Shaken>,
    task: JoinHandle<()>,
}

impl std::fmt::Debug for QuicListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuicListener").field("local", &self.local).finish_non_exhaustive()
    }
}

/// What each handshake of a listener needs.
#[derive(Clone)]
struct Shake {
    alpn: &'static [u8],
    own: PublicKey,
    within: Duration,
    linger: Duration,
}

pub(super) fn bind(quic: &Quic, socket: Socket) -> Result<QuicListener, CarrierError> {
    if quic.max_handshakes == 0 {
        return Err(err(CarrierErrorKind::Usage, "max_handshakes is 0: give 1 or more"));
    }
    let tls = peer::server_config(quic.provider.clone(), quic.identity.clone(), quic.clients.clone(), &[quic.alpn])
        .map_err(|e| err(CarrierErrorKind::Unavailable, e.to_string()))?;
    let crypto = QuicServerConfig::try_from(tls).map_err(|e| err(CarrierErrorKind::Unavailable, e.to_string()))?;
    let mut config = pod_quic::endpoint::server_config(Arc::new(crypto))
        .map_err(|f| err(CarrierErrorKind::Unavailable, f.to_string()))?;
    let v6 = match &socket {
        Socket::System(addr) => addr.is_ipv6(),
        Socket::Over(datagrams) => datagrams.local_addr().is_ok_and(|a| a.is_ipv6()),
    };
    // The client opens no stream: the listener opens the one stream.
    config.transport_config(Arc::new(quic.transport_config(v6, 0)));
    let endpoint = match socket {
        Socket::System(addr) => {
            let udp = std::net::UdpSocket::bind(addr).map_err(|e| CarrierError::io(NAME, format!("bind {addr}"), e))?;
            pod_quic::endpoint::on_socket(udp, Some(config))
        }
        Socket::Over(datagrams) => pod_quic::endpoint::over(datagrams, Some(config)),
    }
    .map_err(|f| err(CarrierErrorKind::Unavailable, f.to_string()))?;
    let local = endpoint.local_addr().map_err(|e| CarrierError::io(NAME, "the address of the endpoint", e))?;
    let (out, results) = mpsc::channel(QUEUE);
    let limit = Arc::new(Semaphore::new(quic.max_handshakes));
    let shake =
        Shake { alpn: quic.alpn, own: quic.identity.public(), within: quic.handshake_timeout, linger: quic.linger };
    let task = tokio::spawn(serve(endpoint.clone(), limit, shake, out));
    Ok(QuicListener { local, endpoint, results, task })
}

impl QuicListener {
    /// The bound address: the port that the system chose for port 0.
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// The next client whose key is pinned, and that key. A client that
    /// failed its handshake is an error of one accept: the listener stays
    /// open.
    pub async fn accept_key(&mut self) -> Result<(BoxIo, PublicKey), CarrierError> {
        match self.results.recv().await {
            Some(result) => result,
            None => Err(err(CarrierErrorKind::Io(io::ErrorKind::BrokenPipe), "the task of the listener ended")),
        }
    }
}

impl Listener for QuicListener {
    fn address(&self) -> String {
        self.local.to_string()
    }

    fn accept(&mut self) -> Pending<'_, BoxIo> {
        Box::pin(async move { self.accept_key().await.map(|(io, _)| io) })
    }

    fn accept_peer(&mut self) -> Pending<'_, Accepted> {
        Box::pin(async move { self.accept_key().await.map(|(io, key)| Accepted { io, peer: Some(key) }) })
    }
}

impl Drop for QuicListener {
    fn drop(&mut self) {
        self.task.abort();
        self.endpoint.set_server_config(None);
    }
}

/// Take the connections and shake hands with each, at most `limit` at
/// once. The drop of this task drops the handshakes under way.
async fn serve(endpoint: Endpoint, limit: Arc<Semaphore>, shake: Shake, out: mpsc::Sender<Shaken>) {
    let mut shakes = JoinSet::new();
    loop {
        let Ok(permit) = limit.clone().acquire_owned().await else { return };
        while shakes.try_join_next().is_some() {}
        let Some(incoming) = endpoint.accept().await else { return };
        let (shake, out) = (shake.clone(), out.clone());
        shakes.spawn(async move {
            let from = incoming.remote_address();
            let result = tokio::time::timeout(shake.within, handshake(incoming, from, &shake)).await;
            let result = result.unwrap_or_else(|_| {
                Err(err(
                    CarrierErrorKind::Io(io::ErrorKind::TimedOut),
                    format!("the QUIC handshake of {from} took more than {:?}", shake.within),
                ))
            });
            let _ = out.send(result).await;
            drop(permit);
        });
    }
}

/// The handshake of one client, its ALPN, its key, the stream and the
/// byte of acceptance on it.
async fn handshake(incoming: Incoming, from: SocketAddr, shake: &Shake) -> Shaken {
    let conn =
        incoming.await.map_err(|e| connection_error(format!("the QUIC handshake of {from}"), e, &from, &shake.own))?;
    check(&conn, shake.alpn, &from)?;
    let key = proved(&conn, &from)?;
    let (mut send, recv) =
        conn.open_bi().await.map_err(|e| connection_error(format!("the stream of {from}"), e, &from, &shake.own))?;
    send.write_all(&[ACCEPTED])
        .await
        .map_err(|e| CarrierError::io(NAME, format!("the acceptance of {from}"), e.into()))?;
    Ok((Box::new(QuicIo::new(send, recv, conn).linger(shake.linger, None)) as BoxIo, key))
}
