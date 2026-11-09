//! The byte stream between the two peers, after the relay paired them.
//!
//! The TLS object of OpenSSL holds the context that the SRP functions
//! configured, and the context points into the buffers of the context. The
//! field order of [`Stream`] keeps those alive for the whole session.

use std::fmt;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use openssl::ssl::ErrorCode;
use pod_core::BoxedIo;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_openssl::SslStream;

use crate::srp::{SSL_R_UNEXPECTED_EOF_WHILE_READING, Srp};

/// An encrypted stream between two peers. The relay carries the bytes; this
/// type carries the session.
pub struct Stream {
    stream: SslStream<BoxedIo>,
    /// True once the peer's end of stream has been read.
    peer_finished: bool,
    /// The context of the session and its buffers. The stream holds the
    /// context, and the context points into this value, so the value drops
    /// last. The field is never read again: it keeps the context alive.
    _srp: Srp,
}

impl Stream {
    /// True once the peer's end of stream has been read.
    pub fn peer_finished(&self) -> bool {
        self.peer_finished
    }

    pub(crate) fn new(stream: SslStream<BoxedIo>, srp: Srp) -> Stream {
        Stream { stream, peer_finished: false, _srp: srp }
    }
}

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stream").finish_non_exhaustive()
    }
}

impl AsyncRead for Stream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        // The end of a session arrives in one of three ways, and all three
        // mean the same thing to the caller: a close_notify (`ZeroReturn`), a
        // peer that closed the transport with no close_notify (an
        // "unexpected eof" error), and a bare read of zero bytes.
        //
        // Each of them can be reported before OpenSSL hands over the records
        // that arrived in the same read. A stream that stops there loses the
        // tail of a transfer, so the records that OpenSSL still holds are read
        // first and the end is reported on the next call.
        match Pin::new(&mut this.stream).poll_read(cx, buf) {
            Poll::Ready(Err(error)) if is_end(&error) => this.end_of_stream(cx, buf),
            Poll::Ready(Ok(())) if buf.filled().is_empty() => this.end_of_stream(cx, buf),
            other => other,
        }
    }
}

impl Stream {
    /// Report the end of the peer, after the records that OpenSSL holds.
    fn end_of_stream(&mut self, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        if self.stream.ssl().pending() > 0 {
            // Records from the same read are still decrypted. Poll again
            // rather than report an end, and let the next call return them.
            cx.waker().wake_by_ref();
            return Pin::new(&mut self.stream).poll_read(cx, buf);
        }
        self.peer_finished = true;
        Poll::Ready(Ok(()))
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
        // A shutdown before this side has read the peer's end of stream ends
        // the session while the peer still has bytes to deliver: over this
        // relay, the first side to finish decides when the session ends. Do
        // nothing until the peer has finished; the caller ends the process,
        // and the socket closes with the last record already sent.
        if !self.peer_finished {
            return Poll::Ready(Ok(()));
        }
        // The inner shutdown sends the close_notify that gsocket also sends
        // (lib/gsocket-ssl.c) and then closes the write direction. It does not
        // wait for the close_notify of the peer.
        Pin::new(&mut self.get_mut().stream).poll_shutdown(cx)
    }
}

/// True when the error is the peer closing the transport with no
/// close_notify.
fn is_end(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::UnexpectedEof {
        return true;
    }
    let Some(ssl) = error.get_ref().and_then(|source| source.downcast_ref::<openssl::ssl::Error>()) else {
        return false;
    };
    ssl.code() == ErrorCode::ZERO_RETURN
        || ssl.ssl_error().is_some_and(|stack| {
            stack.errors().iter().any(|error| error.reason_code() == SSL_R_UNEXPECTED_EOF_WHILE_READING)
        })
}
