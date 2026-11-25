//! The bytes of a leg past the mark of a move (PodUtils/podnet#756). This
//! side passes the mark with its own bytes and holds; then the far end's
//! application writes one byte at a time. The far end sends its window of
//! payload, each byte in a `DATA` record of its own, so the leg carries 14
//! bytes for each byte of the window: more than the window and two chunks,
//! and less than `resume::after_mark`.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll, ready};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf};
use tokio::sync::oneshot;

use pod_session::pump::{self, Watch};
use pod_session::resume::after_mark;
use pod_session::{Carry, Decoder, Link};

const KIB: u64 = 1 << 10;
/// The mark of the move.
const MARK: u64 = 128 * KIB;
/// The window of each end.
const WINDOW: usize = 32 << 10;
/// The ask's bound past the mark: the window, two chunks of 32 KiB and 64
/// bytes.
const ASKED: u64 = WINDOW as u64 + 64 * KIB + 64;

/// An application that gives `piece` bytes at each read, with no end, once
/// its gate opens, and takes each byte written to it.
struct Source {
    piece: usize,
    gate: Option<oneshot::Receiver<()>>,
}

impl AsyncRead for Source {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        if let Some(gate) = self.gate.as_mut() {
            let _ = ready!(Pin::new(gate).poll(cx));
            self.gate = None;
        }
        let n = self.piece.min(buf.remaining());
        buf.put_slice(&vec![7; n]);
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for Source {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// One way of the stand-in relay: each byte counted in `carried`.
async fn one_way(
    mut from: tokio::io::ReadHalf<DuplexStream>,
    mut to: tokio::io::WriteHalf<DuplexStream>,
    carried: Arc<AtomicU64>,
) {
    let mut buf = vec![0u8; 64 * 1024];
    while let Ok(n) = from.read(&mut buf).await {
        if n == 0 || to.write_all(&buf[..n]).await.is_err() {
            return;
        }
        carried.fetch_add(n as u64, Ordering::SeqCst);
    }
}

/// A leg through a stand-in relay that counts its bytes, both ways.
fn counted_leg(carried: &Arc<AtomicU64>) -> (DuplexStream, DuplexStream) {
    let (client, relay_client) = tokio::io::duplex(1 << 20);
    let (relay_far, far) = tokio::io::duplex(1 << 20);
    let (cr, cw) = tokio::io::split(relay_client);
    let (fr, fw) = tokio::io::split(relay_far);
    tokio::spawn(one_way(cr, fw, carried.clone()));
    tokio::spawn(one_way(fr, cw, carried.clone()));
    (client, far)
}

#[tokio::test(start_paused = true)]
async fn records_of_one_byte_past_the_mark_stay_under_the_bound() {
    let carried = Arc::new(AtomicU64::new(0));
    let (client_leg, far_leg) = counted_leg(&carried);
    let (open, gate) = oneshot::channel();
    let mut client_app = Source { piece: 32 << 10, gate: None };
    let mut far_app = Source { piece: 1, gate: Some(gate) };
    let watch = Watch::moving_at(MARK).within(Some(WINDOW));
    let far_watch = Watch::far_end().within(Some(WINDOW));
    let mut carry = Carry::new(Link::new(0, 0).with_replay(1 << 20));
    let mut far_carry = Carry::new(Link::new(0, 0).with_replay(1 << 20));
    let client = pump::run(&mut client_app, client_leg, Decoder::new(), &mut carry, &watch);
    let far = pump::run(&mut far_app, far_leg, Decoder::new(), &mut far_carry, &far_watch);
    tokio::pin!(client, far);

    // This side passes the mark with its own bytes, and holds.
    let held = async {
        while !watch.hold.on() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    };
    tokio::select! {
        ended = client.as_mut() => panic!("the leg ended: {:?}", ended.end),
        ended = far.as_mut() => panic!("the far end's leg ended: {:?}", ended.end),
        () = held => {}
    }
    // Then the far end's application writes one byte at a time, until the
    // far end waits for an acknowledgement that does not come.
    open.send(()).unwrap();
    tokio::select! {
        ended = client.as_mut() => panic!("the leg ended: {:?}", ended.end),
        ended = far.as_mut() => panic!("the far end's leg ended: {:?}", ended.end),
        () = tokio::time::sleep(Duration::from_secs(10)) => {}
    }
    let carried = carried.load(Ordering::SeqCst);
    assert!(watch.hold.on(), "the leg still holds");
    assert!(carried > MARK + ASKED, "the heads of the records pass the asked bound: {carried} bytes");
    assert!(carried >= MARK + 14 * WINDOW as u64, "14 bytes for each byte of the window: {carried} bytes");
    let bound = MARK + after_mark(WINDOW);
    assert!(carried < bound, "the leg carried {carried} bytes, under the bound of {bound}");
}
