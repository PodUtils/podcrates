//! The accept side of the ws carrier: the accept loop of `pod-ws` on one
//! listener, with the TLS that pins the key of each client. A session waits
//! for [`WsListener::accept_key`] before `ready`: it holds a slot of the
//! caps, and the deadline of `ready` cuts a session that waits too long. A
//! client that refused the key of this listener is an error of one accept
//! (`KeyRefused`), as on the tcp carrier; each other failed handshake is
//! skipped (call C390).

use std::io;
use std::net::SocketAddr;

use pod_identity::PublicKey;
use pod_tls::peer;
use pod_ws::serve::Accepted as Session;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::{ALPN, Beat, NAME, Ws, carry, err};
use crate::carrier::{Accepted, BoxIo, CarrierError, CarrierErrorKind, Listener, Pending};
use crate::pin;

/// What the accept loop gives to the listener.
enum Event {
    Session(Box<Session>),
    /// A client refused the key of this listener in its TLS handshake.
    KeyRefused(CarrierError),
}

/// A listener of the ws carrier. Its drop ends the accept loop and each
/// connection that did not reach an accept.
pub struct WsListener {
    local: SocketAddr,
    sessions: mpsc::UnboundedReceiver<Event>,
    task: JoinHandle<()>,
    beat: Beat,
}

impl std::fmt::Debug for WsListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WsListener").field("local", &self.local).finish_non_exhaustive()
    }
}

pub(super) async fn bind(ws: &Ws, addr: SocketAddr) -> Result<WsListener, CarrierError> {
    ws.check()?;
    let cfg = pod_ws::serve::Serve { protocol: ws.protocol, ..ws.serve };
    cfg.check().map_err(|why| err(CarrierErrorKind::Usage, why))?;
    let config = peer::server_config(ws.provider.clone(), ws.identity.clone(), ws.clients.clone(), &[ALPN])
        .map_err(|e| err(CarrierErrorKind::Unavailable, e.to_string()))?;
    let listener =
        tokio::net::TcpListener::bind(addr).await.map_err(|e| CarrierError::io(NAME, format!("bind {addr}"), e))?;
    let local = listener.local_addr().map_err(|e| CarrierError::io(NAME, format!("the address of {addr}"), e))?;
    let (out, sessions) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        let refused = out.clone();
        let on = move |session| {
            let _ = out.send(Event::Session(Box::new(session)));
        };
        let failed = move |from: SocketAddr, e: io::Error| {
            let e = pin::tls_error(NAME, format!("the TLS handshake of {from}"), e);
            if e.kind == CarrierErrorKind::KeyRefused {
                let _ = refused.send(Event::KeyRefused(e));
            } else {
                tracing::debug!(error = %e, "a TLS handshake of a ws listener failed");
            }
        };
        if let Err(e) = pod_ws::serve::serve_reporting(vec![listener], config, cfg, on, failed).await {
            tracing::debug!(error = %e, "the accept loop of a ws listener ended");
        }
    });
    Ok(WsListener { local, sessions, task, beat: ws.beat })
}

impl WsListener {
    /// The bound address: the port that the system chose for port 0.
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// The next client whose key is pinned, and that key. A session that a
    /// deadline or an eviction cut while it waited is skipped. A client that
    /// refused the key of this listener is an error of one accept
    /// (`KeyRefused`): the listener stays open.
    pub async fn accept_key(&mut self) -> Result<(BoxIo, PublicKey), CarrierError> {
        loop {
            let session = match self.sessions.recv().await {
                Some(Event::Session(session)) => Some(*session),
                Some(Event::KeyRefused(e)) => return Err(e),
                None => None,
            };
            let Some(session) = session else {
                return Err(err(
                    CarrierErrorKind::Io(io::ErrorKind::BrokenPipe),
                    "the accept loop of the listener ended",
                ));
            };
            let Some(key) = session.key else {
                // The verifier of the pins ends no handshake with no key.
                continue;
            };
            if !session.gate.ready() {
                tracing::debug!(peer = %session.peer, "a session of a ws listener was cut before its accept");
                continue;
            }
            return Ok((carry(session.session, self.beat), key));
        }
    }
}

impl Listener for WsListener {
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

impl Drop for WsListener {
    fn drop(&mut self) {
        self.task.abort();
    }
}
