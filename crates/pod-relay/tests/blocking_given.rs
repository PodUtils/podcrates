//! The facade's node of a pair that its user gives as parts (call C130), and
//! its lines (call C131), against the stand-in relay on the loopback, as
//! podvm's `node --relay --name --node-token` runs it.
#![cfg(all(feature = "blocking", feature = "plain-ws"))]

// The shared stand-in serves other tests too: not each of its items is used here.
#[allow(dead_code)]
mod stand_in;

mod support;

use std::sync::{Arc, mpsc};

use pod_relay::blocking::{Client, Config, Local, NodeChannel, NodeOptions, Stopper};
use pod_relay::pair::Pair;
use pod_relay::reverse::{Exit, Wire};
use pod_ws::frame;
use stand_in::{ID1, LIMIT, NAME, NODE, StandIn, data, open, pipe, ready};

fn client() -> Client {
    Client::new(Config { wire: Wire::PlainLoopback, timeout: LIMIT, ..Config::new(support::context()) })
        .expect("a client")
}

#[test]
fn a_node_of_given_parts_serves_and_says_each_socket() {
    let (stand_in, relay) = StandIn::new();
    let mut pair = Pair::for_node(relay, NAME, NODE).expect("a node part");
    let client = client();
    let stopper = Stopper::new();
    let (said, lines) = mpsc::channel::<String>();
    let said = std::sync::Mutex::new(said);
    let options = NodeOptions {
        channel: NodeChannel::Off,
        say: Some(Arc::new(move |line: String| {
            let _ = said.lock().unwrap().send(line);
        })),
        ..NodeOptions::default()
    };
    let echo = |_: &str| {
        let (reader, writer) = pipe();
        Ok(Local::new(reader, writer))
    };
    std::thread::scope(|s| {
        let node = s.spawn(|| client.run_node(&mut pair, echo, &options, &stopper));
        let _guard = StopOnPanic(&stopper);
        let (head, mut peer) = stand_in.accept();
        assert!(head.starts_with(&format!("GET /v1/node/{NAME} HTTP/1.1\r\n")), "{head}");
        assert!(head.contains(&format!("X-Relay-Token: {NODE}\r\n")), "the given node token, in its header");
        let online = lines.recv_timeout(LIMIT).expect("a line");
        assert!(online.starts_with("online: "), "{online}");
        peer.text(r#"{"type":"hello","version":1,"maxFrameBytes":65536,"maxSessions":64}"#);
        peer.text(&open(ID1));
        assert_eq!(peer.next_text(), ready(ID1));
        peer.send(frame::OPCODE_BINARY, &data(ID1, b"ping"));
        assert_eq!(peer.node_bytes(4), vec![(ID1.to_string(), b"ping".to_vec())]);
        // The relay ends the socket: the node says so, and connects again.
        peer.close(1000, "");
        let ended = lines.recv_timeout(LIMIT).expect("a line");
        assert!(ended.contains("relay close 1000") && ended.contains("connecting again"), "{ended}");
        let (_, mut peer) = stand_in.accept();
        let again = lines.recv_timeout(LIMIT).expect("a line");
        assert!(again.starts_with("online again: "), "{again}");
        stopper.stop();
        let end = peer.next(LIMIT).expect("a Close of the socket");
        assert_eq!(end.opcode, frame::OPCODE_CLOSE);
        peer.close(1000, "");
        let exit = node.join().unwrap();
        assert!(matches!(exit, Ok(Exit::Stopped)), "{exit:?}");
    });
    assert_eq!(pair.name, NAME);
}

/// A node that cannot reach its relay says each failed try, with the next
/// one (call C132), where it said nothing before.
#[test]
fn a_node_says_each_try_that_does_not_connect() {
    let port = {
        let gone = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        gone.local_addr().unwrap().port()
    };
    let relay = pod_relay::Relay { host: "127.0.0.1".into(), port };
    let mut pair = Pair::for_node(relay, NAME, NODE).expect("a node part");
    let client = client();
    let stopper = Stopper::new();
    let (said, lines) = mpsc::channel::<String>();
    let said = std::sync::Mutex::new(said);
    let options = NodeOptions {
        channel: NodeChannel::Off,
        say: Some(Arc::new(move |line: String| {
            let _ = said.lock().unwrap().send(line);
        })),
        ..NodeOptions::default()
    };
    let refuse = |_: &str| -> Result<Local, String> { Err("no session".into()) };
    std::thread::scope(|s| {
        let node = s.spawn(|| client.run_node(&mut pair, refuse, &options, &stopper));
        let _guard = StopOnPanic(&stopper);
        let line = lines.recv_timeout(LIMIT).expect("a line");
        assert!(line.starts_with("the node could not connect to the relay ("), "{line}");
        assert!(line.contains("; connecting again in "), "{line}");
        assert!(!line.contains(NODE), "no token in a line");
        stopper.stop();
        let exit = node.join().unwrap();
        assert!(matches!(exit, Ok(Exit::Stopped)), "{exit:?}");
    });
}

/// A node token that the relay refuses at the upgrade (`403`) ends the node:
/// the expiry of a pair of parts is not known, so the refusal is no expiry.
#[test]
fn a_refused_node_token_ends_a_node_of_given_parts() {
    let (stand_in, relay) = StandIn::new();
    let mut pair = Pair::for_node(relay, NAME, NODE).expect("a node part");
    let client = client();
    let options = NodeOptions { channel: NodeChannel::Off, ..NodeOptions::default() };
    let refuse = |_: &str| -> Result<Local, String> { Err("no session".into()) };
    std::thread::scope(|s| {
        let node = s.spawn(|| client.run_node(&mut pair, refuse, &options, &Stopper::new()));
        stand_in.refuse(403, "reverse: forbidden");
        let exit = node.join().unwrap();
        assert!(matches!(exit, Ok(Exit::Forbidden)), "{exit:?}");
    });
}

/// Stops the node when an assertion fails, so that the scope's join ends and
/// the test fails rather than waits for ever.
struct StopOnPanic<'a>(&'a Stopper);

impl Drop for StopOnPanic<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.stop();
        }
    }
}
