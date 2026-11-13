//! The accept side of the h2 carrier. One task accepts the TCP connections
//! and runs the handshake of each (TLS, the prefaces of HTTP/2, the POST
//! and its 200) in a task of its own, at most `max_handshakes` at once: a
//! peer that sends nothing holds only its own handshake, until its time
//! limit. The result of each handshake waits in a queue for
//! [`H2Listener::accept_key`].

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use pod_identity::PublicKey;
use pod_tls::peer;
use tokio::net::TcpStream;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::{JoinHandle, JoinSet};
use tokio_rustls::TlsAcceptor;

use super::{ALPN, H2, NAME, WINDOW, err, h2_error};
use crate::carrier::{Accepted, BoxIo, CarrierError, CarrierErrorKind, Listener, Pending};
use crate::pin;

/// The result of one handshake.
type Shaken = Result<(BoxIo, PublicKey), CarrierError>;

/// The results of handshakes that wait for an accept. A handshake that
/// ended waits with its permit, so the streams that wait are at most
/// `max_handshakes`.
const QUEUE: usize = 8;

/// The pause after a failed accept of a TCP connection (no file
/// descriptor left, for example), so that the task does not spin.
const ACCEPT_PAUSE: Duration = Duration::from_millis(50);

/// A listener of the h2 carrier. Its drop ends the task of its accepts
/// and each handshake under way.
pub struct H2Listener {
    local: SocketAddr,
    results: mpsc::Receiver<Shaken>,
    task: JoinHandle<()>,
}

impl std::fmt::Debug for H2Listener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("H2Listener").field("local", &self.local).finish_non_exhaustive()
    }
}

pub(super) async fn bind(h2: &H2, addr: SocketAddr) -> Result<H2Listener, CarrierError> {
    if h2.max_handshakes == 0 {
        return Err(err(CarrierErrorKind::Usage, "max_handshakes is 0: give 1 or more"));
    }
    let config = peer::server_config(h2.provider.clone(), h2.identity.clone(), h2.clients.clone(), &[ALPN])
        .map_err(|e| err(CarrierErrorKind::Unavailable, e.to_string()))?;
    let listener =
        tokio::net::TcpListener::bind(addr).await.map_err(|e| CarrierError::io(NAME, format!("bind {addr}"), e))?;
    let local = listener.local_addr().map_err(|e| CarrierError::io(NAME, format!("the address of {addr}"), e))?;
    let (out, results) = mpsc::channel(QUEUE);
    let limit = Arc::new(Semaphore::new(h2.max_handshakes));
    let task = tokio::spawn(serve(listener, TlsAcceptor::from(config), limit, h2.handshake_timeout, out));
    Ok(H2Listener { local, results, task })
}

impl H2Listener {
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

impl Listener for H2Listener {
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

impl Drop for H2Listener {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Accept the TCP connections and shake hands with each, at most `limit`
/// at once. The drop of this task drops the handshakes under way.
async fn serve(
    listener: tokio::net::TcpListener,
    acceptor: TlsAcceptor,
    limit: Arc<Semaphore>,
    within: Duration,
    out: mpsc::Sender<Shaken>,
) {
    let mut shakes = JoinSet::new();
    loop {
        let Ok(permit) = limit.clone().acquire_owned().await else { return };
        while shakes.try_join_next().is_some() {}
        match listener.accept().await {
            Ok((tcp, from)) => {
                let (acceptor, out) = (acceptor.clone(), out.clone());
                shakes.spawn(async move {
                    let result = handshake(acceptor, tcp, from, within).await;
                    let _ = out.send(result).await;
                    drop(permit);
                });
            }
            Err(e) => {
                drop(permit);
                if out.send(Err(CarrierError::io(NAME, "accept", e))).await.is_err() {
                    return;
                }
                tokio::time::sleep(ACCEPT_PAUSE).await;
            }
        }
    }
}

/// The handshake of one client within `within`: TLS with its ALPN and its
/// key, the prefaces of HTTP/2, and its POST, which gets 200 and a body
/// with no end. A second request on the connection is refused.
async fn handshake(acceptor: TlsAcceptor, tcp: TcpStream, from: SocketAddr, within: Duration) -> Shaken {
    // The frames of a link are small; Nagle would hold a PING behind them.
    let _ = tcp.set_nodelay(true);
    let run = async {
        let tls =
            acceptor.accept(tcp).await.map_err(|e| pin::tls_error(NAME, format!("the TLS handshake of {from}"), e))?;
        let conn = tls.get_ref().1;
        pin::check_alpn(NAME, conn.alpn_protocol(), ALPN, &from)?;
        let key = pin::proved(NAME, conn.peer_certificates(), &from)?;
        let mut connection = ::h2::server::Builder::new()
            .initial_window_size(WINDOW)
            .initial_connection_window_size(WINDOW)
            .max_concurrent_streams(1)
            .handshake::<_, Bytes>(tls)
            .await
            .map_err(|e| h2_error(format!("the prefaces of HTTP/2 of {from}"), e))?;
        let (request, mut respond) = match connection.accept().await {
            Some(Ok(asked)) => asked,
            Some(Err(e)) => return Err(h2_error(format!("the request of {from}"), e)),
            None => {
                let text = format!("{from} closed its connection with no request");
                return Err(err(CarrierErrorKind::Io(io::ErrorKind::UnexpectedEof), text));
            }
        };
        if request.method() != http::Method::POST {
            let refused = http::Response::builder().status(http::StatusCode::METHOD_NOT_ALLOWED).body(());
            if let Ok(refused) = refused {
                let _ = respond.send_response(refused, true);
            }
            drive(connection);
            let text = format!("{from} asked with {}, not POST", request.method());
            return Err(err(CarrierErrorKind::Io(io::ErrorKind::InvalidData), text));
        }
        let answer = http::Response::builder()
            .status(http::StatusCode::OK)
            .header("content-type", "application/octet-stream")
            .body(())
            .map_err(|e| err(CarrierErrorKind::Io(io::ErrorKind::InvalidData), format!("the answer: {e}")))?;
        let send = respond.send_response(answer, false).map_err(|e| h2_error(format!("the answer to {from}"), e))?;
        drive(connection);
        Ok((Box::new(pod_ws::h2_road::duplex(send, request.into_body())) as BoxIo, key))
    };
    tokio::time::timeout(within, run).await.unwrap_or_else(|_| {
        Err(err(
            CarrierErrorKind::Io(io::ErrorKind::TimedOut),
            format!("the handshake of {from} took more than {within:?}"),
        ))
    })
}

/// Drive the connection of one link: its frames move while this task
/// runs, and each later request is refused.
fn drive<S>(mut connection: ::h2::server::Connection<S, Bytes>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
{
    tokio::spawn(async move {
        while let Some(asked) = connection.accept().await {
            match asked {
                Ok((_, mut respond)) => respond.send_reset(::h2::Reason::REFUSED_STREAM),
                Err(e) => {
                    tracing::debug!(error = %e, "the connection of an h2 listener ended");
                    return;
                }
            }
        }
    });
}
