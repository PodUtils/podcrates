//! The relay's Close behind a ping that failed (PodUtils/podssh#360, ask 4):
//! the node reads ahead for it, bounded by time and by bytes, not by a
//! count of frames. A stand-in relay writes the frames on a pipe.
#![cfg(feature = "pair")]

use std::time::Duration;

use pod_relay::reverse::closes::{LOOK_BYTES, LOOK_WAIT, pending};
use pod_ws::RelaySession;
use pod_ws::frame::{self, Frame, Role};
use tokio::io::{AsyncWriteExt, DuplexStream};

/// The size of a frame of another session's data.
const CHUNK: usize = 64 * 1024;

fn node(stream: DuplexStream) -> RelaySession<DuplexStream> {
    RelaySession::new(stream, Vec::new(), None, Duration::from_secs(5))
}

fn data() -> Vec<u8> {
    frame::encode(&Frame { fin: true, opcode: frame::OPCODE_BINARY, payload: vec![7; CHUNK] }, Role::Server, [0; 4])
}

fn close(code: u16, reason: &str) -> Vec<u8> {
    let payload = pod_ws::close::close_payload(Some(code), reason);
    frame::encode(&Frame { fin: true, opcode: frame::OPCODE_CLOSE, payload }, Role::Server, [0; 4])
}

/// More than 8 frames of 64 KiB before the Close: the node reads the Close
/// and its code, not "the link ended with no Close".
#[tokio::test]
async fn a_close_behind_more_than_eight_frames_of_64_kib_is_read() {
    let (ours, mut relay) = tokio::io::duplex(1024 * 1024);
    let writer = tokio::spawn(async move {
        for _ in 0..12 {
            relay.write_all(&data()).await.unwrap();
        }
        relay.write_all(&close(1003, "data before ready")).await.unwrap();
        relay
    });
    let session = node(ours);
    let close = tokio::time::timeout(Duration::from_secs(10), pending(&session)).await.unwrap();
    let close = close.expect("the relay's Close behind 12 frames of 64 KiB");
    assert_eq!((close.code, close.reason.as_str(), close.clean), (1003, "data before ready", true));
    drop(writer.await.unwrap());
}

/// A Close that comes half a second after the ping failed (a slow host) is
/// still read.
#[tokio::test]
async fn a_close_that_comes_late_within_the_wait_is_read() {
    let (ours, mut relay) = tokio::io::duplex(64 * 1024);
    let writer = tokio::spawn(async move {
        tokio::time::sleep(LOOK_WAIT / 2).await;
        relay.write_all(&close(1003, "data before ready")).await.unwrap();
        relay
    });
    let close = pending(&node(ours)).await.expect("the Close within the wait");
    assert_eq!(close.code, 1003);
    drop(writer.await.unwrap());
}

/// A relay that sends data with no end: the read stops after the bound of
/// bytes, with no Close, and does not read on for the whole wait.
#[tokio::test]
async fn a_relay_that_sends_data_with_no_close_ends_the_read_at_the_bound_of_bytes() {
    let (ours, mut relay) = tokio::io::duplex(1024 * 1024);
    let writer = tokio::spawn(async move {
        let frame = data();
        while relay.write_all(&frame).await.is_ok() {}
    });
    let session = node(ours);
    assert!(pending(&session).await.is_none());
    let most = (LOOK_BYTES / CHUNK + 1) as u64;
    assert!(session.frames_received() <= most, "{} frames read, {most} at most", session.frames_received());
    drop(session);
    writer.await.unwrap();
}

/// A read that fails ends the look at once, with no Close.
#[tokio::test]
async fn a_read_that_fails_ends_the_look_with_no_close() {
    let (ours, relay) = tokio::io::duplex(1024);
    drop(relay);
    let began = tokio::time::Instant::now();
    assert!(pending(&node(ours)).await.is_none());
    assert!(began.elapsed() < LOOK_WAIT, "{:?}", began.elapsed());
}
