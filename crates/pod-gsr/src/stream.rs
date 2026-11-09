//! The byte stream between the two peers, after the relay paired them.
//!
//! The TLS object of OpenSSL holds the context that the SRP functions
//! configured, and the context points into the buffers of the context. The
//! field order of [`Stream`] keeps those alive for the whole session.

use std::fmt;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use foreign_types::ForeignTypeRef;
use openssl::ssl::ErrorCode;
use pod_core::BoxedIo;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_openssl::SslStream;

use crate::End;
use crate::srp::{SSL_R_UNEXPECTED_EOF_WHILE_READING, Srp};

/// An encrypted stream between two peers. The relay carries the bytes; this
/// type carries the session.
pub struct Stream {
    stream: SslStream<Transport>,
    /// How the peer's side ended, once a read gave its end.
    end: Option<End>,
    /// True once this side sent its close_notify.
    notified: bool,
    /// The context of the session and its buffers. The stream holds the
    /// context, and the context points into this value, so the value drops
    /// last. The field is never read again: it keeps the context alive.
    _srp: Srp,
}

impl Stream {
    /// True once the peer's end of stream has been read.
    pub fn peer_finished(&self) -> bool {
        self.end.is_some()
    }

    /// How the peer's side ended: none until a read gives the end of the
    /// stream. A read gives 0 bytes at both ends; this record tells a
    /// close_notify ([`End::Clean`]) from a transport that ended without one
    /// ([`End::Cut`]).
    pub fn end(&self) -> Option<End> {
        self.end
    }

    pub(crate) fn new(stream: SslStream<Transport>, srp: Srp) -> Stream {
        Stream { stream, end: None, notified: false, _srp: srp }
    }

    /// Report the end of the peer, after the records that OpenSSL holds.
    fn end_of_stream(&mut self, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        if self.stream.ssl().pending() > 0 {
            // Records from the same read are still decrypted. Poll again
            // rather than report an end, and let the next call return them.
            cx.waker().wake_by_ref();
            return Pin::new(&mut self.stream).poll_read(cx, buf);
        }
        self.end = Some(if self.close_notify_received() { End::Clean } else { End::Cut });
        Poll::Ready(Ok(()))
    }

    /// True when OpenSSL read the close_notify of the peer.
    fn close_notify_received(&self) -> bool {
        // SAFETY: the pointer is the live SSL object of `self.stream`.
        let state = unsafe { openssl_sys::SSL_get_shutdown(self.stream.ssl().as_ptr()) };
        state & openssl_sys::SSL_RECEIVED_SHUTDOWN != 0
    }
}

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stream").field("end", &self.end).finish_non_exhaustive()
    }
}

impl AsyncRead for Stream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        // OpenSSL reads 0 bytes into a full buffer: that is no end.
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if this.end.is_some() {
            return Poll::Ready(Ok(()));
        }
        // The end of a session arrives in one of three ways, and all three
        // give a read of 0 bytes to the caller: a close_notify (`ZeroReturn`,
        // or a bare read of zero bytes), and a peer that closed the transport
        // with no close_notify (an "unexpected eof" error, or a bare read of
        // zero bytes). `end` records which one it was.
        //
        // Each of them can be reported before OpenSSL hands over the records
        // that arrived in the same read. A stream that stops there loses the
        // tail of a transfer, so the records that OpenSSL still holds are read
        // first and the end is reported on the next call.
        let before = buf.filled().len();
        match Pin::new(&mut this.stream).poll_read(cx, buf) {
            Poll::Ready(Err(error)) if is_clean(&error) || is_cut(&error) => this.end_of_stream(cx, buf),
            Poll::Ready(Ok(())) if buf.filled().len() == before => this.end_of_stream(cx, buf),
            other => other,
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().stream).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        // The close_notify ends this side's writes, as the SSL_shutdown of
        // gsocket does (lib/gsocket-ssl.c), and the peer reads it as
        // `End::Clean`. It is a record that the relay carries: it does not
        // end the session.
        if !this.notified && this.end != Some(End::Cut) {
            this.stream.get_mut().hold = true;
            ready!(Pin::new(&mut this.stream).poll_shutdown(cx))?;
            this.notified = true;
        }
        // The transport stays open until this side has read the peer's end:
        // over this relay, the first side to close its transport decides when
        // the session ends, while the peer may still have bytes to deliver.
        if this.end.is_none() {
            return Poll::Ready(Ok(()));
        }
        let transport = this.stream.get_mut();
        transport.hold = false;
        Pin::new(transport).poll_shutdown(cx)
    }
}

/// The transport under the TLS session. While `hold` is set, a shutdown
/// flushes and does not close the write direction, so that a close_notify
/// can go out on a transport that stays open.
pub(crate) struct Transport {
    io: BoxedIo,
    hold: bool,
}

impl Transport {
    pub(crate) fn new(io: BoxedIo) -> Transport {
        Transport { io, hold: false }
    }
}

impl AsyncRead for Transport {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().io).poll_read(cx, buf)
    }
}

impl AsyncWrite for Transport {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().io).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().io).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.hold {
            return Pin::new(&mut this.io).poll_flush(cx);
        }
        Pin::new(&mut this.io).poll_shutdown(cx)
    }
}

/// True when the error is the close_notify of the peer.
fn is_clean(error: &io::Error) -> bool {
    ssl_error(error).is_some_and(|ssl| ssl.code() == ErrorCode::ZERO_RETURN)
}

/// True when the error is a transport that ended with no close_notify.
fn is_cut(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::UnexpectedEof {
        return true;
    }
    ssl_error(error).is_some_and(|ssl| {
        ssl.ssl_error().is_some_and(|stack| {
            stack.errors().iter().any(|error| error.reason_code() == SSL_R_UNEXPECTED_EOF_WHILE_READING)
        })
    })
}

fn ssl_error(error: &io::Error) -> Option<&openssl::ssl::Error> {
    error.get_ref().and_then(|source| source.downcast_ref::<openssl::ssl::Error>())
}
