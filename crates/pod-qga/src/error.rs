//! The error of a call. Each error names the call (`guest-exec`) and what
//! went wrong: no agent at the socket, a refusal of the agent in its own
//! words, the bound of the call, a closed connection, an answer that breaks
//! the protocol, a limit of the caller, or a value that cannot go into a
//! request. [`Error::class`] gives the class of `pod_core`; a
//! [`pod_core::Failure`] made from an error is at [`Stage::Host`] and keeps
//! the error as its cause.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use pod_core::text::one_line;
use pod_core::{Class, Failure, Stage};

/// The result of a call.
pub type Result<T> = std::result::Result<T, Error>;

/// The longest text of the agent that an error keeps.
pub(crate) const MAX_TEXT: usize = 512;

/// The error of one call.
#[derive(Debug)]
pub struct Error {
    call: String,
    kind: ErrorKind,
}

/// What went wrong.
#[derive(Debug)]
#[non_exhaustive]
pub enum ErrorKind {
    /// No agent answers at the socket: the path does not exist, it is not a
    /// socket, or the connection was refused.
    NoAgent { socket: PathBuf, cause: io::Error },
    /// The agent refused the call: the `class` and the `desc` of its
    /// `"error"` member.
    Agent { class: String, desc: String },
    /// The call did not end within its bound.
    Timeout { after: Duration },
    /// The command of a [`crate::Client::wait_exec`] did not end within
    /// its limit. It still runs in the guest.
    NotEnded { pid: i64, after: Duration },
    /// The agent closed the connection before the answer.
    Closed,
    /// The answer breaks the protocol: no JSON, no `"return"`, a field that
    /// is not there, an answer over the size limit.
    Protocol(String),
    /// A limit of the caller stopped the call: a file over the limit of
    /// [`crate::Client::read_file`].
    Limit { what: String, limit: u64 },
    /// A value of the caller cannot go into a request.
    Usage(String),
    /// A read or a write of the socket failed.
    Io(io::Error),
}

impl Error {
    // The client, on Unix only, makes the errors.
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn new(call: &str, kind: ErrorKind) -> Error {
        Error { call: call.to_string(), kind }
    }

    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn protocol(call: &str, why: impl Into<String>) -> Error {
        Error::new(call, ErrorKind::Protocol(why.into()))
    }

    /// The call: the name of the command of the agent.
    pub fn call(&self) -> &str {
        &self.call
    }

    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    /// The class of the error of the agent, when the agent refused.
    pub fn agent_class(&self) -> Option<&str> {
        match &self.kind {
            ErrorKind::Agent { class, .. } => Some(class),
            _ => None,
        }
    }

    /// What a caller can do about the error.
    pub fn class(&self) -> Class {
        match &self.kind {
            ErrorKind::NoAgent { cause, .. } => match cause.kind() {
                io::ErrorKind::PermissionDenied => Class::Refused,
                io::ErrorKind::TimedOut => Class::Timeout,
                _ => Class::Unavailable,
            },
            ErrorKind::Agent { class, .. } => match class.as_str() {
                "CommandNotFound" => Class::Unsupported,
                "CommandDisabled" => Class::Policy,
                "DeviceNotFound" => Class::NotFound,
                _ => Class::Unavailable,
            },
            ErrorKind::Timeout { .. } | ErrorKind::NotEnded { .. } => Class::Timeout,
            ErrorKind::Closed => Class::Unavailable,
            ErrorKind::Protocol(_) => Class::Protocol,
            ErrorKind::Limit { .. } => Class::Policy,
            ErrorKind::Usage(_) => Class::Usage,
            ErrorKind::Io(e) => Class::of_io(e.kind()),
        }
    }

    /// The stage of a [`Failure`] of this crate: the agent of a local guest.
    pub fn stage(&self) -> Stage {
        Stage::Host
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let call = &self.call;
        match &self.kind {
            ErrorKind::NoAgent { socket, cause } => {
                write!(f, "{call}: no guest agent answers at {}: {cause}", socket.display())
            }
            ErrorKind::Agent { class, desc } => {
                write!(f, "{call}: the guest agent refused the call ({}): {}", far_text(class), far_text(desc))
            }
            ErrorKind::Timeout { after } => {
                write!(f, "{call}: the guest agent gave no answer within {} ms", after.as_millis())
            }
            ErrorKind::NotEnded { pid, after } => {
                write!(f, "{call}: the command {pid} did not end within {} ms; it still runs", after.as_millis())
            }
            ErrorKind::Closed => write!(f, "{call}: the guest agent closed the connection before the answer"),
            ErrorKind::Protocol(why) => write!(f, "{call}: the answer of the guest agent breaks the protocol: {why}"),
            ErrorKind::Limit { what, limit } => write!(f, "{call}: {what} is over the limit of {limit} bytes"),
            ErrorKind::Usage(why) => write!(f, "{call}: {why}"),
            ErrorKind::Io(e) => write!(f, "{call}: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            ErrorKind::NoAgent { cause, .. } | ErrorKind::Io(cause) => Some(cause),
            _ => None,
        }
    }
}

impl From<Error> for Failure {
    fn from(e: Error) -> Failure {
        Failure::caused(e.stage(), e.class(), e.to_string(), e)
    }
}

/// `text` of the agent on one line, cut to [`MAX_TEXT`] characters: the
/// guest is not trusted, and its text goes to a terminal.
pub(crate) fn far_text(text: &str) -> String {
    let line = one_line(text);
    match line.char_indices().nth(MAX_TEXT) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_has_its_class_and_names_its_call() {
        let agent = |class: &str| Error::new("guest-exec", ErrorKind::Agent { class: class.into(), desc: "d".into() });
        assert_eq!(agent("CommandNotFound").class(), Class::Unsupported);
        assert_eq!(agent("CommandDisabled").class(), Class::Policy);
        assert_eq!(agent("DeviceNotFound").class(), Class::NotFound);
        assert_eq!(agent("GenericError").class(), Class::Unavailable);
        assert_eq!(Error::new("x", ErrorKind::Closed).class(), Class::Unavailable);
        assert_eq!(Error::new("x", ErrorKind::Timeout { after: Duration::from_secs(1) }).class(), Class::Timeout);
        assert_eq!(Error::protocol("x", "y").class(), Class::Protocol);
        assert_eq!(Error::new("x", ErrorKind::Limit { what: "f".into(), limit: 1 }).class(), Class::Policy);
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        assert_eq!(Error::new("x", ErrorKind::NoAgent { socket: "/s".into(), cause: denied }).class(), Class::Refused);
        let e = agent("GenericError");
        assert_eq!(e.agent_class(), Some("GenericError"));
        assert!(e.to_string().starts_with("guest-exec: "), "{e}");
        let f: Failure = e.into();
        assert_eq!((f.stage, f.class), (Stage::Host, Class::Unavailable));
    }

    /// The text of the guest goes to a terminal: one line, cut.
    #[test]
    fn the_text_of_the_agent_is_one_line_and_cut() {
        let long = format!("a\nb\u{1b}[31m{}", "x".repeat(2000));
        let e = Error::new("guest-ping", ErrorKind::Agent { class: "GenericError".into(), desc: long });
        let text = e.to_string();
        assert!(!text.contains('\n') && !text.contains('\u{1b}'), "{text:?}");
        assert!(text.chars().count() < 700, "{}", text.len());
    }
}
