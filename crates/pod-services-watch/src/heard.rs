//! A stream that tells whether the far end sent a byte. A TLS handshake
//! that the network ends before the first byte of the server is no answer
//! (call C153); one that ends after it is an answer of another kind.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use pod_core::BoxedIo;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// The stream, and the flag that its first byte sets.
pub struct Heard {
    io: BoxedIo,
    heard: Arc<AtomicBool>,
}

/// The flag of a [`Heard`] stream, which the caller keeps after the stream
/// goes to the handshake.
pub struct Flag(Arc<AtomicBool>);

impl Flag {
    /// True when the far end sent a byte.
    pub fn any(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

impl Heard {
    /// `io` as a stream that notes its first byte, and the flag.
    pub fn wrap(io: BoxedIo) -> (BoxedIo, Flag) {
        let heard = Arc::new(AtomicBool::new(false));
        (Box::new(Heard { io, heard: heard.clone() }), Flag(heard))
    }
}

impl AsyncRead for Heard {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let polled = Pin::new(&mut self.io).poll_read(cx, buf);
        if buf.filled().len() > before {
            self.heard.store(true, Ordering::Relaxed);
        }
        polled
    }
}

impl AsyncWrite for Heard {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.io).poll_write(cx, buf)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.io).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.io.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}
