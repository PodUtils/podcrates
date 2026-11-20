//! The control of a broker on `pod-control`, protocol 1: JSON lines with
//! `"schema": 1`, an id in each request and its answer, `ok`, and one
//! answer for a name that is not there or above the tier of the caller.
//! The catalogue of the broker adds three rows ([`extras`]) to the rows of
//! `pod-control`:
//!
//! ```text
//! {"schema":1,"id":1,"method":"auth","params":{"token":"MASTER"}}
//! {"schema":1,"id":1,"ok":true,"result":{"tier":"master"}}
//! {"schema":1,"id":2,"method":"load","params":{"job":"J","token":"T","ttl_s":600,"policy":{"schema":1,"mode":"open"}}}
//! {"schema":1,"id":2,"ok":true,"result":{"job":"J","mode":"open","rules":0,"expires_in_s":600}}
//! {"schema":1,"id":3,"method":"drop","params":{"job":"J"}}
//! {"schema":1,"id":3,"ok":true,"result":{"job":"J","dropped":true}}
//! {"schema":1,"id":4,"method":"list"}
//! {"schema":1,"id":4,"ok":true,"result":{"jobs":[{"job":"J","mode":"open","rules":0,"expires_in_s":590,"open":1}]}}
//! {"schema":1,"id":5,"ok":false,"error":{"code":"bad-params","detail":"..."}}
//! ```
//!
//! `load` has the master tier: a job token and its secrets are a new
//! principal, whose effect outlives the token of the caller. `drop` has the
//! operate tier and `list` the read tier. An answer never holds a token or
//! the value of a secret.
//!
//! A host with a control socket of its own adds [`extras`] to its catalogue
//! and answers them with [`Broker::control`]. A host with no such socket
//! serves [`Control`] with [`serve_control`] (Unix).

use std::sync::Arc;
use std::time::Duration;

use pod_control::{Catalogue, Code, Context, Drain, Extra, Fault, Host, Metrics, Outcome, Settings, Tier};
use pod_core::Class;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::watch;

use crate::broker::Broker;
use crate::credentials::Secrets;
use crate::jobs::{JobInfo, Load, Token};
use crate::policy::Policy;

/// The longest request line of the control: 1 MiB, for a policy with many
/// rules and the values of its secrets.
pub const MAX_LINE: usize = 1 << 20;

/// The rows of the broker in a catalogue of `pod-control`: `load` (master,
/// its effect outlives the token), `drop` (operate) and `list` (read).
pub fn extras() -> Vec<Extra> {
    let load = "Load a job: its id, its token, its TTL in seconds, its policy and the values of its secrets.";
    vec![
        Extra { outlives: true, ..Extra::new("load", Tier::Master, load) },
        Extra::new("drop", Tier::Operate, "End a job and each of its connections."),
        Extra::new("list", Tier::Read, "Each job with its mode, its rules, its TTL and its open connections."),
    ]
}

/// The catalogue of a broker: the rows of `pod-control` and [`extras`].
pub fn catalogue() -> Catalogue {
    Catalogue::new(extras()).expect("the rows of the broker are names of the wire, once each, with their tiers")
}

/// The settings of a control of a broker: those of `pod-control`, with a
/// line of [`MAX_LINE`]. The peer of the user of the broker has the operate
/// tier; `load` takes `auth` with [`Settings::master`], or a host that sets
/// [`Settings::same_user`] to [`Tier::Master`].
pub fn settings(program: &str, version: &str) -> Settings {
    Settings { line_max: MAX_LINE, ..Settings::new(program, version) }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoadParams {
    job: String,
    token: Token,
    ttl_s: u64,
    policy: Value,
    #[serde(default)]
    secrets: Secrets,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DropParams {
    job: String,
}

/// The params of `method` as `T`. The words of serde can quote a value,
/// and a value can be a token: only the names of the fields go back.
fn params<'a, T: Deserialize<'a>>(method: &str, params: &'a Value) -> Result<T, Fault> {
    T::deserialize(params).map_err(|e| {
        let text = e.to_string();
        let named = text.starts_with("missing field") || text.starts_with("unknown field");
        let words = if named { text } else { "a value has the wrong type or form".to_string() };
        Fault::new(Code::BadParams, format!("the params of {method}: {words}"))
    })
}

fn info(job: &JobInfo) -> Value {
    let mut info = json!({
        "job": job.job,
        "mode": job.mode.word(),
        "rules": job.rules,
        "expires_in_s": job.expires_in.as_secs(),
        "open": job.open,
    });
    // Only when a rule is flagged: the host of each allow rule under an
    // open suffix (T-161).
    if !job.flagged.is_empty() {
        info["flagged"] = json!(job.flagged);
    }
    info
}

/// A failure of the broker as a fault of the wire: a bad request is
/// `bad-params`, a refusal `refused`, each other failure `failed`.
fn fault(e: &pod_core::Failure) -> Fault {
    let code = match e.class {
        Class::Usage => Code::BadParams,
        Class::Refused => Code::Refused,
        _ => Code::Failed,
    };
    Fault::new(code, e.text())
}

impl Broker {
    /// The answer to `load`, `drop` or `list` of [`extras`]. The server of
    /// the control checks the tier of the caller before this call; another
    /// name is `unknown-method`.
    pub fn control(&self, method: &str, params_value: &Value) -> Outcome {
        match method {
            "load" => {
                let p: LoadParams = params(method, params_value)?;
                let policy = Policy::from_value(p.policy).map_err(|e| fault(&e))?;
                let load =
                    Load { job: p.job, token: p.token, ttl: Duration::from_secs(p.ttl_s), policy, secrets: p.secrets };
                let job = self.load(load).map_err(|e| fault(&e))?;
                let mut answer = info(&job);
                answer.as_object_mut().map(|o| o.remove("open"));
                if let Some(pem) = &job.ca_pem {
                    answer["ca_pem"] = json!(pem);
                }
                Ok(answer)
            }
            "drop" => {
                let DropParams { job } = params(method, params_value)?;
                let dropped = self.drop_job(&job);
                Ok(json!({ "job": job, "dropped": dropped }))
            }
            "list" => {
                if !(params_value.is_null() || params_value.as_object().is_some_and(serde_json::Map::is_empty)) {
                    return Err(Fault::new(Code::BadParams, "the params of list: list takes none"));
                }
                Ok(json!({ "jobs": self.list().iter().map(info).collect::<Vec<_>>() }))
            }
            _ => Err(Fault::unknown_method()),
        }
    }
}

/// What the control asked of the host of the broker. The crate does not
/// exit: the host reads it ([`Control::ending`]) and ends by its own rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// `drain`: the control takes no new `load`; each job ends at its drop
    /// or its TTL. With `exit`, the host exits after the last job.
    Drain { exit: bool },
    /// `stop`: each job and each of its connections ended; the host exits.
    Stop,
}

impl Ending {
    /// The word of `status`: `drain`, `drain-exit` or `stop`.
    pub fn word(self) -> &'static str {
        match self {
            Ending::Drain { exit: false } => "drain",
            Ending::Drain { exit: true } => "drain-exit",
            Ending::Stop => "stop",
        }
    }
}

/// The host of `pod-control` for a broker: `status` gives the jobs,
/// `metrics` the counts of the audit lines ([`Broker::counts`]) and the jobs
/// and connections open now, and the rows of [`extras`] go to
/// [`Broker::control`].
pub struct Control {
    broker: Arc<Broker>,
    ending: watch::Sender<Option<Ending>>,
}

impl Control {
    pub fn new(broker: Arc<Broker>) -> Control {
        Control { broker, ending: watch::Sender::new(None) }
    }

    /// The last `drain` or `stop`, now and at each change.
    pub fn ending(&self) -> watch::Receiver<Option<Ending>> {
        self.ending.subscribe()
    }

    fn jobs(&self) -> Value {
        json!(self.broker.list().iter().map(info).collect::<Vec<_>>())
    }
}

impl Host for Control {
    fn status(&mut self, _: &mut Context) -> Outcome {
        let ending = self.ending.borrow().map(Ending::word);
        Ok(json!({ "jobs": self.jobs(), "ending": ending }))
    }

    fn metrics(&mut self, _: &mut Context) -> Result<Metrics, Fault> {
        let jobs = self.broker.list();
        let open: usize = jobs.iter().map(|j| j.open).sum();
        let mut metrics = Metrics { counters: self.broker.counts(), ..Metrics::default() };
        metrics.gauges.insert("jobs".into(), i64::try_from(jobs.len()).unwrap_or(i64::MAX));
        metrics.gauges.insert("open".into(), i64::try_from(open).unwrap_or(i64::MAX));
        Ok(metrics)
    }

    fn reload(&mut self, _: &mut Context) -> Outcome {
        Err(Fault::new(Code::Unsupported, "the broker reads no configuration: its host reads it"))
    }

    fn drain(&mut self, drain: Drain, _: &mut Context) -> Outcome {
        self.ending.send_replace(Some(Ending::Drain { exit: drain.exit }));
        Ok(json!({ "draining": true, "exit": drain.exit, "jobs": self.broker.list().len() }))
    }

    fn stop(&mut self, _: &mut Context) -> Outcome {
        self.ending.send_replace(Some(Ending::Stop));
        let dropped = self.broker.list().iter().filter(|j| self.broker.drop_job(&j.job)).count();
        Ok(json!({ "stopping": true, "dropped": dropped }))
    }

    fn extra(&mut self, name: &str, params: &Value, _: &mut Context) -> Outcome {
        if name == "load" && self.ending.borrow().is_some() {
            return Err(Fault::new(Code::Busy, "the broker is draining: it takes no new job"));
        }
        self.broker.control(name, params)
    }
}

#[cfg(unix)]
pub use unix::{ControlServer, serve_control};

#[cfg(unix)]
mod unix {
    use std::path::Path;

    use pod_control::unix::Server;

    use super::*;

    /// A control of a broker that serves. Its drop stops the accept of new
    /// connections and removes its socket.
    pub struct ControlServer {
        server: Server,
        ending: watch::Receiver<Option<Ending>>,
    }

    impl ControlServer {
        pub fn server(&self) -> &Server {
            &self.server
        }

        /// The last `drain` or `stop` of a client ([`Control::ending`]).
        pub fn ending(&self) -> watch::Receiver<Option<Ending>> {
            self.ending.clone()
        }
    }

    /// Bind `path` with [`Settings::mode`] (0600) and serve the control of
    /// `broker` there with the server of `pod-control`: std threads, and one
    /// thread that answers each request in order. A peer of another user is
    /// refused unless [`Settings::uids`] names it. Call it inside a runtime of
    /// tokio: the thread of the answers enters it, so that a `load` starts the
    /// timer of its TTL. A live socket at `path` is `AddrInUse`, a stale one
    /// is replaced (`pod_control::unix::bind`).
    pub fn serve_control(broker: Arc<Broker>, path: &Path, settings: Settings) -> std::io::Result<ControlServer> {
        let runtime = tokio::runtime::Handle::try_current().ok();
        let (server, incoming) = pod_control::unix::serve(path, settings, catalogue())?;
        let mut control = Control::new(broker);
        let ending = control.ending();
        std::thread::Builder::new().name("pod-broker-control".into()).spawn(move || {
            let _entered = runtime.as_ref().map(tokio::runtime::Handle::enter);
            // Ends when the server and each of its connections end.
            incoming.serve(&mut control);
        })?;
        Ok(ControlServer { server, ending })
    }
}
