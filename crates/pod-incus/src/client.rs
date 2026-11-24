//! The client: the settings that the caller fills (the path of the socket,
//! the project, the limits), and the shape of each call: one connection,
//! one request, one answer; a sync answer gives its metadata, an async one
//! its operation, which the crate waits for.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pod_core::BoxedIo;
use pod_proxy::http::Response;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::envelope::{self, Reply};
use crate::error::{Error, ErrorKind, Result};
use crate::operation::Operation;
use crate::wire::{self, Body, Request};

/// The settings of a client. The crate reads no variable of the
/// environment: the caller gives the path of the socket
/// (`INCUS_DIR/unix.socket` of the `incusd` that it uses).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The Unix socket of the REST API.
    pub socket: PathBuf,
    /// The project of each call (`?project=`); `None`: the server's default.
    pub project: Option<String>,
    /// The `User-Agent` of each request; empty: none.
    pub user_agent: String,
    /// The limit of the connection to the socket.
    pub connect_timeout: Duration,
    /// The largest body of an answer that the crate reads whole: an
    /// envelope, a file, a log of exec, the console log.
    pub max_body: usize,
    /// The limit of each wait for a background operation; `None`: no limit.
    pub wait: Option<Duration>,
    /// The limit of the write of each frame on a websocket of exec.
    pub write_timeout: Duration,
    /// The limit of the silence of incus in an answer: the head, and each
    /// read of the body ([`ErrorKind::AnswerTimeout`]). A wait for an
    /// operation is not read under it: it has its own bound (`wait`).
    /// `None`: no limit.
    pub answer_timeout: Option<Duration>,
}

impl Settings {
    /// The settings for the socket at `socket`: the default project, no
    /// user agent, 5 s for the connection, 64 MiB for a body, no limit for
    /// a wait, 30 s for the write of a frame, 30 s of silence in an answer.
    pub fn new(socket: impl Into<PathBuf>) -> Settings {
        Settings {
            socket: socket.into(),
            project: None,
            user_agent: String::new(),
            connect_timeout: Duration::from_secs(5),
            max_body: 64 * 1024 * 1024,
            wait: None,
            write_timeout: Duration::from_secs(30),
            // incusd answers each call that is not a wait at once; a socket
            // that takes the connection and says nothing must not hold the
            // caller (PodUtils/podvm#653).
            answer_timeout: Some(Duration::from_secs(30)),
        }
    }
}

/// The bound of the read of an answer that incus sent before it closed a
/// connection whose request was still going out: the answer is in the
/// buffer of the socket, or it does not come.
const LATE_ANSWER: Duration = Duration::from_secs(5);

/// A client of one incus server. Each call opens its own connection, so a
/// client can make calls from many tasks at once.
///
/// SIGPIPE: incus can refuse a body (an upload, a file) before its end and
/// close the connection. The next write of the crate to that socket raises
/// SIGPIPE, and a process that does not ignore SIGPIPE ends there, with no
/// error and no answer of incus. The crate sets no signal: the caller sets
/// SIGPIPE to ignore before its first upload (the runtime of Rust does so
/// for a binary, unless the caller set it back). With SIGPIPE ignored, the
/// write fails with EPIPE, and the crate reads and gives the answer of
/// incus.
#[derive(Debug, Clone)]
pub struct Client {
    settings: Arc<Settings>,
}

impl Client {
    pub fn new(settings: Settings) -> Client {
        Client { settings: Arc::new(settings) }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// `path` with `query` and the project.
    pub(crate) fn target(&self, path: &str, query: &[(&str, &str)]) -> String {
        let mut query = query.to_vec();
        if let Some(project) = &self.settings.project {
            query.push(("project", project));
        }
        wire::target(path, &query)
    }

    /// A connection to the socket.
    pub(crate) async fn open(&self, call: &str) -> Result<BoxedIo> {
        wire::connect(call, &self.settings.socket, self.settings.connect_timeout).await
    }

    /// One request and its answer, whose body has `max` bytes at most.
    pub(crate) async fn exchange(&self, call: &str, request: Request, max: usize) -> Result<Response> {
        match self.request(call, request).await? {
            Sent::Answer(answer) => Ok(answer),
            Sent::Stream(mut stream) => {
                wire::read(&mut stream, max, self.settings.answer_timeout).await.map_err(|e| Error::io(call, e))
            }
        }
    }

    /// One request on a new connection: the connection for its answer.
    async fn request(&self, call: &str, request: Request) -> Result<Sent> {
        let mut stream = self.open(call).await?;
        tracing::debug!(call, "incus call");
        if let Err(e) = wire::send(&mut stream, &self.settings.user_agent, request).await {
            // incus can refuse a body before its end and close: its answer
            // then says why, where the write only sees the close.
            let gone = matches!(e.kind(), io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset);
            if gone && let Some(answer) = self.answer_after_cut(call, &mut stream).await? {
                return Ok(Sent::Answer(answer));
            }
            return Err(sent(call, e));
        }
        Ok(Sent::Stream(stream))
    }

    /// The answer that incus sent before it closed a connection whose
    /// request was still going out, within [`LATE_ANSWER`] (or
    /// `answer_timeout`, where it is shorter). `None` where no head came:
    /// the error of the write then says what happened. An error of the
    /// body (a refusal by size, a silence) is the error of the call: the
    /// answer came, and it says more than the close.
    async fn answer_after_cut(&self, call: &str, stream: &mut BoxedIo) -> Result<Option<Response>> {
        let bound = Some(self.settings.answer_timeout.map_or(LATE_ANSWER, |bound| bound.min(LATE_ANSWER)));
        let Ok((mut answer, rest)) = wire::read_head_within(stream, bound).await else { return Ok(None) };
        let max = self.settings.max_body;
        let (body, skimmed) =
            wire::envelope_body(stream, &answer, rest, max, bound).await.map_err(|e| Error::io(call, e))?;
        if skimmed {
            tracing::debug!(call, "an error envelope over the bound, read with its strings cut");
        }
        answer.body = body;
        Ok(Some(answer))
    }

    /// One request whose answer is an envelope. An `error` envelope over
    /// `max_body` is read with its strings cut ([`wire::read_envelope`]).
    pub(crate) async fn envelope(&self, call: &str, request: Request) -> Result<Reply> {
        self.envelope_within(call, request, self.settings.answer_timeout).await
    }

    /// [`Client::envelope`] with `bound` for the silence of the answer: the
    /// wait for an operation gives `None` and keeps its own bound.
    pub(crate) async fn envelope_within(&self, call: &str, request: Request, bound: Option<Duration>) -> Result<Reply> {
        let answer = match self.request(call, request).await? {
            Sent::Answer(answer) => answer,
            Sent::Stream(mut stream) => {
                let max = self.settings.max_body;
                let (answer, skimmed) =
                    wire::read_envelope(&mut stream, max, bound).await.map_err(|e| Error::io(call, e))?;
                if skimmed {
                    tracing::debug!(call, "an error envelope over the bound, read with its strings cut");
                }
                answer
            }
        };
        envelope::read(call, answer.status, &answer.body)
    }

    /// `GET path?query`: the metadata of its sync answer.
    pub(crate) async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, &str)]) -> Result<T> {
        let call = format!("GET {path}");
        let request =
            Request { method: "GET", target: self.target(path, query), headers: Vec::new(), body: Body::Empty };
        envelope::sync(&call, self.envelope(&call, request).await?)
    }

    /// `METHOD path` with a JSON body: the metadata of its sync answer.
    pub(crate) async fn send_json<T: DeserializeOwned>(
        &self,
        method: &'static str,
        path: &str,
        body: &impl Serialize,
    ) -> Result<T> {
        let call = format!("{method} {path}");
        let request = self.json_request(&call, method, path, body)?;
        envelope::sync(&call, self.envelope(&call, request).await?)
    }

    /// `METHOD path` with `body` and `headers`: the background operation of
    /// its answer, waited for to its success.
    pub(crate) async fn run(
        &self,
        method: &'static str,
        path: &str,
        headers: Vec<(String, String)>,
        body: Body,
    ) -> Result<Operation> {
        let call = format!("{method} {path}");
        let operation = self.begin(&call, Request { method, target: self.target(path, &[]), headers, body }).await?;
        self.finish(&call, &operation.id, self.settings.wait).await
    }

    /// `METHOD path` with a JSON body: [`Client::run`].
    pub(crate) async fn run_json(&self, method: &'static str, path: &str, body: &impl Serialize) -> Result<Operation> {
        let call = format!("{method} {path}");
        let data = serde_json::to_vec(body).map_err(|e| Error::usage(&call, format!("the JSON of the body: {e}")))?;
        self.run(method, path, Vec::new(), Body::Bytes { content_type: "application/json", data }).await
    }

    /// The operation of the async answer of `request`.
    pub(crate) async fn begin(&self, call: &str, request: Request) -> Result<Operation> {
        let operation = envelope::operation(call, self.envelope(call, request).await?)?;
        tracing::debug!(call, id = operation.id, "incus operation started");
        Ok(operation)
    }

    pub(crate) fn json_request(
        &self,
        call: &str,
        method: &'static str,
        path: &str,
        body: &impl Serialize,
    ) -> Result<Request> {
        let data = serde_json::to_vec(body).map_err(|e| Error::usage(call, format!("the JSON of the body: {e}")))?;
        let body = Body::Bytes { content_type: "application/json", data };
        Ok(Request { method, target: self.target(path, &[]), headers: Vec::new(), body })
    }

    /// `GET path?query` whose good answer is raw bytes (200), and whose
    /// other answer is an error envelope.
    pub(crate) async fn raw(&self, path: &str, query: &[(&str, &str)]) -> Result<Response> {
        let call = format!("GET {path}");
        let request =
            Request { method: "GET", target: self.target(path, query), headers: Vec::new(), body: Body::Empty };
        let answer = self.exchange(&call, request, self.settings.max_body).await?;
        if answer.status == 200 {
            return Ok(answer);
        }
        Err(refused(&call, answer.status, &answer.reason, &answer.body))
    }
}

/// The error of an answer of `status` that is not the good answer of a
/// call: the error envelope of incus, or the status and its reason.
pub(crate) fn refused(call: &str, status: u16, reason: &str, body: &[u8]) -> Error {
    match envelope::read(call, status, body) {
        Err(e) => e,
        Ok(_) => Error::new(call, ErrorKind::Incus { status, code: status, error: reason.to_string() }),
    }
}

/// A request that went out: the connection of its answer, or the answer
/// that incus gave before the end of the request.
enum Sent {
    Stream(BoxedIo),
    Answer(Response),
}

/// The error of a request that did not go out whole.
fn sent(call: &str, e: io::Error) -> Error {
    if e.kind() == io::ErrorKind::InvalidInput { Error::usage(call, e.to_string()) } else { Error::io(call, e) }
}
