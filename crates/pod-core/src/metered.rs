//! A stream that counts its bytes both ways and gives a signal at a limit:
//! the relay closes a session at a byte budget, so a caller moves before it.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::Notify;

/// A stream with a count of its bytes, read and written.
pub struct Metered<T> {
    inner: T,
    used: u64,
    limit: u64,
    spent: Option<Arc<Notify>>,
    flag: Option<Arc<AtomicBool>>,
}

impl<T> Metered<T> {
    /// Notify `spent` once, when `limit` bytes went through.
    pub fn new(inner: T, limit: u64, spent: Arc<Notify>) -> Self {
        Self { inner, used: 0, limit, spent: Some(spent), flag: None }
    }

    /// [`Metered::new`] that sets `flag` at the limit, in place of a notice.
    pub fn with_flag(inner: T, limit: u64, flag: Arc<AtomicBool>) -> Self {
        Self { inner, used: 0, limit, spent: None, flag: Some(flag) }
    }

    /// The bytes that went through, both ways.
    pub fn used(&self) -> u64 {
        self.used
    }

    pub fn into_inner(self) -> T {
        self.inner
    }

    fn count(&mut self, n: usize) {
        self.used = self.used.saturating_add(n as u64);
        if self.used >= self.limit {
            if let Some(spent) = self.spent.take() {
                spent.notify_one();
            }
            if let Some(flag) = self.flag.take() {
                flag.store(true, Ordering::Relaxed);
            }
        }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for Metered<T> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let this = &mut *self;
        let poll = Pin::new(&mut this.inner).poll_read(cx, buf);
        let n = buf.filled().len() - before;
        this.count(n);
        poll
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for Metered<T> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        let this = &mut *self;
        let poll = Pin::new(&mut this.inner).poll_write(cx, data);
        if let Poll::Ready(Ok(n)) = poll {
            this.count(n);
        }
        poll
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = &mut *self;
        let poll = Pin::new(&mut this.inner).poll_write_vectored(cx, bufs);
        if let Poll::Ready(Ok(n)) = poll {
            this.count(n);
        }
        poll
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn the_limit_gives_its_signal_once_and_counts_both_ways() {
        let (a, mut b) = tokio::io::duplex(64);
        let flag = Arc::new(AtomicBool::new(false));
        let mut m = Metered::with_flag(a, 8, flag.clone());
        m.write_all(b"12345").await.unwrap();
        assert!(!flag.load(Ordering::Relaxed));
        b.write_all(b"678").await.unwrap();
        let mut buf = [0u8; 3];
        m.read_exact(&mut buf).await.unwrap();
        assert_eq!(m.used(), 8);
        assert!(flag.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn a_notice_wakes_its_waiter_at_the_limit() {
        let (a, _b) = tokio::io::duplex(64);
        let spent = Arc::new(Notify::new());
        let mut m = Metered::new(a, 4, spent.clone());
        let waiter = spent.notified();
        m.write_all(b"four").await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), waiter).await.expect("the notice came");
    }
}
