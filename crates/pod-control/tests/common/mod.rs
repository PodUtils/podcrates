//! A host for the tests, and a server of it on a socket in a new directory.

#![allow(dead_code)]

#[cfg(unix)]
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pod_control::{Catalogue, Context, Drain, Extra, Fault, Host, Metrics, Outcome, Settings, Tier};
use serde_json::{Value, json};

/// What the host was asked, in order.
pub type Log = Arc<Mutex<Vec<String>>>;

pub struct Recorder {
    pub log: Log,
}

impl Host for Recorder {
    fn status(&mut self, cx: &mut Context) -> Outcome {
        self.log.lock().unwrap().push(format!("status {}", cx.tier));
        Ok(
            json!({ "schema": 1, "slots": [{ "name": "a", "boundary": "microvm", "hold": null, "principal": "shared" }] }),
        )
    }

    fn metrics(&mut self, _: &mut Context) -> Result<Metrics, Fault> {
        self.log.lock().unwrap().push("metrics".into());
        let mut m = Metrics::default();
        m.counters.insert("jobs_done".into(), 3);
        m.gauges.insert("jobs_running".into(), 1);
        Ok(m)
    }

    fn reload(&mut self, _: &mut Context) -> Outcome {
        self.log.lock().unwrap().push("reload".into());
        Ok(json!("reloaded"))
    }

    fn drain(&mut self, drain: Drain, _: &mut Context) -> Outcome {
        self.log.lock().unwrap().push(format!("drain exit={}", drain.exit));
        Ok(json!({ "draining": true, "in_flight": 2 }))
    }

    fn stop(&mut self, _: &mut Context) -> Outcome {
        self.log.lock().unwrap().push("stop".into());
        Ok(json!("stopping"))
    }

    /// `ask` calls the client back `n` times, each call with `pad` bytes,
    /// and gives the count of the answers and the first fault.
    fn extra(&mut self, name: &str, params: &Value, cx: &mut Context) -> Outcome {
        self.log.lock().unwrap().push(name.to_string());
        let n = params["n"].as_u64().unwrap_or(1);
        let pad = "x".repeat(params["pad"].as_u64().unwrap_or(0) as usize);
        let mut answered = 0;
        for i in 0..n {
            match cx.call("confirm", json!({ "i": i, "pad": pad })) {
                Ok(mut answer) => {
                    pod_control::wire::wipe(&mut answer);
                    answered += 1;
                }
                Err(e) => return Ok(json!({ "answered": answered, "fault": e.to_string() })),
            }
        }
        Ok(json!({ "answered": answered }))
    }
}

pub fn catalogue() -> Catalogue {
    Catalogue::new(vec![Extra::new("ask", Tier::Read, "Call the client back n times.")]).unwrap()
}

/// A new directory for the socket of one test.
#[cfg(unix)]
pub fn dir(name: &str) -> PathBuf {
    let d = pod_testkit::host::socket_dir(&format!("pod-control-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A server of a [`Recorder`] at `DIR/ctl.sock`, its host in a thread of
/// its own.
#[cfg(unix)]
pub fn start(name: &str, settings: Settings) -> (pod_control::unix::Server, PathBuf, Log) {
    let path = dir(name).join("ctl.sock");
    let (server, incoming) = pod_control::unix::serve(&path, settings, catalogue()).unwrap();
    let log = Log::default();
    let mut host = Recorder { log: log.clone() };
    std::thread::spawn(move || incoming.serve(&mut host));
    (server, path, log)
}

pub fn settings() -> Settings {
    Settings::new("pod-control-test", "1.0.0")
}
