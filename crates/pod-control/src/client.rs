//! The client: one request at a time, the calls back of the server
//! answered on the same connection, each with its budget.

use std::fmt;
use std::io;
use std::time::Duration;

use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::catalogue::Tier;
use crate::frame::{Line, Lines, Stream};
use crate::host::{Drain, Metrics};
use crate::server::{CALLBACKS_PER_CONNECTION, CALLBACKS_PER_REQUEST, Token};
use crate::wire::{self, Code, Fault, Inbound, PROTOCOL};

/// The settings of a client.
#[derive(Debug, Clone)]
pub struct ClientSettings {
    /// The longest answer line: 16 MiB.
    pub line_max: usize,
    /// The longest wait for one whole line of the server.
    pub read_timeout: Duration,
    /// The longest wait to write one whole line.
    pub write_timeout: Duration,
    /// The calls back of the server that one request takes.
    pub callbacks_per_request: usize,
    /// The calls back of all the requests of the connection.
    pub callbacks_per_connection: usize,
}

impl Default for ClientSettings {
    fn default() -> ClientSettings {
        ClientSettings {
            line_max: 16 << 20,
            read_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(5),
            callbacks_per_request: CALLBACKS_PER_REQUEST,
            callbacks_per_connection: CALLBACKS_PER_CONNECTION,
        }
    }
}

/// Why a call gave no result.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// No whole line of the server came in time.
    Timeout,
    /// The server closed the connection.
    Closed,
    /// A line of the server is longer than [`ClientSettings::line_max`].
    TooLong,
    /// The server broke the protocol.
    Protocol(String),
    /// The server sent more calls back than the budget: the connection is
    /// dropped.
    Budget,
    /// The server answered with an error.
    Remote(Fault),
}

impl Error {
    /// The class of the failure, for the exit code of a command line.
    pub fn class(&self) -> pod_core::Class {
        use pod_core::Class;
        match self {
            Error::Io(e) if e.kind() == io::ErrorKind::NotFound => Class::NotFound,
            Error::Io(_) | Error::Closed => Class::Unavailable,
            Error::Timeout => Class::Timeout,
            Error::TooLong | Error::Protocol(_) => Class::Protocol,
            Error::Budget => Class::Policy,
            Error::Remote(f) => f.code.class(),
        }
    }

    /// The code of an error of the server.
    pub fn code(&self) -> Option<Code> {
        match self {
            Error::Remote(f) => Some(f.code),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "the control connection: {e}"),
            Error::Timeout => f.write_str("the control server did not answer in time"),
            Error::Closed => f.write_str("the control server closed the connection"),
            Error::TooLong => f.write_str("a line of the control server is too long"),
            Error::Protocol(why) => write!(f, "the control server broke the protocol: {why}"),
            Error::Budget => f.write_str("the control server called back more than the budget"),
            Error::Remote(fault) => write!(f, "the control server answered {fault}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error::Io(e)
    }
}

/// The result of `version`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Version {
    pub program: String,
    pub version: String,
    pub protocol: u32,
}

/// One row of `methods`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct MethodInfo {
    pub name: String,
    pub tier: String,
    pub route: String,
    pub summary: String,
}

/// What answers a call back of the server: its result, or an error.
pub type Back<'a> = &'a mut dyn FnMut(&str, &Value) -> Result<Value, Fault>;

/// A connection to a control server.
pub struct Client {
    lines: Lines<Box<dyn Stream>>,
    settings: ClientSettings,
    next: u64,
    callbacks: usize,
    broken: bool,
}

impl Client {
    pub fn new(stream: impl Stream + 'static, settings: ClientSettings) -> io::Result<Client> {
        let mut stream: Box<dyn Stream> = Box::new(stream);
        stream.set_write_timeout(Some(settings.write_timeout))?;
        Ok(Client { lines: Lines::new(stream, settings.line_max), settings, next: 1, callbacks: 0, broken: false })
    }

    /// Connect to the Unix socket at `path`.
    #[cfg(unix)]
    pub fn connect_unix(path: &std::path::Path, settings: ClientSettings) -> Result<Client, Error> {
        Ok(Client::new(crate::unix::connect(path)?, settings)?)
    }

    /// Connect to the named pipe `name` (`\\.\pipe\NAME`).
    #[cfg(windows)]
    pub fn connect_pipe(name: &str, settings: ClientSettings) -> Result<Client, Error> {
        Ok(Client::new(crate::pipe::connect(name)?, settings)?)
    }

    /// Call `method` with `params` (an object). A call back of the server is
    /// answered `unsupported`. The params and the line are wiped after the
    /// write: they can hold a token.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        self.call_with(method, params, &mut |_, _| Err(Fault::new(Code::Unsupported, "this client takes no call back")))
    }

    /// Call `method`, and answer each call back of the server with `back`.
    pub fn call_with(&mut self, method: &str, mut params: Value, back: Back<'_>) -> Result<Value, Error> {
        if self.broken {
            wire::wipe(&mut params);
            return Err(Error::Closed);
        }
        let id = self.next;
        self.next += 1;
        // The server can answer `busy` or `refused` and close before the
        // request is written: read its line before the write error counts.
        let written =
            self.lines.write_line(&wire::request_line(id, method, &params, None), self.settings.write_timeout);
        wire::wipe(&mut params);
        let result = self.answer_of(id, back);
        match (&result, written) {
            (Err(Error::Closed), Err(e)) => Err(Error::Io(e)),
            _ => result,
        }
    }

    fn answer_of(&mut self, id: u64, back: Back<'_>) -> Result<Value, Error> {
        let mut used = 0usize;
        loop {
            let text = match self.next_text() {
                Ok(t) => t,
                Err(e) => {
                    self.broken = true;
                    return Err(e);
                }
            };
            match wire::parse(&text) {
                Ok(Inbound::Answer { id: got, outcome }) if got == Some(id) || got.is_none() => {
                    return outcome.map_err(Error::Remote);
                }
                Ok(Inbound::Answer { outcome, .. }) => {
                    wire::wipe_outcome(outcome);
                    return self.broke("an answer to another request");
                }
                Ok(Inbound::Request(req)) if req.for_id == Some(id) => {
                    if used >= self.settings.callbacks_per_request
                        || self.callbacks >= self.settings.callbacks_per_connection
                    {
                        self.broken = true;
                        return Err(Error::Budget);
                    }
                    used += 1;
                    self.callbacks += 1;
                    let outcome = back(&req.method, &req.params);
                    let line = wire::answer_line(Some(req.id), &outcome);
                    let written = self.lines.write_line(&line, self.settings.write_timeout);
                    wire::wipe_outcome(outcome);
                    written?;
                }
                Ok(Inbound::Request(_)) => return self.broke("a call back for no request in flight"),
                Err(_) => return self.broke("a line that is not of the protocol"),
            }
        }
    }

    fn broke(&mut self, why: &str) -> Result<Value, Error> {
        self.broken = true;
        Err(Error::Protocol(why.into()))
    }

    fn next_text(&mut self) -> Result<Zeroizing<String>, Error> {
        loop {
            match self.lines.next_line(self.settings.read_timeout)? {
                Line::Text(t) if t.trim().is_empty() => continue,
                Line::Text(t) => return Ok(t),
                Line::End => return Err(Error::Closed),
                Line::Timeout => return Err(Error::Timeout),
                Line::TooLong => return Err(Error::TooLong),
                Line::NotUtf8 => return Err(Error::Protocol("a line that is not UTF-8".into())),
            }
        }
    }

    /// Write `bytes` as they are (a line end is added when they have none),
    /// and give the next line of the server as JSON: `None` when it closed.
    /// For a test of a server ([`crate::conformance`]).
    pub fn raw(&mut self, bytes: &[u8]) -> Result<Option<Value>, Error> {
        let stream = self.lines.stream();
        let written = stream.write_all(bytes).and_then(|()| match bytes.last() {
            Some(b'\n') => Ok(()),
            _ => stream.write_all(b"\n"),
        });
        match self.next_text() {
            Ok(t) => crate::json::read(&t).map(Some).ok_or_else(|| Error::Protocol("a line that is not JSON".into())),
            Err(Error::Closed) => written.map(|()| None).map_err(Error::Io),
            Err(e) => Err(e),
        }
    }

    pub fn version(&mut self) -> Result<Version, Error> {
        let v: Version = typed(self.call("version", json!({}))?)?;
        if v.protocol != PROTOCOL {
            return Err(Error::Protocol(format!("the protocol {} is not {PROTOCOL}", v.protocol)));
        }
        Ok(v)
    }

    /// The methods that this connection may call.
    pub fn methods(&mut self) -> Result<Vec<MethodInfo>, Error> {
        let v = self.call("methods", json!({}))?;
        typed(v.get("methods").cloned().unwrap_or(Value::Null))
    }

    /// Give the master token; the connection then has the master tier.
    pub fn auth(&mut self, token: &Token) -> Result<Tier, Error> {
        let v = self.call("auth", json!({ "token": token.expose() }))?;
        v.get("tier").and_then(Value::as_str).and_then(Tier::from_name).ok_or_else(|| Error::Protocol("no tier".into()))
    }

    pub fn status(&mut self) -> Result<Value, Error> {
        self.call("status", json!({}))
    }

    pub fn metrics(&mut self) -> Result<Metrics, Error> {
        typed(self.call("metrics", json!({}))?)
    }

    pub fn reload(&mut self) -> Result<Value, Error> {
        self.call("reload", json!({}))
    }

    pub fn drain(&mut self, drain: Drain) -> Result<Value, Error> {
        self.call("drain", json!({ "exit": drain.exit }))
    }

    pub fn stop(&mut self) -> Result<Value, Error> {
        self.call("stop", json!({}))
    }
}

fn typed<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, Error> {
    serde_json::from_value(v).map_err(|_| Error::Protocol("a result of the wrong form".into()))
}
