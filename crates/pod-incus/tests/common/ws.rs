//! The websockets of an operation of exec in the stand-in: the upgrade of
//! `GET /1.0/operations/ID/websocket?secret=`, and the command, which
//! starts when 0, 1 and 2 are connected, as incus starts it. The data goes
//! in binary messages and each output ends with an empty text message (the
//! barrier of incus); the stand-in then closes each socket with no Close,
//! as incus does.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_proxy::http::Request;
use pod_ws::RelaySession;
use pod_ws::frame::{OPCODE_BINARY, OPCODE_TEXT};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::sync::Notify;

use super::{Seen, Shared, error};

pub type Session = Arc<RelaySession<UnixStream>>;

/// The sockets of each operation, by fd, and the operations whose command
/// started.
#[derive(Default)]
pub struct Sockets {
    pub by_op: BTreeMap<String, BTreeMap<String, Session>>,
}

pub async fn serve(
    shared: Arc<Shared>,
    mut stream: UnixStream,
    request: Request,
    rest: Vec<u8>,
    path: String,
    query: BTreeMap<String, String>,
) {
    let seen = Seen {
        method: request.method.clone(),
        path: path.clone(),
        query: query.clone(),
        headers: request.headers.clone(),
        body: Vec::new(),
    };
    shared.state.lock().unwrap().requests.push(seen);
    shared.arrived.notify_waiters();
    let id = path.trim_start_matches("/1.0/operations/").trim_end_matches("/websocket").to_string();
    let secret = query.get("secret").cloned().unwrap_or_default();
    let fds = shared.state.lock().unwrap().ops.get(&id).map(|op| op.json["metadata"]["fds"].clone());
    let Some(fds) = fds else {
        let _ = stream.write_all(&error(404, "Operation not found")).await;
        return;
    };
    let fds = fds.as_object().cloned().unwrap_or_default();
    let fd = fds.iter().find(|(_, v)| v.as_str() == Some(secret.as_str())).map(|(k, _)| k.clone());
    let Some(fd) = fd else {
        let _ = stream.write_all(&error(403, "Forbidden")).await;
        return;
    };
    let key = match pod_ws::server::check_upgrade(&request) {
        Ok(key) => key,
        Err(refused) => {
            let _ = stream.write_all(&refused.answer()).await;
            return;
        }
    };
    if stream.write_all(&pod_ws::server::switching(&key)).await.is_err() {
        return;
    }
    let session = Arc::new(pod_ws::server::session(stream, rest, None));
    let ready = {
        let mut sockets = shared.sockets.lock().unwrap();
        let of_op = sockets.by_op.entry(id.clone()).or_default();
        of_op.insert(fd, session);
        let ready = ["0", "1", "2"].iter().all(|fd| of_op.contains_key(*fd)) && !of_op.contains_key("started");
        if ready {
            // A mark that the command started: the session of stdin again.
            let mark = of_op["0"].clone();
            of_op.insert("started".into(), mark);
        }
        ready
    };
    if ready {
        tokio::spawn(command(shared, id));
    }
}

/// The sockets of `id`, taken out of the table.
fn take(shared: &Shared, id: &str) -> BTreeMap<String, Session> {
    shared.sockets.lock().unwrap().by_op.remove(id).unwrap_or_default()
}

/// The command of the operation `id`, by the plan of the state.
async fn command(shared: Arc<Shared>, id: String) {
    let plan = shared.state.lock().unwrap().plan.clone();
    let sockets = shared.sockets.lock().unwrap().by_op[&id].clone();
    let (stdin, stdout, stderr) = (sockets["0"].clone(), sockets["1"].clone(), sockets["2"].clone());
    let signal = Arc::new((Notify::new(), Mutex::new(None::<i64>)));
    let control = sockets.get("control").cloned().map(|control| {
        let (shared, signal) = (shared.clone(), signal.clone());
        tokio::spawn(async move {
            while let Ok(frame) = control.read_frame().await {
                if frame.opcode != OPCODE_TEXT && frame.opcode != OPCODE_BINARY {
                    break;
                }
                let message: Value = serde_json::from_slice(&frame.payload).unwrap_or_default();
                if message["command"] == "signal" {
                    let number = message["signal"].as_i64().unwrap_or(0);
                    shared.state.lock().unwrap().signals.push(number);
                    *signal.1.lock().unwrap() = Some(number);
                    signal.0.notify_one();
                }
            }
        })
    });
    let half = plan.stdout.len() / 2;
    for part in [&plan.stdout[..half], &plan.stdout[half..]] {
        if !part.is_empty() {
            let _ = stdout.send_binary(part).await;
        }
    }
    if !plan.stderr.is_empty() {
        let _ = stderr.send_binary(&plan.stderr).await;
    }
    if plan.echo_stdin {
        while let Ok(frame) = stdin.read_frame().await {
            match frame.opcode {
                OPCODE_BINARY => {
                    shared.state.lock().unwrap().stdin_seen.extend_from_slice(&frame.payload);
                    let _ = stdout.send_binary(&frame.payload).await;
                }
                OPCODE_TEXT if frame.payload.is_empty() => break,
                _ => break,
            }
        }
    }
    let mut exit = plan.exit;
    if plan.until_signal {
        let _ = tokio::time::timeout(Duration::from_secs(10), signal.0.notified()).await;
        exit = 128 + signal.1.lock().unwrap().unwrap_or(0);
    }
    let _ = stdout.send_text("").await;
    let _ = stderr.send_text("").await;
    {
        let mut state = shared.state.lock().unwrap();
        state.end_op(&id, json!({"return": exit}));
    }
    shared.changed.notify_waiters();
    if let Some(reader) = control {
        reader.abort();
        let _ = reader.await;
    }
    drop((stdin, stdout, stderr, sockets));
    drop(take(&shared, &id));
}
