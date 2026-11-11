//! The state of the stand-in and the answer of each call.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use super::{Seen, Shared, error, http, sync};

/// What a command of exec does in the stand-in.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub exit: i64,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// stdout gets each byte of stdin after `stdout`, as `cat` does.
    pub echo_stdin: bool,
    /// The command ends only at a signal of the control socket; its exit
    /// code is then 128 and the signal.
    pub until_signal: bool,
}

/// An operation of the stand-in.
#[derive(Debug, Clone)]
pub struct Op {
    pub json: Value,
}

impl Op {
    pub fn done(&self) -> bool {
        self.json["status_code"].as_u64().unwrap_or(0) >= 200
    }
}

/// The state of the stand-in.
#[derive(Debug, Default)]
pub struct State {
    pub requests: Vec<Seen>,
    /// The extensions of `GET /1.0`; empty: those of incus 6.0 that the
    /// crate needs.
    pub extensions: Option<Vec<String>>,
    pub instances: BTreeMap<String, Value>,
    pub aliases: Vec<(String, String)>,
    /// Each image that came: its parts (a form name, a file name, the bytes).
    pub images: Vec<Vec<(String, String, Vec<u8>)>>,
    pub files: BTreeMap<(String, String), (Vec<u8>, String, String)>,
    pub logs: BTreeMap<String, Vec<u8>>,
    pub ops: BTreeMap<String, Op>,
    pub plan: Plan,
    pub console: Vec<u8>,
    /// The operations do not end.
    pub hold: bool,
    /// The operations end with this failure.
    pub fail: Option<String>,
    /// Each request of this method and path is refused with this status and
    /// these words.
    pub refuse: Option<(String, String, u16, String)>,
    pub stdin_seen: Vec<u8>,
    pub signals: Vec<i64>,
}

impl State {
    /// A new operation with `metadata`, running.
    pub fn new_op(&mut self, class: &str, metadata: Value) -> String {
        let id = format!("0b5e{:04}-aa00-4000-8000-000000000000", self.ops.len());
        let json = json!({"id": id, "class": class, "description": "stand-in", "created_at": "2026-11-11T00:00:00Z",
            "updated_at": "2026-11-11T00:00:00Z", "status": "Running", "status_code": 103, "resources": null,
            "metadata": metadata, "may_cancel": false, "err": "", "location": "none"});
        self.ops.insert(id.clone(), Op { json });
        id
    }

    /// End the operation `id`: a success with `metadata` added, or the
    /// failure of the state.
    pub fn end_op(&mut self, id: &str, metadata: Value) {
        let fail = self.fail.clone();
        let Some(op) = self.ops.get_mut(id) else { return };
        if let (Some(into), Value::Object(add)) = (op.json["metadata"].as_object_mut(), &metadata) {
            into.extend(add.clone());
        } else if !metadata.is_null() {
            op.json["metadata"] = metadata;
        }
        match fail {
            Some(err) => {
                op.json["status"] = "Failure".into();
                op.json["status_code"] = 400.into();
                op.json["err"] = err.into();
            }
            None => {
                op.json["status"] = "Success".into();
                op.json["status_code"] = 200.into();
            }
        }
    }
}

/// The answer of a background operation: 202 with its `Location`.
fn started(state: &State, id: &str) -> Vec<u8> {
    let op = state.ops[id].json.clone();
    let body = json!({"type": "async", "status": "Operation created", "status_code": 100,
        "operation": format!("/1.0/operations/{id}"), "error_code": 0, "error": "", "metadata": op});
    let headers = [("Content-Type", "application/json".to_string()), ("Location", format!("/1.0/operations/{id}"))];
    http(202, &headers, body.to_string().as_bytes())
}

/// Start an operation that ends with `metadata` at once (unless the state
/// holds it).
pub fn run(shared: &Arc<Shared>, class: &str, metadata: Value, end: Value) -> Vec<u8> {
    let mut state = shared.state.lock().unwrap();
    let id = state.new_op(class, metadata);
    if !state.hold {
        state.end_op(&id, end);
    }
    let answer = started(&state, &id);
    shared.changed.notify_waiters();
    answer
}

fn server(state: &State) -> Value {
    let needed = ["instances", "container_exec_recording", "container_exec_user_group_cwd", "console"];
    let more = ["directory_manipulation", "file_symlinks", "file_append", "operation_wait"];
    let extensions: Vec<String> =
        state.extensions.clone().unwrap_or_else(|| needed.iter().chain(more.iter()).map(|s| s.to_string()).collect());
    json!({"api_extensions": extensions, "api_status": "stable", "api_version": "1.0", "auth": "trusted",
        "public": false, "auth_methods": ["tls"], "config": {}, "environment": {"addresses": null,
        "architectures": ["x86_64", "i686"], "certificate": "made-up", "driver": "lxc | qemu",
        "driver_version": "6.0.0 | 8.2.1", "kernel": "Linux", "kernel_version": "6.8.0", "server": "incus",
        "server_name": "stand-in", "server_pid": 7, "server_version": "6.0.0", "storage": "dir"}})
}

fn instance_json(name: &str, kind: &str, status: &str) -> Value {
    let code = if status == "Running" { 103 } else { 102 };
    json!({"name": name, "description": "", "status": status, "status_code": code, "type": kind,
        "architecture": "x86_64", "created_at": "2026-11-11T00:00:00Z", "last_used_at": "0001-01-01T00:00:00Z",
        "location": "none", "project": "default", "ephemeral": false, "stateful": false, "profiles": ["default"],
        "config": {"volatile.base_image": "ab"}, "devices": null, "expanded_config": {}, "expanded_devices": {}})
}

/// The answer of each call of the crate.
pub async fn answer(shared: &Arc<Shared>, seen: Seen) -> Vec<u8> {
    let (method, path) = (seen.method.clone(), seen.path.clone());
    {
        let mut state = shared.state.lock().unwrap();
        state.requests.push(seen.clone());
        shared.arrived.notify_waiters();
        if let Some((m, p, status, words)) = &state.refuse
            && *m == method
            && path.starts_with(p.as_str())
        {
            return error(*status, words);
        }
    }
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match (method.as_str(), parts.as_slice()) {
        ("GET", ["1.0"]) => sync(server(&shared.state.lock().unwrap())),
        ("GET", ["1.0", "operations", id]) => match shared.state.lock().unwrap().ops.get(*id) {
            Some(op) => sync(op.json.clone()),
            None => error(404, "Operation not found"),
        },
        ("GET", ["1.0", "operations", id, "wait"]) => wait(shared, id, &seen).await,
        ("DELETE", ["1.0", "operations", id]) => {
            let mut state = shared.state.lock().unwrap();
            let Some(op) = state.ops.get_mut(*id) else { return error(404, "Operation not found") };
            op.json["status"] = "Canceled".into();
            op.json["status_code"] = 401.into();
            op.json["err"] = "Operation canceled".into();
            drop(state);
            shared.changed.notify_waiters();
            sync(json!({}))
        }
        ("POST", ["1.0", "images"]) => super::store::image(shared, &seen),
        ("POST", ["1.0", "images", "aliases"]) => {
            let body: Value = serde_json::from_slice(&seen.body).unwrap_or_default();
            let (name, target) =
                (body["name"].as_str().unwrap_or_default(), body["target"].as_str().unwrap_or_default());
            shared.state.lock().unwrap().aliases.push((name.to_string(), target.to_string()));
            sync(json!({}))
        }
        ("POST", ["1.0", "instances"]) => {
            let body: Value = serde_json::from_slice(&seen.body).unwrap_or_default();
            let Some(name) = body["name"].as_str() else { return error(400, "No name provided") };
            let kind = body["type"].as_str().unwrap_or("container");
            let status = if body["start"].as_bool() == Some(true) { "Running" } else { "Stopped" };
            shared.state.lock().unwrap().instances.insert(name.to_string(), instance_json(name, kind, status));
            run(shared, "task", json!({}), Value::Null)
        }
        ("GET", ["1.0", "instances"]) => {
            let state = shared.state.lock().unwrap();
            if seen.query.get("recursion").map(String::as_str) == Some("1") {
                sync(Value::Array(state.instances.values().cloned().collect()))
            } else {
                sync(state.instances.keys().map(|n| format!("/1.0/instances/{n}")).collect())
            }
        }
        ("GET", ["1.0", "instances", name]) => match shared.state.lock().unwrap().instances.get(*name) {
            Some(instance) => sync(instance.clone()),
            None => error(404, "Instance not found"),
        },
        ("DELETE", ["1.0", "instances", name]) => {
            let mut state = shared.state.lock().unwrap();
            match state.instances.get(*name).map(|i| i["status"] == "Running") {
                None => return error(404, "Instance not found"),
                Some(true) => return error(400, "Instance is running"),
                Some(false) => state.instances.remove(*name),
            };
            drop(state);
            run(shared, "task", json!({}), Value::Null)
        }
        ("PUT", ["1.0", "instances", name, "state"]) => {
            let body: Value = serde_json::from_slice(&seen.body).unwrap_or_default();
            let mut state = shared.state.lock().unwrap();
            let Some(instance) = state.instances.get_mut(*name) else { return error(404, "Instance not found") };
            let (status, code) = match body["action"].as_str() {
                Some("start") => ("Running", 103),
                Some("stop") => ("Stopped", 102),
                other => return error(400, &format!("Unknown action: '{}'", other.unwrap_or_default())),
            };
            instance["status"] = status.into();
            instance["status_code"] = code.into();
            drop(state);
            run(shared, "task", json!({}), Value::Null)
        }
        ("GET", ["1.0", "instances", _, "console"]) => {
            let console = shared.state.lock().unwrap().console.clone();
            http(200, &[("Content-Type", "application/octet-stream".into())], &console)
        }
        (_, ["1.0", "instances", name, "files"]) => super::store::file(shared, name, &seen),
        ("POST", ["1.0", "instances", name, "exec"]) => exec(shared, name, &seen),
        ("GET", ["1.0", "instances", _, "logs", "exec-output", _]) => {
            match shared.state.lock().unwrap().logs.get(&path) {
                Some(log) => http(200, &[("Content-Type", "application/octet-stream".into())], log),
                None => error(404, "Log file not found"),
            }
        }
        ("DELETE", ["1.0", "instances", _, "logs", "exec-output", _]) => {
            match shared.state.lock().unwrap().logs.remove(&path) {
                Some(_) => sync(json!({})),
                None => error(404, "Log file not found"),
            }
        }
        _ => error(404, "not found"),
    }
}

/// `GET /1.0/operations/ID/wait?timeout=N`: the state at the end of the
/// operation or at the limit.
async fn wait(shared: &Arc<Shared>, id: &str, seen: &Seen) -> Vec<u8> {
    let limit: i64 = seen.query.get("timeout").and_then(|t| t.parse().ok()).unwrap_or(-1);
    let deadline = (limit >= 0).then(|| tokio::time::Instant::now() + Duration::from_secs(limit as u64));
    loop {
        let changed = shared.changed.notified();
        let current = match shared.state.lock().unwrap().ops.get(id) {
            Some(op) => op.clone(),
            None => return error(404, "Operation not found"),
        };
        if current.done() || deadline.is_some_and(|d| tokio::time::Instant::now() >= d) {
            return sync(current.json);
        }
        match deadline {
            Some(deadline) => {
                let _ = tokio::time::timeout_at(deadline, changed).await;
            }
            None => changed.await,
        }
    }
}

fn exec(shared: &Arc<Shared>, name: &str, seen: &Seen) -> Vec<u8> {
    let body: Value = serde_json::from_slice(&seen.body).unwrap_or_default();
    let mut state = shared.state.lock().unwrap();
    if !state.instances.get(name).is_some_and(|i| i["status"] == "Running") {
        return error(400, "Instance is not running");
    }
    if body["record-output"] == true && body["wait-for-websocket"] == true {
        return error(400, "Cannot use \"record-output\" in combination with \"wait-for-websocket\"");
    }
    if body["wait-for-websocket"] == true {
        let id = state.new_op("websocket", json!({}));
        let fds: BTreeMap<&str, String> =
            ["0", "1", "2", "control"].into_iter().map(|fd| (fd, format!("{id}-secret-{fd}"))).collect();
        state.ops.get_mut(&id).unwrap().json["metadata"] = json!({"fds": fds, "command": body["command"],
            "environment": body["environment"], "interactive": false});
        return started(&state, &id);
    }
    let id = state.new_op("task", Value::Null);
    let plan = state.plan.clone();
    let logs = format!("/1.0/instances/{name}/logs/exec-output");
    let (out, err) = (format!("{logs}/exec_{id}.stdout"), format!("{logs}/exec_{id}.stderr"));
    let mut end = json!({"return": plan.exit});
    if body["record-output"] == true {
        state.logs.insert(out.clone(), plan.stdout);
        state.logs.insert(err.clone(), plan.stderr);
        end["output"] = json!({"1": out, "2": err});
    }
    if !state.hold {
        state.end_op(&id, end);
    }
    let answer = started(&state, &id);
    shared.changed.notify_waiters();
    answer
}
