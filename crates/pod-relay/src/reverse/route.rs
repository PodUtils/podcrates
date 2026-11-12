//! The local side of each session of a node socket: its opener, its two
//! copies, the bytes that wait for it, and its end. The relay has no flow
//! control for one session and no half close, so the node bounds each wait
//! itself: a local side that reads no more ends its own session
//! ([`offer`]); after the relay's close, the bytes that wait for it go
//! within `Settings::drain` ([`closed_by_relay`]); and with
//! `Settings::half_close`, the end of the local side's writes waits for the
//! operator's last bytes ([`to_local`]).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{Notify, mpsc, oneshot};

use super::framing::SessionId;
use super::framing::legs::CHUNK_BYTES;
use super::node::Handler;
use super::serve::{Out, Outbox};

/// The local side of a session: where its data goes, the bytes that wait for
/// it and how many may, and its two copies (to it, then from it), which stop
/// at once when it stalls.
pub(super) struct Route {
    data: mpsc::UnboundedSender<Vec<u8>>,
    waiting: Arc<AtomicUsize>,
    limit: usize,
    copies: [tokio::task::AbortHandle; 2],
}

/// The local side of each session that has one, by id.
pub(super) type Routes = Arc<Mutex<HashMap<SessionId, Route>>>;

/// The sessions whose local side opens now.
pub(super) type Pending = Arc<Mutex<HashSet<SessionId>>>;

/// What the opener of a session takes of the node's settings.
#[derive(Clone, Copy)]
pub(super) struct Copies {
    /// How long the handler may take to open the local side.
    pub open_limit: Duration,
    /// `Settings::half_close`.
    pub half_close: Option<Duration>,
}

pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// The sessions that have a local side, or one that opens now.
pub(super) fn live(routes: &Routes, pending: &Pending) -> usize {
    lock(routes).len() + lock(pending).len()
}

/// Whether `id` has a local side, or one that opens now. A second `open` of
/// it is no new session: a second local side would take its bytes, and its
/// second `ready` would answer an `open` that the relay sent once.
pub(super) fn known(routes: &Routes, pending: &Pending, id: &SessionId) -> bool {
    lock(routes).contains_key(id) || lock(pending).contains(id)
}

/// Give a frame's payload to its session without waiting. A queue that holds
/// its handler's bytes already means that the local side reads no more;
/// waiting for it would stop every session of the socket, as the relay has
/// no flow control for one session. So that session ends here: true when it
/// did.
pub(super) fn offer(routes: &Routes, id: SessionId, payload: &[u8]) -> bool {
    let mut routes = lock(routes);
    let Some(route) = routes.get(&id) else { return false };
    if route.waiting.load(Ordering::SeqCst) + payload.len() > route.limit {
        if let Some(route) = routes.remove(&id) {
            for copy in route.copies {
                copy.abort();
            }
        }
        return true;
    }
    route.waiting.fetch_add(payload.len(), Ordering::SeqCst);
    // Sent; or the local writer is gone, and the session's own end tells the
    // relay.
    let _ = route.data.send(payload.to_vec());
    false
}

/// The relay closed `id`. Nothing that the local side writes goes out now,
/// so its reads stop at once; the bytes that wait for it go within `drain`,
/// then the node drops them and the local side. A local side that never
/// reads would hold its copy, and the connection, for as long as the node
/// runs (podnet T-194).
pub(super) fn closed_by_relay(routes: &Routes, id: &SessionId, drain: Duration) {
    let Some(route) = lock(routes).remove(id) else { return };
    let Route { copies: [to_local, from_local], .. } = route;
    from_local.abort();
    // The route's sender is dropped here: the copy ends once its bytes went.
    tokio::spawn(async move {
        tokio::time::sleep(drain).await;
        to_local.abort();
    });
}

/// The end of a session that the node makes: its route goes, and the relay
/// is told with `close {id}`; nothing when the route went already (the
/// relay's close, a stall, the end of the socket), as those tell the relay
/// themselves.
async fn end(routes: &Routes, tx: &Outbox, id: SessionId) {
    if lock(routes).remove(&id).is_some() {
        let _ = tx.send(Out::LocalEnd(id)).await;
    }
}

/// The two ends of a half close: the bits of [`Ends`] that each copy sets.
const READ_END: u8 = 1;
const WRITE_END: u8 = 2;

/// Which copies of a session ended, under a half close: the one that sets
/// its bit last ends the session, so the session ends once, and always.
#[derive(Default)]
struct Ends {
    bits: AtomicU8,
    /// The copy from the local side met its end.
    read_end: Notify,
}

impl Ends {
    /// Set `bit`; true when the other copy ended before.
    fn set(&self, bit: u8) -> bool {
        let other = if bit == READ_END { WRITE_END } else { READ_END };
        self.bits.fetch_or(bit, Ordering::SeqCst) & other != 0
    }

    /// Ends once the local side's writes ended and no byte came for `idle`
    /// since; a byte that comes makes a new future of this.
    async fn quiet(&self, idle: Duration) {
        while self.bits.load(Ordering::SeqCst) & READ_END == 0 {
            self.read_end.notified().await;
        }
        tokio::time::sleep(idle).await;
    }
}

/// Open the local side of `id`, and only then queue `ready`; on a refusal or
/// after `open_limit`, `reject` with the reason. Then copy between the local
/// side and the socket until either ends.
pub(super) async fn open<H: Handler>(
    id: SessionId,
    handler: Arc<H>,
    copies: Copies,
    tx: Outbox,
    routes: Routes,
    pending: Pending,
) {
    let opened = tokio::time::timeout(copies.open_limit, handler.open(id)).await;
    let stream = match opened {
        Ok(Ok(stream)) => stream,
        Ok(Err(reason)) => {
            lock(&pending).remove(&id);
            let _ = tx.send(Out::Reject(id, reason)).await;
            return;
        }
        Err(_) => {
            lock(&pending).remove(&id);
            let reason = format!("the local side did not open within {} s", copies.open_limit.as_secs());
            let _ = tx.send(Out::Reject(id, reason)).await;
            return;
        }
    };
    let (mut local_read, local_write) = tokio::io::split(stream);
    let (in_tx, in_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (waiting, limit) = (Arc::new(AtomicUsize::new(0)), handler.queue_bytes());
    let ends = copies.half_close.map(|idle| (idle, Arc::new(Ends::default())));
    let to_local =
        tokio::spawn(to_local(id, local_write, in_rx, waiting.clone(), ends.clone(), tx.clone(), routes.clone()));
    // The local reader starts once `ready` is queued: the writer drops data
    // of a session that is not readied.
    let (go, readied) = oneshot::channel::<()>();
    let from_local = tokio::spawn({
        let (tx, routes) = (tx.clone(), routes.clone());
        async move {
            if readied.await.is_err() {
                return;
            }
            let mut buf = vec![0u8; CHUNK_BYTES];
            let read_end = loop {
                match local_read.read(&mut buf).await {
                    Ok(0) => break true,
                    Err(_) => break false,
                    Ok(n) => {
                        if tx.send(Out::Data(id, buf[..n].to_vec())).await.is_err() {
                            break false;
                        }
                    }
                }
            };
            // A half close: the operator's last bytes may still come, and
            // the copy to the local side ends the session, unless it ended.
            if let Some((_, ends)) = ends.filter(|_| read_end)
                && !ends.set(READ_END)
            {
                ends.read_end.notify_one();
                return;
            }
            end(&routes, &tx, id).await;
        }
    });
    // The route exists before `ready`, so the first bytes after it find it.
    let route = Route { data: in_tx, waiting, limit, copies: [to_local.abort_handle(), from_local.abort_handle()] };
    lock(&routes).insert(id, route);
    lock(&pending).remove(&id);
    let _ = tx.send(Out::Ready(id)).await;
    let _ = go.send(());
}

/// The copy to the local side, until the relay's close or a failed write.
/// With a half close, the end of the local side's writes does not end the
/// session: the relay has no half close, so the operator's last bytes may
/// still come. The session ends when none came for `idle` (podnet T-196).
async fn to_local<W: AsyncWrite + Unpin>(
    id: SessionId,
    mut write: W,
    mut data: mpsc::UnboundedReceiver<Vec<u8>>,
    waiting: Arc<AtomicUsize>,
    half: Option<(Duration, Arc<Ends>)>,
    tx: Outbox,
    routes: Routes,
) {
    loop {
        let bytes = match &half {
            None => data.recv().await,
            Some((idle, ends)) => tokio::select! {
                bytes = data.recv() => bytes,
                () = ends.quiet(*idle) => break,
            },
        };
        let Some(bytes) = bytes else { break };
        if write.write_all(&bytes).await.is_err() {
            break;
        }
        // Written: the bytes wait no more.
        waiting.fetch_sub(bytes.len(), Ordering::SeqCst);
    }
    let _ = write.shutdown().await;
    if let Some((_, ends)) = half
        && ends.set(WRITE_END)
    {
        end(&routes, &tx, id).await;
    }
}
