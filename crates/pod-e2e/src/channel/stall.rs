//! The lines of a wait of the carry that holds (PodUtils/podssh#283). The
//! layer of `pod-session` says when the channel takes no byte of it, but it
//! cannot tell a channel that holds from a reader of the channel's
//! application that stopped; the channel's own line tells them apart.
//!
//! The rule and the time are those of the layer's pump
//! ([`pod_session::pump::STALL_AFTER`]): a write of the carry that takes no
//! byte for that time gives one line, with the bytes of its way. A write to
//! the application waits while bytes of the peer wait for it; a write to the
//! layer waits while bytes of the application wait for it. Each byte taken
//! ends the wait, and the rest start a new one. A read waits for the peer or
//! for the application, which can be quiet with no fault, so it gives no
//! line. The steps before the carry have limits of their own
//! ([`crate::HANDSHAKE_LIMIT`]). The layer's pump looks at its waits from a
//! task of its own, as its halves run apart; each write of the carry has
//! its own timer here, as the carry awaits each write in turn.

use std::future::Future;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use pod_core::{Event, Events};
use pod_session::pump::STALL_AFTER;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::ends::EVENT_WAIT;

/// Which way a write of the carry goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Way {
    /// From the layer to the application.
    In,
    /// From the application to the layer.
    Out,
}

/// The waits of one carry: where their lines go, and the bytes of each way.
pub(crate) struct Waits {
    events: Option<Arc<dyn Events>>,
    /// The bytes read from the layer, then those written to the application.
    inbound: [AtomicU64; 2],
    /// The bytes read from the application, then those written to the layer.
    outbound: [AtomicU64; 2],
}

impl Waits {
    pub(crate) fn new(events: Option<Arc<dyn Events>>) -> Waits {
        Waits { events, inbound: Default::default(), outbound: Default::default() }
    }

    fn of(&self, way: Way) -> &[AtomicU64; 2] {
        match way {
            Way::In => &self.inbound,
            Way::Out => &self.outbound,
        }
    }

    /// `n` more bytes came in to go `way`.
    pub(crate) fn read(&self, way: Way, n: usize) {
        self.of(way)[0].fetch_add(n as u64, Ordering::Relaxed);
    }

    /// Write all of `bytes` the `way` they go to `w`, and flush them: a
    /// write that takes no byte for [`STALL_AFTER`] says its line once.
    /// `unopened` are the bytes of the layer that wait behind them.
    pub(crate) async fn write<W>(&self, way: Way, w: &mut W, mut bytes: &[u8], unopened: usize) -> io::Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        while !bytes.is_empty() {
            let left = bytes.len();
            let n = self.within(w.write(bytes), way, left, unopened).await?;
            if n == 0 {
                return Err(io::ErrorKind::WriteZero.into());
            }
            self.of(way)[1].fetch_add(n as u64, Ordering::Relaxed);
            bytes = &bytes[n..];
        }
        self.within(w.flush(), way, 0, unopened).await
    }

    /// `step`, which says the line of its wait once when it holds for
    /// [`STALL_AFTER`], and then goes on.
    async fn within<F: Future>(&self, step: F, way: Way, left: usize, unopened: usize) -> F::Output {
        let Some(events) = &self.events else { return step.await };
        tokio::pin!(step);
        match tokio::time::timeout(STALL_AFTER, step.as_mut()).await {
            Ok(out) => out,
            Err(_) => {
                let [read, wrote] = self.of(way).each_ref().map(|n| n.load(Ordering::Relaxed));
                events.event(Event::info(EVENT_WAIT, line(way, STALL_AFTER.as_secs(), left, unopened, (read, wrote))));
                step.await
            }
        }
    }
}

/// The line of a write that went `way` and took no byte for `secs`, while
/// `left` bytes wait for it: `read` bytes came in for the way, and `wrote`
/// went out.
pub(crate) fn line(way: Way, secs: u64, left: usize, unopened: usize, (read, wrote): (u64, u64)) -> String {
    match way {
        Way::In => format!(
            "the application took no byte of the channel for {secs} s, while {left} bytes wait for it; the channel \
             read {read} bytes from the layer, {unopened} of them not yet opened, and wrote {wrote} bytes to the \
             application"
        ),
        Way::Out => format!(
            "the layer took no byte of the channel for {secs} s, while {left} bytes wait for it; the channel read \
             {read} bytes from the application and wrote {wrote} bytes to the layer"
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use pod_identity::Identity;
    use tokio::io::{AsyncReadExt, DuplexStream};

    use super::*;
    use crate::Channel;

    #[test]
    fn each_way_gives_the_bytes_of_its_sides() {
        let inbound = line(Way::In, 20, 31_744, 18, (32_805, 1024));
        assert_eq!(
            inbound,
            "the application took no byte of the channel for 20 s, while 31744 bytes wait for it; the channel read \
             32805 bytes from the layer, 18 of them not yet opened, and wrote 1024 bytes to the application"
        );
        let outbound = line(Way::Out, 20, 4115, 0, (8192, 4115));
        assert_eq!(
            outbound,
            "the layer took no byte of the channel for 20 s, while 4115 bytes wait for it; the channel read 8192 \
             bytes from the application and wrote 4115 bytes to the layer"
        );
    }

    /// The operator's and the node's channels over one pipe, as the layer
    /// gives them.
    async fn pair() -> (Channel<DuplexStream>, Channel<DuplexStream>) {
        let (ours, theirs) = tokio::io::duplex(1 << 20);
        let (op, node) = (Identity::from_seed(&[1; 32]), Identity::from_seed(&[2; 32]));
        let operator = async { crate::initiate(ours, &op).await.unwrap().accept(None).await.unwrap() };
        let node = async { crate::respond(theirs, &node).await.unwrap().admit().await.unwrap() };
        tokio::join!(operator, node)
    }

    /// The texts of the events of the kind `e2e.wait`.
    fn kept() -> (Arc<Mutex<Vec<String>>>, Arc<dyn Events>) {
        let lines: Arc<Mutex<Vec<String>>> = Arc::default();
        let into = lines.clone();
        let events: Arc<dyn Events> = Arc::new(move |event: Event| {
            assert_eq!(event.kind, EVENT_WAIT);
            into.lock().unwrap().push(event.text);
        });
        (lines, events)
    }

    /// The node's target sends 32 KiB; the operator's application takes
    /// 1 KiB and then reads no more: one line after 20 s, with the bytes
    /// held, and none before; a quiet application and a quiet target give
    /// none. A byte taken ends the wait, and the new wait gives its line
    /// only after a new 20 s.
    #[tokio::test(start_paused = true)]
    async fn an_application_that_stops_reading_gives_one_stall_line_each_wait() {
        let (operator, node) = pair().await;
        let (lines, events) = kept();
        let (app, mut user) = tokio::io::duplex(1024);
        let (target, mut far) = tokio::io::duplex(1 << 16);
        far.write_all(&[7; 32_768]).await.unwrap();
        let carries = async { tokio::join!(operator.with_events(events).carry(app), node.carry(target)) };
        tokio::pin!(carries);
        let secs = Duration::from_secs;
        assert!(tokio::time::timeout(secs(19), carries.as_mut()).await.is_err());
        assert!(lines.lock().unwrap().is_empty(), "no line in 19 s");
        assert!(tokio::time::timeout(secs(90), carries.as_mut()).await.is_err());
        let first = "the application took no byte of the channel for 20 s, while 31744 bytes wait for it; the \
                     channel read 32787 bytes from the layer, 0 of them not yet opened, and wrote 1024 bytes to the \
                     application";
        assert_eq!(*lines.lock().unwrap(), [first]);
        // The application takes 1 KiB more: that wait ends, and a new one
        // starts.
        user.read_exact(&mut [0; 1024]).await.unwrap();
        assert!(tokio::time::timeout(secs(19), carries.as_mut()).await.is_err());
        assert_eq!(lines.lock().unwrap().len(), 1, "no new line in 19 s of the new wait");
        assert!(tokio::time::timeout(secs(2), carries.as_mut()).await.is_err());
        let second = first.replace("31744 bytes wait", "30720 bytes wait").replace("wrote 1024", "wrote 2048");
        assert_eq!(*lines.lock().unwrap(), [first.to_string(), second]);
        drop((user, far));
    }
}
