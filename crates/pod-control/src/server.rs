//! The server side of one connection, for each transport: the admission of
//! the peer, the frame, the dispatch and the calls back with their budgets.
//! The host answers in its own thread, from [`Incoming`].

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use zeroize::Zeroizing;

use crate::catalogue::{Catalogue, Tier};
use crate::dispatch::{self, Session, Target};
use crate::frame::{Line, Lines, Stream};
use crate::host::{CallFault, Context, Host, Outcome, Peer, ToConn};
use crate::wire::{self, Code, Fault, Inbound, Request, Wiped};

/// A secret that raises a connection to the master tier. It is compared in
/// constant time, its `Debug` shows no byte of it, and its text is wiped at
/// its drop.
#[derive(Clone)]
pub struct Token(Zeroizing<String>);

impl Token {
    pub fn new(text: impl Into<String>) -> Token {
        Token(Zeroizing::new(text.into()))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn matches(&self, given: &str) -> bool {
        pod_core::entropy::same_secret(self.0.as_bytes(), given.as_bytes())
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(..)")
    }
}

/// The settings of a server.
#[derive(Debug, Clone)]
pub struct Settings {
    /// The name of the program, for `version`.
    pub program: String,
    /// The version of the program, for `version`.
    pub version: String,
    /// The longest request line, without its line end.
    pub line_max: usize,
    /// The longest wait for one whole line of the client: a request. A
    /// connection with no line in this time closes.
    pub read_timeout: Duration,
    /// The longest wait to write one whole line: an answer. A call back has
    /// one deadline, `write_timeout + read_timeout`, for its line and the
    /// answer of the client, blank lines too.
    pub write_timeout: Duration,
    /// The longest wait for the answer of the host. Past it, `status` gets
    /// the last snapshot (`Server::snapshot`), and another method `busy`.
    pub answer_wait: Duration,
    /// The connections served at one time; one more gets `busy`.
    pub clients: usize,
    /// The calls back that one request can make.
    pub callbacks_per_request: usize,
    /// The calls back of all the requests of one connection.
    pub callbacks_per_connection: usize,
    /// The tier of a peer of the user of the server.
    pub same_user: Tier,
    /// Other uids that may connect, each with its tier (Unix; the mode of the
    /// socket must let them in).
    pub uids: Vec<(u32, Tier)>,
    /// The mode of the socket (Unix).
    pub mode: u32,
    /// The master token of `auth`; with none, no caller has the master tier.
    pub master: Option<Token>,
}

/// The longest request line of [`Settings::new`]: 64 KiB.
pub const LINE_MAX: usize = 64 * 1024;
/// The calls back of one request of [`Settings::new`].
pub const CALLBACKS_PER_REQUEST: usize = 16;
/// The calls back of one connection of [`Settings::new`].
pub const CALLBACKS_PER_CONNECTION: usize = 256;

impl Settings {
    pub fn new(program: impl Into<String>, version: impl Into<String>) -> Settings {
        Settings {
            program: program.into(),
            version: version.into(),
            line_max: LINE_MAX,
            read_timeout: Duration::from_secs(5),
            write_timeout: Duration::from_secs(5),
            answer_wait: Duration::from_secs(5),
            clients: 8,
            callbacks_per_request: CALLBACKS_PER_REQUEST,
            callbacks_per_connection: CALLBACKS_PER_CONNECTION,
            same_user: Tier::Operate,
            uids: Vec::new(),
            mode: 0o600,
            master: None,
        }
    }
}

/// The last result of `status`, which a `status` gets while the host is
/// busy past [`Settings::answer_wait`].
#[derive(Debug, Clone, Default)]
pub struct Snapshot(Arc<Mutex<Option<Value>>>);

impl Snapshot {
    pub fn set(&self, status: Value) {
        if let Ok(mut s) = self.0.lock() {
            *s = Some(status);
        }
    }

    pub fn get(&self) -> Option<Value> {
        self.0.lock().ok().and_then(|s| s.clone())
    }
}

/// What the threads of one server share.
pub(crate) struct Shared {
    pub settings: Settings,
    pub catalogue: Catalogue,
    pub snapshot: Snapshot,
    pub active: AtomicUsize,
    pub closed: AtomicBool,
}

/// A delegated request for the host. Its params are wiped at its drop, also
/// when the host drops it with no answer.
pub struct Call {
    target: Target,
    method: String,
    params: Value,
    cx: Context,
    reply: Sender<ToConn>,
}

impl Call {
    /// The wire name of the method.
    pub fn method(&self) -> &str {
        &self.method
    }

    /// Answer the call with `host`. The answer is wiped at its drop: also
    /// when the connection stopped its wait (`busy`) and does not read it.
    pub fn answer(mut self, host: &mut dyn Host) {
        let outcome = dispatch::delegate(&self.target, host, &self.params, &mut self.cx);
        let _ = self.reply.send(ToConn::Answer(outcome.map(Wiped)));
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        wire::wipe(&mut self.params);
    }
}

/// The delegated requests of a server, for the thread of the host.
pub struct Incoming(pub(crate) Receiver<Call>);

impl Incoming {
    /// Answer each request that waits now with `host`, after a wait of at
    /// most `wait` for the first one. Gives the count answered.
    pub fn answer_waiting(&self, host: &mut dyn Host, wait: Duration) -> usize {
        let Ok(first) = self.0.recv_timeout(wait) else { return 0 };
        first.answer(host);
        let mut n = 1;
        while let Ok(call) = self.0.try_recv() {
            call.answer(host);
            n += 1;
        }
        n
    }

    /// Answer each request with `host` until the server closes.
    pub fn serve(&self, host: &mut dyn Host) {
        while let Ok(call) = self.0.recv() {
            call.answer(host);
        }
    }

    /// The next request, for a host that routes it itself.
    pub fn recv_timeout(&self, wait: Duration) -> Option<Call> {
        self.0.recv_timeout(wait).ok()
    }
}

/// The tier of a new peer, or `None`: the peer is refused.
pub(crate) fn admit(settings: &Settings, peer: &Peer, own_uid: Option<u32>, own_sid: Option<&str>) -> Option<Tier> {
    if let (Some(theirs), Some(ours)) = (peer.uid, own_uid) {
        if theirs == ours {
            return Some(settings.same_user);
        }
        return settings.uids.iter().find(|(u, _)| *u == theirs).map(|(_, t)| *t);
    }
    match (peer.sid.as_deref(), own_sid) {
        (Some(theirs), Some(ours)) if theirs == ours => Some(settings.same_user),
        _ => None,
    }
}

/// A slot of [`Settings::clients`], given back at its drop.
pub(crate) struct Slot(Arc<Shared>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Take a slot, or `None` when each is in use. A refused connection does
/// not count: [`Shared::active`] is never above [`Settings::clients`].
pub(crate) fn take_slot(shared: &Arc<Shared>) -> Option<Slot> {
    let clients = shared.settings.clients;
    let taken = shared.active.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| (n < clients).then_some(n + 1));
    taken.ok().map(|_| Slot(shared.clone()))
}

/// Write one refusal of a connection (`busy`, `refused`) and end it.
pub(crate) fn refuse<S: Stream>(mut stream: S, fault: Fault, settings: &Settings) {
    let _ = stream.set_write_timeout(Some(settings.write_timeout));
    let _ = stream.write_all(wire::answer_line(None, &Err(fault)).as_bytes());
}

/// The counts of calls back of one connection.
struct Budget {
    connection: usize,
}

/// Serve one admitted connection until it ends.
pub(crate) fn serve_connection<S: Stream>(
    stream: S,
    peer: Peer,
    tier: Tier,
    shared: &Shared,
    calls: &Sender<Call>,
) -> std::io::Result<()> {
    let settings = &shared.settings;
    let mut lines = Lines::new(stream, settings.line_max);
    let mut session = Session { settings, catalogue: &shared.catalogue, tier, auth_tried: false, close: false };
    let mut budget = Budget { connection: 0 };
    loop {
        let text = match lines.next_line(settings.read_timeout)? {
            Line::End | Line::Timeout => return Ok(()),
            Line::TooLong => {
                let fault = Fault::new(Code::TooLong, format!("a line is {} bytes at most", settings.line_max));
                return lines.write_line(&wire::answer_line(None, &Err(fault)), settings.write_timeout);
            }
            Line::NotUtf8 => {
                let fault = Fault::new(Code::NotUtf8, "a line is UTF-8");
                lines.write_line(&wire::answer_line(None, &Err(fault)), settings.write_timeout)?;
                continue;
            }
            Line::Text(t) if t.trim().is_empty() => continue,
            Line::Text(t) => t,
        };
        let (id, outcome) = match wire::parse(&text) {
            Err((id, fault)) => (id, Err(fault)),
            Ok(Inbound::Answer { id, outcome }) => {
                wire::wipe_outcome(outcome);
                (id, Err(Fault::new(Code::BadRequest, "an answer with no call back")))
            }
            Ok(Inbound::Request(req)) => {
                let id = req.id;
                (Some(id), request(req, &peer, &mut session, &mut budget, &mut lines, shared, calls))
            }
        };
        let written = lines.write_line(&wire::answer_line(id, &outcome), settings.write_timeout);
        wire::wipe_outcome(outcome);
        written?;
        if session.close {
            return Ok(());
        }
    }
}

fn request<S: Stream>(
    mut req: Request,
    peer: &Peer,
    session: &mut Session<'_>,
    budget: &mut Budget,
    lines: &mut Lines<S>,
    shared: &Shared,
    calls: &Sender<Call>,
) -> Outcome {
    if req.for_id.is_some() {
        return Err(Fault::new(Code::BadRequest, "a request of a client serves no request"));
    }
    let target = dispatch::find(session.catalogue, &req.method, session.tier)?;
    if let Target::Local(f) = target {
        return f(session, &req.params);
    }
    let settings = session.settings;
    let (tx, rx) = mpsc::channel();
    // Past the deadline of a call back (`call_back`): the wait ends first
    // only when a thread stops.
    let wait = settings.read_timeout + settings.write_timeout + Duration::from_secs(1);
    let cx = Context::new(peer.clone(), session.tier, tx.clone(), wait);
    let status = req.method == "status";
    let (method, params) = (std::mem::take(&mut req.method), std::mem::take(&mut req.params));
    let call = Call { target, method, params, cx, reply: tx };
    if calls.send(call).is_err() {
        return Err(Fault::new(Code::Busy, "the program takes no request now"));
    }
    let mut used = 0usize;
    let mut spent = false;
    let mut deadline = Instant::now() + settings.answer_wait;
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            // Past the budget, the answer of the host is wiped at its drop.
            Ok(ToConn::Answer(outcome)) => {
                return if spent {
                    Err(Fault::new(Code::Budget, "the calls back of the request are spent"))
                } else {
                    outcome.map(Wiped::take)
                };
            }
            Ok(ToConn::Back { method, params, reply }) => {
                if used >= settings.callbacks_per_request || budget.connection >= settings.callbacks_per_connection {
                    spent = true;
                    session.close |= budget.connection >= settings.callbacks_per_connection;
                    let _ = reply.send(Err(CallFault::Budget));
                    continue;
                }
                used += 1;
                budget.connection += 1;
                let result = call_back(lines, req.id, used as u64, &method, &params.0, settings);
                drop(params);
                if result == Err(CallFault::Gone) {
                    session.close = true;
                }
                // When `Context::call` stopped its wait, the send fails and
                // the answer of the client is wiped at its drop.
                let _ = reply.send(result.map(Wiped));
                deadline = Instant::now() + settings.answer_wait;
            }
            Err(RecvTimeoutError::Timeout) => {
                let last = if status { shared.snapshot.get() } else { None };
                return last.ok_or_else(|| Fault::new(Code::Busy, "the program did not answer in time"));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(Fault::new(Code::Internal, "the program dropped the request"));
            }
        }
    }
}

/// One call back to the client, and its answer: the next line must be the
/// answer with the id of the call. The write, the read and each blank line
/// share one deadline, so the wait of [`Context::call`] ends after it.
fn call_back<S: Stream>(
    lines: &mut Lines<S>,
    for_id: u64,
    id: u64,
    method: &str,
    params: &Value,
    settings: &Settings,
) -> Result<Value, CallFault> {
    let deadline = Instant::now() + settings.write_timeout + settings.read_timeout;
    let line = wire::request_line(id, method, params, Some(for_id));
    lines.write_line_by(&line, deadline).map_err(|_| CallFault::Gone)?;
    drop(line);
    loop {
        match lines.next_line_by(deadline).map_err(|_| CallFault::Gone)? {
            Line::Text(t) if t.trim().is_empty() => continue,
            Line::Text(t) => {
                return match wire::parse(&t) {
                    Ok(Inbound::Answer { id: Some(got), outcome }) if got == id => outcome.map_err(CallFault::Remote),
                    Ok(Inbound::Answer { outcome, .. }) => {
                        wire::wipe_outcome(outcome);
                        Err(CallFault::Gone)
                    }
                    _ => Err(CallFault::Gone),
                };
            }
            _ => return Err(CallFault::Gone),
        }
    }
}
