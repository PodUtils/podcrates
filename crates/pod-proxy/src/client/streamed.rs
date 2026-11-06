//! The answer of [`super::Client::send_streamed`]: its head, and its body as
//! a reader over the connection that it owns.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use pod_core::{Class, Failure, Stage};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};

use super::MAX_HEAD;
use crate::http::{self, Body, Framing, Idle, Response};

/// The read half of the connection of an answer.
type Reader = Box<dyn AsyncRead + Send + Unpin>;

/// The answer to a request, with its body still on the connection. It reads
/// the body as it comes ([`AsyncRead`]): by `Content-Length`, by chunks, or
/// to the close. A body that ends before its length or its last chunk is an
/// `UnexpectedEof` error; no bytes in the request's
/// [`super::Request::body_idle`] is a `TimedOut` error. The answer to a
/// `HEAD`, 1xx, 204 and 304 has no body.
pub struct Streamed {
    pub status: u16,
    pub reason: String,
    /// Header names lower-cased, values trimmed, in arrival order.
    pub headers: Vec<(String, String)>,
    /// The URL that gave this answer: after redirects, the last one.
    pub url: String,
    body: Body<Idle<Reader>>,
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
    pub(super) async fn into_response(self) -> Result<Response, Failure> {
        let Streamed { status, reason, headers, url, body } = self;
        let body = body.to_end().await.map_err(reading_failure)?;
        Ok(Response { status, reason, headers, body, url })
    }
}

impl AsyncRead for Streamed {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, out: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.body).poll_read(cx, out)
    }
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
pub(super) async fn start<S>(mut io: S, url: &str, head: &str, body: &[u8], bounds: Bounds) -> Result<Streamed, Failure>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let sent = async {
        io.write_all(head.as_bytes()).await?;
        io.write_all(body).await?;
        io.flush().await
    };
    sent.await
        .map_err(|e| Failure::caused(Stage::Connect, Class::of_io(e.kind()), format!("sending the request: {e}"), e))?;
    let mut io: Reader = Box::new(io);
    let (answer, rest) = http::read_head(&mut io, Vec::new(), MAX_HEAD).await.map_err(reading_failure)?;
    let framing = Framing::of(&answer, bounds.head_only);
    let body = Body::new(Idle::new(io, bounds.idle), rest, framing, bounds.limit).map_err(reading_failure)?;
    let (status, reason, headers, url) = (answer.status, answer.reason, answer.headers, url.to_string());
    Ok(Streamed { status, reason, headers, url, body })
}

fn reading_failure(e: io::Error) -> Failure {
    Failure::caused(Stage::Connect, Class::of_io(e.kind()), format!("reading the answer: {e}"), e)
}
