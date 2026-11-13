//! A stream whose first bytes were read before HTTP/2 takes it: the dial
//! reads the first bytes of the listener to learn that the listener took
//! its key, and HTTP/2 reads them again from here.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, ReadBuf};

/// The bytes of one read of the first answer.
const FIRST: usize = 4096;

/// A stream with bytes that came before its next read.
pub(super) struct Primed<S> {
    first: Vec<u8>,
    at: usize,
    inner: S,
}

impl<S: AsyncRead + Unpin> Primed<S> {
    /// Read the first bytes of `inner`: an end of the stream is an error.
    pub(super) async fn read(mut inner: S) -> io::Result<Primed<S>> {
        let mut first = vec![0u8; FIRST];
        let n = inner.read(&mut first).await?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        first.truncate(n);
        Ok(Primed { first, at: 0, inner })
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Primed<S> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
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

impl<S: AsyncWrite + Unpin> AsyncWrite for Primed<S> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, data)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt as _;

    #[tokio::test]
    async fn the_first_bytes_are_read_again_then_the_rest() {
        let (near, mut far) = tokio::io::duplex(64);
        far.write_all(b"first").await.unwrap();
        let mut primed = Primed::read(near).await.unwrap();
        far.write_all(b" rest").await.unwrap();
        drop(far);
        let mut got = Vec::new();
        primed.read_to_end(&mut got).await.unwrap();
        assert_eq!(got, b"first rest");
        let (near, far) = tokio::io::duplex(64);
        drop(far);
        let e = Primed::read(near).await.err().unwrap();
        assert_eq!(e.kind(), io::ErrorKind::UnexpectedEof);
    }
}
