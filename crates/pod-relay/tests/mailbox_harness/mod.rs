//! The mailboxes of the tests, on the loopback: a stand-in in Rust
//! ([`stand_in`]), and magic-wormhole's own mailbox server (magic-wormhole-
//! mailbox-server, in Python, on Twisted and Autobahn): pod-wormhole's
//! harness (`crates/pod-wormhole/tests/harness/`), whose peer of the Python
//! library stays there. The Python server is software that this crate did
//! not write. A host with no Python, or without the packages, skips it with
//! the reason, unless `POD_WORMHOLE_TEST_SERVER=required`, which a workflow
//! sets, so that a skip there is a failure.
//!
//!   python3 -m pip install magic-wormhole-mailbox-server
//!
//! `POD_WORMHOLE_TEST_PYTHON` names the interpreter (a virtualenv's).

// Each test binary uses a part of it.
#![allow(dead_code)]

use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use pod_wormhole::Config;
use pod_ws::url::WsUrl;

pub mod stand_in;
// The MQTT brokers of pod-wormhole's tests (a stand-in, and Mosquitto),
// for the runs that meet at a broker (T-277).
#[cfg(feature = "plain-ws")]
#[path = "../../../pod-wormhole/tests/harness/broker.rs"]
pub mod broker;

/// How long a server may take to listen, and a peer to say a line.
pub const LIMIT: Duration = Duration::from_secs(60);

/// What the Python of this host can run.
fn required() -> bool {
    std::env::var("POD_WORMHOLE_TEST_SERVER").is_ok_and(|v| v == "required")
}

fn pythons() -> Vec<String> {
    match std::env::var("POD_WORMHOLE_TEST_PYTHON") {
        Ok(named) if !named.trim().is_empty() => vec![named],
        _ => vec!["python3".into(), "python".into()],
    }
}

fn imports(python: &str, modules: &str) -> bool {
    let probe = Command::new(python)
        .args(["-c", &format!("import {modules}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    probe.is_ok_and(|status| status.success())
}

/// A Python that imports `modules`; `None`, with the reason on stderr, when
/// none does and the run does not require it.
pub fn python(modules: &str, package: &str) -> Option<String> {
    let found = pythons().into_iter().find(|p| imports(p, modules));
    if found.is_none() {
        let why = format!("no Python with {package} here (pip install {package})");
        assert!(!required(), "POD_WORMHOLE_TEST_SERVER=required: {why}");
        eprintln!("did not run: {why}");
    }
    found
}

fn free_port() -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a port of the loopback");
    listener.local_addr().expect("its address").port()
}

/// A mailbox's URL where nothing listens: a port of the loopback just let go.
pub fn nobody_there() -> WsUrl {
    WsUrl::parse(&format!("ws://127.0.0.1:{}/v1", free_port())).unwrap()
}

/// The AppID of the tests.
pub const APPID: &str = "example.org/pod-wormhole/test";

/// The way of the tests: a direct dial and TLS on ring.
pub fn way() -> pod_ws::Way {
    use std::sync::Arc;

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls = Arc::new(pod_tls::Context::new(provider, pod_tls::Settings::new(pod_tls::Words::new("pod-wormhole"))));
    let settings = pod_proxy::Settings::new("pod-wormhole", pod_proxy::ProxyChoice::Direct);
    let dial = pod_proxy::Dialer::new(settings, Arc::new(pod_proxy::resolve::System::new()));
    pod_ws::Way::new(tls, Arc::new(dial), "pod-wormhole/test")
}

/// The settings of the tests for `appid`.
pub fn config_for(appid: &str) -> Config {
    Config::new(appid, "pod-wormhole-test", "0.0.0", way())
}

/// The settings of the tests for [`APPID`].
pub fn config() -> Config {
    config_for(APPID)
}

/// magic-wormhole's mailbox server, listening on the loopback.
pub struct Server {
    child: Child,
    /// Its WebSocket, `ws://127.0.0.1:PORT/v1`.
    pub url: WsUrl,
    log: PathBuf,
    dir: PathBuf,
}

impl Server {
    /// The server; `None` when this host cannot run it.
    pub fn start(tag: &str) -> Option<Server> {
        let python = python("wormhole_mailbox_server, autobahn, twisted", "magic-wormhole-mailbox-server")?;
        let dir = std::env::temp_dir().join(format!("pod-wormhole-mailbox-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let log = dir.join("server.log");
        let port = free_port();
        // `python -m twisted` is `twist`, which runs the plugin in the
        // foreground; its state in a database of the scratch directory.
        let child = Command::new(&python)
            .args(["-m", "twisted", "wormhole-mailbox"])
            .arg(format!("--port=tcp:{port}:interface=127.0.0.1"))
            .arg(format!("--channel-db={}", dir.join("relay.sqlite").display()))
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(std::fs::File::create(&log).expect("the log"))
            .stderr(std::fs::File::create(dir.join("server.err")).expect("the log of errors"))
            .spawn()
            .expect("the mailbox server starts");
        let server = Server { child, url: WsUrl::parse(&format!("ws://127.0.0.1:{port}/v1")).unwrap(), log, dir };
        let at = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let deadline = Instant::now() + LIMIT;
        while TcpStream::connect_timeout(&at, Duration::from_millis(200)).is_err() {
            assert!(Instant::now() < deadline, "the mailbox server did not listen in time: {}", server.said());
            std::thread::sleep(Duration::from_millis(100));
        }
        Some(server)
    }

    /// What the server wrote, for a message.
    pub fn said(&self) -> String {
        let out = std::fs::read_to_string(&self.log).unwrap_or_default();
        let err = std::fs::read_to_string(self.log.with_file_name("server.err")).unwrap_or_default();
        format!("{out}{err}")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Each mailbox that this host can run for a test: the stand-in, and
/// magic-wormhole's server, by name, with what keeps each running.
pub async fn mailboxes(tag: &str) -> Vec<(&'static str, WsUrl, Keep)> {
    let mut all = Vec::new();
    let stand = stand_in::start().await;
    all.push(("the stand-in", stand.url.clone(), Keep::Stand(stand)));
    if let Some(server) = Server::start(tag) {
        all.push(("magic-wormhole's mailbox server", server.url.clone(), Keep::Python(server)));
    }
    all
}

/// What keeps a mailbox running while a test uses it.
#[allow(dead_code)]
pub enum Keep {
    Stand(stand_in::Stand),
    Python(Server),
}
