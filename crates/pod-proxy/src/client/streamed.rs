//! The answer of [`super::Client::send_streamed`]: its head, and its body as
//! a reader over the connection that it owns.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use pod_core::{BoxedIo, Class, Failure, Stage};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, ReadBuf};

use super::MAX_HEAD;
use super::pool::{Key, Pool};
use crate::http::{self, Body, Framing, Idle, Response};

/// The answer to a request, with its body still on the connection. It reads
/// the body as it comes ([`AsyncRead`]): by `Content-Length`, by chunks, or
/// to the close. A body that ends before its length or its last chunk is an
/// `UnexpectedEof` error; no bytes in the request's
/// [`super::Request::body_idle`] is a `TimedOut` error. The answer to a
/// `HEAD`, 1xx, 204 and 304 has no body. A drop after the end of the body
/// gives the connection back to the pool of the client
/// ([`super::Client::with_keep_alive`]); a drop before it closes the
/// connection.
pub struct Streamed {
    pub status: u16,
    pub reason: String,
    /// Header names lower-cased, values trimmed, in arrival order.
    pub headers: Vec<(String, String)>,
    /// The URL that gave this answer: after redirects, the last one.
    pub url: String,
    /// `None` only during the drop.
    body: Option<Body<Idle<BoxedIo>>>,
    /// The connection can carry a next request: `HTTP/1.1`, no
    /// `Connection: close`, not an upgrade.
    reusable: bool,
    /// The pool that takes the connection back after the end of the body.
    home: Option<(Arc<Pool>, Key)>,
}

impl std::fmt::Debug for Streamed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Streamed").field("status", &self.status).field("reason", &self.reason).finish_non_exhaustive()
    }
}

impl Streamed {
    /// A status of 2xx.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The first value of a header, by case-insensitive name.
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers.iter().find(|(n, _)| *n == name).map(|(_, v)| v.as_str())
    }

    /// The whole answer, its body read into memory.
    pub(super) async fn into_response(mut self) -> Result<Response, Failure> {
        let mut body = Vec::new();
        self.read_to_end(&mut body).await.map_err(reading_failure)?;
        let (status, reason, headers, url) = (
            self.status,
            std::mem::take(&mut self.reason),
            std::mem::take(&mut self.headers),
            std::mem::take(&mut self.url),
        );
        Ok(Response { status, reason, headers, body, url })
    }

    /// Give the connection back to `pool` under `key` after the end of the
    /// body, when it can carry a next request.
    pub(super) fn homed(mut self, pool: &Arc<Pool>, key: Key) -> Streamed {
        if self.reusable {
            self.home = Some((pool.clone(), key));
        }
        self
    }
}

impl AsyncRead for Streamed {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, out: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        match &mut self.body {
            Some(body) => Pin::new(body).poll_read(cx, out),
            None => Poll::Ready(Ok(())),
        }
    }
}

impl Drop for Streamed {
    fn drop(&mut self) {
        let (Some((pool, key)), Some(body)) = (self.home.take(), self.body.take()) else { return };
        if let Some(idle) = body.into_reusable() {
            pool.put(key, idle.into_inner());
        }
    }
}

/// A request that failed on its connection.
pub(super) struct Failed {
    pub failure: Failure,
    /// No byte of the answer came: a stale idle connection fails so, and an
    /// idempotent request can go again on a new one.
    pub silent: bool,
}

/// How the body of an answer is read.
#[derive(Clone, Copy)]
pub(super) struct Bounds {
    pub head_only: bool,
    /// The largest body; `None`: no bound.
    pub limit: Option<u64>,
    /// The longest wait for the next bytes of the body; `None`: no limit.
    pub idle: Option<Duration>,
}

/// Write the head and the body of a request to `url` on `io`, and read the
/// head of the answer.
pub(super) async fn start(
    mut io: BoxedIo,
    url: &str,
    head: &str,
    body: &[u8],
    bounds: Bounds,
) -> Result<Streamed, Failed> {
    let sent = async {
        io.write_all(head.as_bytes()).await?;
        io.write_all(body).await?;
        io.flush().await
    };
    sent.await.map_err(|e| {
        let failure = Failure::caused(Stage::Connect, Class::of_io(e.kind()), format!("sending the request: {e}"), e);
        Failed { failure, silent: true }
    })?;
    let mut first = vec![0u8; 4096];
    let n = match io.read(&mut first).await {
        Ok(n) => n,
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => 0,
        Err(e) => return Err(Failed { failure: reading_failure(e), silent: true }),
    };
    if n == 0 {
        let e = io::Error::new(io::ErrorKind::UnexpectedEof, "the connection closed before the response headers ended");
        return Err(Failed { failure: reading_failure(e), silent: true });
    }
    first.truncate(n);
    let loud = |e| Failed { failure: reading_failure(e), silent: false };
    let (answer, rest, http11) = http::read_head_versioned(&mut io, first, MAX_HEAD).await.map_err(loud)?;
    let framing = Framing::of(&answer, bounds.head_only);
    let body = Body::new(Idle::new(io, bounds.idle), rest, framing, bounds.limit).map_err(loud)?;
    let closes = answer
        .headers
        .iter()
        .filter(|(name, _)| name == "connection")
        .flat_map(|(_, value)| value.split(','))
        .any(|token| token.trim().eq_ignore_ascii_case("close"));
    let reusable = http11 && !closes && answer.status >= 200;
    let (status, reason, headers, url) = (answer.status, answer.reason, answer.headers, url.to_string());
    Ok(Streamed { status, reason, headers, url, body: Some(body), reusable, home: None })
}

fn reading_failure(e: io::Error) -> Failure {
    Failure::caused(Stage::Connect, Class::of_io(e.kind()), format!("reading the answer: {e}"), e)
}
