//! The credit windows: a stream that stops reading holds only its own
//! window, and the other streams go on; a writer waits for credit and
//! splits its bytes into frames; a reader that keeps up grows its window.

mod common;

use std::sync::Arc;

use common::{RawPeer, echo, limit, pattern};
use pod_link::{Frame, INITIAL_WINDOW, Link, Role, Settings};
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::timeout;

/// A small link: 32 KiB for a stream, 64 KiB for the link, so that two
/// stalled streams would take the link's whole window.
fn small() -> Settings {
    Settings { stream_window: 32 << 10, link_window: 64 << 10, frame_size: 4 << 10, ..Settings::default() }
}

/// Write while the credit that this end holds now allows; give the bytes
/// that went. A write with no credit is pending at once, with no I/O.
fn fill(stream: &mut pod_link::Stream) -> usize {
    use std::task::{Context, Poll, Waker};
    let chunk = vec![0x55; 4096];
    let mut cx = Context::from_waker(Waker::noop());
    let mut sent = 0;
    while let Poll::Ready(n) = std::pin::Pin::new(&mut *stream).poll_write(&mut cx, &chunk) {
        sent += n.unwrap();
    }
    sent
}

/// One stream that never reads and one that read, then stopped, hold
/// their windows; 50 other streams still echo 100 KiB each.
async fn a_stalled_reader_starves_no_other(dialer: Link, listener: Link) {
    let listener = Arc::new(listener);
    // A stream whose reader never reads: its window stays at 16 KiB.
    let mut never = dialer.open().unwrap();
    let held_never = timeout(limit(), listener.accept()).await.unwrap().unwrap();
    assert_eq!(fill(&mut never), INITIAL_WINDOW as usize);
    // A stream whose reader took 200 KiB and then stopped: its window grew
    // to the setting, and it holds no more than that.
    let mut grown = dialer.open().unwrap();
    let mut held_grown = timeout(limit(), listener.accept()).await.unwrap().unwrap();
    let writer = tokio::spawn(async move {
        grown.write_all(&pattern(9, 200 << 10)).await.unwrap();
        grown
    });
    let mut first = vec![0; 200 << 10];
    held_grown.read_exact(&mut first).await.unwrap();
    let mut grown = writer.await.unwrap();
    let more = fill(&mut grown);
    assert!(more <= 32 << 10, "{more} bytes past a stalled reader with a window of 32 KiB");
    assert!(held_never.buffered() <= INITIAL_WINDOW as usize);
    // The other streams go on.
    let server = listener.clone();
    tokio::spawn(async move {
        while let Ok(mut s) = server.accept().await {
            tokio::spawn(async move {
                let (mut r, mut w) = tokio::io::split(&mut s);
                let _ = tokio::io::copy(&mut r, &mut w).await;
                let _ = w.shutdown().await;
            });
        }
    });
    let dialer = Arc::new(dialer);
    let mut tasks = Vec::new();
    for i in 0..50u32 {
        let stream = dialer.open().unwrap();
        tasks.push(tokio::spawn(async move {
            let data = pattern(i, 100 << 10);
            let (mut r, mut w) = tokio::io::split(stream);
            let send = data.clone();
            let writer = tokio::spawn(async move {
                w.write_all(&send).await.unwrap();
                w.shutdown().await.unwrap();
                w
            });
            let mut back = Vec::new();
            r.read_to_end(&mut back).await.unwrap();
            drop(writer.await.unwrap());
            assert!(back == data, "stream {i}");
        }));
    }
    for t in tasks {
        timeout(limit(), t).await.expect("the other streams move").unwrap();
    }
    assert_eq!(held_never.buffered(), INITIAL_WINDOW as usize, "the stalled bytes wait");
    drop((never, grown, held_never, held_grown));
}

on_both!(
    a_stalled_reader_starves_no_other_over_memory,
    a_stalled_reader_starves_no_other_over_unix,
    small(),
    a_stalled_reader_starves_no_other
);

/// 4 MiB and some bytes on one stream arrive whole, through windows that
/// grow from 16 KiB.
async fn four_mebibytes(dialer: Link, listener: Link) {
    let _server = echo(listener);
    let stream = dialer.open().unwrap();
    let data = pattern(4, (4 << 20) + 12_345);
    let (mut r, mut w) = tokio::io::split(stream);
    let send = data.clone();
    let writer = tokio::spawn(async move {
        w.write_all(&send).await.unwrap();
        w.shutdown().await.unwrap();
        w
    });
    let mut back = Vec::new();
    timeout(limit(), r.read_to_end(&mut back)).await.unwrap().unwrap();
    drop(writer.await.unwrap());
    assert!(back == data, "{} bytes back of {}", back.len(), data.len());
}

on_both!(four_mebibytes_over_memory, four_mebibytes_over_unix, Settings::default(), four_mebibytes);

/// A writer sends 16 KiB on a new stream, in frames of the frame size, and
/// then waits for the peer's credit of the stream and of the link.
#[tokio::test]
async fn a_writer_waits_for_credit_and_splits_its_frames() {
    let settings = Settings { frame_size: 1 << 10, ..Settings::default() };
    let (link, mut peer) = RawPeer::start(Role::Dialer, settings).await;
    let mut stream = link.open().unwrap();
    let data = pattern(3, 40 << 10);
    let send = data.clone();
    let writer = tokio::spawn(async move {
        stream.write_all(&send).await.unwrap();
        stream
    });
    assert_eq!(peer.until(|f| matches!(f, Frame::Open(_))).await, Frame::Open(1));
    let mut got = Vec::new();
    while got.len() < INITIAL_WINDOW as usize {
        match peer.until(|f| matches!(f, Frame::Data(..))).await {
            Frame::Data(1, body) => {
                assert!(body.len() <= 1 << 10, "a frame of {} bytes", body.len());
                got.extend(body);
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(got.len(), INITIAL_WINDOW as usize, "no byte past the first credit");
    // Credit for the stream alone: the link's 16 KiB are spent, so nothing
    // moves; then credit for the link.
    peer.send(Frame::Window(1, 24 << 10)).await;
    peer.send(Frame::Ping(5)).await;
    assert_eq!(peer.until(|f| matches!(f, Frame::Data(..) | Frame::Pong(_))).await, Frame::Pong(5));
    peer.send(Frame::Window(0, 24 << 10)).await;
    while got.len() < data.len() {
        if let Frame::Data(1, body) = peer.until(|f| matches!(f, Frame::Data(..))).await {
            got.extend(body);
        }
    }
    assert!(got == data);
    drop(timeout(limit(), writer).await.unwrap().unwrap());
}

/// A reader that empties its buffer when it gives credit back doubles the
/// window of its stream; the link's credit goes back with it.
#[tokio::test]
async fn a_reader_that_keeps_up_grows_its_window() {
    let (link, mut peer) = RawPeer::start(Role::Listener, Settings::default()).await;
    // The listener's link window goes first: 8 MiB less the first 16 KiB.
    assert_eq!(peer.next().await, Some(Frame::Window(0, (8 << 20) - INITIAL_WINDOW)));
    peer.send(Frame::Open(1)).await;
    peer.send(Frame::Data(1, vec![1; INITIAL_WINDOW as usize])).await;
    let mut stream = timeout(limit(), link.accept()).await.unwrap().unwrap();
    let mut buf = vec![0; 64 << 10];
    let mut n = 0;
    while n < INITIAL_WINDOW as usize {
        n += stream.read(&mut buf).await.unwrap();
    }
    let mut credits = Vec::new();
    while credits.len() < 2 {
        credits.push(peer.until(|f| matches!(f, Frame::Window(..))).await);
    }
    credits.sort_by_key(Frame::stream);
    assert_eq!(credits, [Frame::Window(0, INITIAL_WINDOW), Frame::Window(1, 2 * INITIAL_WINDOW)]);
    // The window is now 32 KiB: 32 KiB more come with no error.
    peer.send(Frame::Data(1, vec![2; 2 * INITIAL_WINDOW as usize])).await;
    let mut more = 0;
    while more < 2 * INITIAL_WINDOW as usize {
        more += timeout(limit(), stream.read(&mut buf)).await.unwrap().unwrap();
    }
    assert!(link.ended().is_none());
}
