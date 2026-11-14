//! The poll carriers (T-422 and T-495, PodUtils/podnet#515 and #521): a
//! link over requests of HTTP/1.1 with TLS carries streams in each form of
//! the bytes down, through a way out that loses the answer of a POST and
//! cuts an answer down; a proxy that cuts idle connections holds no link
//! back; the accept gives the key of the client; another key is refused on
//! each side and named; a silent peer holds no other one; a silent listener
//! ends the dial at its time limit; an address out of its form is a usage
//! error.

#![cfg(feature = "poll")]

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::poll::{Relay, at, bound, carrier};
use common::tcp::{Direct, Fails, id, ring};
use common::{echo, limit, pattern};
use pod_core::Class;
use pod_identity::KeyName;
use pod_link::poll::{self, Form, Poll};
use pod_link::tcp::{Refusal, refusal};
use pod_link::{Carrier, CarrierErrorKind, Cost, Facts, Link, Listener, Role, Settings};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

/// A link in `form` through a relay that loses the answer of one POST and
/// cuts one answer down: 3 streams of `size` bytes each way come back
/// whole, and the listener dropped the bytes up that came again and sent
/// the bytes down again.
async fn carries(form: Form, size: usize) {
    let relay = Arc::new(Relay::lossy());
    let (server, client) = (id(1), id(2));
    let listening = carrier(1, Arc::new(Direct::default()), &[client.public()], form);
    let dialing = carrier(2, relay.clone(), &[], form);
    let mut door = bound(&listening).await;
    assert_eq!(door.address(), door.local_addr().to_string());
    let address = at(&server.public(), &door);
    let both = async { tokio::join!(dialing.dial(&address), door.accept_peer()) };
    let (near, far) = timeout(limit(), both).await.expect("the dial and the accept in time");
    let (near, far) = (near.expect("the dial"), far.expect("the accept"));
    assert_eq!(far.peer, Some(client.public()), "the accept gives the key of the client");
    assert!(relay.dials.load(Ordering::SeqCst) >= 1, "the dial goes through the way out of the caller");

    let dialer = Link::new(near, Role::Dialer, Settings::default()).unwrap();
    let _echo = echo(Link::new(far.io, Role::Listener, Settings::default()).unwrap());
    let mut moves = Vec::new();
    for seed in 0..3u32 {
        let mut stream = dialer.open().unwrap();
        moves.push(tokio::spawn(async move {
            let data = pattern(seed, size);
            let (mut reader, mut writer) = tokio::io::split(&mut stream);
            let send = async {
                writer.write_all(&data).await.unwrap();
                writer.shutdown().await.unwrap();
            };
            let mut back = Vec::new();
            let (_, read) = tokio::join!(send, reader.read_to_end(&mut back));
            read.unwrap();
            assert!(back == data, "stream {seed}: the bytes came back whole");
        }));
    }
    for each in moves {
        timeout(limit(), each).await.expect("the echo in time").unwrap();
    }
    dialer.close().await;
    let stats = door.stats();
    assert_eq!(relay.faults.cuts.load(Ordering::SeqCst), 2, "the relay lost an answer up and cut one down");
    assert!(stats.duplicate > 0, "the POST again after a lost answer gave no byte twice: {stats:?}");
    assert!(stats.resent > 0, "the request again after a cut gave the bytes down again: {stats:?}");
    assert_eq!(stats.sessions, 1, "{stats:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_link_over_the_long_poll_carries_streams_across_a_lost_answer() {
    pod_testkit::need_loopback!();
    carries(Form::LongPoll, 256 << 10).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_link_over_sse_carries_streams_across_a_lost_answer() {
    pod_testkit::need_loopback!();
    carries(Form::Sse, 512 << 10).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_link_over_a_chunked_stream_carries_streams_across_a_lost_answer() {
    pod_testkit::need_loopback!();
    carries(Form::Chunked, 512 << 10).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_link_over_the_simplex_poll_carries_streams_across_a_lost_answer() {
    pod_testkit::need_loopback!();
    carries(Form::Simplex, 256 << 10).await;
}

/// Lines each answered over the raw byte streams of `form`, through a
/// proxy that cuts each connection silent for 300 ms, with a pause of 1 s;
/// then the end of each side reaches the other.
async fn across_cuts(form: Form) {
    let relay = Arc::new(Relay::cutting(Duration::from_millis(300)));
    let (server, client) = (id(1), id(2));
    let listening = carrier(1, Arc::new(Direct::default()), &[client.public()], form);
    let mut dialing = carrier(2, relay.clone(), &[], form);
    dialing.hold = Duration::from_secs(2);
    let mut door = bound(&listening).await;
    let address = at(&server.public(), &door);
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (near, far) = timeout(limit(), both).await.expect("the dial and the accept in time");
    let (mut near, (mut far, _)) = (near.expect("the dial"), far.expect("the accept"));
    for n in 0..20u32 {
        if n == 10 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        let line = format!("line {n}\n");
        near.write_all(line.as_bytes()).await.unwrap();
        let mut got = vec![0u8; line.len()];
        timeout(limit(), far.read_exact(&mut got)).await.expect("the line up in time").unwrap();
        far.write_all(&got).await.unwrap();
        let mut back = vec![0u8; line.len()];
        timeout(limit(), near.read_exact(&mut back)).await.expect("the line down in time").unwrap();
        assert_eq!(back, line.as_bytes());
    }
    assert!(relay.faults.cuts.load(Ordering::SeqCst) >= 1, "the proxy cut an idle connection");
    near.shutdown().await.unwrap();
    let mut rest = Vec::new();
    timeout(limit(), far.read_to_end(&mut rest)).await.expect("the end up in time").unwrap();
    assert!(rest.is_empty(), "the shutdown of the dialer is the end of its bytes");
    far.shutdown().await.unwrap();
    timeout(limit(), near.read_to_end(&mut rest)).await.expect("the end down in time").unwrap();
    assert!(rest.is_empty(), "the shutdown of the listener is the end of its bytes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_proxy_that_cuts_idle_connections_holds_no_long_poll_back() {
    pod_testkit::need_loopback!();
    across_cuts(Form::LongPoll).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_proxy_that_cuts_idle_connections_holds_no_push_back() {
    pod_testkit::need_loopback!();
    across_cuts(Form::Sse).await;
    across_cuts(Form::Chunked).await;
    across_cuts(Form::Simplex).await;
}

#[tokio::test]
async fn the_dialer_refuses_a_listener_of_another_key_and_names_it() {
    pod_testkit::need_loopback!();
    let (server, client, stranger) = (id(1), id(2), id(3));
    let listening = carrier(1, Arc::new(Direct::default()), &[client.public()], Form::LongPoll);
    let dialing = carrier(2, Arc::new(Direct::default()), &[], Form::LongPoll);
    let mut door = bound(&listening).await;
    let address = at(&stranger.public(), &door);
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (e, far) = timeout(limit(), both).await.expect("the refusal in time");
    let e = e.err().expect("a dial took a listener of another key");
    assert_eq!(e.kind, CarrierErrorKind::Refused, "{e}");
    assert_eq!(e.class().exit_code(), 77);
    assert_eq!(refusal(&e), Some(Refusal::Unpinned(server.public())), "{e}");
    assert!(e.to_string().contains(&server.public().fingerprint()), "the error names the key seen: {e}");
    let far = far.err().expect("the listener took a dialer that refused its key");
    assert_eq!(far.kind, CarrierErrorKind::KeyRefused, "the listener learns that its key was refused: {far}");
    assert_eq!(far.class().exit_code(), 77);
}

#[tokio::test]
async fn the_listener_refuses_a_client_of_another_key_and_stays_open() {
    pod_testkit::need_loopback!();
    let (server, client, stranger) = (id(1), id(2), id(4));
    let listening = carrier(1, Arc::new(Direct::default()), &[client.public()], Form::Simplex);
    let mut door = bound(&listening).await;
    let address = at(&server.public(), &door);
    let other = carrier(4, Arc::new(Direct::default()), &[], Form::Simplex);
    let both = async { tokio::join!(other.dial(&address), door.accept_key()) };
    let (near, far) = timeout(limit(), both).await.expect("the refusal in time");
    let near = near.err().expect("a client of another key dialed");
    assert_eq!(near.kind, CarrierErrorKind::KeyRefused, "the dialer learns of the refusal at the dial: {near}");
    assert_eq!(near.class().exit_code(), 77);
    assert!(near.to_string().contains(&stranger.public().fingerprint()), "{near}");
    let far = far.err().expect("the listener took a client of another key");
    assert_eq!(far.kind, CarrierErrorKind::Refused, "{far}");
    assert_eq!(refusal(&far), Some(Refusal::Unpinned(stranger.public())), "{far}");

    let good = carrier(2, Arc::new(Direct::default()), &[], Form::Simplex);
    let both = async { tokio::join!(good.dial(&address), door.accept_key()) };
    let (near, far) = timeout(limit(), both).await.expect("the dial and the accept in time");
    near.expect("the pinned client after a refusal");
    assert_eq!(far.expect("the listener stays open").1, client.public());
}

#[tokio::test]
async fn a_peer_that_sends_nothing_holds_no_other_handshake() {
    pod_testkit::need_loopback!();
    let (server, client) = (id(1), id(2));
    let mut listening = carrier(1, Arc::new(Direct::default()), &[client.public()], Form::LongPoll);
    listening.handshake_timeout = Duration::from_secs(300);
    let dialing = carrier(2, Arc::new(Direct::default()), &[], Form::LongPoll);
    let mut door = bound(&listening).await;
    let _silent = tokio::net::TcpStream::connect(door.local_addr()).await.unwrap();
    let address = at(&server.public(), &door);
    let both = async { tokio::join!(dialing.dial(&address), door.accept_key()) };
    let (near, far) = timeout(Duration::from_secs(20), both).await.expect("the good client is not held");
    near.unwrap();
    assert_eq!(far.unwrap().1, client.public());

    let mut quick = carrier(1, Arc::new(Direct::default()), &[client.public()], Form::LongPoll);
    quick.handshake_timeout = Duration::from_millis(300);
    let mut door = bound(&quick).await;
    let _silent = tokio::net::TcpStream::connect(door.local_addr()).await.unwrap();
    let e = timeout(limit(), door.accept_key()).await.expect("the limit of the handshake").err().unwrap();
    assert_eq!(e.kind, CarrierErrorKind::Io(std::io::ErrorKind::TimedOut), "{e}");
    assert_eq!(e.class(), Class::Timeout);
}

#[tokio::test]
async fn a_silent_listener_ends_the_dial_at_its_time_limit() {
    pod_testkit::need_loopback!();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local = listener.local_addr().unwrap();
    let _hold = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((tcp, _)) = listener.accept().await {
            held.push(tcp);
        }
    });
    let mut dialing = carrier(2, Arc::new(Direct::default()), &[], Form::Sse);
    dialing.handshake_timeout = Duration::from_millis(300);
    let address = format!("{}@https://{local}/link", id(1).public().hex());
    let e = timeout(limit(), dialing.dial(&address)).await.expect("the time limit of the dial").err().unwrap();
    assert_eq!(e.kind, CarrierErrorKind::Io(std::io::ErrorKind::TimedOut), "{e}");
    assert_eq!(e.class(), Class::Timeout);
}

#[tokio::test]
async fn an_address_or_a_setting_out_of_its_form_is_a_usage_error() {
    let carrier_ = carrier(1, Arc::new(Direct::default()), &[], Form::LongPoll);
    let key = id(2).public().hex();
    for bad in [
        "https://127.0.0.1:7000/link".to_string(),
        "nokey@https://127.0.0.1:7000/".into(),
        format!("{key}@http://127.0.0.1:7000/link"),
        format!("{key}@wss://127.0.0.1:7000/link"),
        format!("{key}@127.0.0.1:7000"),
        format!("{key}@https://127.0.0.1:7000/link?x=1"),
    ] {
        let e = carrier_.dial(&bad).await.err().unwrap_or_else(|| panic!("{bad} dialed"));
        assert_eq!(e.kind, CarrierErrorKind::Usage, "{bad}: {e}");
        assert_eq!(e.class().exit_code(), 64, "{bad}");
    }
    for bad in ["nowhere", "localhost:7000", "127.0.0.1"] {
        assert_eq!(carrier_.listen(bad).await.err().unwrap().kind, CarrierErrorKind::Usage, "{bad}");
    }
    let (named, url) = carrier_.address(&format!("{}@HTTPS://[::1]:8443/l", id(2).public().fingerprint())).unwrap();
    assert!(matches!(named, KeyName::Fingerprint(_)) && named.matches(&id(2).public()));
    assert_eq!((url.host.as_str(), url.port, url.path.as_str()), ("::1", 8443, "/l"));
    let mut each = Vec::new();
    for change in 0..7 {
        let mut p = carrier_.clone();
        match change {
            0 => p.hold = Duration::from_millis(500),
            1 => p.hold = Duration::from_secs(61),
            2 => p.idle = p.hold,
            3 => p.answer_timeout = Duration::ZERO,
            4 => p.max_handshakes = 0,
            5 => p.max_sessions = 0,
            _ => p.max_bytes = poll::MIN_BYTES - 1,
        }
        each.push(p.check().err().map(|e| e.kind));
        assert_eq!(p.listen("127.0.0.1:0").await.err().unwrap().kind, CarrierErrorKind::Usage);
    }
    assert!(each.iter().all(|k| *k == Some(CarrierErrorKind::Usage)), "{each:?}");
    assert!(carrier_.check().is_ok());
}

#[tokio::test]
async fn each_form_has_its_name_and_its_cost_and_a_failure_of_the_way_out_keeps_its_class() {
    let p = Poll::new(ring(), id(1), Arc::new(Direct::default()));
    assert_eq!((p.name(), p.form, p.facts()), ("poll", Form::LongPoll, Facts { cost: Cost::Medium, ..Facts::STREAM }));
    let names: Vec<_> = [Form::LongPoll, Form::Sse, Form::Chunked, Form::Simplex]
        .into_iter()
        .map(|f| {
            let p = p.clone().with_form(f);
            (p.name(), p.facts().cost, f.batch())
        })
        .collect();
    assert_eq!(
        names,
        [
            ("poll", Cost::Medium, poll::BATCH),
            ("sse", Cost::High, poll::BATCH),
            ("chunked", Cost::Medium, poll::BATCH),
            ("httppoll", Cost::Highest, poll::SIMPLEX_BATCH),
        ]
    );
    assert_eq!((p.hold, p.idle, p.max_sessions, p.push), (poll::HOLD, poll::IDLE, poll::MAX_SESSIONS, true));
    assert_eq!((p.max_bytes, poll::MIN_BYTES), (poll::MAX_BYTES, poll::BATCH));
    if pod_testkit::host::loopback().is_ok() {
        assert!(p.available().is_ok(), "{:?}", p.available());
    }
    let key = id(1).public().hex();
    for (class, kind) in [
        (Class::Refused, CarrierErrorKind::Refused),
        (Class::NotFound, CarrierErrorKind::NotFound),
        (Class::Unavailable, CarrierErrorKind::Unavailable),
    ] {
        let p = carrier(2, Arc::new(Fails(class)), &[], Form::Simplex);
        let e = p.dial(&format!("{key}@https://192.0.2.1:7000/")).await.err().unwrap();
        assert_eq!(e.kind, kind, "{class:?}: {e}");
        assert!(e.to_string().contains("test-fails") && e.to_string().contains("httppoll"), "{e}");
    }
    let shown = format!("{p:?}");
    assert!(shown.contains(&id(1).public().fingerprint()) && shown.contains("test-direct"), "{shown}");
}
