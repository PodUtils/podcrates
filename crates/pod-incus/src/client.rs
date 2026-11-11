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
}

impl Settings {
    /// The settings for the socket at `socket`: the default project, no
    /// user agent, 5 s for the connection, 64 MiB for a body, no limit for
    /// a wait, 30 s for the write of a frame.
    pub fn new(socket: impl Into<PathBuf>) -> Settings {
        Settings {
            socket: socket.into(),
            project: None,
            user_agent: String::new(),
            connect_timeout: Duration::from_secs(5),
            max_body: 64 * 1024 * 1024,
            wait: None,
            write_timeout: Duration::from_secs(30),
        }
    }
}

/// A client of one incus server. Each call opens its own connection, so a
/// client can make calls from many tasks at once.
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
        let mut stream = self.open(call).await?;
        tracing::debug!(call, "incus call");
        if let Err(e) = wire::send(&mut stream, &self.settings.user_agent, request).await {
            // incus can refuse a body before its end and close: its answer
            // then says why, where the write only sees the close.
            let gone = matches!(e.kind(), io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset);
            if gone && let Ok(answer) = wire::read(&mut stream, self.settings.max_body).await {
                return Ok(answer);
            }
            return Err(sent(call, e));
        }
        wire::read(&mut stream, max).await.map_err(|e| Error::io(call, e))
    }

    /// One request whose answer is an envelope.
    pub(crate) async fn envelope(&self, call: &str, request: Request) -> Result<Reply> {
        let answer = self.exchange(call, request, self.settings.max_body).await?;
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
        match envelope::read(&call, answer.status, &answer.body) {
            Err(e) => Err(e),
            Ok(_) => Err(Error::new(
                &call,
                ErrorKind::Incus { status: answer.status, code: answer.status, error: answer.reason.clone() },
            )),
        }
    }
}

/// The error of a request that did not go out whole.
fn sent(call: &str, e: io::Error) -> Error {
    if e.kind() == io::ErrorKind::InvalidInput { Error::usage(call, e.to_string()) } else { Error::io(call, e) }
}
