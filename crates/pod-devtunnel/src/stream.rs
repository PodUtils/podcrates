//! A stream to a port of the tunnel's host: one `direct-tcpip` channel of
//! the SSH session, as `AsyncRead + AsyncWrite`. The end of a stream says
//! who ended it: the far end's EOF or close is the end of the bytes
//! (`Ok(0)`), and the end of the session under it (a reconnect, a relay
//! that went away) is an error of the kind `ConnectionAborted`. The crate
//! never opens a stream again by itself.

use std::fmt;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use russh::client::Msg;
use russh::{Channel, ChannelMsg, ChannelReadHalf, ChannelWriteHalf};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// The words of the error of a stream whose session ended.
pub const SESSION_ENDED: &str = "the relay session of the stream ended";

type Next = Pin<Box<dyn Future<Output = (ChannelReadHalf, Option<ChannelMsg>)> + Send>>;

/// How the read side stands.
enum Reading {
    Idle(ChannelReadHalf),
    Waiting(Next),
    /// The far end ended the bytes.
    Ended,
    /// The session ended under the stream.
    Aborted,
}

/// A stream to a port of the host.
pub struct Stream {
    port: u16,
    session: u64,
    reading: Reading,
    pending: Vec<u8>,
    at: usize,
    writer: Pin<Box<dyn AsyncWrite + Send>>,
    /// The write half, kept for the close at the drop.
    half: Option<ChannelWriteHalf<Msg>>,
}

impl Stream {
    pub(crate) fn new(channel: Channel<Msg>, port: u16, session: u64) -> Stream {
        let (read, half) = channel.split();
        let writer = Box::pin(half.make_writer());
        Stream { port, session, reading: Reading::Idle(read), pending: Vec::new(), at: 0, writer, half: Some(half) }
    }

    /// The port of the host.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The number of the relay session of the stream: 1 for the first,
    /// one more for each reconnect.
    pub fn session(&self) -> u64 {
        self.session
    }
}

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stream").field("port", &self.port).field("session", &self.session).finish_non_exhaustive()
    }
}

fn aborted() -> io::Error {
    io::Error::new(io::ErrorKind::ConnectionAborted, SESSION_ENDED)
}

impl AsyncRead for Stream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        loop {
            if this.at < this.pending.len() {
                let n = buf.remaining().min(this.pending.len() - this.at);
                buf.put_slice(&this.pending[this.at..this.at + n]);
                this.at += n;
                return Poll::Ready(Ok(()));
            }
            match std::mem::replace(&mut this.reading, Reading::Ended) {
                Reading::Idle(mut read) => {
                    this.reading = Reading::Waiting(Box::pin(async move {
                        let message = read.wait().await;
                        (read, message)
                    }));
                }
                Reading::Waiting(mut next) => match next.as_mut().poll(cx) {
                    Poll::Pending => {
                        this.reading = Reading::Waiting(next);
                        return Poll::Pending;
                    }
                    Poll::Ready((read, message)) => match message {
                        Some(ChannelMsg::Data { data }) => {
                            this.pending = data.to_vec();
                            this.at = 0;
                            this.reading = Reading::Idle(read);
                        }
                        // The far end ended the bytes: an end, not an error.
                        Some(ChannelMsg::Eof | ChannelMsg::Close) => return Poll::Ready(Ok(())),
                        Some(_) => this.reading = Reading::Idle(read),
                        // No close came: the session went with the channel.
                        None => {
                            this.reading = Reading::Aborted;
                            return Poll::Ready(Err(aborted()));
                        }
                    },
                },
                Reading::Ended => return Poll::Ready(Ok(())),
                Reading::Aborted => {
                    this.reading = Reading::Aborted;
                    return Poll::Ready(Err(aborted()));
                }
            }
        }
    }
}

/// A write error of a channel whose session is gone is the same abort as
/// the read side's.
fn write_error(e: io::Error) -> io::Error {
    match e.kind() {
        io::ErrorKind::BrokenPipe | io::ErrorKind::NotConnected | io::ErrorKind::ConnectionReset => aborted(),
        _ => e,
    }
}

impl AsyncWrite for Stream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        self.get_mut().writer.as_mut().poll_write(cx, buf).map_err(write_error)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().writer.as_mut().poll_flush(cx).map_err(write_error)
    }

    /// An EOF to the host: the far end reads the end, and can still write.
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().writer.as_mut().poll_shutdown(cx).map_err(write_error)
    }
}

impl Drop for Stream {
    /// A close of the channel, so that the host frees its connection.
    fn drop(&mut self) {
        if let Some(half) = self.half.take()
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            runtime.spawn(async move {
                let _ = half.close().await;
            });
        }
    }
}
