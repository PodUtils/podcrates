//! A failure with its stage and its class. The stage names the part of the
//! path that broke (podnet calls it the hop); the class says what a caller
//! can do (podnet maps it to an exit code). A consumer reads both from the
//! error that a crate returns, and keeps its own words and its own codes.

use std::error::Error;
use std::fmt;
use std::io;

/// The part of the path that broke.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    /// The HTTP or SOCKS5 proxy.
    Proxy,
    /// Name resolution.
    Dns,
    /// The TCP or UDP connection to the far end.
    Connect,
    /// The network ended a connection before or during the TLS handshake:
    /// a policy of the host or of its network, not trust or the clock.
    Network,
    /// TLS with the far end.
    Tls,
    /// The WebSocket upgrade or its frames.
    WebSocket,
    /// QUIC.
    Quic,
    /// The relay: its token, its host, its contract.
    Relay,
    /// The registration of a tunnel with the edge.
    EdgeRegister,
    /// An edge connection after it registered.
    EdgeStream,
    /// The origin behind a tunnel.
    Origin,
    /// WARP: its registration or its session.
    Warp,
    /// The resumable layer or the end-to-end channel.
    Session,
    /// A rendezvous: a mailbox or a transit relay.
    Rendezvous,
    /// The local host: a probe of it, its file systems, its limits.
    Host,
    /// An OCI registry or its token service: an answer, a document, a
    /// digest of a blob.
    Registry,
}

impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Stage::Proxy => "proxy",
            Stage::Dns => "dns",
            Stage::Connect => "connect",
            Stage::Network => "network",
            Stage::Tls => "tls",
            Stage::WebSocket => "websocket",
            Stage::Quic => "quic",
            Stage::Relay => "relay",
            Stage::EdgeRegister => "edge-register",
            Stage::EdgeStream => "edge-stream",
            Stage::Origin => "origin",
            Stage::Warp => "warp",
            Stage::Session => "session",
            Stage::Rendezvous => "rendezvous",
            Stage::Host => "host",
            Stage::Registry => "registry",
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What a caller can do about a failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    /// A value that the caller gave cannot be used (a bad argument).
    Usage,
    /// A configuration that cannot be used.
    Config,
    /// A service or a path is not available.
    Unavailable,
    /// A path or a name that does not exist: apart from `Unavailable`, so
    /// that a caller tells it from no space or a closed service.
    NotFound,
    /// A service refused for now; a later try can work.
    TempFail,
    /// A wait ended at its time limit.
    Timeout,
    /// The far end broke the protocol.
    Protocol,
    /// The far end refused the credentials or the identity.
    Refused,
    /// A credential that worked is gone: a revoked token, a deleted service
    /// token, device or node. A restart does not help.
    Revoked,
    /// A local policy refused the action (an egress rule, an allow list).
    Policy,
    /// The host, the build or the far end does not support the action.
    Unsupported,
    /// A file or a directory has the wrong permissions or the wrong owner.
    /// A tool chooses it for a local mode check; [`Class::of_io`] keeps
    /// `Refused` for a credential that the far end refused.
    Permission,
    /// The far end and this end have no protocol or suite in common.
    Suite,
    /// An output (a file or a directory) that the tool must make cannot be
    /// made or written. An input path that does not exist stays `NotFound`,
    /// and a mode check of a file stays `Permission`.
    CantCreate,
    /// A fault inside the tool, or a session that ended before its end with
    /// no known fault of the far end or of the network. A far end that broke
    /// the protocol stays `Protocol`.
    Internal,
    /// An input was there and its content is wrong: a received file or
    /// entry that fails its check, a state file that cannot be read, a
    /// passphrase that does not open its key file. A value of the caller
    /// stays `Usage`, and the far end that broke the protocol stays
    /// `Protocol`.
    Data,
}

impl Class {
    /// Each class, in the order of the table of exit codes.
    pub const ALL: [Class; 16] = [
        Class::Usage,
        Class::Config,
        Class::Unavailable,
        Class::NotFound,
        Class::TempFail,
        Class::Timeout,
        Class::Protocol,
        Class::Refused,
        Class::Revoked,
        Class::Policy,
        Class::Unsupported,
        Class::Permission,
        Class::Suite,
        Class::CantCreate,
        Class::Internal,
        Class::Data,
    ];

    /// The exit code of a tool that ends on this class. The codes follow
    /// sysexits(3), so that a supervisor reads one table for each tool;
    /// `Revoked` and `Suite` share 78 with `Config`, so that one
    /// `RestartPreventExitStatus=78` stops a restart loop for each of them.
    /// `Policy` and `Unsupported` take 79 and 80, past the range of
    /// sysexits, as no code there has their meaning. `CantCreate` (73) and
    /// `Internal` (70) are not in the list of codes that stop a restart: a
    /// later try can work after a freed disk or a one-time fault. `Data`
    /// (65) is not in the list either: a later input can be correct.
    pub fn exit_code(self) -> u8 {
        match self {
            Class::Usage => 64,
            Class::NotFound => 66,
            Class::Unavailable => 69,
            Class::TempFail | Class::Timeout => 75,
            Class::Protocol => 76,
            Class::Refused => 77,
            Class::Config | Class::Revoked => 78,
            Class::Policy => 79,
            Class::Unsupported => 80,
            Class::Permission => 71,
            Class::Suite => 78,
            Class::CantCreate => 73,
            Class::Internal => 70,
            Class::Data => 65,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Class::Usage => "usage",
            Class::Config => "config",
            Class::Unavailable => "unavailable",
            Class::NotFound => "notfound",
            Class::TempFail => "tempfail",
            Class::Timeout => "timeout",
            Class::Protocol => "protocol",
            Class::Refused => "refused",
            Class::Revoked => "revoked",
            Class::Policy => "policy",
            Class::Unsupported => "unsupported",
            Class::Permission => "permission",
            Class::Suite => "suite",
            Class::CantCreate => "cantcreate",
            Class::Internal => "internal",
            Class::Data => "data",
        }
    }

    /// The class of an I/O error by its kind.
    pub fn of_io(kind: io::ErrorKind) -> Class {
        use io::ErrorKind as K;
        match kind {
            K::TimedOut | K::WouldBlock => Class::Timeout,
            K::InvalidInput => Class::Usage,
            K::InvalidData | K::UnexpectedEof => Class::Protocol,
            K::PermissionDenied => Class::Refused,
            K::NotFound => Class::NotFound,
            _ => Class::Unavailable,
        }
    }
}

impl fmt::Display for Class {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A failure of a crate: its stage, its class, its text and its cause.
#[derive(Debug)]
pub struct Failure {
    pub stage: Stage,
    pub class: Class,
    text: String,
    source: Option<Box<dyn Error + Send + Sync + 'static>>,
}

impl Failure {
    /// The error of I/O of the chain of causes, when one is: a caller keeps
    /// its kind (and the path that its text names) with no parse of words.
    pub fn io_cause(&self) -> Option<&io::Error> {
        let mut next = self.source.as_deref().map(|e| e as &(dyn Error + 'static));
        while let Some(e) = next {
            if let Some(io) = e.downcast_ref::<io::Error>() {
                return Some(io);
            }
            next = e.source();
        }
        None
    }

    pub fn new(stage: Stage, class: Class, text: impl Into<String>) -> Failure {
        Failure { stage, class, text: text.into(), source: None }
    }

    /// A failure whose cause is `source`. The text comes before the text of
    /// the cause in [`Failure::chain`].
    pub fn caused(
        stage: Stage,
        class: Class,
        text: impl Into<String>,
        source: impl Into<Box<dyn Error + Send + Sync + 'static>>,
    ) -> Failure {
        Failure { stage, class, text: text.into(), source: Some(source.into()) }
    }

    /// The step `step` at `stage` that the I/O error `e` ended, with the
    /// class of its kind. The text is the step and `e` is the cause, so that
    /// a reader that joins the chain says the words of `e` once.
    pub fn io(stage: Stage, step: impl Into<String>, e: io::Error) -> Failure {
        let class = Class::of_io(e.kind());
        Failure { stage, class, text: step.into(), source: Some(Box::new(e)) }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text and the text of each cause, joined with `: `. A cause whose
    /// text is in the chain already is not named again: a text that quotes
    /// its cause and adds what to do (the hint of a TLS failure) stays as it
    /// is.
    pub fn chain(&self) -> String {
        chain_of(self)
    }

    /// The cause, for a layer that says the words of this failure in its
    /// own way and keeps the cause.
    pub fn into_source(self) -> Option<Box<dyn Error + Send + Sync + 'static>> {
        self.source
    }

    /// The text of the chain with no innermost typed cause, and that cause:
    /// for a layer that names this failure among others in its own text and
    /// keeps the typed cause (a refusal, an error of I/O) for a caller that
    /// reads it. The innermost typed cause is the first cause that is not a
    /// `Failure`; a chain of `Failure`s alone gives its text and no cause.
    /// The new text with the cause as its source says each cause once on a
    /// join, unless a text of the chain quotes the words of the cause.
    /// [`Failure::chain`] of it leaves out the cause when the new text holds
    /// the same words already (another failure with the same cause).
    pub fn into_root(self) -> (String, Option<Box<dyn Error + Send + Sync + 'static>>) {
        let mut text = self.text;
        let mut next = self.source;
        while let Some(e) = next {
            match e.downcast::<Failure>() {
                Ok(inner) => {
                    if !text.contains(&inner.text) {
                        text.push_str(": ");
                        text.push_str(&inner.text);
                    }
                    next = inner.source;
                }
                Err(root) => return (text, Some(root)),
            }
        }
        (text, None)
    }

    /// The same failure at another stage, for a layer that knows better
    /// where it broke.
    pub fn at(mut self, stage: Stage) -> Failure {
        self.stage = stage;
        self
    }
}

/// The text of `error` and of each cause, as [`Failure::chain`] says them.
pub fn chain_of(error: &(dyn Error + 'static)) -> String {
    let mut out = error.to_string();
    let mut next = error.source();
    while let Some(e) = next {
        let more = e.to_string();
        if !out.contains(&more) {
            out.push_str(": ");
            out.push_str(&more);
        }
        next = e.source();
    }
    out
}

/// Each text of the chain of `error` joined with `: `, with no check: the
/// line of a reader that walks the sources (anyhow's `{:#}`). A failure that
/// keeps the rule of the step gives the line of [`chain_of`].
pub fn joined(error: &(dyn Error + 'static)) -> String {
    let mut out = error.to_string();
    let mut next = error.source();
    while let Some(e) = next {
        out.push_str(": ");
        out.push_str(&e.to_string());
        next = e.source();
    }
    out
}

/// `lead: text`, with the words of `lead` once: a `text` that starts with
/// them (a layer below named the request or the target) stays as it is.
pub fn lead(lead: &str, text: &str) -> String {
    if starts_with_words(text, lead) { text.to_string() } else { format!("{lead}: {text}") }
}

/// `text` with no leading `lead: `: the words of a layer below that the
/// caller says in its own way.
pub fn strip_lead<'a>(text: &'a str, lead: &str) -> &'a str {
    text.strip_prefix(lead).and_then(|rest| rest.strip_prefix(": ")).unwrap_or(text)
}

/// True when `text` starts with the words of `lead`, and a word ends there:
/// `POST https://a/b` does not start `POST https://a/bc: ...`.
fn starts_with_words(text: &str, lead: &str) -> bool {
    let ends = |rest: &str| rest.is_empty() || rest.starts_with(": ") || rest.starts_with([' ', ';', ',']);
    !lead.is_empty() && text.strip_prefix(lead).is_some_and(ends)
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl Error for Failure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_deref().map(|e| e as &(dyn Error + 'static))
    }
}

#[cfg(test)]
#[path = "failure_tests.rs"]
mod tests;
