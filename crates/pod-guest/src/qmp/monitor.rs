//! One live QMP connection: each request with an `id`, the answer of each
//! request by its `id` or by the order of the requests, a bound of the
//! caller for one answer, the late answer of a request that had no answer
//! in its bound, and the events in between.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use pod_core::{Class, Failure, Stage};
use socket2::{Domain, SockAddr, Socket, Type};

use super::{is_event, response_ok};
use crate::{Result, failure};

/// The default bound of one answer ([`Monitor::set_answer_wait`]).
pub const ANSWER_WAIT: Duration = Duration::from_secs(5);

/// The number of events that a monitor keeps for
/// [`Monitor::take_events`]; an older event goes first.
pub const EVENTS_KEPT: usize = 256;

/// The prefix of the `id` of each request of a [`Monitor`].
const ID: &str = "pod-qmp-";

/// The time between two tries of a connection.
const CONNECT_EVERY: Duration = Duration::from_millis(100);

/// One live QMP connection.
///
/// QEMU serves one client of a monitor at a time, and its socket queues
/// one more connection (`listen` with a backlog of 1). So a caller drops
/// its connection before it opens a new one to the same emulator. A
/// connection in the queue gets no greeting while another one is open,
/// and a connection to a full queue fails ([`Monitor::connect`]). The late
/// answer of a request comes on the connection that sent it: read it with
/// [`Monitor::late`], not on a new connection.
pub struct Monitor {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
    /// The bytes of a line whose end did not come before a bound.
    partial: Vec<u8>,
    /// The bound of one answer.
    wait: Duration,
    /// The number of the next request.
    next: u64,
    /// The numbers of the requests with no answer read, in their order.
    owed: VecDeque<u64>,
    /// The events read and not taken, the oldest first.
    events: VecDeque<String>,
}

/// An I/O failure of the QMP connection.
fn io(what: &str, e: std::io::Error) -> Failure {
    Failure::caused(Stage::Host, Class::of_io(e.kind()), format!("qmp {what}: {e}"), e)
}

impl Monitor {
    /// Connect to `path` within `wait`, and do the handshake of the
    /// capabilities.
    ///
    /// The emulator makes the socket when it starts, so a caller that just
    /// started it polls: a try that fails is made again until `wait`. Each
    /// try is a connect with `O_NONBLOCK`, which never waits in the kernel:
    /// a connection to the full queue of the monitor fails at once. At the
    /// bound, the [`Class::Timeout`] failure tells the cause of the last
    /// try: the queue is full, the socket never appeared, or no emulator
    /// listens on it. The greeting has its own bound, [`ANSWER_WAIT`]: a
    /// connection in the queue behind an open one gets no greeting.
    pub fn connect(path: &Path, wait: Duration) -> Result<Monitor> {
        let end = Instant::now() + wait;
        let s = loop {
            match try_connect(path) {
                Ok(s) => break s,
                Err(e) => {
                    let left = end.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Err(no_connection(path, wait, &e));
                    }
                    std::thread::sleep(left.min(CONNECT_EVERY));
                }
            }
        };
        let _ = s.set_write_timeout(Some(ANSWER_WAIT));
        let reader = BufReader::new(s.try_clone().map_err(|e| io("connect", e))?);
        let mut m = Monitor {
            writer: s,
            reader,
            partial: Vec::new(),
            wait: ANSWER_WAIT,
            next: 1,
            owed: VecDeque::new(),
            events: VecDeque::new(),
        };
        // The greeting comes first with no request; read it before a
        // request, or the first response read is the greeting.
        m.read_line(Instant::now() + ANSWER_WAIT).map_err(|e| match e.class {
            Class::Timeout => failure(
                Class::Timeout,
                format!(
                    "qmp: no greeting from {} within {:.1}s: the emulator serves one client at a time, so this \
                     connection waits behind another open connection, or the emulator holds its lock",
                    path.display(),
                    ANSWER_WAIT.as_secs_f64()
                ),
            ),
            _ => e,
        })?;
        m.call("qmp_capabilities")?;
        Ok(m)
    }

    /// The bound of one answer of each later request ([`ANSWER_WAIT`] at
    /// the connection). A zero bound is [`ANSWER_WAIT`].
    pub fn set_answer_wait(&mut self, wait: Duration) {
        self.wait = if wait.is_zero() { ANSWER_WAIT } else { wait };
    }

    /// One line, up to `until`. A bound in the middle of a line keeps its
    /// part for the next read.
    fn read_line(&mut self, until: Instant) -> Result<String> {
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(failure(Class::Timeout, "qmp read: no line within the bound"));
            }
            let _ = self.reader.get_ref().set_read_timeout(Some(left));
            match self.reader.read_until(b'\n', &mut self.partial) {
                Ok(0) => return Err(failure(Class::Unavailable, "qmp closed the connection")),
                // A line with no end is the last one before the close.
                Ok(_) => return Ok(String::from_utf8_lossy(&std::mem::take(&mut self.partial)).into_owned()),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) => return Err(io("read", e)),
            }
        }
    }

    /// Send one request with no arguments, `{"execute":EXECUTE}`, and read
    /// its response.
    pub fn call(&mut self, execute: &str) -> Result<()> {
        self.send(&format!("{{\"execute\":\"{execute}\"}}"))
    }

    pub(super) fn send(&mut self, body: &str) -> Result<()> {
        self.ask(body).map(drop)
    }

    /// Send one request, `body` as it is, and return the line of its
    /// response within the bound of [`Monitor::set_answer_wait`]. A
    /// response with `"error"` is a [`Class::Protocol`] failure that holds
    /// the line; no answer within the bound is [`Class::Timeout`], and
    /// [`Monitor::late`] reads it after.
    pub fn ask(&mut self, body: &str) -> Result<String> {
        let wait = self.wait;
        self.ask_within(body, wait)
    }

    /// [`Monitor::ask`] with the bound `wait` for this answer alone.
    ///
    /// The request gets an `id` member at its end: QMP gives it back in the
    /// answer. Each event before the answer goes to
    /// [`Monitor::take_events`], and the late answer of an earlier request
    /// is not read as this one.
    pub fn ask_within(&mut self, body: &str, wait: Duration) -> Result<String> {
        let n = self.next;
        self.next += 1;
        let line = with_id(body, n);
        self.writer
            .write_all(line.as_bytes())
            .and_then(|()| self.writer.write_all(b"\n"))
            .and_then(|()| self.writer.flush())
            .map_err(|e| io("write", e))?;
        self.owed.push_back(n);
        self.answer(n, body, wait)
    }

    /// The late answer of the last request whose answer did not come in
    /// its bound, within `wait`; the checks of [`Monitor::ask`]. With no
    /// such request it is [`Class::Usage`].
    pub fn late(&mut self, wait: Duration) -> Result<String> {
        let n = *self.owed.back().ok_or_else(|| failure(Class::Usage, "qmp: no request waits for its answer"))?;
        self.answer(n, "the request before", wait)
    }

    /// True while a request has no answer read: [`Monitor::late`] can read
    /// it.
    pub fn owes(&self) -> bool {
        !self.owed.is_empty()
    }

    /// Each event read since the last call, the oldest first (at most
    /// [`EVENTS_KEPT`]).
    pub fn take_events(&mut self) -> Vec<String> {
        self.events.drain(..).collect()
    }

    /// Read up to `wait` for the answer of request `n`. An event is not
    /// an answer: QMP can send `RESET`, `STOP`, `MIGRATION` and others at
    /// any time. An answer with an `id` of this monitor names its request;
    /// an answer with none (a monitor that gives back no `id`) is the
    /// answer of the oldest request with no answer, as QMP answers in the
    /// order of the requests.
    fn answer(&mut self, n: u64, body: &str, wait: Duration) -> Result<String> {
        let until = Instant::now() + wait;
        loop {
            let line = self.read_line(until).map_err(|e| match e.class {
                Class::Timeout => {
                    failure(Class::Timeout, format!("qmp: no answer to {body} within {:.1}s", wait.as_secs_f64()))
                }
                _ => e,
            })?;
            if is_event(&line) {
                if self.events.len() == EVENTS_KEPT {
                    self.events.pop_front();
                }
                self.events.push_back(line.trim_end().to_string());
                continue;
            }
            let of = match id_of(&line) {
                Some(id) => {
                    // An answer comes after each answer of an earlier request.
                    self.owed.retain(|&o| o > id);
                    id
                }
                None => match self.owed.pop_front() {
                    Some(o) => o,
                    None => continue,
                },
            };
            if of != n {
                continue;
            }
            return if response_ok(&line) {
                Ok(line)
            } else {
                Err(failure(Class::Protocol, format!("qmp refused {body}: {}", line.trim())))
            };
        }
    }

    /// Ask the emulator to exit. The response can be missing: the emulator
    /// can close the connection during `quit`, and that is not a failure.
    pub fn quit(&mut self) {
        let _ = self.writer.write_all(b"{\"execute\":\"quit\"}\n").and_then(|()| self.writer.flush());
        let _ = self.read_line(Instant::now() + ANSWER_WAIT);
    }
}

/// One try of a connection to the socket at `path` that never waits in the
/// kernel.
///
/// A blocking `connect` to a listener whose queue is full waits in the
/// kernel with no bound (`unix_wait_for_peer`). A Unix stream socket of
/// Linux has no connection in progress: a `connect` with `O_NONBLOCK` ends
/// at once, connected or with `EAGAIN` for a full queue. The stream is
/// blocking again after the connect: each read has its own bound.
fn try_connect(path: &Path) -> std::io::Result<UnixStream> {
    let s = Socket::new(Domain::UNIX, Type::STREAM, None)?;
    s.set_nonblocking(true)?;
    s.connect(&SockAddr::unix(path)?)?;
    s.set_nonblocking(false)?;
    Ok(UnixStream::from(std::os::fd::OwnedFd::from(s)))
}

/// The failure of a connection to `path` that did not come within `wait`,
/// by the error `e` of the last try.
fn no_connection(path: &Path, wait: Duration, e: &std::io::Error) -> Failure {
    let (p, s) = (path.display(), wait.as_secs_f64());
    let text = match e.kind() {
        ErrorKind::WouldBlock => format!(
            "the queue of the QMP socket {p} is full for {s:.1}s: the emulator serves one client at a time and \
             queues one more, so other connections to it are open; close them before a new one"
        ),
        ErrorKind::NotFound => format!("the QMP socket {p} never appeared within {s:.1}s"),
        ErrorKind::ConnectionRefused => {
            format!("the QMP socket {p} refused each connection for {s:.1}s: no emulator listens on it")
        }
        _ => format!("qmp connect to {p}: no connection within {s:.1}s: {e}"),
    };
    failure(Class::Timeout, text)
}

/// `body` with the member `"id":"pod-qmp-N"` at its end, where it is a JSON
/// object; else `body` as it is.
pub(super) fn with_id(body: &str, n: u64) -> String {
    let t = body.trim();
    match t.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
        Some(inner) if !inner.trim().is_empty() => format!("{{{inner},\"id\":\"{ID}{n}\"}}"),
        _ => body.to_string(),
    }
}

/// The number of an `id` of this crate in an answer line.
pub(super) fn id_of(line: &str) -> Option<u64> {
    let mut rest = line;
    while let Some(at) = rest.find("\"id\"") {
        rest = &rest[at + 4..];
        let Some(value) = rest.trim_start().strip_prefix(':') else { continue };
        let Some(value) = value.trim_start().strip_prefix('"').and_then(|v| v.strip_prefix(ID)) else { continue };
        let end = value.find('"')?;
        if let Ok(n) = value[..end].parse() {
            return Some(n);
        }
    }
    None
}
