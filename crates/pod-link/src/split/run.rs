//! The four tasks of one end of a split session. Each half is a duplex
//! carrier: its main direction carries the bytes of one end (up: the
//! dialer's, down: the listener's), and its other direction carries beats
//! alone. Each direction that waits for frames sees one at least each
//! `lost_after`: a frame of data, an end, or a beat that each sender writes
//! after a silence of `beat`. A silence, a failure or an early end of a
//! direction is the loss of its half: the session ends with
//! [`super::Lost`], and no half takes over the bytes of the other. The end
//! that saw the loss says it in a `LOST` frame on the other half, so that
//! each end names the same half. That end then closes its halves, so a
//! write or a read of the other half at this end can fail before the
//! `LOST` frame is read: such a failure waits a short time for the readers
//! of this end to take a `LOST` frame that came before the close
//! ([`Shared::lose_later`]).

use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _, DuplexStream, ReadHalf, WriteHalf};
use tokio::sync::watch;
use tokio::time::timeout;

use super::wire::{self, FRAME, kind};
use super::{Half, Lost};
use crate::carrier::BoxIo;
use crate::pipes::{Ends, Pipes, State, pipes, until};

/// The times of the beats.
#[derive(Debug, Clone, Copy)]
pub(super) struct Timing {
    pub beat: Duration,
    pub lost_after: Duration,
}

/// The two halves of one end: `out` carries the bytes of this end, `into`
/// the bytes of the peer.
pub(super) struct End {
    pub out: BoxIo,
    pub out_half: Half,
    pub into: BoxIo,
    pub in_half: Half,
}

/// The end of the bytes of each side.
#[derive(Debug, Clone, Copy, Default)]
struct Progress {
    sent_end: bool,
    got_end: bool,
}

struct Shared {
    ends: Arc<Ends>,
    progress: watch::Sender<Progress>,
    timing: Timing,
    /// The half whose loss ended the session.
    lost: Mutex<Option<Half>>,
}

impl Shared {
    fn lose(&self, half: Half, why: impl Into<String>) {
        self.lost.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert(half);
        let why = why.into();
        let make = move || io::Error::new(io::ErrorKind::ConnectionAborted, Lost { half, why: why.clone() });
        self.ends.fail_with(Arc::new(make));
    }

    /// The loss of `half` for a cause that the close of the far end after
    /// a loss of the other half also gives: a write or a read that fails,
    /// or a carrier that ends. The `LOST` frame of the far end came before
    /// its close and waits in a buffer of this end: its reader takes it in
    /// [`grace`]. A reset that drops the frame leaves this cause, so the
    /// session ends at once and not after a silence of `lost_after`.
    async fn lose_later(&self, half: Half, why: String) {
        let mut state = self.ends.watch();
        if timeout(grace(self.timing), until(&mut state, &[State::Failed])).await.is_err() {
            self.lose(half, why);
        }
    }

    fn progress(&self) -> Progress {
        *self.progress.borrow()
    }

    /// Wait for the end of the session: a fault, or the end of the bytes
    /// of each side.
    async fn over(&self) {
        let (mut progress, mut state) = (self.progress.subscribe(), self.ends.watch());
        tokio::select! {
            _ = progress.wait_for(|p| p.sent_end && p.got_end) => {}
            () = until(&mut state, &[State::Failed]) => {}
        }
    }
}

/// Start the tasks of `end`: the byte stream of the link.
pub(super) fn run(end: End, timing: Timing) -> Pipes {
    let ends = Ends::new();
    let (io, far) = pipes(FRAME, ends.clone());
    let s = Arc::new(Shared { ends, progress: watch::channel(Progress::default()).0, timing, lost: Mutex::new(None) });
    let (out_r, out_w) = tokio::io::split(end.out);
    let (in_r, in_w) = tokio::io::split(end.into);
    tokio::spawn(send_main(out_w, far.to_peer, end.out_half, s.clone()));
    tokio::spawn(recv_main(in_r, far.from_peer, end.in_half, s.clone()));
    tokio::spawn(beat_back(in_w, end.in_half, s.clone()));
    tokio::spawn(hear_back(out_r, end.out_half, s));
    io
}

/// Say the loss of the other half on `own`, within a beat.
async fn tell(w: &mut WriteHalf<BoxIo>, s: &Shared, own: Half) {
    let lost = *s.lost.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(half) = lost.filter(|h| *h != own) {
        let body = [wire::half_byte(half == Half::Up)];
        let _ = timeout(s.timing.beat, wire::write(w, kind::LOST, &body)).await;
    }
}

/// The most time that a failure of a write or a read waits for a `LOST`
/// frame that waits in a buffer: a beat, at most half a second.
fn grace(timing: Timing) -> Duration {
    timing.beat.min(Duration::from_millis(500))
}

/// The half of the body of a `LOST` frame.
fn told(body: &[u8]) -> Half {
    if body == [1] { Half::Up } else { Half::Down }
}

/// The bytes of the link in frames on the main direction of `half`, a beat
/// after each silence, and an end at the end of the link's bytes.
async fn send_main(mut w: WriteHalf<BoxIo>, mut from: DuplexStream, half: Half, s: Arc<Shared>) {
    let mut state = s.ends.watch();
    let mut buf = vec![0u8; FRAME];
    loop {
        let read = tokio::select! {
            read = from.read(&mut buf) => read,
            () = tokio::time::sleep(s.timing.beat) => {
                if let Err(e) = wire::write(&mut w, kind::BEAT, &[]).await {
                    return s.lose_later(half, format!("a beat: {}", pod_core::text::io_words(&e))).await;
                }
                continue;
            }
            () = until(&mut state, &[State::Failed]) => return tell(&mut w, &s, half).await,
        };
        // The link closes its bytes after the loss ended it: that is no end
        // of its bytes, and the peer must hear the loss.
        if s.ends.failed() {
            return tell(&mut w, &s, half).await;
        }
        let (kind, n) = match read {
            Ok(0) | Err(_) => (kind::END, 0),
            Ok(n) => (kind::DATA, n),
        };
        if let Err(e) = wire::write(&mut w, kind, &buf[..n]).await {
            return s.lose_later(half, format!("the bytes of this end: {}", pod_core::text::io_words(&e))).await;
        }
        if kind == kind::END {
            s.progress.send_modify(|p| p.sent_end = true);
            return;
        }
    }
}

/// The frames of the main direction of `half` into the link, until the end
/// of the peer's bytes. A link that dropped its byte stream takes no more
/// bytes; the end of the peer then comes within `lost_after`, or is taken
/// as come.
async fn recv_main(mut r: ReadHalf<BoxIo>, mut to: DuplexStream, half: Half, s: Arc<Shared>) {
    let (mut state, lost_after) = (s.ends.watch(), s.timing.lost_after);
    let mut gone = false;
    loop {
        // A frame read in part must not stop: the drop of the link is seen
        // between frames, and a beat comes each `beat`.
        gone |= *state.borrow() == State::Gone;
        let frame = tokio::select! {
            frame = timeout(lost_after, wire::read(&mut r)) => frame,
            () = until(&mut state, &[State::Failed]) => return,
        };
        let frame = match frame {
            Err(_) if gone => (kind::END, Vec::new()),
            Err(_) => return s.lose(half, format!("no frame for {lost_after:?}")),
            Ok(Err(e)) => {
                return s.lose_later(half, format!("the bytes of the peer: {}", pod_core::text::io_words(&e))).await;
            }
            Ok(Ok(None)) => {
                return s.lose_later(half, "the carrier ended before the end of the bytes of the peer".into()).await;
            }
            Ok(Ok(Some(frame))) => frame,
        };
        match frame.0 {
            kind::DATA => {
                if !gone && to.write_all(&frame.1).await.is_err() {
                    gone = true;
                }
            }
            kind::BEAT => {}
            kind::LOST => return s.lose(told(&frame.1), "the peer lost it"),
            kind::END => {
                let _ = to.shutdown().await;
                s.progress.send_modify(|p| p.got_end = true);
                return;
            }
            other => return s.lose(half, format!("a frame of type {other} on the main direction")),
        }
    }
}

/// A beat after each silence of `beat` on the other direction of `half`,
/// until the session is over.
async fn beat_back(mut w: WriteHalf<BoxIo>, half: Half, s: Arc<Shared>) {
    loop {
        tokio::select! {
            () = tokio::time::sleep(s.timing.beat) => {}
            () = s.over() => return tell(&mut w, &s, half).await,
        }
        if let Err(e) = wire::write(&mut w, kind::BEAT, &[]).await {
            // A peer whose bytes ended may close its half.
            if !s.progress().got_end {
                s.lose_later(half, format!("a beat: {}", pod_core::text::io_words(&e))).await;
            }
            return;
        }
    }
}

/// The beats on the other direction of `half`. Its end is no loss once one
/// side ended its bytes: the peer closes its halves after it read the end
/// of this side's bytes, or after it wrote the end of its own.
async fn hear_back(mut r: ReadHalf<BoxIo>, half: Half, s: Arc<Shared>) {
    let lost_after = s.timing.lost_after;
    loop {
        let frame = tokio::select! {
            frame = timeout(lost_after, wire::read(&mut r)) => frame,
            () = s.over() => return,
        };
        let ended = |p: Progress| p.sent_end || p.got_end;
        match frame {
            Err(_) => return s.lose(half, format!("no beat for {lost_after:?}")),
            Ok(Ok(Some((kind::BEAT, _)))) => {}
            Ok(Ok(Some((kind::LOST, body)))) => return s.lose(told(&body), "the peer lost it"),
            Ok(Ok(Some((other, _)))) => return s.lose(half, format!("a frame of type {other} in place of a beat")),
            Ok(Ok(None)) if ended(s.progress()) => return,
            Ok(Ok(None)) => return s.lose_later(half, "the peer closed the carrier".into()).await,
            Ok(Err(_)) if ended(s.progress()) => return,
            Ok(Err(e)) => {
                return s.lose_later(half, format!("the beats of the peer: {}", pod_core::text::io_words(&e))).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared() -> Arc<Shared> {
        let timing = Timing { beat: Duration::from_millis(50), lost_after: Duration::from_millis(400) };
        Arc::new(Shared {
            ends: Ends::new(),
            progress: watch::channel(Progress::default()).0,
            timing,
            lost: Mutex::new(None),
        })
    }

    fn lost(s: &Shared) -> Option<Half> {
        *s.lost.lock().unwrap()
    }

    /// The far end saw the silence of the up half, said `LOST` on the down
    /// half, and closed it: the failed beat on the down half comes before
    /// the reader of the down half takes the `LOST` frame, which names the
    /// half.
    #[tokio::test(start_paused = true)]
    async fn a_failed_write_waits_for_the_lost_frame_that_names_the_half() {
        let s = shared();
        let late = tokio::spawn({
            let s = s.clone();
            async move { s.lose_later(Half::Down, "a beat: broken pipe".into()).await }
        });
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert_eq!(lost(&s), None, "a failed write ended the session at once");
        s.lose(Half::Up, "the peer lost it");
        late.await.unwrap();
        assert_eq!(lost(&s), Some(Half::Up));
    }

    /// With no `LOST` frame, the failure names its own half after the
    /// grace, not after `lost_after`: a reset ends the session at once.
    #[tokio::test(start_paused = true)]
    async fn a_failed_write_alone_names_its_half_after_the_grace() {
        let s = shared();
        let started = tokio::time::Instant::now();
        s.lose_later(Half::Down, "a beat: broken pipe".into()).await;
        assert_eq!(lost(&s), Some(Half::Down));
        assert_eq!(started.elapsed(), s.timing.beat);
        let slow = Timing { beat: Duration::from_secs(10), lost_after: Duration::from_secs(30) };
        assert_eq!(grace(slow), Duration::from_millis(500));
    }
}
