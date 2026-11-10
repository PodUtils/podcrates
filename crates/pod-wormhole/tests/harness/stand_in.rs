//! A mailbox in Rust, on the loopback, that speaks the server protocol of
//! magic-wormhole's mailbox server, version 1, for the tests that need no
//! Python: `bind`, `list`, `allocate`, `claim`, `open`, `add`, `release` and
//! `close`, with an `ack` for each command, a nameplate of at most two
//! sides (`crowded` for a third), a mailbox that each side opens and closes,
//! and each message sent to each side that has the mailbox open, the
//! sender's too. One application's nameplates are never another's. A test
//! can tamper with the body of a phase, and reads the moods of the closes.
//! [`scripted`] gives one connection to a script, for the faults.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::sync::{Arc, Mutex};

use pod_proxy::http::request;
use pod_ws::url::WsUrl;
use pod_ws::{RelaySession, frame, server};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

type Tx = UnboundedSender<String>;

#[derive(Default)]
struct Space {
    /// Nameplate, the sides that claimed it, and the sides that released
    /// it: it is free when each side that claimed it released it, and a
    /// third side is crowded out while it lives, whatever was released.
    plates: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)>,
    boxes: HashMap<String, Mbox>,
}

#[derive(Default)]
struct Mbox {
    messages: Vec<Value>,
    open: Vec<(String, Tx)>,
    seen: BTreeSet<String>,
    closed: BTreeSet<String>,
}

#[derive(Default)]
struct State {
    spaces: HashMap<String, Space>,
    moods: Vec<(String, String)>,
    tamper: Option<String>,
    counter: u64,
}

/// A running stand-in.
pub struct Stand {
    /// Its WebSocket, `ws://127.0.0.1:PORT/v1`.
    pub url: WsUrl,
    state: Arc<Mutex<State>>,
    task: JoinHandle<()>,
}

impl Stand {
    /// The moods of each close so far, as (side, mood).
    pub fn moods(&self) -> Vec<(String, String)> {
        self.state.lock().unwrap().moods.clone()
    }

    /// From now on, flip a bit in the body of each message of `phase`: a
    /// mailbox that changes what it carries.
    pub fn tamper(&self, phase: &str) {
        self.state.lock().unwrap().tamper = Some(phase.to_string());
    }

    /// The nameplates held now at `appid`.
    pub fn nameplates(&self, appid: &str) -> Vec<String> {
        let state = self.state.lock().unwrap();
        state.spaces.get(appid).map(|s| s.plates.keys().cloned().collect()).unwrap_or_default()
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// A stand-in on a free port of the loopback.
pub async fn start() -> Stand {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port of the loopback");
    let port = listener.local_addr().expect("its address").port();
    let state = Arc::new(Mutex::new(State::default()));
    let shared = state.clone();
    let task = tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            tokio::spawn(serve(tcp, shared.clone()));
        }
    });
    Stand { url: WsUrl::parse(&format!("ws://127.0.0.1:{port}/v1")).unwrap(), state, task }
}

/// A listener that gives its first connection, upgraded, to `script`, and
/// no other: the URL, and the task of the script.
pub async fn scripted<F, Fut>(script: F) -> (WsUrl, JoinHandle<()>)
where
    F: FnOnce(RelaySession<TcpStream>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port of the loopback");
    let port = listener.local_addr().expect("its address").port();
    let task = tokio::spawn(async move {
        let Ok((tcp, _)) = listener.accept().await else { return };
        if let Some(session) = upgraded(tcp).await {
            script(session).await;
        }
    });
    (WsUrl::parse(&format!("ws://127.0.0.1:{port}/v1")).unwrap(), task)
}

async fn upgraded(mut tcp: TcpStream) -> Option<RelaySession<TcpStream>> {
    let (head, pending) = request::read_request(&mut tcp, Vec::new()).await.ok()?;
    let key = server::check_upgrade(&head).ok()?;
    tcp.write_all(&server::switching(&key)).await.ok()?;
    Some(server::session(tcp, pending, None))
}

/// The welcome that a mailbox sends first.
pub fn welcome() -> String {
    json!({ "type": "welcome", "welcome": { "motd": "a stand-in" } }).to_string()
}

struct Conn {
    appid: Option<String>,
    side: Option<String>,
}

async fn serve(tcp: TcpStream, state: Arc<Mutex<State>>) {
    let Some(session) = upgraded(tcp).await else { return };
    let session = Arc::new(session);
    let (tx, mut rx) = unbounded_channel::<String>();
    let writer = {
        let session = session.clone();
        tokio::spawn(async move {
            while let Some(text) = rx.recv().await {
                if session.send_text(&text).await.is_err() {
                    break;
                }
            }
        })
    };
    let _ = tx.send(welcome());
    let mut conn = Conn { appid: None, side: None };
    while let Ok(f) = session.read_frame().await {
        if f.opcode != frame::OPCODE_TEXT {
            if f.opcode == frame::OPCODE_CLOSE {
                break;
            }
            continue;
        }
        let Ok(command) = serde_json::from_slice::<Value>(&f.payload) else { break };
        let _ = tx.send(json!({ "type": "ack", "id": command["id"] }).to_string());
        let mut state = state.lock().unwrap();
        for reply in handle(&mut state, &mut conn, &tx, &command) {
            let _ = tx.send(reply.to_string());
        }
    }
    writer.abort();
}

fn error(why: &str, command: &Value) -> Vec<Value> {
    vec![json!({ "type": "error", "error": why, "orig": command })]
}

fn handle(state: &mut State, conn: &mut Conn, tx: &Tx, command: &Value) -> Vec<Value> {
    let text = |key: &str| command[key].as_str().unwrap_or_default().to_string();
    let kind = text("type");
    if kind == "bind" {
        (conn.appid, conn.side) = (Some(text("appid")), Some(text("side")));
        return Vec::new();
    }
    let (Some(appid), Some(side)) = (conn.appid.clone(), conn.side.clone()) else {
        return error("must bind first", command);
    };
    let tamper = state.tamper.clone();
    state.counter += 1;
    let counter = state.counter;
    let mut moods = Vec::new();
    let space = state.spaces.entry(appid).or_default();
    let reply = match kind.as_str() {
        "list" => {
            let all: Vec<Value> = space.plates.keys().map(|id| json!({ "id": id })).collect();
            vec![json!({ "type": "nameplates", "nameplates": all })]
        }
        "allocate" => {
            let free = (1u32..).map(|n| n.to_string()).find(|n| !space.plates.contains_key(n)).unwrap();
            space.plates.entry(free.clone()).or_default().0.insert(side);
            vec![json!({ "type": "allocated", "nameplate": free })]
        }
        "claim" => {
            let plate = space.plates.entry(text("nameplate")).or_default();
            if !plate.0.contains(&side) && plate.0.len() >= 2 {
                return error("crowded", command);
            }
            plate.0.insert(side);
            let id = format!("mb{}", text("nameplate"));
            space.boxes.entry(id.clone()).or_default();
            vec![json!({ "type": "claimed", "mailbox": id })]
        }
        "open" => {
            let Some(mailbox) = space.boxes.get_mut(&text("mailbox")) else { return error("no such mailbox", command) };
            mailbox.seen.insert(side.clone());
            for message in &mailbox.messages {
                let _ = tx.send(message.to_string());
            }
            mailbox.open.push((side, tx.clone()));
            Vec::new()
        }
        "add" => {
            let Some(id) = mailbox_of(space, &side) else { return error("no open mailbox", command) };
            let mailbox = space.boxes.get_mut(&id).unwrap();
            let mut body = text("body");
            if tamper.as_deref() == Some(text("phase").as_str()) {
                let last = body.pop().unwrap_or('0');
                body.push(if last == '0' { '1' } else { '0' });
            }
            let message = json!({ "type": "message", "side": side, "phase": text("phase"), "body": body, "id": format!("{counter:04x}") });
            mailbox.messages.push(message.clone());
            for (_, other) in &mailbox.open {
                let _ = other.send(message.to_string());
            }
            Vec::new()
        }
        "release" => {
            if let Some(plate) = space.plates.get_mut(&text("nameplate")) {
                plate.1.insert(side);
                if plate.1.len() >= plate.0.len() {
                    space.plates.remove(&text("nameplate"));
                }
            }
            vec![json!({ "type": "released" })]
        }
        "close" => {
            moods.push((side.clone(), text("mood")));
            if let Some(mailbox) = space.boxes.get_mut(&text("mailbox")) {
                mailbox.open.retain(|(s, _)| *s != side);
                mailbox.closed.insert(side);
                if mailbox.closed.len() >= mailbox.seen.len() {
                    space.boxes.remove(&text("mailbox"));
                }
            }
            vec![json!({ "type": "closed" })]
        }
        _ => error("unknown command", command),
    };
    state.moods.extend(moods);
    reply
}

fn mailbox_of(space: &Space, side: &str) -> Option<String> {
    space.boxes.iter().find(|(_, m)| m.open.iter().any(|(s, _)| s == side)).map(|(id, _)| id.clone())
}
