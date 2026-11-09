//! A stand-in for the GSRN relay, and the tests that use it.
//!
//! The stand-in speaks the control messages of the relay: it reads `LISTEN`
//! and `CONNECT`, sends `START` with the flag of each side, reads `ACCEPT`,
//! and then carries the bytes. It stands in for a service that is not on the
//! test host.

use std::net::SocketAddr;
use std::time::Duration;

use pod_core::{BoxFuture, Class, Connected, Dial, Failure, Stage, Target};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio::time::timeout;

use pod_gsr::{Role, Settings, open};

const LISTEN: u8 = 0x01;
const CONNECT: u8 = 0x02;
const START: u8 = 0x05;
const STATUS: u8 = 0x07;
const MESSAGE: usize = 32;
const HELLO: usize = 128;
const ADDR_AT: usize = 32;

/// How the stand-in pairs the two ends.
#[derive(Clone, Copy)]
struct Policy {
    /// Pair only the two ends of the same address.
    match_addr: bool,
    /// Answer each connector with a fatal status.
    refuse: bool,
}

const PAIR: Policy = Policy { match_addr: true, refuse: false };
const PAIR_ANY: Policy = Policy { match_addr: false, refuse: false };
const REFUSE: Policy = Policy { match_addr: true, refuse: true };

struct Hello {
    key: [u8; 16],
    listener: bool,
    stream: TcpStream,
}

/// Start the stand-in and give its address.
async fn serve(policy: Policy) -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a socket");
    let addr = listener.local_addr().expect("its address");
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
                    if policy.refuse && !hello.listener {
                        let mut stream = hello.stream;
                        let mut status = [0u8; MESSAGE];
                        status[0] = STATUS;
                        status[1] = 0x02;
                        status[2] = 0x02;
                        let _ = stream.write_all(&status).await;
                        continue;
                    }
                    let index = waiting.iter().position(|other| {
                        other.listener != hello.listener && (!policy.match_addr || other.key == hello.key)
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
    (addr, task)
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

/// A dialer that connects to the address of the target.
struct Direct;

impl Dial for Direct {
    fn label(&self) -> String {
        "direct".into()
    }

    fn dial<'a>(&'a self, target: &'a Target, _deadline: Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move {
            let stream = TcpStream::connect(target.to_string()).await.map_err(|e| Failure::io(Stage::Connect, e))?;
            Ok(Connected::new(Box::new(stream)))
        })
    }
}

fn settings_at(addr: SocketAddr) -> Settings {
    let mut settings = Settings::of_data().expect("the data file");
    settings.domain = addr.ip().to_string();
    settings.port = addr.port();
    settings.sharded = false;
    settings
}

fn listen(settings: &Settings, secret: &'static str) -> JoinHandle<Result<pod_gsr::Stream, Failure>> {
    let settings = settings.clone();
    tokio::spawn(async move {
        let mut entropy = pod_core::entropy::OsEntropy;
        open(&Direct, &settings, Role::Listen, secret, &mut entropy, Instant::now() + Duration::from_secs(30)).await
    })
}

async fn connect(settings: &Settings, secret: &'static str) -> Result<pod_gsr::Stream, Failure> {
    let mut entropy = pod_core::entropy::OsEntropy;
    open(&Direct, settings, Role::Connect, secret, &mut entropy, Instant::now() + Duration::from_secs(30)).await
}

#[tokio::test]
async fn one_megabyte_crosses_the_relay() {
    pod_testkit::need_loopback!();
    let (addr, relay) = serve(PAIR).await;
    let settings = settings_at(addr);
    let secret = "pod-gsr-loopback";

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

    let mut answer = [0u8; 4];
    let (written, read) =
        tokio::join!(async { listener.write_all(b"pong").await }, async { connector.read_exact(&mut answer).await },);
    written.expect("the answer");
    read.expect("the answer arrives");
    assert_eq!(&answer, b"pong");

    drop((listener, connector));
    relay.abort();
}

#[tokio::test]
async fn the_end_of_the_peer_ends_the_stream() {
    pod_testkit::need_loopback!();
    let (addr, relay) = serve(PAIR).await;
    let settings = settings_at(addr);
    let secret = "pod-gsr-close";

    let listener = listen(&settings, secret);
    let mut connector = connect(&settings, secret).await.expect("a connector");
    let mut listener = listener.await.expect("the task").expect("a listener");

    connector.write_all(b"bye").await.expect("the bytes");
    let mut bytes = [0u8; 3];
    listener.read_exact(&mut bytes).await.expect("the bytes arrive");
    assert_eq!(&bytes, b"bye");

    drop(connector);
    let mut extra = [0u8; 1];
    let read = timeout(Duration::from_secs(5), listener.read(&mut extra)).await.expect("an end").expect("a read");
    assert_eq!(read, 0, "the peer closed the transport with no close_notify");

    relay.abort();
}

#[tokio::test]
async fn different_secrets_fail_the_handshake() {
    pod_testkit::need_loopback!();
    let (addr, relay) = serve(PAIR_ANY).await;
    let settings = settings_at(addr);

    let listener = listen(&settings, "pod-gsr-secret-one");
    let failure = timeout(Duration::from_secs(10), connect(&settings, "pod-gsr-secret-two"))
        .await
        .expect("an answer")
        .expect_err("the secrets differ");
    assert_eq!((failure.stage, failure.class), (Stage::Tls, Class::Refused), "{}", failure.chain());

    let _ = timeout(Duration::from_secs(5), listener).await;
    relay.abort();
}

#[tokio::test]
async fn a_connector_with_no_listener_is_unavailable() {
    pod_testkit::need_loopback!();
    let (addr, relay) = serve(REFUSE).await;
    let settings = settings_at(addr);

    let failure = connect(&settings, "pod-gsr-no-listener").await.expect_err("no listener");
    assert_eq!((failure.stage, failure.class), (Stage::Relay, Class::Unavailable));
    assert!(failure.text().contains("no peer"), "{}", failure.text());

    relay.abort();
}
