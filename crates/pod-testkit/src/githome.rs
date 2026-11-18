//! A fake of githome's source of jobs for the CI of podrunner (feature
//! `githome`; the binary `pod-fake-githome`). It has two parts:
//!
//! * [`FakeGithome`] (`pod-fake-githome serve`): the state on the loopback.
//!   It holds the queued jobs, the claims, the minted credentials, the
//!   reports, the refusals, the withdrawals and the audit lines. The
//!   routes `/_runner/source/OP` answer each operation of the source wire
//!   for one session; the control API of the test is under `/_fake/`
//!   (module `control`).
//! * [`serve_lines`] (`pod-fake-githome source`): the program of a
//!   `[[source]]` of the kind `program`. It reads the requests of
//!   podrunner on stdin and writes the replies on stdout, by
//!   `pod_job::source`. Each start of the program is a new session: a
//!   session is one host of podrunner.
//!
//! The rules of each operation are in module `wire`. A queued job is
//! offered to each session whose host facts it fits, until a session claims
//! it; the claim decides, as `already-claimed` tells. Each credential is
//! made up.

mod control;
mod offer;
mod program;
mod wire;

use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::net::TcpListener;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use pod_job::source::{Event, HostFacts, SourceCode};
use pod_job::{JobId, JobOffer};
use serde_json::{Map, Value, json};

use crate::fake::{body, json_answer};
use crate::stand::{Answer, Request, Stand};
pub use program::serve_lines;

/// The environment variable that gives `pod-fake-githome source` the base
/// URL of `pod-fake-githome serve`.
pub const SERVER_VAR: &str = "POD_FAKE_GITHOME";
/// The header of the session of a call to `/_runner/source/`.
pub const SESSION_HEADER: &str = "X-Pod-Fake-Session";
/// The name of the source in `hello` until the control API sets another.
pub const SOURCE_NAME: &str = "githome";
/// The longest wait of `next`: podrunner kills a source child that gives
/// no reply in 15 s after its long poll.
pub const WAIT_MAX: Duration = Duration::from_secs(10);

/// The names of the records that the control API reads, each one at
/// `GET /_fake/NAME`.
pub const LOGS: [&str; 8] =
    ["reports", "refusals", "withdrawals", "releases", "audits", "mints", "assignments", "hosts"];

/// A running fake of githome's source.
pub struct FakeGithome {
    stand: Stand,
    shared: Arc<Shared>,
}

/// What the routes share.
pub(crate) struct Shared {
    /// `http://ADDR/`.
    base: String,
    state: Mutex<State>,
    /// A change of the jobs or of the events: a long poll looks again.
    changed: Condvar,
}

pub(crate) struct State {
    source: String,
    operator_controlled: bool,
    /// Each operation of the wire is `unavailable`.
    unavailable: bool,
    /// Each `mint` is `source-revoked`.
    revoked: bool,
    jobs: Vec<Job>,
    sessions: Vec<Session>,
    /// The last id of a job, a session or a mint.
    last_id: u64,
    /// The records of [`LOGS`], in order.
    logs: BTreeMap<&'static str, Vec<Value>>,
}

/// Where a job is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    Queued,
    Claimed,
    /// Claimed, with a report: `release` ends it.
    Reported,
    Done,
    Cancelled,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Queued => "queued",
            Status::Claimed => "claimed",
            Status::Reported => "reported",
            Status::Done => "done",
            Status::Cancelled => "cancelled",
        }
    }
}

pub(crate) struct Job {
    offer: JobOffer,
    status: Status,
    /// The session and the runner name of the claim.
    claim: Option<(u64, String)>,
    /// The last report of the claim.
    report: Option<Map<String, Value>>,
    minted: bool,
    /// The next `mint` fails with this code.
    mint_error: Option<SourceCode>,
    /// The sessions that refused the job: they never get it again.
    refused_by: Vec<u64>,
}

impl Job {
    fn id(&self) -> &JobId {
        &self.offer.job_id
    }

    /// Whether the session `id` holds the claim of the job.
    fn claimed_by(&self, id: u64) -> bool {
        matches!(self.status, Status::Claimed | Status::Reported) && self.claim.as_ref().is_some_and(|c| c.0 == id)
    }
}

/// One start of the program: one host of podrunner.
pub(crate) struct Session {
    id: u64,
    host: Option<HostFacts>,
    /// The jobs that `next` offered to the session and that it did not
    /// withdraw or release, with the last step: `offered`, `acked`,
    /// `claimed`.
    held: BTreeMap<JobId, &'static str>,
    /// The events of the control API for the next `next`.
    events: VecDeque<Event>,
}

impl FakeGithome {
    /// A fake on a free port of 127.0.0.1.
    pub fn start() -> io::Result<FakeGithome> {
        FakeGithome::start_on(TcpListener::bind("127.0.0.1:0")?)
    }

    /// A fake on the caller's listener.
    pub fn start_on(listener: TcpListener) -> io::Result<FakeGithome> {
        let base = format!("http://{}/", listener.local_addr()?);
        let state = State {
            source: SOURCE_NAME.to_string(),
            operator_controlled: true,
            unavailable: false,
            revoked: false,
            jobs: Vec::new(),
            sessions: Vec::new(),
            last_id: 0,
            logs: LOGS.iter().map(|name| (*name, Vec::new())).collect(),
        };
        let shared = Arc::new(Shared { base, state: Mutex::new(state), changed: Condvar::new() });
        let routes = shared.clone();
        let stand = Stand::http_on(listener, move |req| route(&routes, req))?;
        Ok(FakeGithome { stand, shared })
    }

    /// The base URL, `http://127.0.0.1:PORT/`: the value of [`SERVER_VAR`].
    pub fn base(&self) -> &str {
        &self.shared.base
    }

    /// The HTTP stand-in under the fake.
    pub fn stand(&self) -> &Stand {
        &self.stand
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl State {
    fn next_id(&mut self) -> u64 {
        self.last_id += 1;
        self.last_id
    }

    fn log(&mut self, name: &'static str, value: Value) {
        self.logs.entry(name).or_default().push(value);
    }

    fn job(&mut self, id: &JobId) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|j| j.id() == id)
    }

    fn session(&mut self, id: u64) -> Option<&mut Session> {
        self.sessions.iter_mut().find(|s| s.id == id)
    }

    fn new_session(&mut self) -> u64 {
        let id = self.next_id();
        self.sessions.push(Session { id, host: None, held: BTreeMap::new(), events: VecDeque::new() });
        id
    }
}

/// Each request: the wire of a session, the control API.
fn route(shared: &Shared, req: &Request) -> Answer {
    let path = req.path().to_string();
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    match parts.as_slice() {
        ["_runner", "source", "session"] if req.method == "POST" => {
            json_answer(201, &json!({ "session": shared.lock().new_session() }))
        }
        ["_runner", "source", op] if req.method == "POST" => wire::route(shared, req, op),
        ["_fake", rest @ ..] => control::route(shared, req, rest),
        _ => error(404, "no such route"),
    }
}

/// An error of the routes: `{"message": TEXT}`.
fn error(status: u16, message: &str) -> Answer {
    json_answer(status, &json!({ "message": message }))
}

/// The session of the header [`SESSION_HEADER`] of a call, when the fake
/// knows it.
fn session_of(shared: &Shared, req: &Request) -> Option<u64> {
    let id = req.header(SESSION_HEADER)?.trim().parse().ok()?;
    shared.lock().sessions.iter().any(|s| s.id == id).then_some(id)
}
