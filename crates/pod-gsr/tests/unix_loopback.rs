//! A stand-in for the GSRN relay over a Unix socket, and the tests that use
//! it.
//!
//! The tests of `tests/loopback.rs` bind a TCP port, which this host refuses.
//! This file speaks the same control messages over an `AF_UNIX` listener, so
//! the byte path, the pairing and the close behaviour are exercised on a host
//! that denies `bind(AF_INET)`.
//!
//! The stand-in reads `LISTEN` and `CONNECT`, pairs the two ends, sends
//! `START` with the flag of each side, reads `ACCEPT` of both, and then
//! carries the bytes.

// The stand-in listens on an AF_UNIX socket; tokio has no UnixListener on Windows.
#![cfg(unix)]

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pod_core::{BoxFuture, Class, Connected, Dial, Failure, Stage, Target};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio::time::timeout;

use pod_gsr::{End, Role, Settings, open};

const LISTEN: u8 = 0x01;
const CONNECT: u8 = 0x02;
const START: u8 = 0x05;
const STATUS: u8 = 0x07;
const MESSAGE: usize = 32;
const HELLO: usize = 128;
const ADDR_AT: usize = 32;

/// How the stand-in pairs the two ends.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Policy {
    /// Pair only the two ends that name the same address.
    MatchAddress,
    /// Pair any listener with any connector.
    Any,
    /// Answer each connector with a fatal status.
    Refuse,
}

struct Hello {
    key: [u8; 16],
    listener: bool,
    stream: UnixStream,
}

/// A path in the scratch directory, unique to this process and this call.
fn socket_path(tag: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("a clock").as_nanos();
    pod_testkit::host::socket_path(&format!("pod-gsr-{tag}-{}-{nanos}.sock", std::process::id()))
}

/// Start the stand-in and give its path.
async fn serve(tag: &str, policy: Policy) -> (PathBuf, JoinHandle<()>) {
    let path = socket_path(tag);
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).expect("a socket path");
    let (sent, mut received) = mpsc::unbounded_channel::<Hello>();
    let task = tokio::spawn(async move {
        let mut waiting: Vec<Hello> = Vec::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let Ok((mut stream, _)) = accepted else { return };
                    let sent = sent.clone();
                    tokio::spawn(async move {
                        let mut head = [0u8; HELLO];
                        if stream.read_exact(&mut head).await.is_err() {
                            return;
                        }
                        if head[0] != LISTEN && head[0] != CONNECT {
                            return;
                        }
                        let key: [u8; 16] = head[ADDR_AT..ADDR_AT + 16].try_into().expect("16 bytes");
                        let _ = sent.send(Hello { key, listener: head[0] == LISTEN, stream });
                    });
                }
                Some(hello) = received.recv() => {
                    if policy == Policy::Refuse && !hello.listener {
                        let mut stream = hello.stream;
                        let mut status = [0u8; MESSAGE];
                        status[0] = STATUS;
                        status[1] = 0x02;
                        status[2] = 0x02;
                        let _ = stream.write_all(&status).await;
                        continue;
                    }
                    let index = waiting.iter().position(|other| {
                        other.listener != hello.listener
                            && (policy == Policy::Any || other.key == hello.key)
                    });
                    match index {
                        Some(index) => {
                            let peer = waiting.remove(index);
                            tokio::spawn(carry(peer, hello));
                        }
                        None => waiting.push(hello),
                    }
                }
            }
        }
    });
    (path, task)
}

/// Give each end its `START`, read its `ACCEPT`, and carry the bytes.
async fn carry(peer: Hello, other: Hello) {
    let (mut listener, mut connector) =
        if peer.listener { (peer.stream, other.stream) } else { (other.stream, peer.stream) };
    let mut server = [0u8; MESSAGE];
    server[0] = START;
    server[1] = 0x01;
    let mut client = [0u8; MESSAGE];
    client[0] = START;
    client[1] = 0x02;
    if listener.write_all(&server).await.is_err() || connector.write_all(&client).await.is_err() {
        return;
    }
    let mut accept = [0u8; MESSAGE];
    if listener.read_exact(&mut accept).await.is_err() || connector.read_exact(&mut accept).await.is_err() {
        return;
    }
    let _ = tokio::io::copy_bidirectional(&mut listener, &mut connector).await;
}

/// A dialer that opens the socket path that the test gives it.
struct OverUnix;

impl Dial for OverUnix {
    fn label(&self) -> String {
        "unix".into()
    }

    fn dial<'a>(&'a self, target: &'a Target, _deadline: Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move {
            let stream = UnixStream::connect(target.host()).await.map_err(|e| Failure::io(Stage::Connect, e))?;
            Ok(Connected::new(Box::new(stream)))
        })
    }
}

fn settings_at(path: &std::path::Path) -> Settings {
    let mut settings = Settings::of_data().expect("the data file");
    settings.domain = path.to_string_lossy().to_string();
    settings.port = 1;
    settings.sharded = false;
    settings
}

fn listen(settings: &Settings, secret: &'static str) -> JoinHandle<Result<pod_gsr::Stream, Failure>> {
    let settings = settings.clone();
    tokio::spawn(async move {
        let mut entropy = pod_core::entropy::OsEntropy;
        open(&OverUnix, &settings, Role::Listen, secret, &mut entropy, Instant::now() + Duration::from_secs(30)).await
    })
}

async fn connect(settings: &Settings, secret: &'static str) -> Result<pod_gsr::Stream, Failure> {
    let mut entropy = pod_core::entropy::OsEntropy;
    open(&OverUnix, settings, Role::Connect, secret, &mut entropy, Instant::now() + Duration::from_secs(30)).await
}

#[tokio::test]
async fn one_megabyte_crosses_the_relay_over_a_unix_socket() {
    let (path, relay) = serve("megabyte", Policy::MatchAddress).await;
    let settings = settings_at(&path);
    let secret = "pod-gsr-unix-loopback";

    let listener = listen(&settings, secret);
    let mut connector = connect(&settings, secret).await.expect("a connector");
    let mut listener = listener.await.expect("the task").expect("a listener");

    let payload: Vec<u8> = (0u8..=255).cycle().take(1 << 20).collect();
    let mut received = vec![0u8; payload.len()];
    let (written, read) = tokio::join!(async { connector.write_all(&payload).await }, async {
        listener.read_exact(&mut received).await
    },);
    written.expect("the payload");
    read.expect("the payload arrives");
    assert_eq!(Sha256::digest(&received), Sha256::digest(&payload));

    drop((listener, connector));
    relay.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn both_directions_cross_and_the_peer_end_ends_the_stream() {
    let (path, relay) = serve("both", Policy::MatchAddress).await;
    let settings = settings_at(&path);
    let secret = "pod-gsr-unix-both";

    let listener = listen(&settings, secret);
    let mut connector = connect(&settings, secret).await.expect("a connector");
    let mut listener = listener.await.expect("the task").expect("a listener");

    connector.write_all(b"from the connector").await.expect("the bytes");
    let mut bytes = [0u8; 18];
    listener.read_exact(&mut bytes).await.expect("the bytes arrive");
    assert_eq!(&bytes, b"from the connector");

    listener.write_all(b"and back").await.expect("the answer");
    let mut answer = [0u8; 8];
    connector.read_exact(&mut answer).await.expect("the answer arrives");
    assert_eq!(&answer, b"and back");

    drop(connector);
    let mut extra = [0u8; 1];
    let read = timeout(pod_testkit::host::scaled(Duration::from_secs(10)), listener.read(&mut extra))
        .await
        .expect("an end")
        .expect("a read");
    assert_eq!(read, 0, "the peer closed the transport with no close_notify");

    relay.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn different_secrets_fail_the_handshake() {
    let (path, relay) = serve("secrets", Policy::Any).await;
    let settings = settings_at(&path);

    let listener = listen(&settings, "pod-gsr-secret-one");
    let failure = timeout(pod_testkit::host::scaled(Duration::from_secs(20)), connect(&settings, "pod-gsr-secret-two"))
        .await
        .expect("an answer")
        .expect_err("the secrets differ");
    assert_eq!((failure.stage, failure.class), (Stage::Tls, Class::Refused), "{}", failure.chain());

    let _ = timeout(pod_testkit::host::scaled(Duration::from_secs(5)), listener).await;
    relay.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_connector_with_no_listener_is_unavailable() {
    let (path, relay) = serve("refuse", Policy::Refuse).await;
    let settings = settings_at(&path);

    let failure = connect(&settings, "pod-gsr-unix-no-listener").await.expect_err("no listener");
    assert_eq!((failure.stage, failure.class), (Stage::Relay, Class::Unavailable));
    assert!(failure.text().contains("no peer"), "{}", failure.text());

    relay.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_reader_sees_every_byte_before_the_end() {
    // The peer sends 256 KiB and then closes without a close_notify, which is
    // what a peer of this relay does. A reader that reports the end at the
    // first zero-return loses the records that arrived with it, so this test
    // reads to a counted end rather than to the first empty read.
    let (path, relay) = serve("tail", Policy::MatchAddress).await;
    let settings = settings_at(&path);
    let secret = "pod-gsr-unix-tail";

    let listener = listen(&settings, secret);
    let mut connector = connect(&settings, secret).await.expect("a connector");
    let mut listener = listener.await.expect("the task").expect("a listener");

    let payload: Vec<u8> = (0u8..=255).cycle().take(256 * 1024).collect();
    let expected = payload.len();
    let sending = {
        let payload = payload.clone();
        tokio::spawn(async move {
            connector.write_all(&payload).await.expect("the payload");
            drop(connector);
        })
    };

    let mut received = Vec::with_capacity(expected);
    let mut buf = [0u8; 16384];
    while received.len() < expected {
        let n = timeout(pod_testkit::host::scaled(Duration::from_secs(10)), listener.read(&mut buf))
            .await
            .expect("no stall")
            .expect("a read");
        assert_ne!(n, 0, "the stream ended after {} of {expected} bytes", received.len());
        received.extend_from_slice(&buf[..n]);
    }
    sending.await.expect("the sender");
    assert_eq!(Sha256::digest(&received), Sha256::digest(&payload));
    assert!(!listener.peer_finished(), "the loop stopped at the byte count, so no end was read yet");

    // One more read gets the end itself, and only then is the peer finished.
    let n = timeout(pod_testkit::host::scaled(Duration::from_secs(10)), listener.read(&mut buf))
        .await
        .expect("no stall")
        .expect("a read");
    assert_eq!(n, 0, "the end of the peer");
    assert!(listener.peer_finished(), "the end of the peer was read");

    relay.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn every_byte_arrives_before_a_close_with_no_close_notify() {
    // The peer writes a megabyte and then drops the connection without a
    // close_notify, which is what a peer of this relay does. The reader counts
    // bytes, so a lost tail shows up as a mismatch rather than as a short body
    // reported complete.
    for run in 0..4 {
        let (path, relay) = serve("drop", Policy::MatchAddress).await;
        let settings = settings_at(&path);
        let secret = "pod-gsr-unix-drop";
        let listener = listen(&settings, secret);
        let mut connector = connect(&settings, secret).await.expect("a connector");
        let mut listener = listener.await.expect("the task").expect("a listener");

        let payload: Vec<u8> = (0u8..=255).cycle().take(1 << 20).collect();
        let expected = payload.len();
        let sending = {
            let payload = payload.clone();
            tokio::spawn(async move {
                connector.write_all(&payload).await.expect("the payload");
                tokio::time::sleep(Duration::from_millis(50)).await;
                drop(connector);
            })
        };

        let mut received = Vec::with_capacity(expected);
        let mut buf = [0u8; 16384];
        while received.len() < expected {
            let n = timeout(pod_testkit::host::scaled(Duration::from_secs(10)), listener.read(&mut buf))
                .await
                .expect("no stall")
                .expect("a read");
            assert_ne!(n, 0, "run {run}: the stream ended after {} of {expected}", received.len());
            received.extend_from_slice(&buf[..n]);
        }
        sending.await.expect("the sender");
        assert_eq!(Sha256::digest(&received), Sha256::digest(&payload), "run {run}");
        relay.abort();
        let _ = std::fs::remove_file(&path);
    }
}

#[tokio::test]
async fn a_shutdown_before_the_peer_ends_does_not_end_the_session() {
    // A caller that finishes sending must not end the session: the relay ends
    // it when either side closes, and the peer may still have bytes to
    // deliver. This is the truncation that the copy of this crate measured as
    // 128 KiB of 1 MiB.
    let (path, relay) = serve("shutdown", Policy::MatchAddress).await;
    let settings = settings_at(&path);
    let secret = "pod-gsr-unix-shutdown";

    let listener = listen(&settings, secret);
    let mut sender = connect(&settings, secret).await.expect("a connector");
    let mut listener = listener.await.expect("the task").expect("a listener");

    // The sender has nothing more to send and shuts its side down.
    sender.write_all(b"the last words").await.expect("the bytes");
    AsyncWriteExt::shutdown(&mut sender).await.expect("a shutdown");

    // The session must still carry the bytes that were already written.
    let mut bytes = [0u8; 14];
    listener.read_exact(&mut bytes).await.expect("the bytes arrive");
    assert_eq!(&bytes, b"the last words");

    relay.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_drop_of_the_peer_ends_the_stream_as_a_cut() {
    // The connector writes and drops its transport with no shutdown: the
    // listener reads to the end, and the end is a cut, not a clean end.
    let (path, relay) = serve("cut", Policy::MatchAddress).await;
    let settings = settings_at(&path);
    let secret = "pod-gsr-cut";
    let listener = listen(&settings, secret);
    let mut connector = connect(&settings, secret).await.expect("a connector");
    let mut listener = listener.await.expect("the task").expect("a listener");

    let sending = tokio::spawn(async move {
        connector.write_all(&vec![7u8; 1 << 20]).await.expect("the payload");
        drop(connector);
    });
    let mut received = Vec::new();
    timeout(pod_testkit::host::scaled(Duration::from_secs(10)), listener.read_to_end(&mut received))
        .await
        .expect("an end")
        .expect("a read of 0 bytes at the end");
    sending.await.expect("the sender");
    assert_eq!(listener.end(), Some(End::Cut), "{} bytes before the cut", received.len());
    assert!(listener.peer_finished());

    relay.abort();
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_shutdown_of_the_peer_ends_the_stream_as_clean() {
    // The connector writes and shuts its side down: its close_notify goes
    // out and its transport stays open, so the listener reads each byte and
    // a clean end. Then the listener shuts down, and the connector reads a
    // clean end too.
    let (path, relay) = serve("clean", Policy::MatchAddress).await;
    let settings = settings_at(&path);
    let secret = "pod-gsr-clean";
    let listener = listen(&settings, secret);
    let mut connector = connect(&settings, secret).await.expect("a connector");
    let mut listener = listener.await.expect("the task").expect("a listener");

    let payload: Vec<u8> = (0u8..=255).cycle().take(1 << 20).collect();
    let (written, read) = tokio::join!(
        async {
            connector.write_all(&payload).await?;
            AsyncWriteExt::shutdown(&mut connector).await
        },
        async {
            let mut received = Vec::new();
            timeout(pod_testkit::host::scaled(Duration::from_secs(10)), listener.read_to_end(&mut received))
                .await
                .map(|r| r.map(|_| received))
        },
    );
    written.expect("the payload and the shutdown");
    let received = read.expect("an end").expect("a read of 0 bytes at the end");
    assert_eq!(Sha256::digest(&received), Sha256::digest(&payload));
    assert_eq!(listener.end(), Some(End::Clean));
    assert_eq!(connector.end(), None, "the listener has not finished");

    AsyncWriteExt::shutdown(&mut listener).await.expect("the shutdown after the peer's end");
    let mut rest = Vec::new();
    timeout(pod_testkit::host::scaled(Duration::from_secs(10)), connector.read_to_end(&mut rest))
        .await
        .expect("an end")
        .expect("a read");
    assert!(rest.is_empty());
    assert_eq!(connector.end(), Some(End::Clean));

    relay.abort();
    let _ = std::fs::remove_file(&path);
}
