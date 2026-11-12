//! One stream of a link: a byte stream in each direction, with the credit
//! of the peer, a half close and a reset.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::error::{Error, ResetCode};
use crate::frame::Frame;
use crate::state::{Shared, State};

/// One stream of a [`crate::Link`]. It reads and writes bytes in order.
///
/// * A write takes what the credit of the stream and of the link allow,
///   up to one frame ([`crate::Settings::frame_size`]), and waits when
///   there is no credit. A flush returns at once: the link writes its
///   queue in order.
/// * A shutdown sends `CLOSE`: the peer reads the end of the stream, and
///   can still write.
/// * A drop before the end closes the stream: `CLOSE` for this side, and a
///   reset ([`ResetCode::CANCEL`]) when the peer did not close its side.
///   The bytes that wait unread go back to the link's credit.
pub struct Stream {
    id: u32,
    shared: Arc<Shared>,
}

impl Stream {
    pub(crate) fn new(id: u32, shared: Arc<Shared>) -> Stream {
        Stream { id, shared }
    }

    /// The id of the stream: odd when the dialer opened it, even when the
    /// listener did.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// End the stream in both directions with `code`. The bytes that wait
    /// unread are dropped; each later read and write fails.
    pub fn reset(&self, code: ResetCode) {
        let id = self.id;
        self.shared.with(|s| {
            if s.ended.is_some() {
                return;
            }
            let Some(slot) = s.streams.get_mut(&id) else { return };
            if slot.reset.is_some() {
                return;
            }
            slot.reset = Some(Error::Reset { id, code, by_peer: false });
            let dropped = std::mem::take(&mut slot.buf).len();
            let wakers = [slot.read_waker.take(), slot.write_waker.take()];
            for w in wakers {
                s.wake(w);
            }
            s.push_out(Frame::Reset(id, code.0));
            s.discard(dropped);
        });
    }

    /// The bytes that wait unread in the stream.
    pub fn buffered(&self) -> usize {
        self.shared.with(|s| s.streams.get(&self.id).map_or(0, |slot| slot.buf.len()))
    }
}

impl std::fmt::Debug for Stream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stream").field("id", &self.id).finish()
    }
}

impl State {
    fn read(&mut self, id: u32, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let Some(slot) = self.streams.get_mut(&id) else {
            return Poll::Ready(Err(Error::Closed { by_peer: false }.into()));
        };
        if !slot.buf.is_empty() {
            let n = buf.remaining().min(slot.buf.len());
            let (a, b) = slot.buf.as_slices();
            let first = n.min(a.len());
            buf.put_slice(&a[..first]);
            buf.put_slice(&b[..n - first]);
            slot.buf.drain(..n);
            slot.to_return += n as u64;
            self.to_return += n as u64;
            self.return_credit(Some(id));
            return Poll::Ready(Ok(()));
        }
        if slot.remote_fin {
            return Poll::Ready(Ok(()));
        }
        if let Some(e) = slot.reset.clone().or_else(|| self.ended.clone()) {
            return Poll::Ready(Err(e.into()));
        }
        slot.read_waker = Some(cx.waker().clone());
        Poll::Pending
    }

    fn write(&mut self, id: u32, cx: &mut Context<'_>, bytes: &[u8]) -> Poll<io::Result<usize>> {
        let ended = self.ended.clone();
        let (closing, frame_size, room) = (self.closing, self.settings.frame_size, self.queue_limit());
        let room = room.saturating_sub(self.queued);
        let link_credit = self.send_credit;
        let Some(slot) = self.streams.get_mut(&id) else {
            return Poll::Ready(Err(Error::Closed { by_peer: false }.into()));
        };
        if let Some(e) = slot.reset.clone().or(ended) {
            return Poll::Ready(Err(e.into()));
        }
        if slot.local_fin {
            return Poll::Ready(Err(Error::WriteClosed(id).into()));
        }
        if closing {
            return Poll::Ready(Err(Error::Closed { by_peer: false }.into()));
        }
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let stream_credit = slot.send_credit;
        let n = (stream_credit.min(link_credit) as usize).min(frame_size).min(room).min(bytes.len());
        if n == 0 {
            slot.write_waker = Some(cx.waker().clone());
            if stream_credit > 0 {
                self.block(id);
            }
            return Poll::Pending;
        }
        slot.send_credit -= n as u64;
        self.send_credit -= n as u64;
        self.push_out(Frame::Data(id, bytes[..n].to_vec()));
        Poll::Ready(Ok(n))
    }

    fn shutdown(&mut self, id: u32) -> Poll<io::Result<()>> {
        let ended = self.ended.clone();
        let Some(slot) = self.streams.get_mut(&id) else {
            return Poll::Ready(Err(Error::Closed { by_peer: false }.into()));
        };
        if slot.local_fin {
            return Poll::Ready(Ok(()));
        }
        if let Some(e) = slot.reset.clone().or(ended) {
            return Poll::Ready(Err(e.into()));
        }
        slot.local_fin = true;
        self.push_out(Frame::Close(id));
        Poll::Ready(Ok(()))
    }

    /// The handle of a stream is gone: close it and free its slot.
    fn dropped(&mut self, id: u32) {
        let Some(slot) = self.streams.remove(&id) else { return };
        if self.ended.is_some() {
            return;
        }
        let live = slot.reset.is_none();
        if live && !slot.local_fin {
            self.push_out(Frame::Close(id));
        }
        if live && !slot.remote_fin {
            self.push_out(Frame::Reset(id, ResetCode::CANCEL.0));
        }
        self.discard(slot.buf.len());
    }
}

impl AsyncRead for Stream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let id = self.id;
        self.shared.with(|s| s.read(id, cx, buf))
    }
}

impl AsyncWrite for Stream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, bytes: &[u8]) -> Poll<io::Result<usize>> {
        let id = self.id;
        self.shared.with(|s| s.write(id, cx, bytes))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let id = self.id;
        self.shared.with(|s| s.shutdown(id))
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let id = self.id;
        self.shared.with(|s| s.dropped(id));
    }
}
