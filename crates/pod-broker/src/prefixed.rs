//! A stream whose first bytes were read already: the bytes that a client
//! sent after its request's head belong to the tunnel or to the body.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// `first`, then the bytes of `inner`; writes go to `inner`.
pub struct Prefixed<S> {
    first: Vec<u8>,
    at: usize,
    inner: S,
}

impl<S> Prefixed<S> {
    pub fn new(first: Vec<u8>, inner: S) -> Prefixed<S> {
        Prefixed { first, at: 0, inner }
    }

    /// Put `bytes` before the bytes that are not read yet: the bytes after a
    /// head that a reader took belong to the next read.
    pub fn put_back(&mut self, mut bytes: Vec<u8>) {
        bytes.extend_from_slice(&self.first[self.at..]);
        self.first = bytes;
        self.at = 0;
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Prefixed<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        if this.at < this.first.len() {
            let n = (this.first.len() - this.at).min(buf.remaining());
            buf.put_slice(&this.first[this.at..this.at + n]);
            this.at += n;
            if this.at == this.first.len() {
                this.first = Vec::new();
                this.at = 0;
            }
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Prefixed<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
