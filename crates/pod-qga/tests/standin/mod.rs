//! A stand-in agent on a Unix socket in the temporary directory, with no
//! QEMU: it answers each call of the crate as `qemu-ga` does (the schema
//! `qga/qapi-schema.json` of QEMU), with a model of a guest: its files, its
//! handles and its commands. A byte 0xFF gets the parse error of the agent,
//! as `qemu-ga` writes it. A fault of a test takes the place of an answer.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

/// What the stand-in does with one request.
pub enum Reply {
    /// `{"return": VALUE}`
    Return(Value),
    /// `{"error": {"class": CLASS, "desc": DESC}}`
    Refuse(&'static str, String),
    /// These bytes, as they are.
    Raw(Vec<u8>),
    /// No answer.
    Silent,
    /// The end of the connection.
    Close,
}

/// The guest of the stand-in.
#[derive(Default)]
pub struct Guest {
    /// Each request, in order.
    pub seen: Vec<Value>,
    pub files: BTreeMap<String, Vec<u8>>,
    /// The handles that are open: the path and the read offset.
    pub open: BTreeMap<i64, (String, usize)>,
    /// The commands: the exec request and the count of the status asks.
    pub execs: BTreeMap<i64, (Value, usize)>,
    /// The asks of a status before a command ends.
    pub runs_for: usize,
    pub shutdown: Option<String>,
    next: i64,
}

fn arg<'a>(req: &'a Value, key: &str) -> &'a Value {
    &req["arguments"][key]
}

impl Guest {
    /// The names of the calls that the stand-in got.
    pub fn calls(&self) -> Vec<String> {
        self.seen.iter().map(|r| r["execute"].as_str().unwrap_or("").to_string()).collect()
    }

    fn answer(&mut self, req: &Value) -> Reply {
        self.next += 1;
        match req["execute"].as_str().unwrap_or("") {
            "guest-sync-delimited" => {
                let mut b = vec![0xFF];
                b.extend_from_slice(json!({ "return": arg(req, "id") }).to_string().as_bytes());
                b.push(b'\n');
                Reply::Raw(b)
            }
            "guest-ping" => Reply::Return(json!({})),
            "guest-exec" => {
                let pid = 4000 + self.next;
                self.execs.insert(pid, (req["arguments"].clone(), 0));
                Reply::Return(json!({ "pid": pid }))
            }
            "guest-exec-status" => {
                let pid = arg(req, "pid").as_i64().unwrap_or(-1);
                let runs_for = self.runs_for;
                let Some((exec, asked)) = self.execs.get_mut(&pid) else {
                    return Reply::Refuse("GenericError", format!("Invalid parameter 'pid' {pid}"));
                };
                *asked += 1;
                if *asked <= runs_for {
                    return Reply::Return(json!({ "exited": false }));
                }
                // The command echoes its arguments and its stdin.
                let mut out: Vec<u8> = exec["arg"]
                    .as_array()
                    .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "))
                    .unwrap_or_default()
                    .into_bytes();
                if let Some(i) = exec["input-data"].as_str() {
                    out.extend(STANDARD.decode(i).unwrap_or_default());
                }
                let exec = exec.clone();
                self.execs.remove(&pid);
                Reply::Return(json!({
                    "exited": true, "exitcode": 7,
                    "out-data": STANDARD.encode(&out), "err-data": STANDARD.encode(exec["path"].as_str().unwrap_or("")),
                }))
            }
            "guest-file-open" => {
                let path = arg(req, "path").as_str().unwrap_or("").to_string();
                match arg(req, "mode").as_str().unwrap_or("r") {
                    "w" => {
                        self.files.insert(path.clone(), Vec::new());
                    }
                    _ if !self.files.contains_key(&path) => {
                        return Reply::Refuse("GenericError", format!("failed to open file '{path}' (mode: 'r')"));
                    }
                    _ => {}
                }
                let h = 1000 + self.next;
                self.open.insert(h, (path, 0));
                Reply::Return(json!(h))
            }
            "guest-file-read" => {
                let h = arg(req, "handle").as_i64().unwrap_or(-1);
                let count = arg(req, "count").as_u64().unwrap_or(4096) as usize;
                let Some((path, at)) = self.open.get_mut(&h) else {
                    return Reply::Refuse("GenericError", format!("handle '{h}' has not been found"));
                };
                let data = &self.files[path.as_str()];
                let end = (*at + count).min(data.len());
                let part = data[*at..end].to_vec();
                *at = end;
                Reply::Return(json!({
                    "count": part.len(), "buf-b64": STANDARD.encode(&part), "eof": end == data.len(),
                }))
            }
            "guest-file-write" => {
                let h = arg(req, "handle").as_i64().unwrap_or(-1);
                let Some((path, _)) = self.open.get(&h) else {
                    return Reply::Refuse("GenericError", format!("handle '{h}' has not been found"));
                };
                let data = STANDARD.decode(arg(req, "buf-b64").as_str().unwrap_or("")).unwrap_or_default();
                self.files.get_mut(path.as_str()).expect("an open file").extend_from_slice(&data);
                Reply::Return(json!({ "count": data.len(), "eof": false }))
            }
            "guest-file-close" => {
                let h = arg(req, "handle").as_i64().unwrap_or(-1);
                match self.open.remove(&h) {
                    Some(_) => Reply::Return(json!({})),
                    None => Reply::Refuse("GenericError", format!("handle '{h}' has not been found")),
                }
            }
            "guest-shutdown" => {
                self.shutdown = Some(arg(req, "mode").as_str().unwrap_or("powerdown").to_string());
                Reply::Close
            }
            other => Reply::Refuse("CommandNotFound", format!("The command {other} has not been found")),
        }
    }
}

/// A fault of a test: `Some` takes the place of the answer of the guest.
pub type Fault = Box<dyn FnMut(&Value) -> Option<Reply> + Send>;

/// A running stand-in.
pub struct Agent {
    pub path: PathBuf,
    pub guest: Arc<Mutex<Guest>>,
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

impl Agent {
    /// A stand-in that writes `prelude` on each connection before it reads
    /// (old answers of an earlier client), and asks `fault` first.
    pub fn start(name: &str, prelude: &[u8], fault: Fault) -> Agent {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let path = pod_testkit::host::socket_path(&format!("pod-qga-{name}-{}-{n}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).expect("bind the stand-in");
        let guest = Arc::new(Mutex::new(Guest::default()));
        let (g, prelude) = (guest.clone(), prelude.to_vec());
        let fault = Arc::new(Mutex::new(fault));
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(conn) = conn else { return };
                serve(conn, &prelude, &g, &fault);
            }
        });
        Agent { path, guest }
    }

    /// A stand-in with no fault and no old answer.
    pub fn plain(name: &str) -> Agent {
        Agent::start(name, b"", Box::new(|_| None))
    }

    pub fn settings(&self) -> pod_qga::Settings {
        pod_qga::Settings::new(&self.path)
    }
}

fn serve(conn: UnixStream, prelude: &[u8], guest: &Mutex<Guest>, fault: &Mutex<Fault>) {
    let mut w = conn.try_clone().expect("clone the connection");
    if w.write_all(prelude).is_err() {
        return;
    }
    let mut r = BufReader::new(conn);
    loop {
        let mut line = Vec::new();
        match r.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        // Each 0xFF resets the parser of qemu-ga, which answers the bad
        // byte with an error.
        let mut at = 0;
        while line.get(at) == Some(&0xFF) {
            at += 1;
            let e = br#"{"error": {"class": "GenericError", "desc": "JSON parse error, invalid byte"}}"#;
            if w.write_all(e).and_then(|()| w.write_all(b"\n")).is_err() {
                return;
            }
        }
        let Ok(req) = serde_json::from_slice::<Value>(&line[at..]) else {
            continue;
        };
        guest.lock().unwrap().seen.push(req.clone());
        let reply = (fault.lock().unwrap())(&req).unwrap_or_else(|| guest.lock().unwrap().answer(&req));
        let bytes = match reply {
            Reply::Return(v) => format!("{}\n", json!({ "return": v })).into_bytes(),
            Reply::Refuse(class, desc) => {
                format!("{}\n", json!({ "error": { "class": class, "desc": desc } })).into_bytes()
            }
            Reply::Raw(b) => b,
            Reply::Silent => continue,
            Reply::Close => return,
        };
        if w.write_all(&bytes).is_err() {
            return;
        }
    }
}
