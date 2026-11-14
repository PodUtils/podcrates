//! The byte stream of the quic carrier: the one bidirectional stream of a
//! connection, the dial's side of its handshake, and the errors of QUIC as
//! errors of the carrier.

use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use pod_core::Target;
use pod_identity::PublicKey;
use pod_quic::noq::{
    self, ClientConfig, Connection, ConnectionError, Endpoint, ReadError, ReadExactError, RecvStream, SendStream,
    TransportErrorCode, VarInt,
};
use rustls::pki_types::CertificateDer;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::{ACCEPTED, NAME, err};
use crate::carrier::{CarrierError, CarrierErrorKind};
use crate::pin;

/// How long the close of a dial's connection may take to leave, before its
/// endpoint goes.
const CLOSE_WAIT: Duration = Duration::from_millis(500);

/// The one stream of a connection of the quic carrier. A drop ends the
/// stream, and a task waits until the peer acknowledged each byte (at most
/// the linger), then closes the connection.
pub(crate) struct QuicIo {
    send: Option<SendStream>,
    recv: RecvStream,
    conn: Connection,
    /// A dial's own endpoint: the task waits for the close to leave.
    endpoint: Option<Endpoint>,
    linger: Duration,
}

impl QuicIo {
    pub(super) fn new(send: SendStream, recv: RecvStream, conn: Connection) -> QuicIo {
        QuicIo { send: Some(send), recv, conn, endpoint: None, linger: super::LINGER }
    }

    pub(super) fn linger(mut self, linger: Duration, endpoint: Option<Endpoint>) -> QuicIo {
        self.linger = linger;
        self.endpoint = endpoint;
        self
    }

    fn send(&mut self) -> Pin<&mut SendStream> {
        Pin::new(self.send.as_mut().expect("the send stream lives until the drop"))
    }
}

impl fmt::Debug for QuicIo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QuicIo").field("connection", &self.conn.stable_id()).finish_non_exhaustive()
    }
}

impl AsyncRead for QuicIo {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        AsyncRead::poll_read(Pin::new(&mut self.recv), cx, buf)
    }
}

impl AsyncWrite for QuicIo {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        AsyncWrite::poll_write(self.send(), cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        AsyncWrite::poll_flush(self.send(), cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        AsyncWrite::poll_shutdown(self.send(), cx)
    }
}

impl Drop for QuicIo {
    fn drop(&mut self) {
        let Some(mut send) = self.send.take() else { return };
        // The end of the stream; a stream that the shutdown ended keeps it.
        let _ = send.finish();
        let (conn, endpoint, linger) = (self.conn.clone(), self.endpoint.take(), self.linger);
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            conn.close(VarInt::from_u32(0), b"");
            return;
        };
        // A close at once would drop the bytes that the peer did not
        // acknowledge: the close waits for them.
        runtime.spawn(async move {
            let _ = tokio::time::timeout(linger, send.stopped()).await;
            conn.close(VarInt::from_u32(0), b"");
            if let Some(endpoint) = endpoint {
                let _ = tokio::time::timeout(CLOSE_WAIT, endpoint.wait_idle()).await;
            }
        });
    }
}

/// The dial's side of the handshake: the TLS of QUIC, the ALPN, the key of
/// the listener, and the byte of acceptance on the stream that the
/// listener opened.
pub(super) async fn dial(
    endpoint: &Endpoint,
    config: ClientConfig,
    addr: SocketAddr,
    name: &str,
    alpn: &[u8],
    own: &PublicKey,
    target: &Target,
) -> Result<(QuicIo, PublicKey), CarrierError> {
    let connecting = endpoint
        .connect_with(config, addr, name)
        .map_err(|e| err(CarrierErrorKind::Usage, format!("connect to {target} ({addr}): {e}")))?;
    let conn =
        connecting.await.map_err(|e| connection_error(format!("the QUIC handshake with {target}"), e, target, own))?;
    check(&conn, alpn, target)?;
    let key = proved(&conn, target)?;
    let (send, mut recv) = conn.accept_bi().await.map_err(|e| answer_error(target, own, e))?;
    let mut word = [0u8];
    match recv.read_exact(&mut word).await {
        Ok(()) if word[0] == ACCEPTED => Ok((QuicIo::new(send, recv, conn), key)),
        Ok(()) => Err(err(
            CarrierErrorKind::Io(io::ErrorKind::InvalidData),
            format!("the listener at {target} sent {} where its acceptance goes", word[0]),
        )),
        Err(ReadExactError::ReadError(ReadError::ConnectionLost(e))) => Err(answer_error(target, own, e)),
        Err(ReadExactError::FinishedEarly(_)) => Err(err(
            CarrierErrorKind::Io(io::ErrorKind::UnexpectedEof),
            format!("the listener at {target} ended the stream before it took this end"),
        )),
        Err(ReadExactError::ReadError(e)) => {
            Err(CarrierError::io(NAME, format!("the acceptance of the listener at {target}"), e.into()))
        }
    }
}

/// The ALPN of the handshake must be `alpn`.
pub(super) fn check(conn: &Connection, alpn: &[u8], peer: &dyn fmt::Display) -> Result<(), CarrierError> {
    let data = conn.handshake_data().and_then(|d| d.downcast::<noq::crypto::rustls::HandshakeData>().ok());
    pin::check_alpn(NAME, data.as_ref().and_then(|d| d.protocol.as_deref()), alpn, peer)
}

/// The key that the peer proved ([`pin::proved`]).
pub(super) fn proved(conn: &Connection, peer: &dyn fmt::Display) -> Result<PublicKey, CarrierError> {
    let certificates = conn.peer_identity().and_then(|d| d.downcast::<Vec<CertificateDer<'static>>>().ok());
    pin::proved(NAME, certificates.as_deref().map(Vec::as_slice), peer)
}

/// The alerts of TLS with which a listener refuses the key of a dialer.
fn refused_by_peer(code: TransportErrorCode) -> bool {
    use rustls::AlertDescription as A;
    [A::CertificateUnknown, A::BadCertificate, A::AccessDenied, A::UnsupportedCertificate, A::CertificateRequired]
        .into_iter()
        .any(|alert| code == TransportErrorCode::crypto(u8::from(alert)))
}

/// The error of the wait for the stream of the listener: a close with an
/// alert of a refused certificate is a refusal of the key of this end.
fn answer_error(target: &Target, own: &PublicKey, e: ConnectionError) -> CarrierError {
    connection_error(format!("the acceptance of the listener at {target}"), e, target, own)
}

/// A failed connection as an error of the carrier: a key that the pin of
/// this end refused, with its typed [`pod_tls::peer::Refusal`]; a key of
/// this end that the peer refused (`KeyRefused`); the silence; else the
/// kind of I/O that fits.
pub(super) fn connection_error(
    text: String,
    e: ConnectionError,
    peer: &dyn fmt::Display,
    own: &PublicKey,
) -> CarrierError {
    match &e {
        ConnectionError::TransportError(t) => {
            if let Some(tls) = t.crypto.as_ref().and_then(|c| c.downcast_ref::<rustls::Error>()) {
                return pin::tls_error(NAME, text, io::Error::new(io::ErrorKind::InvalidData, tls.clone()));
            }
            CarrierError::with_source(
                NAME,
                CarrierErrorKind::Io(io::ErrorKind::InvalidData),
                text,
                io::Error::new(io::ErrorKind::InvalidData, e),
            )
        }
        ConnectionError::ConnectionClosed(close) if refused_by_peer(close.error_code) => CarrierError::with_source(
            NAME,
            CarrierErrorKind::KeyRefused,
            format!("the peer at {peer} refused the key of this end, {}", own.fingerprint()),
            io::Error::new(io::ErrorKind::PermissionDenied, e),
        ),
        _ => {
            let kind = match e {
                ConnectionError::TimedOut => io::ErrorKind::TimedOut,
                ConnectionError::VersionMismatch => io::ErrorKind::Unsupported,
                ConnectionError::LocallyClosed => io::ErrorKind::ConnectionAborted,
                _ => io::ErrorKind::ConnectionReset,
            };
            CarrierError::with_source(NAME, CarrierErrorKind::Io(kind), text, io::Error::new(kind, e))
        }
    }
}
