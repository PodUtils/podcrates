//! What the host implements: the delegated methods of the catalogue, and
//! the context of one request, with the calls back to the client.

use std::collections::BTreeMap;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::catalogue::Tier;
use crate::wire::{Code, Fault, Wiped};

/// The answer of the host to one request: the result, or an error that the
/// client gets.
pub type Outcome = Result<Value, Fault>;

/// The params of `drain`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drain {
    /// Exit after the last work in flight ends. Without it, the program
    /// keeps running and takes no new work.
    #[serde(default = "yes")]
    pub exit: bool,
}

fn yes() -> bool {
    true
}

impl Default for Drain {
    fn default() -> Drain {
        Drain { exit: true }
    }
}

/// The result of `metrics`: counters that only grow, and gauges.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metrics {
    pub counters: BTreeMap<String, u64>,
    pub gauges: BTreeMap<String, i64>,
}

impl Metrics {
    /// The text format of Prometheus, each name after `prefix` and `_`; a
    /// byte of a name that Prometheus does not take becomes `_`.
    pub fn prometheus(&self, prefix: &str) -> String {
        let clean = |name: &str| -> String {
            name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect()
        };
        let mut out = String::new();
        for (name, value) in &self.counters {
            let name = format!("{}_{}", clean(prefix), clean(name));
            out.push_str(&format!("# TYPE {name} counter\n{name} {value}\n"));
        }
        for (name, value) in &self.gauges {
            let name = format!("{}_{}", clean(prefix), clean(name));
            out.push_str(&format!("# TYPE {name} gauge\n{name} {value}\n"));
        }
        out
    }
}

/// Who is at the other end of a connection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Peer {
    /// The effective uid (Unix).
    pub uid: Option<u32>,
    /// The process id, where the system gives it.
    pub pid: Option<u32>,
    /// The SID of the user, as its string (Windows).
    pub sid: Option<String>,
}

/// Why a call back to the client gave no result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallFault {
    /// The budget of the request or of the connection is spent: the request
    /// answers `budget`.
    Budget,
    /// The client answered with an error.
    Remote(Fault),
    /// The connection or the request ended, or the client broke the frame.
    Gone,
}

impl std::fmt::Display for CallFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallFault::Budget => f.write_str("the budget of calls back is spent"),
            CallFault::Remote(fault) => write!(f, "the client answered {fault}"),
            CallFault::Gone => f.write_str("the request or its connection ended"),
        }
    }
}

impl std::error::Error for CallFault {}

/// A message from the host side to the thread of a connection. A value in
/// it is wiped at its drop: also in a send that fails, and in a channel that
/// ends with the message unread.
pub(crate) enum ToConn {
    Answer(Result<Wiped, Fault>),
    Back { method: String, params: Wiped, reply: Sender<Result<Wiped, CallFault>> },
}

/// The context of one request: the peer, its tier, and the calls back.
pub struct Context {
    pub peer: Peer,
    pub tier: Tier,
    back: Option<Sender<ToConn>>,
    wait: Duration,
}

impl Context {
    pub(crate) fn new(peer: Peer, tier: Tier, back: Sender<ToConn>, wait: Duration) -> Context {
        Context { peer, tier, back: Some(back), wait }
    }

    /// A context with no connection, for a host that answers a request of
    /// its own (a signal, a test): each call back is [`CallFault::Gone`].
    pub fn detached(peer: Peer, tier: Tier) -> Context {
        Context { peer, tier, back: None, wait: Duration::ZERO }
    }

    /// Call `method` of the client on the connection of this request, and
    /// wait for its answer. The budget of calls back of the request and of
    /// its connection holds: past it, each call is [`CallFault::Budget`].
    /// The params are wiped on each path; an answer that comes after the
    /// wait is wiped at its drop.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, CallFault> {
        let params = Wiped(params);
        let back = self.back.as_ref().ok_or(CallFault::Gone)?;
        let (reply, answer) = mpsc::channel();
        back.send(ToConn::Back { method: method.to_string(), params, reply }).map_err(|_| CallFault::Gone)?;
        answer.recv_timeout(self.wait).map_or(Err(CallFault::Gone), |result| result.map(Wiped::take))
    }
}

/// What a program implements: the delegated methods of the catalogue.
/// `version`, `methods` and `auth` are local: the server answers them.
pub trait Host {
    /// The state of the program now: a JSON object of its own schema.
    fn status(&mut self, cx: &mut Context) -> Outcome;

    fn metrics(&mut self, cx: &mut Context) -> Result<Metrics, Fault>;

    /// Read the configuration again and use it.
    fn reload(&mut self, cx: &mut Context) -> Outcome;

    /// Take no new work and let the work in flight end; exit after it with
    /// [`Drain::exit`].
    fn drain(&mut self, drain: Drain, cx: &mut Context) -> Outcome;

    /// Stop now: end the work in flight by the rule of the program, and exit.
    fn stop(&mut self, cx: &mut Context) -> Outcome;

    /// A method of [`crate::Extra`]; its tier is already checked.
    fn extra(&mut self, name: &str, params: &Value, cx: &mut Context) -> Outcome {
        let _ = (name, params, cx);
        Err(Fault::new(Code::Unsupported, "the program has no such extra method"))
    }
}
