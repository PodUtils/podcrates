//! The body of a response as a reader: by `Content-Length`, by chunks, or
//! to the end of the stream. The buffered reading ([`super::read_answer`])
//! and the streamed answer of [`crate::client::Streamed`] both read through
//! it, so that the two keep the same framing and the same errors.

use std::future::Future as _;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, ready};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, ReadBuf};
use tokio::time::Sleep;

use super::{Response, find, invalid};

/// The longest line of a chunk size or of a trailer.
const MAX_LINE: usize = 4096;

/// The most bytes of trailers after the last chunk.
const MAX_TRAILERS: usize = super::MAX_HEAD;

/// How the end of a body is found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    /// No body: the answer to a `HEAD`, 1xx, 204 and 304.
    Empty,
    /// `Content-Length` bytes.
    Length(u64),
    /// `Transfer-Encoding: chunked`.
    Chunked,
    /// Each byte until the peer closes.
    ToEnd,
}

impl Framing {
    /// The framing of the body of `head`; `head_only` for the answer to a
    /// `HEAD`. A `Content-Length` that is not a number reads to the end.
    pub fn of(head: &Response, head_only: bool) -> Framing {
        if head_only || head.status < 200 || head.status == 204 || head.status == 304 {
            return Framing::Empty;
        }
        if head.header("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked")) {
            return Framing::Chunked;
        }
        match head.header("content-length").and_then(|v| v.parse::<u64>().ok()) {
            Some(length) => Framing::Length(length),
            None => Framing::ToEnd,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Length(u64),
    ToEnd,
    ChunkSize,
    ChunkData(u64),
    ChunkEnd,
    Trailers(usize),
    Done,
}

/// The body of one response over `S`: the bytes after the head that were
/// already read, then the stream. The end of a TLS stream with no
/// close_notify is the end of the stream; a framed body that ends early is
/// an `UnexpectedEof` error.
#[derive(Debug)]
pub struct Body<S> {
    stream: S,
    buf: Vec<u8>,
    at: usize,
    state: State,
    /// The bytes that the body may still have; `None`: no bound.
    left: Option<u64>,
}

impl<S: AsyncRead + Unpin> Body<S> {
    /// The body of `framing` over `stream`, which starts with `rest`. With
    /// `limit`, a body larger than `limit` bytes is an `InvalidData` error;
    /// a `Content-Length` larger than it fails here.
    pub fn new(stream: S, rest: Vec<u8>, framing: Framing, limit: Option<u64>) -> io::Result<Body<S>> {
        let state = match framing {
            Framing::Empty | Framing::Length(0) => State::Done,
            Framing::Length(length) if limit.is_some_and(|max| length > max) => return Err(too_large()),
            Framing::Length(length) => State::Length(length),
            Framing::Chunked => State::ChunkSize,
            Framing::ToEnd => State::ToEnd,
        };
        Ok(Body { stream, buf: rest, at: 0, state, left: limit })
    }

    /// Each byte of the body.
    pub async fn to_end(mut self) -> io::Result<Vec<u8>> {
        let mut body = Vec::new();
        self.read_to_end(&mut body).await?;
        Ok(body)
    }

    /// Up to `max` bytes into `out`: the buffered ones first, then a read
    /// of the stream. `Ok(0)` at the end of the stream.
    fn poll_take(&mut self, cx: &mut Context<'_>, out: &mut ReadBuf<'_>, max: u64) -> Poll<io::Result<usize>> {
        let want = usize::try_from(max).unwrap_or(usize::MAX).min(out.remaining());
        let have = self.buf.len() - self.at;
        if have > 0 {
            let n = want.min(have);
            out.put_slice(&self.buf[self.at..self.at + n]);
            self.at += n;
            return Poll::Ready(Ok(n));
        }
        let mut into = ReadBuf::new(out.initialize_unfilled_to(want));
        ready!(poll_read_some(Pin::new(&mut self.stream), cx, &mut into))?;
        let n = into.filled().len();
        out.advance(n);
        Poll::Ready(Ok(n))
    }

    /// Read more of the stream into the buffer. `Ok(0)` at its end.
    fn poll_fill(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        if self.at > 0 {
            self.buf.drain(..self.at);
            self.at = 0;
        }
        let mut chunk = [0u8; 4096];
        let mut into = ReadBuf::new(&mut chunk);
        ready!(poll_read_some(Pin::new(&mut self.stream), cx, &mut into))?;
        self.buf.extend_from_slice(into.filled());
        Poll::Ready(Ok(into.filled().len()))
    }

    /// The next line of the framing, with no CRLF.
    fn poll_line(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<Vec<u8>>> {
        loop {
            if let Some(end) = find(&self.buf[self.at..], b"\r\n") {
                let line = self.buf[self.at..self.at + end].to_vec();
                self.at += end + 2;
                return Poll::Ready(Ok(line));
            }
            if self.buf.len() - self.at > MAX_LINE {
                return Poll::Ready(Err(invalid("a line of the chunked body is too long")));
            }
            if ready!(self.poll_fill(cx))? == 0 {
                return Poll::Ready(Err(cut_short()));
            }
        }
    }

    /// Take `n` bytes of the body from the bound.
    fn spend(&mut self, n: u64) -> io::Result<()> {
        if let Some(left) = &mut self.left {
            *left = left.checked_sub(n).ok_or_else(too_large)?;
        }
        Ok(())
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Body<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, out: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        if out.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        loop {
            match this.state {
                State::Done => return Poll::Ready(Ok(())),
                State::Length(left) => {
                    let n = ready!(this.poll_take(cx, out, left))?;
                    if n == 0 {
                        return Poll::Ready(Err(cut_short()));
                    }
                    let left = left - n as u64;
                    this.state = if left == 0 { State::Done } else { State::Length(left) };
                    return Poll::Ready(Ok(()));
                }
                // A reader must not fill `out` and fail: past the bound, the
                // next bytes go to the buffer and fail there.
                State::ToEnd if this.left == Some(0) => {
                    if this.buf.len() > this.at || ready!(this.poll_fill(cx))? > 0 {
                        return Poll::Ready(Err(too_large()));
                    }
                    this.state = State::Done;
                }
                State::ToEnd => {
                    let n = ready!(this.poll_take(cx, out, this.left.unwrap_or(u64::MAX)))?;
                    if n == 0 {
                        this.state = State::Done;
                    }
                    return Poll::Ready(this.spend(n as u64));
                }
                State::ChunkSize => {
                    let line = ready!(this.poll_line(cx))?;
                    let size = chunk_size(&line)?;
                    this.state = if size == 0 {
                        State::Trailers(0)
                    } else {
                        this.spend(size)?;
                        State::ChunkData(size)
                    };
                }
                State::ChunkData(left) => {
                    let n = ready!(this.poll_take(cx, out, left))?;
                    if n == 0 {
                        return Poll::Ready(Err(cut_short()));
                    }
                    let left = left - n as u64;
                    this.state = if left == 0 { State::ChunkEnd } else { State::ChunkData(left) };
                    return Poll::Ready(Ok(()));
                }
                State::ChunkEnd => {
                    while this.buf.len() - this.at < 2 {
                        if ready!(this.poll_fill(cx))? == 0 {
                            return Poll::Ready(Err(cut_short()));
                        }
                    }
                    if &this.buf[this.at..this.at + 2] != b"\r\n" {
                        return Poll::Ready(Err(invalid("a chunk is not followed by CRLF")));
                    }
                    this.at += 2;
                    this.state = State::ChunkSize;
                }
                State::Trailers(seen) => {
                    let line = ready!(this.poll_line(cx))?;
                    let seen = seen + line.len() + 2;
                    if seen > MAX_TRAILERS {
                        return Poll::Ready(Err(invalid("the trailers of the chunked body are too long")));
                    }
                    this.state = if line.is_empty() { State::Done } else { State::Trailers(seen) };
                }
            }
        }
    }
}

/// The size of a chunk from its line; its extensions are passed over.
fn chunk_size(line: &[u8]) -> io::Result<u64> {
    let line = std::str::from_utf8(line).map_err(|_| invalid("a chunk size line is not text"))?;
    let hex = line.split(';').next().unwrap_or_default().trim();
    match u64::from_str_radix(hex, 16) {
        Ok(size) => Ok(size),
        // Hex digits past the width of u64: a size larger than any bound.
        Err(_) if !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()) => Ok(u64::MAX),
        Err(_) => Err(invalid(format!("bad chunk size {hex:?}"))),
    }
}

/// A read of `stream` that takes the end of a TLS stream with no
/// close_notify as the end of the stream: the length of a framed body is
/// checked on its own.
fn poll_read_some<S: AsyncRead>(
    stream: Pin<&mut S>,
    cx: &mut Context<'_>,
    out: &mut ReadBuf<'_>,
) -> Poll<io::Result<()>> {
    match ready!(stream.poll_read(cx, out)) {
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Poll::Ready(Ok(())),
        other => Poll::Ready(other),
    }
}

fn too_large() -> io::Error {
    invalid("the response body is larger than allowed")
}

fn cut_short() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, "the connection closed in the middle of the response body")
}

/// A stream whose each read must make progress within a limit: a large
/// body takes long, but a peer that stops sending ends the read.
pub struct Idle<S> {
    stream: S,
    limit: Option<Duration>,
    /// The end of the wait of the read in progress; `None` between reads.
    wait: Option<Pin<Box<Sleep>>>,
}

impl<S> std::fmt::Debug for Idle<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Idle").field("limit", &self.limit).finish_non_exhaustive()
    }
}

impl<S> Idle<S> {
    /// `stream` with `limit` for each read; `None`: no limit.
    pub fn new(stream: S, limit: Option<Duration>) -> Idle<S> {
        Idle { stream, limit, wait: None }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Idle<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, out: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        if let Poll::Ready(read) = Pin::new(&mut this.stream).poll_read(cx, out) {
            this.wait = None;
            return Poll::Ready(read);
        }
        let Some(limit) = this.limit else { return Poll::Pending };
        let wait = this.wait.get_or_insert_with(|| Box::pin(tokio::time::sleep(limit)));
        ready!(wait.as_mut().poll(cx));
        this.wait = None;
        let text = format!("no byte of the response body in {} ms", limit.as_millis());
        Poll::Ready(Err(io::Error::new(io::ErrorKind::TimedOut, text)))
    }
}
