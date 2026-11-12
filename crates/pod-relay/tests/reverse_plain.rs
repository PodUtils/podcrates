//! podnet's plain mode of the reverse road (T-026), offline: a node's half
//! close, the relay's close of a session whose local side does not read, a
//! second `open` of a live session, the local targets of podnet (TCP and
//! Unix), and the operator that waits for the node's last bytes after the
//! end of its input. The relay is scripted over an in-memory stream, as in
//! `reverse_node.rs` and `reverse_operator.rs`. The names of podnet's tests
//! are kept where a test here takes one of them.
#![cfg(feature = "pair")]

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_relay::reverse::operator::exchange;
use pod_relay::reverse::{
    End, Handler, LocalHandler, Opening, OperatorLimits, Outcome, SessionId, Settings, Target, channel, serve,
};
use pod_ws::frame::{self, Frame, Role};
use pod_ws::session::RelaySession;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::sync::Notify;

#[cfg(feature = "plain-ws")]
#[allow(dead_code)]
mod stand_in;

mod support;

const HELLO: &str = r#"{"type":"hello","version":1,"maxFrameBytes":65536,"maxSessions":64}"#;
const A: &str = "127def7af9734ef39930ae7be7c1e4ee";
const LIMIT: Duration = Duration::from_secs(5);

/// The relay's end of a socket over a pipe.
struct Relay {
    peer: DuplexStream,
    buf: Vec<u8>,
}

impl Relay {
    async fn send(&mut self, opcode: u8, payload: &[u8]) {
        let bytes = frame::encode(&Frame { fin: true, opcode, payload: payload.to_vec() }, Role::Server, [0; 4]);
        self.peer.write_all(&bytes).await.unwrap();
    }

    async fn text(&mut self, text: &str) {
        self.send(frame::OPCODE_TEXT, text.as_bytes()).await;
    }

    async fn data(&mut self, id: &str, payload: &[u8]) {
        self.send(frame::OPCODE_BINARY, &[id.as_bytes(), payload].concat()).await;
    }

    /// The far end's next frame other than a Ping, or `None` after `wait`.
    async fn next(&mut self, wait: Duration) -> Option<Frame> {
        tokio::time::timeout(wait, async {
            loop {
                if let Some((f, used)) = frame::decode(&self.buf, Role::Client).expect("a valid client frame") {
                    self.buf.drain(..used);
                    if f.opcode != frame::OPCODE_PING {
                        return f;
                    }
                    continue;
                }
                let mut chunk = [0u8; 8192];
                let n = self.peer.read(&mut chunk).await.unwrap();
                assert!(n > 0, "the far end closed the stream");
                self.buf.extend_from_slice(&chunk[..n]);
            }
        })
        .await
        .ok()
    }

    async fn expect_text(&mut self) -> String {
        let f = self.next(LIMIT).await.expect("a frame in time");
        assert_eq!(f.opcode, frame::OPCODE_TEXT, "{f:?}");
        String::from_utf8(f.payload).unwrap()
    }
}

/// Each session's local side is one end of a pipe; the test takes the other.
#[derive(Default, Clone)]
struct Local {
    ends: Arc<Mutex<HashMap<SessionId, DuplexStream>>>,
    opens: Arc<AtomicUsize>,
}

impl Handler for Local {
    type Stream = DuplexStream;

    fn open(&self, id: SessionId) -> Opening<DuplexStream> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        let (node_end, test_end) = tokio::io::duplex(64 * 1024);
        self.ends.lock().unwrap().insert(id, test_end);
        Box::pin(async move { Ok(node_end) })
    }
}

impl Local {
    async fn end(&self) -> DuplexStream {
        for _ in 0..250 {
            if let Some(end) = self.ends.lock().unwrap().remove(&SessionId::parse(A.as_bytes()).unwrap()) {
                return end;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("no local side");
    }
}

/// A node over one pipe with `settings`, the scripted relay on the other end.
fn node<H: Handler>(handler: H, settings: Settings) -> (Relay, Arc<Notify>, tokio::task::JoinHandle<End>) {
    let (client, peer) = tokio::io::duplex(1 << 20);
    let stop = Arc::new(Notify::new());
    let stopper = stop.clone();
    let settings = Settings { ping_every: Duration::from_secs(60), ..settings };
    let task = tokio::spawn(async move {
        let session = RelaySession::new(client, Vec::new(), None, LIMIT);
        let mut stop = Box::pin(async move { stopper.notified().await });
        serve(session, Arc::new(handler), settings, &mut stop).await
    });
    (Relay { peer, buf: Vec::new() }, stop, task)
}

fn open(id: &str) -> String {
    format!(r#"{{"type":"open","id":"{id}"}}"#)
}

/// podnet T-196: a target that shuts its write side and still reads gets the
/// operator's input that comes after; the session ends once none came for
/// the half close's time. With no half close, the end of its writes ends the
/// session at once.
#[tokio::test]
async fn a_half_closing_target_gets_the_whole_input() {
    let idle = Duration::from_millis(600);
    let local = Local::default();
    let (mut relay, _stop, _task) = node(local.clone(), Settings { half_close: Some(idle), ..Settings::default() });
    relay.text(HELLO).await;
    relay.text(&open(A)).await;
    assert!(relay.expect_text().await.contains(r#""type":"ready""#));
    let mut target = local.end().await;
    target.write_all(b"HELLO").await.unwrap();
    target.shutdown().await.unwrap();
    let f = relay.next(LIMIT).await.expect("the target's bytes");
    assert_eq!(&f.payload[32..], b"HELLO");
    assert!(relay.next(Duration::from_millis(200)).await.is_none(), "no close while the input may come");
    relay.data(A, b"after the half close").await;
    let mut got = vec![0u8; 20];
    target.read_exact(&mut got).await.unwrap();
    assert_eq!(got, b"after the half close");
    let sent = tokio::time::Instant::now();
    let closed = relay.expect_text().await;
    assert!(closed.contains(r#""type":"close""#) && closed.contains(A), "{closed}");
    assert!(sent.elapsed() >= idle - Duration::from_millis(100), "the close came after the idle time");
    let mut rest = Vec::new();
    target.read_to_end(&mut rest).await.unwrap();
    assert!(rest.is_empty(), "the target sees the end");

    // The default: the end of the target's writes ends the session at once.
    let local = Local::default();
    let (mut relay, _stop, _task) = node(local.clone(), Settings::default());
    relay.text(HELLO).await;
    relay.text(&open(A)).await;
    relay.expect_text().await;
    let mut target = local.end().await;
    target.shutdown().await.unwrap();
    assert!(relay.expect_text().await.contains(r#""type":"close""#));
}

/// podnet T-194: the relay's close ends a session whose target does not
/// read. Its bytes that wait go within the drain's time; then the node drops
/// them and the target, which sees the end. Before, the copy to the target
/// stayed in its write for as long as the target did not read.
#[tokio::test]
async fn a_relay_close_ends_a_session_whose_target_does_not_read() {
    let local = Local::default();
    let drain = Duration::from_millis(300);
    let (mut relay, _stop, _task) = node(local.clone(), Settings { drain, ..Settings::default() });
    relay.text(HELLO).await;
    relay.text(&open(A)).await;
    relay.expect_text().await;
    let mut target = local.end().await;
    // 256 KiB: more than the target's pipe holds, less than the node's queue.
    let chunk = vec![7u8; 64 * 1024];
    for _ in 0..4 {
        relay.data(A, &chunk).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    relay.text(&format!(r#"{{"type":"close","id":"{A}"}}"#)).await;
    tokio::time::sleep(drain * 3).await;
    let mut read = Vec::new();
    tokio::time::timeout(LIMIT, target.read_to_end(&mut read)).await.expect("the target sees the end").unwrap();
    assert!(read.len() < 4 * chunk.len(), "the target read {} bytes, each one", read.len());
}

/// podnet's gate: an `open` of a session that is open is no new session.
#[tokio::test]
async fn a_second_open_of_a_live_session_is_no_new_session() {
    let local = Local::default();
    let (mut relay, _stop, _task) = node(local.clone(), Settings::default());
    relay.text(HELLO).await;
    relay.text(&open(A)).await;
    relay.expect_text().await;
    let mut target = local.end().await;
    relay.text(&open(A)).await;
    assert!(relay.next(Duration::from_millis(500)).await.is_none(), "no second ready");
    assert_eq!(local.opens.load(Ordering::SeqCst), 1, "one local side");
    relay.data(A, b"still one").await;
    let mut got = [0u8; 9];
    target.read_exact(&mut got).await.unwrap();
    assert_eq!(&got, b"still one");
}

/// podnet's targets: a session to a Unix socket carries its bytes, and one
/// to a closed TCP port is refused with a reason that names the target.
#[tokio::test]
async fn a_session_to_a_closed_port_is_refused_with_the_reason() {
    pod_testkit::need_loopback!();
    let dir = pod_testkit::host::socket_dir("pod-relay-plain");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("echo.sock");
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (mut r, mut w) = socket.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    let handler = |target: &str| LocalHandler {
        target: Target::parse(target).unwrap(),
        timeout: LIMIT,
        dialer: support::dialer(),
    };
    let (mut relay, _stop, _task) = node(handler(&format!("unix:{}", path.display())), Settings::default());
    relay.text(HELLO).await;
    relay.text(&open(A)).await;
    assert!(relay.expect_text().await.contains(r#""type":"ready""#));
    relay.data(A, b"over a unix socket").await;
    let f = relay.next(LIMIT).await.expect("the echo");
    assert_eq!(&f.payload[32..], b"over a unix socket");
    let _ = std::fs::remove_dir_all(&dir);

    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().to_string();
    let (mut relay, _stop, _task) = node(handler(&closed), Settings::default());
    relay.text(HELLO).await;
    relay.text(&open(A)).await;
    let refused = relay.expect_text().await;
    assert!(refused.contains(r#""type":"reject""#) && refused.contains(&closed), "{refused}");
}

/// An operator over one pipe: its input `input`, which then ends, and its
/// output for the test to read.
fn operator(
    eof_idle: Option<Duration>,
    input: &'static [u8],
) -> (Relay, tokio::io::ReadHalf<DuplexStream>, tokio::task::JoinHandle<Outcome>) {
    let limits = OperatorLimits {
        ready_limit: Duration::from_secs(3),
        close_limit: Duration::from_secs(2),
        ping_every: Duration::from_secs(60),
        eof_idle,
        ..OperatorLimits::default()
    };
    let (client, peer) = tokio::io::duplex(1 << 20);
    let (io, local) = tokio::io::duplex(1 << 20);
    let task = tokio::spawn(async move {
        let session = RelaySession::new(client, Vec::new(), None, LIMIT);
        exchange(session, io, limits).await
    });
    let (output, mut write) = tokio::io::split(local);
    tokio::spawn(async move {
        write.write_all(input).await.unwrap();
        write.shutdown().await.unwrap();
    });
    (Relay { peer, buf: Vec::new() }, output, task)
}

const READY: &str = r#"{"type":"ready","id":"4914e9e009a64fcb88d47bb8bd1a8417"}"#;

async fn outcome(task: tokio::task::JoinHandle<Outcome>) -> Outcome {
    tokio::time::timeout(LIMIT, task).await.expect("an outcome in time").unwrap()
}

/// podnet issue 109: an input that ends at once still gets the target's
/// answer. The end of the input sends no Close; the Close goes once no byte
/// of the node came for `eof_idle`. With no `eof_idle`, it goes at once.
#[tokio::test]
async fn the_answer_comes_back_after_the_input_ends() {
    let idle = Duration::from_millis(600);
    let (mut relay, mut output, task) = operator(Some(idle), b"PING\n");
    relay.text(READY).await;
    let f = relay.next(LIMIT).await.expect("the input");
    assert_eq!((f.opcode, f.payload.as_slice()), (frame::OPCODE_BINARY, &b"PING\n"[..]));
    assert!(relay.next(Duration::from_millis(300)).await.is_none(), "no Close at the end of the input");
    relay.send(frame::OPCODE_BINARY, b"HELLO:PING\n").await;
    let mut got = [0u8; 11];
    output.read_exact(&mut got).await.unwrap();
    assert_eq!(&got, b"HELLO:PING\n");
    let answered = tokio::time::Instant::now();
    let f = relay.next(LIMIT).await.expect("a Close after the idle time");
    assert_eq!(f.opcode, frame::OPCODE_CLOSE);
    assert!(answered.elapsed() >= idle - Duration::from_millis(100), "the Close waited for the idle time");
    relay.send(frame::OPCODE_CLOSE, &1000u16.to_be_bytes()).await;
    assert_eq!(outcome(task).await, Outcome::LocalEnd);

    let (mut relay, _output, _task) = operator(None, b"PING\n");
    relay.text(READY).await;
    relay.next(LIMIT).await.expect("the input");
    let f = relay.next(LIMIT).await.expect("the Close at the end of the input");
    assert_eq!(f.opcode, frame::OPCODE_CLOSE);
}

/// podssh #84 in podnet: after the end of the input, a close other than 1000
/// is a failure with its code, and the bytes before it come; 1000 is a
/// success.
#[tokio::test]
async fn a_close_with_an_error_code_after_the_input_ends_is_an_error() {
    for (code, success) in [(1009u16, false), (1000, true)] {
        let (mut relay, mut output, task) = operator(Some(Duration::from_secs(15)), b"");
        relay.text(READY).await;
        relay.send(frame::OPCODE_BINARY, b"part").await;
        let mut got = [0u8; 4];
        output.read_exact(&mut got).await.unwrap();
        assert_eq!(&got, b"part");
        relay.send(frame::OPCODE_CLOSE, &[&code.to_be_bytes()[..], b"limit"].concat()).await;
        let ended = outcome(task).await;
        assert_eq!(ended, Outcome::Ended { code, reason: "limit".into() });
        assert_eq!(ended.is_success(), success, "{ended:?}");
    }
}

/// podnet: a node whose token the relay refuses with `401` stops, as at a
/// `403`: a new socket would hear the same. Before, it connected again for
/// as long as it ran.
#[cfg(feature = "plain-ws")]
#[tokio::test(flavor = "multi_thread")]
async fn a_node_whose_token_is_refused_with_401_stops() {
    use pod_relay::reverse::{Exit, NodeConfig, TcpHandler, Wire, run};
    pod_testkit::need_loopback!();
    let (stand_in, relay) = stand_in::StandIn::new();
    let refusals = std::thread::spawn(move || stand_in.refuse(401, "missing or wrong token"));
    let ctx = support::context();
    let handler =
        TcpHandler { host: "127.0.0.1".into(), port: 9, timeout: LIMIT, sshd: false, dialer: support::dialer() };
    let mut config = NodeConfig {
        pair: stand_in::test_pair(&relay, stand_in::NAME, 0),
        label: None,
        trust: &pod_ws::Trust::Default,
        ctx: &ctx,
        timeout: LIMIT,
        settings: Settings::default(),
        repair: None,
        wire: Wire::PlainLoopback,
        say: None,
    };
    let running = run(&mut config, Arc::new(handler), std::future::pending());
    let exit = tokio::time::timeout(Duration::from_secs(20), running).await.expect("the node stops");
    assert!(matches!(exit, Exit::Forbidden), "{exit:?}");
    refusals.join().unwrap();
}

/// podnet T-083: a service of the node's process answers each session that
/// the relay opens, with no socket file (`reverse::channel`); a session
/// after the service is gone is refused.
#[tokio::test]
async fn a_service_of_the_process_answers_each_session_through_a_channel() {
    const B: &str = "8f0e7c2a9b1d4e6f8a0b2c4d6e8f0a1b";
    let (handler, mut sessions) = channel(4);
    let (mut relay, _stop, _task) = node(handler, Settings::default());
    let service = tokio::spawn(async move {
        for _ in 0..2 {
            let (id, mut stream) = sessions.recv().await.expect("a session");
            let mut asked = [0u8; 4];
            stream.read_exact(&mut asked).await.unwrap();
            stream.write_all(format!("{}:{}", String::from_utf8_lossy(&asked), id.as_str()).as_bytes()).await.unwrap();
        }
    });
    relay.text(HELLO).await;
    for id in [A, B] {
        relay.text(&open(id)).await;
        assert!(relay.expect_text().await.contains(r#""type":"ready""#), "{id}");
        relay.data(id, b"ping").await;
        let f = relay.next(LIMIT).await.expect("the answer of the service");
        assert_eq!(&f.payload[..32], id.as_bytes());
        assert_eq!(f.payload[32..].to_vec(), format!("ping:{id}").into_bytes());
        // The service drops its end after its answer: the session ends.
        let closed = relay.expect_text().await;
        assert!(closed.contains(r#""type":"close""#) && closed.contains(id), "{closed}");
    }
    service.await.unwrap();
    // The service is gone: the channel is closed, and a new session is refused.
    const C: &str = "0c1d2e3f405162738495a6b7c8d9eafb";
    relay.text(&open(C)).await;
    let refused = relay.expect_text().await;
    assert!(refused.contains(C) && refused.contains("the service of the process is gone"), "{refused}");
}
