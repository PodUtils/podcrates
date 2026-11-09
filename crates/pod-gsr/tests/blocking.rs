//! The blocking face (feature `blocking`) through a stand-in relay on a Unix
//! socket. The stand-in runs on a runtime of its own thread; the two peers
//! run on plain threads with no runtime, as a synchronous consumer does.

// The stand-in listens on an AF_UNIX socket; tokio has no UnixListener on Windows.
#![cfg(unix)]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use pod_core::{BoxFuture, Class, Connected, Dial, Failure, Stage, Target};
use pod_gsr::{End, Role, Settings, blocking};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

const LISTEN: u8 = 0x01;
const START: u8 = 0x05;
const MESSAGE: usize = 32;
const HELLO: usize = 128;

/// A path in the temporary directory, unique to this process and this call.
fn socket_path(tag: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("a clock").as_nanos();
    std::env::temp_dir().join(format!("pod-gsr-b-{tag}-{}-{nanos}.sock", std::process::id()))
}

/// The stand-in relay: it pairs the first listener with the first connector,
/// gives each its `START`, reads each `ACCEPT`, and carries the bytes. It
/// runs on a thread with a runtime of its own, and ends with its runtime.
fn serve(tag: &str) -> (PathBuf, tokio::runtime::Runtime) {
    let path = socket_path(tag);
    let runtime =
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("a runtime");
    let listener = {
        let _in = runtime.enter();
        UnixListener::bind(&path).expect("a socket path")
    };
    runtime.spawn(async move {
        let (mut first, mut second) = (accept(&listener).await, accept(&listener).await);
        let ((a, a_listens), (b, _)) = (&mut first, &mut second);
        let (listener_side, connector_side) = if *a_listens { (a, b) } else { (b, a) };
        let mut server = [0u8; MESSAGE];
        server[0] = START;
        server[1] = 0x01;
        let mut client = [0u8; MESSAGE];
        client[0] = START;
        client[1] = 0x02;
        listener_side.write_all(&server).await.expect("START");
        connector_side.write_all(&client).await.expect("START");
        let mut accept = [0u8; MESSAGE];
        listener_side.read_exact(&mut accept).await.expect("ACCEPT");
        connector_side.read_exact(&mut accept).await.expect("ACCEPT");
        let _ = tokio::io::copy_bidirectional(listener_side, connector_side).await;
    });
    (path, runtime)
}

/// One peer and whether it listens.
async fn accept(listener: &UnixListener) -> (UnixStream, bool) {
    let (mut stream, _) = listener.accept().await.expect("a peer");
    let mut head = [0u8; HELLO];
    stream.read_exact(&mut head).await.expect("LISTEN or CONNECT");
    (stream, head[0] == LISTEN)
}

/// A dialer that opens the socket path that the test gives it. It runs on
/// the private runtime of the blocking face.
struct OverUnix;

impl Dial for OverUnix {
    fn label(&self) -> String {
        "unix".into()
    }

    fn dial<'a>(
        &'a self,
        target: &'a Target,
        _deadline: tokio::time::Instant,
    ) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move {
            let stream = UnixStream::connect(target.host()).await.map_err(|e| Failure::io(Stage::Connect, e))?;
            Ok(Connected::new(Box::new(stream)))
        })
    }
}

fn settings_at(path: &Path) -> Settings {
    let mut settings = Settings::of_data().expect("the data file");
    settings.domain = path.to_string_lossy().to_string();
    settings.port = 1;
    settings.sharded = false;
    settings
}

fn open(settings: &Settings, role: Role, secret: &str) -> Result<blocking::Stream, Failure> {
    let mut entropy = pod_core::entropy::OsEntropy;
    blocking::open(&OverUnix, settings, role, secret, &mut entropy, Instant::now() + Duration::from_secs(30))
}

#[test]
fn one_megabyte_crosses_the_relay_through_the_blocking_face() {
    let (path, relay) = serve("megabyte");
    let settings = settings_at(&path);
    let secret = "pod-gsr-blocking";

    let listener = thread::spawn({
        let settings = settings.clone();
        move || open(&settings, Role::Listen, secret)
    });
    let mut connector = open(&settings, Role::Connect, secret).expect("a connector");
    let mut listener = listener.join().expect("the thread").expect("a listener");
    listener.set_timeout(Some(Duration::from_secs(10)));

    let payload: Vec<u8> = (0u8..=255).cycle().take(1 << 20).collect();
    let sender = thread::spawn({
        let payload = payload.clone();
        move || {
            connector.write_all(&payload).expect("the payload");
            connector.shutdown().expect("the close_notify");
            connector
        }
    });
    let mut received = Vec::new();
    listener.read_to_end(&mut received).expect("each byte, then the end");
    let mut connector = sender.join().expect("the sender");
    assert_eq!(Sha256::digest(&received), Sha256::digest(&payload));
    assert_eq!(listener.end(), Some(End::Clean));
    assert!(listener.peer_finished());

    // The listener answers after the peer's end, and closes.
    listener.write_all(b"pong").expect("the answer");
    listener.shutdown().expect("the shutdown after the peer's end");
    connector.set_timeout(Some(Duration::from_secs(10)));
    let mut answer = Vec::new();
    connector.read_to_end(&mut answer).expect("the answer, then the end");
    assert_eq!(answer, b"pong");
    assert_eq!(connector.end(), Some(End::Clean));

    drop((listener, connector));
    drop(relay);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_read_that_waits_past_its_limit_times_out_and_keeps_the_stream() {
    let (path, relay) = serve("limit");
    let settings = settings_at(&path);
    let secret = "pod-gsr-blocking-limit";

    let listener = thread::spawn({
        let settings = settings.clone();
        move || open(&settings, Role::Listen, secret)
    });
    let mut connector = open(&settings, Role::Connect, secret).expect("a connector");
    let mut listener = listener.join().expect("the thread").expect("a listener");

    listener.set_timeout(Some(Duration::from_millis(100)));
    let mut byte = [0u8; 1];
    let quiet = listener.read(&mut byte).expect_err("the peer sends nothing");
    assert_eq!(quiet.kind(), std::io::ErrorKind::TimedOut);

    connector.write_all(b"x").expect("a byte");
    listener.set_timeout(Some(Duration::from_secs(10)));
    assert_eq!(listener.read(&mut byte).expect("the byte"), 1);
    assert_eq!(&byte, b"x");
    assert_eq!(listener.end(), None);

    drop((listener, connector));
    drop(relay);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_call_inside_a_runtime_is_a_usage_failure() {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
    let settings = settings_at(Path::new("/nonexistent/pod-gsr.sock"));
    let failure = runtime.block_on(async { open(&settings, Role::Connect, "pod-gsr-inside") }).expect_err("refused");
    assert_eq!((failure.stage, failure.class), (Stage::Relay, Class::Usage));
    assert!(failure.text().contains("tokio"), "{}", failure.text());
}

#[test]
fn a_read_inside_a_runtime_fails_and_does_not_panic() {
    let (path, relay) = serve("inside");
    let settings = settings_at(&path);
    let secret = "pod-gsr-blocking-inside";

    let listener = thread::spawn({
        let settings = settings.clone();
        move || open(&settings, Role::Listen, secret)
    });
    let _connector = open(&settings, Role::Connect, secret).expect("a connector");
    let mut listener = listener.join().expect("the thread").expect("a listener");

    let runtime = tokio::runtime::Builder::new_current_thread().build().expect("a runtime");
    let read = runtime.block_on(async { listener.read(&mut [0u8; 1]) });
    assert_eq!(read.expect_err("refused").kind(), std::io::ErrorKind::Unsupported);

    drop(relay);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_dial_that_fails_is_a_failure_of_the_dial() {
    let settings = settings_at(Path::new("/nonexistent/pod-gsr.sock"));
    let failure = open(&settings, Role::Connect, "pod-gsr-no-relay").expect_err("no relay");
    assert_eq!(failure.stage, Stage::Connect);
}
