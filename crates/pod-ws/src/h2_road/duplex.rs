//! One stream of HTTP/2 as a byte stream: its DATA frames carry the bytes
//! both ways, and END_STREAM is TCP's half-close. The reader's pace opens
//! the window, so a slow reader holds the far end back.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use h2::{RecvStream, SendStream};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// The most bytes of one write that go in one turn.
const CHUNK: usize = 64 * 1024;

/// The DATA of one stream of HTTP/2 both ways, as a byte stream.
pub struct Stream {
    send: SendStream<Bytes>,
    recv: RecvStream,
    /// The rest of a DATA frame that the reader's buffer did not take.
    pending: Bytes,
    shut: bool,
}

impl Stream {
    /// The stream of `send` and `recv`: a request's body and its answer's.
    pub(crate) fn from_parts(send: SendStream<Bytes>, recv: RecvStream) -> Stream {
        Stream { send, recv, pending: Bytes::new(), shut: false }
    }
}

impl AsyncRead for Stream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        loop {
            if !this.pending.is_empty() {
                let n = this.pending.len().min(buf.remaining());
                buf.put_slice(&this.pending.split_to(n));
                return Poll::Ready(Ok(()));
            }
            match std::task::ready!(this.recv.poll_data(cx)) {
                Some(Ok(chunk)) => {
                    // The window opens as a frame is taken, and no frame is
                    // taken before the last one is read: the reader's pace
                    // holds the far end back.
                    let _ = this.recv.flow_control().release_capacity(chunk.len());
                    this.pending = chunk;
                }
                Some(Err(e)) => return Poll::Ready(Err(io_error(e))),
                None => return Poll::Ready(Ok(())),
            }
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        if data.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let this = &mut *self;
        let want = data.len().min(CHUNK);
        this.send.reserve_capacity(want);
        loop {
            match std::task::ready!(this.send.poll_capacity(cx)) {
                // Capacity that frames queued before took: the next call
                // waits for more.
                Some(Ok(0)) => continue,
                Some(Ok(room)) => {
                    let n = room.min(want);
                    this.send.send_data(Bytes::copy_from_slice(&data[..n]), false).map_err(io_error)?;
                    return Poll::Ready(Ok(n));
                }
                Some(Err(e)) => return Poll::Ready(Err(io_error(e))),
                None => {
                    return Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "the far end ended the stream")));
                }
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // The connection's task sends each frame as the windows let it.
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.shut {
            self.shut = true;
            // END_STREAM: the half-close, which the far end passes to the
            // target as TCP's own.
            let _ = self.send.send_data(Bytes::new(), true);
        }
        Poll::Ready(Ok(()))
    }
}

fn io_error(e: h2::Error) -> io::Error {
    if e.is_io() {
        return e.into_io().unwrap_or_else(|| io::Error::other("HTTP/2's connection failed"));
    }
    io::Error::other(format!("the stream of HTTP/2: {e}"))
}
