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

use super::large::{BodySize, KEPT_HEAD, TooLarge, chunk_size};
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
    /// Past the bound: the start of the body goes to the refusal, with at
    /// most `more` bytes of the stream; `fill` reads the stream for them.
    Over {
        size: BodySize,
        more: u64,
        fill: bool,
    },
    /// The refusal was given; each next read gives it again.
    Refused,
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
    /// The end of the body is known before the end of the stream.
    framed: bool,
    /// The bound of the body, for its refusal.
    bound: Option<u64>,
    /// The bytes of the body given to the reader.
    seen: u64,
    /// The first bytes of the body, [`KEPT_HEAD`] at most, for a refusal;
    /// kept only where the body has a bound.
    kept: Vec<u8>,
    /// The status and the reason of the answer, for a refusal.
    status: Option<(u16, String)>,
    refused: Option<TooLarge>,
    /// The state that [`Body::read_on`] takes after a refusal, and the
    /// state of the refusal.
    resume: Option<(State, State)>,
}

impl<S: AsyncRead + Unpin> Body<S> {
    /// The body of `framing` over `stream`, which starts with `rest`. With
    /// `limit`, a body larger than `limit` bytes is an `InvalidData` error
    /// that carries a [`TooLarge`]; a `Content-Length` larger than it fails
    /// at the first read, with the first bytes of the body.
    pub fn new(stream: S, rest: Vec<u8>, framing: Framing, limit: Option<u64>) -> io::Result<Body<S>> {
        let mut resume = None;
        let state = match framing {
            Framing::Empty | Framing::Length(0) => State::Done,
            Framing::Length(length) if limit.is_some_and(|max| length > max) => {
                let over = State::Over { size: BodySize::Length(length), more: length, fill: true };
                resume = Some((State::Length(length), over));
                over
            }
            Framing::Length(length) => State::Length(length),
            Framing::Chunked => State::ChunkSize,
            Framing::ToEnd => State::ToEnd,
        };
        let framed = framing != Framing::ToEnd;
        let (bound, seen, kept, status, refused) = (limit, 0, Vec::new(), None, None);
        Ok(Body { stream, buf: rest, at: 0, state, left: limit, framed, bound, seen, kept, status, refused, resume })
    }

    /// The body of `answer`: its status and its reason go into a refusal
    /// by size.
    pub fn of_answer(mut self, answer: &Response) -> Body<S> {
        self.status = Some((answer.status, answer.reason.clone()));
        self
    }

    /// The body was read to its end.
    pub fn is_done(&self) -> bool {
        self.state == State::Done
    }

    /// The stream after a body that was read to its end by its length or
    /// its chunks, with no byte after it: the stream can carry a next
    /// answer. `None` for each other body.
    pub fn into_reusable(self) -> Option<S> {
        (self.state == State::Done && self.framed && self.at == self.buf.len()).then_some(self.stream)
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

    /// Count the bytes given to the reader, and keep the first ones for a
    /// refusal.
    fn note(&mut self, given: &[u8]) {
        self.seen += given.len() as u64;
        if let Some(left) = &mut self.left {
            *left = left.saturating_sub(given.len() as u64);
        }
        if self.bound.is_some() && self.kept.len() < KEPT_HEAD {
            let n = given.len().min(KEPT_HEAD - self.kept.len());
            self.kept.extend_from_slice(&given[..n]);
        }
    }

    /// The state after a chunk size line.
    fn after_size(&mut self, size: Result<u64, usize>) -> State {
        let (size, more) = match (size, self.left) {
            (Ok(0), _) => return State::Trailers(0),
            (Ok(size), Some(left)) if size > left => (BodySize::Chunk { before: self.seen, size }, size),
            (Ok(size), _) => return State::ChunkData(size),
            (Err(digits), Some(_)) => (BodySize::PastAnyBound { digits }, u64::MAX),
            (Err(_), None) => return State::ChunkData(u64::MAX),
        };
        let over = State::Over { size, more, fill: true };
        self.resume = Some((State::ChunkData(more), over));
        over
    }

    /// Past the bound: copy the start of the body that `size` declared
    /// into the kept bytes, then the refusal. The copied bytes stay in the
    /// buffer for [`Body::read_on`].
    fn poll_refuse(&mut self, cx: &mut Context<'_>) -> Poll<io::Error> {
        let given = self.given_kept();
        while let State::Over { more, fill, .. } = self.state {
            let peeked = self.kept.len().saturating_sub(given);
            let room = ((KEPT_HEAD - self.kept.len().min(KEPT_HEAD)) as u64).min(more.saturating_sub(peeked as u64));
            if room == 0 {
                break;
            }
            let have = self.buf.len() - self.at - peeked;
            if have > 0 {
                let n = have.min(usize::try_from(room).unwrap_or(usize::MAX));
                let from = self.at + peeked;
                self.kept.extend_from_slice(&self.buf[from..from + n]);
                continue;
            }
            if !fill {
                break;
            }
            // A read that ends or fails ends the head: the refusal stays.
            match self.poll_fill(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(0) | Err(_)) => break,
                Poll::Ready(Ok(_)) => {}
            }
        }
        let State::Over { size, .. } = self.state else {
            return Poll::Ready(
                self.refused.clone().map_or_else(|| invalid("the body was refused"), TooLarge::into_io),
            );
        };
        let (status, reason) = self.status.clone().map_or((None, String::new()), |(s, r)| (Some(s), r));
        let bound = self.bound.unwrap_or(0);
        let refusal = TooLarge { status, reason, size, bound, head: self.kept.clone() };
        self.state = State::Refused;
        self.refused = Some(refusal.clone());
        Poll::Ready(refusal.into_io())
    }

    /// The kept bytes that were given to the reader.
    fn given_kept(&self) -> usize {
        usize::try_from(self.seen).unwrap_or(usize::MAX).min(KEPT_HEAD)
    }

    /// After a refusal by size: read on from the byte where the refusal
    /// came, with at most `more` bytes more (`None`: no bound). The reader
    /// that keeps the start of a large answer and drops the rest takes it.
    /// False where no refusal came: the body is as it was.
    pub fn read_on(&mut self, more: Option<u64>) -> bool {
        if !matches!(self.state, State::Over { .. } | State::Refused) {
            return false;
        }
        let Some((state, over)) = self.resume else { return false };
        let given = self.given_kept();
        self.kept.truncate(given);
        let rest = match state {
            State::Length(rest) | State::ChunkData(rest) => rest,
            _ => 0,
        };
        // A rest that passes the new bound too is refused again.
        self.state = if more.is_some_and(|more| rest > more) { over } else { state };
        self.resume = (self.state == over).then_some((state, over));
        self.left = more;
        self.bound = more.map(|more| self.seen.saturating_add(more));
        self.refused = None;
        true
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for Body<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, out: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        if out.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let start = out.filled().len();
        loop {
            match this.state {
                State::Done => return Poll::Ready(Ok(())),
                State::Over { .. } | State::Refused => return this.poll_refuse(cx).map(Err),
                State::Length(left) => {
                    let n = ready!(this.poll_take(cx, out, left))?;
                    if n == 0 {
                        return Poll::Ready(Err(cut_short()));
                    }
                    this.note(&out.filled()[start..]);
                    let left = left - n as u64;
                    this.state = if left == 0 { State::Done } else { State::Length(left) };
                    return Poll::Ready(Ok(()));
                }
                // A reader must not fill `out` and fail: past the bound, the
                // next bytes go to the buffer and fail there.
                State::ToEnd if this.left == Some(0) => {
                    if this.buf.len() > this.at || ready!(this.poll_fill(cx))? > 0 {
                        let size = BodySize::Seen(this.seen + (this.buf.len() - this.at) as u64);
                        let over = State::Over { size, more: u64::MAX, fill: false };
                        this.resume = Some((State::ToEnd, over));
                        this.state = over;
                        continue;
                    }
                    this.state = State::Done;
                }
                State::ToEnd => {
                    let n = ready!(this.poll_take(cx, out, this.left.unwrap_or(u64::MAX)))?;
                    if n == 0 {
                        this.state = State::Done;
                    }
                    this.note(&out.filled()[start..]);
                    return Poll::Ready(Ok(()));
                }
                State::ChunkSize => {
                    let line = ready!(this.poll_line(cx))?;
                    let size = chunk_size(&line).map_err(invalid)?;
                    this.state = this.after_size(size);
                }
                State::ChunkData(left) => {
                    let n = ready!(this.poll_take(cx, out, left))?;
                    if n == 0 {
                        return Poll::Ready(Err(cut_short()));
                    }
                    this.note(&out.filled()[start..]);
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

    /// The stream, with no limit.
    pub fn into_inner(self) -> S {
        self.stream
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
