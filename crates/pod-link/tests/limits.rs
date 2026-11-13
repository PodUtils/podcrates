//! The limits of a link: the count of streams at each end, the settings,
//! the size of a frame, and a peer that breaks the protocol (it gets a
//! `GOAWAY` with the code `PROTOCOL`).

mod common;

use common::{RawPeer, limit};
use pod_link::frame::{PREFACE, kind};
use pod_link::{
    Error, Frame, FrameError, GoAwayCode, INITIAL_WINDOW, Link, MAX_RECORD, ResetCode, Role, Settings, SettingsError,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

#[tokio::test]
async fn an_open_over_the_own_limit_fails() {
    let two = Settings { max_streams: 2, ..Settings::default() };
    let (dialer, _listener) = common::over_memory(two, Settings::default()).await;
    let a = dialer.open().unwrap();
    let _b = dialer.open().unwrap();
    assert_eq!(dialer.open().unwrap_err(), Error::TooManyStreams(2));
    assert_eq!(pod_core::Failure::from(Error::TooManyStreams(2)).class, pod_core::Class::TempFail);
    drop(a);
    assert_eq!(dialer.open().unwrap().id(), 5, "a freed slot takes a new id");
}

#[tokio::test]
async fn the_peer_refuses_an_open_over_its_limit() {
    let three = Settings { max_streams: 3, ..Settings::default() };
    let (dialer, listener) = common::over_memory(Settings::default(), three).await;
    let mut streams: Vec<_> = (0..5).map(|_| dialer.open().unwrap()).collect();
    for s in &mut streams {
        s.write_all(b"hi").await.unwrap();
    }
    for _ in 0..3 {
        timeout(limit(), listener.accept()).await.unwrap().unwrap();
    }
    for s in &mut streams[3..] {
        let id = s.id();
        let e = timeout(limit(), s.read(&mut [0; 4])).await.unwrap().unwrap_err();
        let e = *e.into_inner().unwrap().downcast::<Error>().unwrap();
        assert_eq!(e, Error::Reset { id, code: ResetCode::REFUSED, by_peer: true });
        assert_eq!(e.class(), pod_core::Class::TempFail);
    }
    assert_eq!(listener.streams(), 0, "the accepted streams were dropped");
    assert!(listener.ended().is_none() && dialer.ended().is_none());
}

#[test]
fn each_setting_out_of_its_limits_is_refused() {
    let d = Settings::default();
    assert_eq!(d.check(), Ok(()));
    let bad = [
        (Settings { stream_window: INITIAL_WINDOW - 1, ..d }, SettingsError::StreamWindow(INITIAL_WINDOW - 1)),
        (Settings { stream_window: (8 << 20) + 1, ..d }, SettingsError::StreamWindow((8 << 20) + 1)),
        (Settings { link_window: (8 << 20) + 1, ..d }, SettingsError::LinkWindow((8 << 20) + 1)),
        (Settings { link_window: 32 << 10, stream_window: 64 << 10, ..d }, SettingsError::LinkWindow(32 << 10)),
        (Settings { max_streams: 0, ..d }, SettingsError::MaxStreams(0)),
        (Settings { max_streams: 4097, ..d }, SettingsError::MaxStreams(4097)),
        (Settings { frame_size: 1023, ..d }, SettingsError::FrameSize(1023)),
        (Settings { frame_size: MAX_RECORD + 1, ..d }, SettingsError::FrameSize(MAX_RECORD + 1)),
        (
            Settings { dead_after: d.keepalive, ..d },
            SettingsError::Keepalive { keepalive: d.keepalive, dead_after: d.keepalive },
        ),
    ];
    for (settings, want) in bad {
        assert_eq!(settings.check(), Err(want));
    }
    let edges =
        Settings { stream_window: 8 << 20, link_window: 8 << 20, max_streams: 4096, frame_size: MAX_RECORD, ..d };
    assert_eq!(edges.check(), Ok(()));
}

#[tokio::test]
async fn a_link_with_bad_settings_does_not_start() {
    let (near, _far) = pod_link::memory::pair(1 << 16);
    let e = Link::new(near, Role::Dialer, Settings { max_streams: 0, ..Settings::default() }).unwrap_err();
    assert_eq!(e, Error::Settings(SettingsError::MaxStreams(0)));
    assert_eq!(e.class(), pod_core::Class::Usage);
}

/// The link ends with `want`, and the raw peer reads a `GOAWAY` with the
/// code `PROTOCOL`.
async fn ends_with_protocol_error(link: Link, mut peer: RawPeer, want: impl Fn(&Error) -> bool) {
    let e = timeout(limit(), link.closed()).await.unwrap();
    assert!(want(&e), "{e:?}");
    assert_eq!(e.class(), pod_core::Class::Protocol);
    assert_eq!(peer.until(|f| matches!(f, Frame::GoAway(_))).await, Frame::GoAway(GoAwayCode::PROTOCOL.0));
    assert_eq!(peer.next().await, None, "the carrier ends after the GOAWAY");
}

#[tokio::test]
async fn bytes_past_the_window_of_a_stream_end_the_link() {
    let (link, mut peer) = RawPeer::start(Role::Listener, Settings::default()).await;
    peer.send(Frame::Open(1)).await;
    peer.send(Frame::Data(1, vec![0; INITIAL_WINDOW as usize])).await;
    peer.send(Frame::Data(1, vec![0; 1])).await;
    ends_with_protocol_error(link, peer, |e| matches!(e, Error::Protocol(t) if t.contains("past its window"))).await;
}

#[tokio::test]
async fn bytes_past_the_window_of_the_link_end_the_link() {
    let small = Settings { stream_window: 16 << 10, link_window: 16 << 10, ..Settings::default() };
    let (link, mut peer) = RawPeer::start(Role::Listener, small).await;
    peer.send(Frame::Open(1)).await;
    peer.send(Frame::Open(3)).await;
    peer.send(Frame::Data(1, vec![0; 10 << 10])).await;
    peer.send(Frame::Data(3, vec![0; 10 << 10])).await;
    ends_with_protocol_error(link, peer, |e| matches!(e, Error::Protocol(t) if t.contains("window of the link"))).await;
}

#[tokio::test]
async fn a_frame_over_one_mebibyte_ends_the_link() {
    let (link, mut peer) = RawPeer::start(Role::Listener, Settings::default()).await;
    peer.send(Frame::Open(1)).await;
    let len = (MAX_RECORD + 1) as u32;
    peer.io.write_all(&[kind::DATA, 0, 0, 0, 1, (len >> 16) as u8, (len >> 8) as u8, len as u8]).await.unwrap();
    ends_with_protocol_error(link, peer, |e| *e == Error::Frame(FrameError::TooLong(MAX_RECORD + 1))).await;
}

#[tokio::test]
async fn an_open_of_the_wrong_parity_ends_the_link() {
    let (link, mut peer) = RawPeer::start(Role::Listener, Settings::default()).await;
    peer.send(Frame::Open(2)).await;
    ends_with_protocol_error(link, peer, |e| matches!(e, Error::Protocol(t) if t.contains("parity"))).await;
}

#[tokio::test]
async fn an_id_used_again_ends_the_link() {
    let (link, mut peer) = RawPeer::start(Role::Listener, Settings::default()).await;
    peer.send(Frame::Open(5)).await;
    peer.send(Frame::Open(3)).await;
    ends_with_protocol_error(link, peer, |e| matches!(e, Error::Protocol(t) if t.contains("used before"))).await;
}

#[tokio::test]
async fn data_on_a_stream_never_opened_ends_the_link() {
    let (link, mut peer) = RawPeer::start(Role::Dialer, Settings::default()).await;
    peer.send(Frame::Data(1, b"x".to_vec())).await;
    ends_with_protocol_error(link, peer, |e| matches!(e, Error::Protocol(t) if t.contains("never opened"))).await;
}

#[tokio::test]
async fn frames_of_a_stream_that_is_gone_are_dropped() {
    let (link, mut peer) = RawPeer::start(Role::Listener, Settings::default()).await;
    peer.send(Frame::Open(1)).await;
    let stream = timeout(limit(), link.accept()).await.unwrap().unwrap();
    drop(stream);
    assert_eq!(peer.until(|f| matches!(f, Frame::Reset(..))).await, Frame::Reset(1, ResetCode::CANCEL.0));
    // Bytes, credit and a close that were on the way: dropped, and the
    // link's credit goes back.
    peer.send(Frame::Data(1, vec![0; INITIAL_WINDOW as usize])).await;
    peer.send(Frame::Window(1, 100)).await;
    peer.send(Frame::Close(1)).await;
    assert_eq!(peer.until(|f| matches!(f, Frame::Window(0, _))).await, Frame::Window(0, INITIAL_WINDOW));
    assert!(link.ended().is_none());
}

#[tokio::test]
async fn a_peer_of_another_protocol_ends_the_link() {
    let (near, mut far) = pod_link::memory::pair(1 << 16);
    let link = Link::new(near, Role::Listener, Settings::default()).unwrap();
    far.write_all(b"GET / HTTP/1.1\r\n\r\n").await.unwrap();
    assert_eq!(timeout(limit(), link.closed()).await.unwrap(), Error::Frame(FrameError::Preface));
    let mut first = [0; 8];
    far.read_exact(&mut first).await.unwrap();
    assert_eq!(&first, PREFACE);
}

/// A peer that sends `PING` and reads no `PONG` ends the link before the
/// answers fill the memory.
#[tokio::test]
async fn a_peer_that_does_not_read_its_answers_ends_the_link() {
    let (near, mut far) = pod_link::memory::pair(64);
    let link = Link::new(near, Role::Listener, Settings::default()).unwrap();
    let mut flood = PREFACE.to_vec();
    for i in 0..10_000u64 {
        Frame::Ping(i).encode(&mut flood).unwrap();
    }
    let writer = tokio::spawn(async move {
        let _ = far.write_all(&flood).await;
        far
    });
    let e = timeout(limit(), link.closed()).await.unwrap();
    assert!(matches!(&e, Error::Protocol(t) if t.contains("does not read")), "{e:?}");
    drop(writer.await.unwrap());
}
