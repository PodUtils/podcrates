//! The byte stream of a link over a carrier of tasks (the poll carriers,
//! the s3 carrier, the split of two carriers), on each side: two pipes (the
//! bytes that came from the peer, the bytes for the peer) whose far ends
//! the tasks of the carrier hold, and the fault that ended them. A task
//! that fails gives its error to the link at the next read or write, not
//! an end of the stream.

use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf};
use tokio::sync::watch;

/// The state of the two ends of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    Live,
    /// The link dropped its byte stream.
    Gone,
    /// A lane failed: the fault says why.
    Failed,
}

/// The maker of the error of a fault: each read and write after it gets
/// one.
type Fault = Arc<dyn Fn() -> io::Error + Send + Sync>;

/// What the lanes and the byte stream of one session share.
pub(crate) struct Ends {
    fault: Mutex<Option<Fault>>,
    state: watch::Sender<State>,
}

impl Ends {
    pub fn new() -> Arc<Ends> {
        Arc::new(Ends { fault: Mutex::new(None), state: watch::channel(State::Live).0 })
    }

    /// End the session with an error; the first fault stays.
    #[cfg_attr(not(feature = "poll"), allow(dead_code))]
    pub fn fail(&self, kind: io::ErrorKind, text: impl Into<String>) {
        let text = text.into();
        self.fail_with(Arc::new(move || io::Error::new(kind, text.clone())));
    }

    /// End the session with the errors of `make` (a typed error inside);
    /// the first fault stays.
    pub fn fail_with(&self, make: Fault) {
        let mut fault = self.fault.lock().unwrap_or_else(|e| e.into_inner());
        if fault.is_none() {
            *fault = Some(make);
        }
        drop(fault);
        self.state.send_replace(State::Failed);
    }

    /// A fault ended the session. It is set before the state turns to
    /// `Failed`, so it is the first sign of a fault.
    pub fn failed(&self) -> bool {
        self.fault.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    pub fn fault(&self) -> Option<io::Error> {
        let fault = self.fault.lock().unwrap_or_else(|e| e.into_inner());
        fault.as_ref().map(|make| make())
    }

    pub fn watch(&self) -> watch::Receiver<State> {
        self.state.subscribe()
    }

    fn gone(&self) {
        self.state.send_if_modified(|s| {
            let live = *s == State::Live;
            if live {
                *s = State::Gone;
            }
            live
        });
    }
}

/// Wait until the state is one of `states`.
pub(crate) async fn until(rx: &mut watch::Receiver<State>, states: &[State]) {
    if rx.wait_for(|s| states.contains(s)).await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// The byte stream that the link reads and writes.
pub(crate) struct Pipes {
    from_peer: DuplexStream,
    to_peer: DuplexStream,
    ends: Arc<Ends>,
}

/// The far ends of the pipes, for the lanes: the bytes of the peer go into
/// the first, the bytes for the peer come out of the second.
pub(crate) struct Far {
    pub from_peer: DuplexStream,
    pub to_peer: DuplexStream,
}

/// A byte stream and its far ends, with pipes of `size` bytes.
pub(crate) fn pipes(size: usize, ends: Arc<Ends>) -> (Pipes, Far) {
    let (from_peer, far_in) = tokio::io::duplex(size);
    let (to_peer, far_out) = tokio::io::duplex(size);
    (Pipes { from_peer, to_peer, ends }, Far { from_peer: far_in, to_peer: far_out })
}

impl Pipes {
    /// The fault that ended the session, when one did.
    #[cfg_attr(not(feature = "smtp"), allow(dead_code))]
    pub(crate) fn fault(&self) -> Option<io::Error> {
        self.ends.fault()
    }
}

impl AsyncRead for Pipes {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        match Pin::new(&mut this.from_peer).poll_read(cx, buf) {
            Poll::Ready(Ok(())) if buf.filled().len() == before && buf.remaining() > 0 => {
                Poll::Ready(this.ends.fault().map_or(Ok(()), Err))
            }
            other => other,
        }
    }
}

impl AsyncWrite for Pipes {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, data: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        // The lanes can still read the pipe: no byte goes after a fault.
        if let Some(e) = this.ends.fault() {
            return Poll::Ready(Err(e));
        }
        match Pin::new(&mut this.to_peer).poll_write(cx, data) {
            Poll::Ready(Err(e)) => Poll::Ready(Err(this.ends.fault().unwrap_or(e))),
            other => other,
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().to_peer).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().to_peer).poll_shutdown(cx)
    }
}

impl Drop for Pipes {
    fn drop(&mut self) {
        self.ends.gone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    #[tokio::test]
    async fn a_fault_comes_in_place_of_the_end() {
        let ends = Ends::new();
        let (mut io, far) = pipes(64, ends.clone());
        drop(far.from_peer);
        let mut got = Vec::new();
        assert_eq!(io.read_to_end(&mut got).await.unwrap(), 0, "a clean end with no fault");
        let ends = Ends::new();
        let (mut io, far) = pipes(64, ends.clone());
        ends.fail(io::ErrorKind::TimedOut, "no answer");
        assert!(ends.failed());
        assert_eq!(io.write(b"x").await.unwrap_err().kind(), io::ErrorKind::TimedOut, "no write after a fault");
        drop(far);
        assert_eq!(io.read(&mut [0; 8]).await.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert_eq!(io.write(b"x").await.unwrap_err().kind(), io::ErrorKind::TimedOut);
        let mut state = ends.watch();
        drop(io);
        assert_eq!(*state.borrow_and_update(), State::Failed, "a drop after a fault keeps the fault");
    }
}
