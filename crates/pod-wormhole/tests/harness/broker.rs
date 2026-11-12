//! The MQTT brokers of the tests, on the loopback: a stand-in in Rust
//! ([`start`]), and Eclipse Mosquitto with a `protocol websockets`
//! listener ([`Mosquitto`]), software that this crate did not write.
//!
//! The stand-in speaks MQTT 3.1.1 over a WebSocket through the codec of the
//! crate: a `CONNECT` gets a `CONNACK` (code 4 for a login that is not its
//! own, when it has one), a `SUBSCRIBE` a `SUBACK`, a `PINGREQ` a
//! `PINGRESP`; a `PUBLISH` goes to each connection subscribed to its topic
//! (by its name, or `#`). A test can make the next publishes reach no
//! subscriber, and reads the topic of each publish and the subprotocol of
//! each upgrade.
//!
//! Mosquitto is the broker that `POD_WORMHOLE_TEST_BROKER` names
//! (`ws://127.0.0.1:PORT/mqtt`, as CI runs it), else the program
//! `POD_WORMHOLE_TEST_MOSQUITTO`, `mosquitto` or `/usr/sbin/mosquitto`
//! started on a free port. A host with neither skips it with the reason,
//! unless `POD_WORMHOLE_TEST_SERVER=required`.

use std::net::{Ipv4Addr, SocketAddr, TcpStream as StdTcp};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pod_proxy::http::request;
use pod_wormhole::mqtt::packet::{self, Packet};
use pod_ws::url::WsUrl;
use pod_ws::{frame, server};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

use super::{LIMIT, free_port, required};

#[derive(Default)]
struct State {
    /// Each subscription: the connection, the filter, and its writer.
    subs: Vec<(u64, String, UnboundedSender<Vec<u8>>)>,
    login: Option<(String, Vec<u8>)>,
    /// The next publishes that reach no subscriber.
    lost: usize,
    published: Vec<String>,
    protocols: Vec<String>,
    connections: u64,
}

/// A running stand-in.
pub struct Stand {
    /// Its WebSocket, `ws://127.0.0.1:PORT/mqtt`.
    pub url: WsUrl,
    state: Arc<Mutex<State>>,
    task: JoinHandle<()>,
}

impl Stand {
    /// The topic of each publish so far, the lost ones too.
    pub fn published(&self) -> Vec<String> {
        self.state.lock().unwrap().published.clone()
    }

    /// The subprotocol that each upgrade offered.
    pub fn protocols(&self) -> Vec<String> {
        self.state.lock().unwrap().protocols.clone()
    }

    /// The next `n` publishes reach no subscriber, as a message at QoS 0
    /// that comes before the other end subscribed.
    pub fn lose(&self, n: usize) {
        self.state.lock().unwrap().lost = n;
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// A stand-in on a free port of the loopback, with no login.
pub async fn start() -> Stand {
    start_with(None).await
}

/// A stand-in that takes the login `user` and `password` alone.
pub async fn start_with(login: Option<(&str, &str)>) -> Stand {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port of the loopback");
    let port = listener.local_addr().expect("its address").port();
    let login = login.map(|(user, password)| (user.to_string(), password.as_bytes().to_vec()));
    let state = Arc::new(Mutex::new(State { login, ..State::default() }));
    let shared = state.clone();
    let task = tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            tokio::spawn(serve(tcp, shared.clone()));
        }
    });
    Stand { url: WsUrl::parse(&format!("ws://127.0.0.1:{port}/mqtt")).unwrap(), state, task }
}

async fn serve(mut tcp: TcpStream, state: Arc<Mutex<State>>) {
    let Ok((head, pending)) = request::read_request(&mut tcp, Vec::new()).await else { return };
    let Ok(key) = server::check_upgrade(&head) else { return };
    let offered = head.header("sec-websocket-protocol").unwrap_or_default().to_string();
    if tcp.write_all(&server::switching(&key)).await.is_err() {
        return;
    }
    let session = Arc::new(server::session(tcp, pending, None));
    let (tx, mut rx) = unbounded_channel::<Vec<u8>>();
    let writer = {
        let session = session.clone();
        tokio::spawn(async move {
            while let Some(bytes) = rx.recv().await {
                if session.send_binary(&bytes).await.is_err() {
                    break;
                }
            }
        })
    };
    let id = {
        let mut state = state.lock().unwrap();
        state.protocols.push(offered);
        state.connections += 1;
        state.connections
    };
    let mut bytes = Vec::new();
    'conn: while let Ok(f) = session.read_frame().await {
        if f.opcode != frame::OPCODE_BINARY {
            break;
        }
        bytes.extend_from_slice(&f.payload);
        while let Ok(Some((packet, used))) = packet::decode(&bytes, 1 << 20) {
            bytes.drain(..used);
            if !handle(&state, id, &tx, packet) {
                break 'conn;
            }
        }
    }
    state.lock().unwrap().subs.retain(|(conn, _, _)| *conn != id);
    // Let the writer send what is queued, then end.
    drop(tx);
    let _ = writer.await;
    let _ = session.send_close(1000, "").await;
}

/// One packet of a client; `false` ends the connection.
fn handle(state: &Mutex<State>, id: u64, tx: &UnboundedSender<Vec<u8>>, packet: Packet) -> bool {
    let send = |p: &Packet| tx.send(packet::encode(p).expect("a packet")).is_ok();
    let mut state = state.lock().unwrap();
    match packet {
        Packet::Connect(c) => {
            let refused = state.login.is_some() && state.login != c.login;
            send(&Packet::ConnAck { present: false, code: if refused { 4 } else { 0 } });
            !refused
        }
        Packet::Subscribe { id: packet_id, filters } => {
            let codes = vec![0; filters.len()];
            for (filter, _) in filters {
                state.subs.push((id, filter, tx.clone()));
            }
            send(&Packet::SubAck { id: packet_id, codes })
        }
        Packet::Publish { topic, payload } => {
            state.published.push(topic.clone());
            if state.lost > 0 {
                state.lost -= 1;
                return true;
            }
            let bytes = packet::encode(&Packet::Publish { topic: topic.clone(), payload }).expect("a packet");
            for (_, filter, to) in &state.subs {
                if *filter == topic || filter == "#" {
                    let _ = to.send(bytes.clone());
                }
            }
            true
        }
        Packet::PingReq => send(&Packet::PingResp),
        _ => false,
    }
}

/// Eclipse Mosquitto, listening on the loopback, or the broker of
/// `POD_WORMHOLE_TEST_BROKER`.
pub struct Mosquitto {
    /// Its WebSocket, `ws://127.0.0.1:PORT/mqtt`.
    pub url: WsUrl,
    child: Option<Child>,
    dir: Option<PathBuf>,
}

impl Mosquitto {
    /// The broker; `None` when this host has none.
    pub fn start(tag: &str) -> Option<Mosquitto> {
        if let Ok(named) = std::env::var("POD_WORMHOLE_TEST_BROKER")
            && !named.trim().is_empty()
        {
            let url = WsUrl::parse(named.trim()).expect("POD_WORMHOLE_TEST_BROKER is ws://HOST:PORT/PATH");
            return Some(Mosquitto { url, child: None, dir: None });
        }
        let Some(program) = program() else {
            let why = "no Mosquitto here (install mosquitto, or set POD_WORMHOLE_TEST_BROKER)";
            assert!(!required(), "POD_WORMHOLE_TEST_SERVER=required: {why}");
            eprintln!("did not run: {why}");
            return None;
        };
        let dir = std::env::temp_dir().join(format!("pod-wormhole-mosquitto-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let port = free_port();
        let config = dir.join("mosquitto.conf");
        let text = format!(
            "listener {port} 127.0.0.1\nprotocol websockets\nallow_anonymous true\npersistence false\nlog_dest stderr\n"
        );
        std::fs::write(&config, text).expect("the configuration");
        let child = Command::new(program)
            .arg("-c")
            .arg(&config)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(dir.join("mosquitto.log")).expect("the log"))
            .spawn()
            .expect("Mosquitto starts");
        let url = WsUrl::parse(&format!("ws://127.0.0.1:{port}/mqtt")).unwrap();
        let broker = Mosquitto { url, child: Some(child), dir: Some(dir) };
        let at = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let deadline = Instant::now() + LIMIT;
        while StdTcp::connect_timeout(&at, Duration::from_millis(200)).is_err() {
            assert!(Instant::now() < deadline, "Mosquitto did not listen in time: {}", broker.said());
            std::thread::sleep(Duration::from_millis(100));
        }
        Some(broker)
    }

    /// What Mosquitto wrote, for a message.
    pub fn said(&self) -> String {
        let log = self.dir.as_ref().map(|d| d.join("mosquitto.log"));
        log.and_then(|log| std::fs::read_to_string(log).ok()).unwrap_or_default()
    }
}

impl Drop for Mosquitto {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(dir) = &self.dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// The program of Mosquitto that this host has.
fn program() -> Option<String> {
    let named = std::env::var("POD_WORMHOLE_TEST_MOSQUITTO").ok().filter(|n| !n.trim().is_empty());
    let all = named.into_iter().chain(["mosquitto".to_string(), "/usr/sbin/mosquitto".to_string()]);
    all.into_iter().find(|p| {
        // `-h` prints the usage and exits: the program is there.
        Command::new(p).arg("-h").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok()
    })
}

/// What keeps a broker running while a test uses it.
#[allow(dead_code)]
pub enum Keep {
    Stand(Stand),
    Mosquitto(Mosquitto),
}

/// Each broker that this host can run for a test: the stand-in, and
/// Mosquitto, by name, with what keeps each running.
pub async fn brokers(tag: &str) -> Vec<(&'static str, WsUrl, Keep)> {
    let stand = start().await;
    let mut all = vec![("the stand-in broker", stand.url.clone(), Keep::Stand(stand))];
    if let Some(mosquitto) = Mosquitto::start(tag) {
        all.push(("Eclipse Mosquitto", mosquitto.url.clone(), Keep::Mosquitto(mosquitto)));
    }
    all
}
