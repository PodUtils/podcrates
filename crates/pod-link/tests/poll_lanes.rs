//! The lanes of the poll carriers (T-422 and T-495): a session answers its
//! own key alone; a session with no request ends on the listener, and a
//! lane that fails ends on the dialer, each with an error; the simplex poll
//! backs off while the link is quiet, and the long poll holds; a listener
//! with no push gives the long poll; a push moves more than the window of
//! the listener with no bytes up; the third 429 in a row ends a dial; the
//! sessions of a listener keep no more bytes down than its cap (T-517).

#![cfg(feature = "poll")]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::LIMIT;
use common::poll::{Relay, at, bound, carrier, identity, key, post, raw};
use common::tcp::Direct;
use pod_link::poll::{self, Form, PollListener};
use pod_link::{Carrier, CarrierErrorKind};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

#[tokio::test]
async fn a_session_answers_the_key_that_opened_it_alone() {
    pod_testkit::need_loopback!();
    let listening = carrier(1, Arc::new(Direct::default()), &[key(2), key(3)], Form::LongPoll);
    let mut door = bound(&listening).await;
    let (id, local) = ("0123456789abcdef0123456789abcdef", door.local_addr());
    let open = post(&format!("/l/open?s={id}"));
    let server = key(1);
    let (status, accepted) = tokio::join!(raw(&server, identity(2), local, &open), async {
        timeout(LIMIT, door.accept_key()).await.expect("the accept in time")
    });
    assert_eq!(status, 201);
    let (mut far, opener) = accepted.expect("the session");
    assert_eq!(opener, key(2));
    let up = format!("/l/up?s={id}&seq=0&ack=0");
    assert_eq!(raw(&key(1), identity(3), local, &open).await, 404, "an open of the id");
    assert_eq!(raw(&key(1), identity(3), local, &post(&up)).await, 404, "a POST of another pinned key");
    assert_eq!(raw(&key(1), identity(2), local, &open).await, 201, "an open again");
    let body = format!("POST {up} HTTP/1.1\r\nHost: t\r\nContent-Length: 2\r\n\r\nhi");
    assert_eq!(raw(&key(1), identity(2), local, &body).await, 202);
    assert_eq!(raw(&key(1), identity(2), local, &body).await, 202, "the same POST again");
    let gap = format!("POST /l/up?s={id}&seq=9&ack=0 HTTP/1.1\r\nHost: t\r\nContent-Length: 1\r\n\r\nx");
    assert_eq!(raw(&key(1), identity(2), local, &gap).await, 409, "a gap in the offsets");
    let mut got = Vec::new();
    let e = timeout(LIMIT, far.read_to_end(&mut got)).await.expect("the end in time").unwrap_err();
    assert_eq!((got.as_slice(), e.kind()), (&b"hi"[..], std::io::ErrorKind::InvalidData), "{e}");
    assert_eq!(door.stats().duplicate, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_session_with_no_request_ends_on_each_side_with_an_error() {
    pod_testkit::need_loopback!();
    let relay = Arc::new(Relay::default());
    let (server, client) = (identity(1), identity(2));
    let mut listening = carrier(1, Arc::new(Direct::default()), &[client.public()], Form::LongPoll);
    let mut dialing = carrier(2, relay.clone(), &[], Form::LongPoll);
    for p in [&mut listening, &mut dialing] {
        (p.hold, p.idle) = (Duration::from_secs(1), Duration::from_secs(2));
    }
    let mut door = bound(&listening).await;
    let address = at(&server.public(), &door);
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (near, far) = timeout(LIMIT, both).await.expect("the dial and the accept in time");
    let (mut near, (mut far, _)) = (near.expect("the dial"), far.expect("the accept"));
    near.write_all(b"ping").await.unwrap();
    let mut got = [0u8; 4];
    far.read_exact(&mut got).await.unwrap();
    relay.faults.kill();
    let started = Instant::now();
    let e = timeout(LIMIT, far.read(&mut got)).await.expect("the end on the listener in time").unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::TimedOut, "{e}");
    let e = timeout(LIMIT, near.read(&mut got)).await.expect("the end on the dialer in time").unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::TimedOut, "{e}");
    assert!(started.elapsed() >= Duration::from_secs(1), "the idle time passed first");
}

/// The requests of the bytes down of a quiet link in `form` in 3 s.
async fn quiet_polls(form: Form) -> u64 {
    let (server, client) = (identity(1), identity(2));
    let listening = carrier(1, Arc::new(Direct::default()), &[client.public()], form);
    let dialing = carrier(2, Arc::new(Direct::default()), &[], form);
    let mut door = bound(&listening).await;
    let address = at(&server.public(), &door);
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (near, far) = timeout(LIMIT, both).await.expect("the dial and the accept in time");
    let (_near, _far) = (near.unwrap(), far.unwrap());
    let before = door.stats();
    tokio::time::sleep(Duration::from_secs(3)).await;
    let after = door.stats();
    (after.shorts - before.shorts) + (after.polls - before.polls)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_simplex_poll_backs_off_while_the_link_is_quiet_and_the_long_poll_holds() {
    pod_testkit::need_loopback!();
    let shorts = quiet_polls(Form::Simplex).await;
    // 100 ms doubled up to 5 s: about 5 polls in 3 s; at a fixed 100 ms, 30.
    assert!((3..=10).contains(&shorts), "{shorts} polls in 3 s");
    let holds = quiet_polls(Form::LongPoll).await;
    assert!(holds <= 2, "{holds} long polls in 3 s");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_listener_with_no_push_gives_the_long_poll() {
    pod_testkit::need_loopback!();
    let (server, client) = (identity(1), identity(2));
    let mut listening = carrier(1, Arc::new(Direct::default()), &[client.public()], Form::Sse);
    listening.push = false;
    let dialing = carrier(2, Arc::new(Direct::default()), &[], Form::Sse);
    let mut door = bound(&listening).await;
    let address = at(&server.public(), &door);
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (near, far) = timeout(LIMIT, both).await.expect("the dial and the accept in time");
    let (mut near, (mut far, _)) = (near.unwrap(), far.unwrap());
    far.write_all(b"down").await.unwrap();
    let mut got = [0u8; 4];
    timeout(LIMIT, near.read_exact(&mut got)).await.expect("the bytes down in time").unwrap();
    assert_eq!(&got, b"down");
    let stats = door.stats();
    assert!(stats.no_push >= 1 && stats.polls >= 1 && stats.pushes == 0, "{stats:?}");
}

#[tokio::test]
async fn the_third_answer_429_in_a_row_ends_the_dial() {
    pod_testkit::need_loopback!();
    let (server, client) = (identity(1), identity(2));
    let mut listening = carrier(1, Arc::new(Direct::default()), &[client.public()], Form::Simplex);
    listening.max_sessions = 1;
    let dialing = carrier(2, Arc::new(Direct::default()), &[], Form::Simplex);
    let mut door = bound(&listening).await;
    let address = at(&server.public(), &door);
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (first, _accepted) = timeout(LIMIT, both).await.expect("the first dial in time");
    let _first = first.expect("the first session");
    let started = Instant::now();
    let e = timeout(LIMIT, dialing.dial(&address)).await.expect("the second dial in time").err().unwrap();
    assert_eq!(e.kind, CarrierErrorKind::Unavailable, "{e}");
    assert!(e.to_string().contains("429"), "{e}");
    assert!(started.elapsed() >= Duration::from_secs(2), "each 429 waited for its Retry-After");
    assert_eq!(door.stats().busy, 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_push_moves_more_than_the_window_with_no_bytes_up() {
    pod_testkit::need_loopback!();
    for form in [Form::Sse, Form::Chunked] {
        let (server, client) = (identity(1), identity(2));
        let listening = carrier(1, Arc::new(Direct::default()), &[client.public()], form);
        let dialing = carrier(2, Arc::new(Direct::default()), &[], form);
        let mut door = bound(&listening).await;
        let address = at(&server.public(), &door);
        let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
        let (near, far) = timeout(LIMIT, both).await.expect("the dial and the accept in time");
        let (mut near, (mut far, _)) = (near.unwrap(), far.unwrap());
        // The dialer acknowledges the bytes down in empty POSTs: the
        // listener keeps 1 MiB at most.
        let data = common::pattern(7, 3 * poll::WINDOW);
        let mut back = vec![0u8; data.len()];
        let (sent, read) = timeout(LIMIT, async { tokio::join!(far.write_all(&data), near.read_exact(&mut back)) })
            .await
            .unwrap_or_else(|_| panic!("{form:?}: 3 MiB down in time"));
        sent.unwrap();
        read.unwrap();
        assert!(back == data, "{form:?}: the bytes came whole");
        assert_eq!(door.stats().pushes, 1, "{form:?}: one push carried each byte");
    }
}

/// Wait until the sessions of `door` keep `bytes`; no count seen passes
/// `max`.
async fn kept(door: &PollListener, bytes: usize, max: usize) {
    let wait = async {
        loop {
            let held = door.stats().held as usize;
            assert!(held <= max, "{held} bytes kept, past the cap of {max}");
            if held == bytes {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    };
    let late = |_| panic!("{bytes} bytes kept in time: {:?}", door.stats());
    timeout(LIMIT, wait).await.unwrap_or_else(late);
}

/// The status of `request` of the client 2 to the listener 1 at `local`.
async fn ask(local: std::net::SocketAddr, request: String) -> u16 {
    raw(&key(1), identity(2), local, &request).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_sessions_keep_no_more_bytes_than_the_cap_and_an_open_with_no_room_gets_429() {
    pod_testkit::need_loopback!();
    let mut listening = carrier(1, Arc::new(Direct::default()), &[key(2)], Form::Simplex);
    let max = poll::WINDOW + poll::WINDOW / 2;
    listening.max_bytes = max;
    let mut door = bound(&listening).await;
    let local = door.local_addr();
    let ids =
        ["0123456789abcdef0123456789abcde1", "0123456789abcdef0123456789abcde2", "0123456789abcdef0123456789abcde3"];
    let open = |n: usize| post(&format!("/l/open?s={}", ids[n]));
    let mut links = Vec::new();
    for (n, after) in [(0, poll::WINDOW), (1, max)] {
        let (status, accepted) = tokio::join!(ask(local, open(n)), timeout(LIMIT, door.accept_key()));
        assert_eq!(status, 201, "the open of session {n}");
        let (mut far, _) = accepted.expect("the accept in time").expect("the session");
        // No dialer reads: the bytes stay on the listener.
        links.push(tokio::spawn(async move {
            far.write_all(&common::pattern(n as u32, poll::WINDOW)).await.unwrap();
            far
        }));
        kept(&door, after, max).await;
    }
    assert_eq!(ask(local, open(2)).await, 429, "the third open finds no room");
    assert_eq!(door.stats().busy, 1);
    // The acknowledgement of the first window gives its bytes back: the
    // second session takes the rest of its window, and the third open finds
    // room.
    let ack = format!("GET /l/out?s={}&since={} HTTP/1.1\r\nHost: test\r\n\r\n", ids[0], poll::WINDOW);
    assert_eq!(ask(local, ack).await, 204);
    kept(&door, poll::WINDOW, max).await;
    let (status, accepted) = tokio::join!(ask(local, open(2)), timeout(LIMIT, door.accept_key()));
    assert_eq!(status, 201, "the third open after the acknowledgement");
    accepted.expect("the accept in time").expect("the third session");
    for link in links {
        let _far = timeout(LIMIT, link).await.expect("each window went in").unwrap();
    }
}
