//! The reconnect against the stand-in: a relay that goes away ends each
//! open stream; the next open connects again with a fresh connect token;
//! the tries are bounded; a refused user token is not tried again.

mod common;

use std::io::ErrorKind;
use std::time::Duration;

use pod_core::Class;
use pod_devtunnel::stream::SESSION_ENDED;
use pod_devtunnel::{Locator, Relay, Step, steps_of};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use common::{connected, plan, report, settings, user, way};

/// The read of a stream within a bound: a test waits for the event.
async fn read_some(stream: &mut pod_devtunnel::Stream) -> std::io::Result<usize> {
    tokio::time::timeout(Duration::from_secs(10), stream.read(&mut [0u8; 64])).await.expect("an end of the read")
}

#[tokio::test]
async fn a_reconnect_ends_each_open_stream_and_a_new_stream_works_with_a_fresh_token() {
    pod_testkit::need_loopback!();
    let (standin, relay, kept) = connected(plan()).await;
    let mut first = relay.open(22).await.unwrap();
    let mut second = relay.open(22).await.unwrap();
    first.write_all(b"a").await.unwrap();
    assert_eq!(read_some(&mut first).await.unwrap(), 1);

    standin.drop_relays();
    for stream in [&mut first, &mut second] {
        let e = read_some(stream).await.unwrap_err();
        assert_eq!(e.kind(), ErrorKind::ConnectionAborted);
        assert_eq!(e.to_string(), SESSION_ENDED);
        // The end stays.
        assert_eq!(read_some(stream).await.unwrap_err().kind(), ErrorKind::ConnectionAborted);
    }

    let mut third = relay.open(22).await.unwrap_or_else(|e| panic!("open after the drop: {}", e.chain()));
    third.write_all(b"again").await.unwrap();
    let mut back = [0u8; 5];
    third.read_exact(&mut back).await.unwrap();
    assert_eq!(&back, b"again");
    assert_eq!((relay.sessions(), third.session(), standin.minted()), (2, 2, 2));
    // Each upgrade took the connect token of its own get.
    assert_eq!(standin.seen().relay_tokens, ["fresh", "fresh"]);
    assert_eq!(kept.kinds(), ["devtunnel.connected", "devtunnel.reconnect", "devtunnel.connected"]);
    assert_eq!(*kept.decisions.lock().unwrap(), [("devtunnel.reconnect".to_string(), "connected".to_string())]);
}

#[tokio::test]
async fn a_reconnect_that_the_caller_asks_for_ends_the_streams_of_a_live_session() {
    pod_testkit::need_loopback!();
    let (standin, relay, _) = connected(plan()).await;
    let mut old = relay.open(22).await.unwrap();
    relay.reconnect().await.unwrap();
    assert_eq!(read_some(&mut old).await.unwrap_err().kind(), ErrorKind::ConnectionAborted);
    assert!(old.write_all(b"late").await.is_err());
    assert_eq!((relay.sessions(), standin.minted()), (2, 2));
    let mut new = relay.open(22).await.unwrap();
    new.write_all(b"z").await.unwrap();
    assert_eq!(read_some(&mut new).await.unwrap(), 1);
}

#[tokio::test]
async fn the_tries_of_a_reconnect_are_bounded() {
    pod_testkit::need_loopback!();
    let (standin, relay, kept) = connected(plan()).await;
    standin.refuse_upgrades(100);
    standin.drop_relays();
    let failure = relay.open(22).await.unwrap_err();
    assert_eq!(steps_of(&failure), [Step::Reconnect, Step::WebSocket], "{}", failure.chain());
    assert_eq!(failure.class, Class::TempFail);
    // The first session's token, then one fresh token for each try.
    assert_eq!(standin.minted(), 1 + 3);
    assert_eq!(standin.seen().relay_tokens.len(), 1 + 3);
    assert!(kept.decisions.lock().unwrap().contains(&("devtunnel.reconnect".into(), "failed".into())));
    // The relay is back: the next open connects.
    standin.refuse_upgrades(0);
    assert!(relay.open(22).await.is_ok());
}

#[tokio::test]
async fn a_refused_user_token_ends_the_reconnect_at_once() {
    pod_testkit::need_loopback!();
    let (standin, relay, _) = connected(plan()).await;
    standin.refuse_user();
    standin.drop_relays();
    let failure = relay.open(22).await.unwrap_err();
    assert_eq!(steps_of(&failure), [Step::Reconnect, Step::Management], "{}", failure.chain());
    assert_eq!(failure.class, Class::Refused);
    // One get of the reconnect, refused, and no other try.
    assert_eq!(standin.seen().user_ok, [true, false]);
    assert_eq!(standin.minted(), 1);
}

#[tokio::test]
async fn a_first_connect_with_a_refused_user_token_names_the_management_api() {
    pod_testkit::need_loopback!();
    let mut other = plan();
    other.authorization = "github another-made-up-token".into();
    let standin = pod_devtunnel::standin::Standin::start(other).await.unwrap();
    let locator = Locator::parse("standin-box").unwrap();
    let failure = Relay::connect(way(), settings(&standin.base()), locator, user(), report().0).await.unwrap_err();
    assert_eq!((steps_of(&failure), failure.class), (vec![Step::Management], Class::Refused));
    assert!(failure.chain().contains("HTTP 401: Unauthorized"), "{}", failure.chain());
    assert_eq!(standin.seen().gets.len(), 1);
}
