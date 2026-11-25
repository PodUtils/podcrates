//! The error of a call. Each error names the call (`POST
//! /1.0/instances/web/exec`) and what went wrong: the error of incus in its
//! own words, a background operation that failed, a socket where no incus
//! answers, an answer that breaks the API, or a value of the caller that
//! cannot go into a request. [`Error::class`] gives the class of
//! `pod_core`; a [`pod_core::Failure`] made from an error is at
//! [`Stage::Host`] and keeps the error as its cause.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use pod_core::text::one_line;
use pod_core::{Class, Failure, Stage};
use pod_proxy::http::{BodySize, TooLarge};

/// The result of a call.
pub type Result<T> = std::result::Result<T, Error>;

/// The longest text of the far end that an error keeps, in characters.
pub(crate) const MAX_TEXT: usize = 512;

/// The characters of the start of a long text of the far end.
const TEXT_HEAD: usize = 256;

/// The characters of the end of the first line, and of the end of the
/// text, that a long text keeps beside its start.
const TEXT_END: usize = 128;

/// The error of one call.
#[derive(Debug)]
pub struct Error {
    call: String,
    kind: ErrorKind,
    /// The background operation of the call, where the error came after
    /// incus started it: a caller can read it (`Client::operation`). The
    /// text of the error stays the same, for a caller that reads the text.
    /// Boxed: a thin pointer keeps the error under the bound of
    /// `clippy::result_large_err`.
    #[expect(clippy::box_collection, reason = "a thin pointer keeps each Result of the crate small")]
    operation: Option<Box<String>>,
}

/// What went wrong.
#[derive(Debug)]
#[non_exhaustive]
pub enum ErrorKind {
    /// No incus answers at the socket: the path does not exist, it is not a
    /// socket, the connection was refused, or this system has no Unix
    /// socket.
    NoServer { socket: PathBuf, cause: io::Error },
    /// incus answered with an error: the HTTP status, the `error_code` and
    /// the `error` of its envelope. `cut` is the size in bytes of an `error`
    /// envelope over `Settings::max_body` that the crate read with each
    /// string cut to its start; `None` where the envelope was whole. The
    /// caller tells "the answer was over the bound" by it, not by the words.
    Incus { status: u16, code: u16, error: String, cut: Option<u64> },
    /// incus refused a write with `If-Match` (412): the ETag of the caller
    /// is not the ETag of the instance now, so the instance changed after
    /// the read. The `error` of incus. The caller reads the instance again,
    /// and decides.
    EtagMismatch { error: String },
    /// A background operation ended with no success: its id, its
    /// `status_code` (400 failure, 401 canceled) and its `err`.
    Operation { id: String, code: u16, err: String },
    /// A wait ended at its time limit; the operation still runs.
    Timeout { id: String, status: String },
    /// The answer of incus breaks the API: no JSON, no envelope, a type that
    /// the call does not take, a field that is not there.
    Protocol(String),
    /// The server does not have what the call needs: an API extension, the
    /// API version.
    Unsupported { what: String },
    /// A value of the caller cannot go into a request.
    Usage(String),
    /// A read or a write failed after the connection opened.
    Io(io::Error),
    /// A websocket of an operation failed.
    WebSocket(String),
    /// The writer of the caller refused a write of the bytes of an answer
    /// (a closed pipe, a full disk): not a fault of incus.
    Output(io::Error),
    /// The answer of incus is larger than `Settings::max_body`, and it is
    /// not an `error` envelope that the crate reads past the bound: the
    /// HTTP status, the size that incus declared or that the crate saw, the
    /// bound, the first bytes of the body, and the `error` of an envelope
    /// of type `error` as far as those bytes hold it (unescaped).
    TooLarge { status: u16, size: BodySize, bound: u64, head: Vec<u8>, error: Option<String> },
    /// incus sent no byte of the head, or of the body, of an answer within
    /// `Settings::answer_timeout`: a socket that takes the connection and
    /// does not answer. The wait for an operation has its own bound.
    AnswerTimeout { phase: AnswerPhase, bound: Duration },
}

/// The part of an answer that a silence of incus stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerPhase {
    /// The status line and the headers.
    Head,
    /// A read of the body.
    Body,
}

impl fmt::Display for AnswerPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AnswerPhase::Head => "head",
            AnswerPhase::Body => "body",
        })
    }
}

/// A silence of incus over the bound of an answer, carried in an
/// `io::Error` to [`Error::io`].
#[derive(Debug)]
pub(crate) struct Silence {
    pub phase: AnswerPhase,
    pub bound: Duration,
}

impl fmt::Display for Silence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "no byte of the {} of the answer in {} ms", self.phase, self.bound.as_millis())
    }
}

impl std::error::Error for Silence {}

impl Error {
    pub(crate) fn new(call: &str, kind: ErrorKind) -> Error {
        Error { call: call.to_string(), kind, operation: None }
    }

    /// The error, with the id of the operation that the call started.
    pub(crate) fn of_operation(mut self, id: &str) -> Error {
        self.operation = Some(Box::new(id.to_string()));
        self
    }

    /// The error, with the size `cut` of an `error` envelope over the bound
    /// that the crate cut: only an error of incus takes it.
    pub(crate) fn cut_at(mut self, cut: Option<u64>) -> Error {
        if let ErrorKind::Incus { cut: at, .. } = &mut self.kind {
            *at = cut;
        }
        self
    }

    /// The error, with a refusal of 412 of incus as
    /// [`ErrorKind::EtagMismatch`]: the error of a write with `If-Match`.
    pub(crate) fn of_condition(self) -> Error {
        match self.kind {
            ErrorKind::Incus { status: 412, error, .. } => Error { kind: ErrorKind::EtagMismatch { error }, ..self },
            _ => self,
        }
    }

    pub(crate) fn protocol(call: &str, why: impl Into<String>) -> Error {
        Error::new(call, ErrorKind::Protocol(why.into()))
    }

    pub(crate) fn usage(call: &str, why: impl Into<String>) -> Error {
        Error::new(call, ErrorKind::Usage(why.into()))
    }

    /// The error of a read or a write; a refusal by size of the reader is
    /// [`ErrorKind::TooLarge`].
    pub(crate) fn io(call: &str, e: io::Error) -> Error {
        if let Some(silence) = e.get_ref().and_then(|inner| inner.downcast_ref::<Silence>()) {
            let (phase, bound) = (silence.phase, silence.bound);
            return Error::new(call, ErrorKind::AnswerTimeout { phase, bound });
        }
        match TooLarge::of(&e) {
            Some(refusal) => Error::too_large(call, refusal),
            None => Error::new(call, ErrorKind::Io(e)),
        }
    }

    pub(crate) fn too_large(call: &str, refusal: &TooLarge) -> Error {
        let mut skim = crate::skim::Skim::new(crate::skim::KEPT_TEXT);
        skim.feed(&refusal.head);
        let error = (skim.top_type() == Some("error")).then(|| skim.partial_error()).flatten();
        let (status, size, bound, head) =
            (refusal.status.unwrap_or(0), refusal.size, refusal.bound, refusal.head.clone());
        Error::new(call, ErrorKind::TooLarge { status, size, bound, head, error })
    }

    /// The call, as its method and its path: `GET /1.0`.
    pub fn call(&self) -> &str {
        &self.call
    }

    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    /// The id of the background operation of the call, where incus started
    /// one: an error of its wait, an operation that failed, a wait at its
    /// limit.
    pub fn operation(&self) -> Option<&str> {
        match &self.kind {
            ErrorKind::Operation { id, .. } | ErrorKind::Timeout { id, .. } => Some(id),
            _ => self.operation.as_deref().map(String::as_str),
        }
    }

    /// The `error` of incus, when incus answered with one.
    pub fn incus_error(&self) -> Option<&str> {
        match &self.kind {
            ErrorKind::Incus { error, .. } | ErrorKind::EtagMismatch { error } => Some(error),
            ErrorKind::Operation { err, .. } => Some(err),
            ErrorKind::TooLarge { error, .. } => error.as_deref(),
            _ => None,
        }
    }

    /// The HTTP status of an error answer of incus.
    pub fn status(&self) -> Option<u16> {
        match &self.kind {
            ErrorKind::Incus { status, .. } => Some(*status),
            ErrorKind::EtagMismatch { .. } => Some(412),
            _ => None,
        }
    }

    /// What a caller can do about the error.
    pub fn class(&self) -> Class {
        match &self.kind {
            ErrorKind::NoServer { cause, .. } => match cause.kind() {
                io::ErrorKind::PermissionDenied => Class::Refused,
                io::ErrorKind::TimedOut => Class::Timeout,
                io::ErrorKind::Unsupported => Class::Unsupported,
                _ => Class::Unavailable,
            },
            ErrorKind::Incus { status, .. } => match status {
                400 => Class::Usage,
                401 | 403 => Class::Refused,
                404 => Class::NotFound,
                409 | 412 => Class::TempFail,
                501 => Class::Unsupported,
                503 => Class::Unavailable,
                _ => Class::Protocol,
            },
            ErrorKind::EtagMismatch { .. } => Class::TempFail,
            ErrorKind::Operation { code: 401, .. } => Class::TempFail,
            ErrorKind::Operation { .. } => Class::Unavailable,
            ErrorKind::Timeout { .. } | ErrorKind::AnswerTimeout { .. } => Class::Timeout,
            ErrorKind::Protocol(_) | ErrorKind::WebSocket(_) => Class::Protocol,
            ErrorKind::Unsupported { .. } => Class::Unsupported,
            ErrorKind::Usage(_) => Class::Usage,
            ErrorKind::Io(e) | ErrorKind::Output(e) => Class::of_io(e.kind()),
            ErrorKind::TooLarge { .. } => Class::of_io(io::ErrorKind::InvalidData),
        }
    }

    /// The stage of a [`Failure`] of this crate: the local incus.
    pub fn stage(&self) -> Stage {
        Stage::Host
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let call = &self.call;
        match &self.kind {
            ErrorKind::NoServer { socket, cause } => {
                write!(f, "{call}: no incus answers at {}: {cause}", socket.display())
            }
            ErrorKind::Incus { status, code, error, .. } => {
                write!(f, "{call}: incus refused the call ({status}, error_code {code}): {}", one_line(error))
            }
            ErrorKind::EtagMismatch { error } => {
                write!(f, "{call}: the instance changed after the read of its ETag (412): {}", one_line(error))
            }
            ErrorKind::Operation { id, code, err } => {
                let how = if *code == 401 { "was canceled" } else { "failed" };
                write!(f, "{call}: the operation {id} {how} ({code}): {}", one_line(err))
            }
            ErrorKind::Timeout { id, status } => {
                write!(f, "{call}: the wait for the operation {id} ended at its limit; it is {status}")
            }
            ErrorKind::Protocol(why) => write!(f, "{call}: the answer of incus breaks the API: {why}"),
            ErrorKind::Unsupported { what } => write!(f, "{call}: the incus server does not have {what}"),
            ErrorKind::Usage(why) => write!(f, "{call}: {why}"),
            ErrorKind::Io(e) => write!(f, "{call}: {e}"),
            ErrorKind::WebSocket(why) => write!(f, "{call}: a websocket of the operation failed: {why}"),
            ErrorKind::Output(e) => write!(f, "{call}: the write of the output: {e}"),
            ErrorKind::AnswerTimeout { phase, bound } => {
                write!(f, "{call}: incus sent no byte of the {phase} of its answer in {} ms", bound.as_millis())
            }
            ErrorKind::TooLarge { status, size, bound, head, error } => {
                write!(
                    f,
                    "{call}: the answer of incus (HTTP {status}) is larger than allowed: {size}, over the bound of {bound} bytes"
                )?;
                match error {
                    Some(error) => write!(f, "; incus said: {}", far_text(error)),
                    None if head.is_empty() => Ok(()),
                    None => write!(f, "; the body starts with {:?}", far_text(&String::from_utf8_lossy(head))),
                }
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            ErrorKind::NoServer { cause, .. } | ErrorKind::Io(cause) | ErrorKind::Output(cause) => Some(cause),
            _ => None,
        }
    }
}

impl From<Error> for Failure {
    fn from(e: Error) -> Failure {
        Failure::caused(e.stage(), e.class(), e.to_string(), e)
    }
}

/// `text` of the far end on one line. A text over [`MAX_TEXT`]
/// characters keeps its start, the end of its first line and its end, with
/// ` … ` where a part goes: a Go error that wraps the stderr of a command
/// has the cause at the end of the first line of the stderr, after a
/// command line and a path that fill the start.
pub(crate) fn far_text(text: &str) -> String {
    let line = one_line(text);
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let n = chars.len();
    if n <= MAX_TEXT {
        return line;
    }
    let first = one_line(text.split('\n').next().unwrap_or_default()).chars().count().min(n);
    let mut parts = vec![(0, TEXT_HEAD)];
    if first > TEXT_HEAD {
        parts.push((first.saturating_sub(TEXT_END).max(TEXT_HEAD), first));
        if n > first {
            parts.push((n.saturating_sub(TEXT_END).max(first), n));
        }
    } else {
        parts.push((n.saturating_sub(2 * TEXT_END).max(TEXT_HEAD), n));
    }
    let byte = |at: usize| chars.get(at).map_or(line.len(), |(byte, _)| *byte);
    let mut out = String::new();
    let mut end = 0;
    for (from, to) in parts.into_iter().filter(|(from, to)| from < to) {
        if from > end {
            out.push_str(" … ");
        }
        out.push_str(&line[byte(from.max(end))..byte(to)]);
        end = to;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text of incusd 6.0 on a full storage pool of the driver `dir`
    /// (PodUtils/podvm#651): the command line of `tar` and a long path fill
    /// the first 512 characters, and the cause ends the first line.
    #[test]
    fn a_long_text_keeps_the_end_of_its_first_line_and_its_end() {
        let path =
            format!("rootfs/usr/lib/python3.12/site-packages/{}/__init__.py", "very-long-package-name/".repeat(8));
        let mut text = format!(
            "Failed creating instance from image: Unpack failed: Failed to run: tar --wildcards --numeric-owner \
             --xattrs-include=* --restrict --force-local -C /var/lib/incus/storage-pools/default/images/abc/rootfs \
             --exclude=rootfs/dev/* -xf -: exit status 2 (tar: {path}: Cannot open: No space left on device"
        );
        for i in 0..200 {
            text.push_str(&format!("\ntar: rootfs/usr/share/doc/{i}: Cannot mkdir: No such file or directory"));
        }
        text.push_str("\ntar: Exiting with failure status due to previous errors)");
        let kept = far_text(&text);
        assert!(kept.starts_with("Failed creating instance from image: Unpack failed: Failed to run: tar "), "{kept}");
        assert!(kept.contains("Cannot open: No space left on device … "), "{kept}");
        assert!(kept.ends_with("Exiting with failure status due to previous errors)"), "{kept}");
        assert!(kept.chars().count() <= MAX_TEXT + 6, "{} characters", kept.chars().count());
        // The cut of 512 characters lost the cause.
        let old: String = one_line(&text).chars().take(MAX_TEXT).collect();
        assert!(!old.contains("No space left on device"), "{old}");
    }

    #[test]
    fn a_short_text_and_a_long_line_are_cut_by_the_same_rule() {
        assert_eq!(far_text("Instance not found"), "Instance not found");
        let line = format!("{}{}", "a".repeat(600), "END");
        let kept = far_text(&line);
        assert!(kept.starts_with(&"a".repeat(TEXT_HEAD)) && kept.ends_with("aEND"), "{kept}");
        assert!(kept.contains(" … "), "{kept}");
        let lines = format!("start\n{}\nlast words", "b ".repeat(400));
        let kept = far_text(&lines);
        assert!(kept.starts_with("start b b") && kept.ends_with("b last words"), "{kept}");
    }
}
