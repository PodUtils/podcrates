//! The read of a file in pieces: `GET /1.0/instances/NAME/files?path=`
//! with a `Range`, its body copied to the writer of the caller one piece
//! at a time, so that the memory of a read is one piece, not the size of
//! the file, and `Settings::max_body` does not bound it.
//!
//! incus gives a file and a link with `http.ServeContent` of Go: a range of
//! one part gets 206 with `Content-Range: bytes START-END/SIZE`, and a start
//! at or past the end gets 416 with `Content-Range: bytes */SIZE`. A server
//! that ignores the `Range` gives 200 and the whole file: the bytes before
//! the start are read and passed over.

use std::io::Write;

use pod_core::http1::parse_content_range;
use pod_proxy::http::{Body as AnswerBody, Framing, MAX_HEAD, Response, read_head};
use tokio::io::AsyncReadExt;

use super::{FileKind, FileMeta, files_path, meta_of};
use crate::client::{Client, refused};
use crate::error::{Error, ErrorKind, Result};
use crate::wire::{self, Body, Request};

/// The one buffer of a read of a file: the bound of its memory.
pub const PIECE: usize = 64 * 1024;

/// A range of the bytes of a file: from `start` to `end` (not included),
/// or to the end of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    pub end: Option<u64>,
}

impl ByteRange {
    /// The bytes from `start` to the end of the file.
    pub fn from(start: u64) -> ByteRange {
        ByteRange { start, end: None }
    }

    /// The value of the header `Range`; `None` for the whole file.
    fn header(self) -> Option<String> {
        match self.end {
            Some(end) if end > self.start => Some(format!("bytes={}-{}", self.start, end - 1)),
            None if self.start == 0 => None,
            _ => Some(format!("bytes={}-", self.start)),
        }
    }
}

/// What a read of a file in pieces gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileRead {
    pub meta: FileMeta,
    /// The offset in the file of the first byte written.
    pub start: u64,
    /// The bytes written to the writer.
    pub written: u64,
    /// The size of the whole file, where the answer gives it.
    pub size: Option<u64>,
    /// The read came to the end of the range or of the file; false where
    /// the limit of the caller stopped it before that end.
    pub ended: bool,
}

impl Client {
    /// `GET /1.0/instances/NAME/files?path=PATH` with `Range` (a start at
    /// or past the end is an empty read), its body written to `out` in
    /// pieces of [`PIECE`] bytes, at most `limit` bytes. Then a flush.
    ///
    /// A directory is a usage error: [`Client::get_file`] gives its
    /// entries. A write to `out` that fails is [`ErrorKind::Output`]. `out`
    /// is a blocking writer: each write is one piece.
    pub async fn get_file_to<W: Write + ?Sized>(
        &self,
        name: &str,
        path: &str,
        range: Option<ByteRange>,
        limit: Option<u64>,
        out: &mut W,
    ) -> Result<FileRead> {
        let files = files_path(name);
        let call = format!("GET {files}");
        let range = range.unwrap_or(ByteRange::from(0));
        let headers = range.header().map(|v| ("Range".to_string(), v)).into_iter().collect();
        let request =
            Request { method: "GET", target: self.target(&files, &[("path", path)]), headers, body: Body::Empty };
        let io = |e| Error::io(&call, e);
        let mut stream = self.open(&call).await?;
        wire::send(&mut stream, &self.settings().user_agent, request).await.map_err(io)?;
        let (answer, rest) = read_head(&mut stream, Vec::new(), MAX_HEAD).await.map_err(io)?;
        let framing = Framing::of(&answer, false);
        let total = || answer.header("content-range").and_then(parse_content_range).and_then(|s| s.total);
        let (from, size) = match answer.status {
            200 => (
                0,
                match framing {
                    Framing::Length(n) => Some(n),
                    _ => None,
                },
            ),
            206 => match answer.header("content-range").and_then(parse_content_range) {
                Some(span) if span.end.is_some() && span.start <= range.start => (span.start, span.total),
                _ => return Err(Error::protocol(&call, range_text(&answer, range.start))),
            },
            416 => {
                let meta = meta_of(&call, &answer)?;
                return Ok(FileRead { meta, start: range.start, written: 0, size: total(), ended: true });
            }
            status => {
                let max = self.settings().max_body;
                let (body, _) = wire::envelope_body(&mut stream, &answer, rest, max).await.map_err(io)?;
                return Err(refused(&call, status, &answer.reason, &body));
            }
        };
        let meta = meta_of(&call, &answer)?;
        if meta.kind == FileKind::Directory {
            return Err(Error::usage(&call, "the path is a directory: get_file gives its entries"));
        }
        let mut body = AnswerBody::new(&mut stream, rest, framing, None).map_err(io)?;
        let mut buf = vec![0u8; PIECE];
        // The bytes before the start: all of them where the server ignored
        // the `Range`.
        let mut skip = range.start - from;
        while skip > 0 {
            let want = skip.min(PIECE as u64) as usize;
            match body.read(&mut buf[..want]).await.map_err(io)? {
                0 => return Ok(FileRead { meta, start: range.start, written: 0, size, ended: true }),
                n => skip -= n as u64,
            }
        }
        let in_range = range.end.map(|end| end.saturating_sub(range.start));
        let cap = match (limit, in_range) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let output = |e| Error::new(&call, ErrorKind::Output(e));
        let mut written = 0u64;
        let mut ended = false;
        loop {
            let want = cap.map_or(PIECE, |c| (c - written).min(PIECE as u64) as usize);
            if want == 0 {
                break;
            }
            let n = body.read(&mut buf[..want]).await.map_err(io)?;
            if n == 0 {
                ended = true;
                break;
            }
            out.write_all(&buf[..n]).map_err(output)?;
            written += n as u64;
        }
        // At the bound: the end of the range, or one byte more to know if the
        // limit of the caller came before the end.
        if !ended {
            ended = in_range.is_some_and(|r| written >= r) || body.read(&mut buf[..1]).await.map_err(io)? == 0;
        }
        out.flush().map_err(output)?;
        Ok(FileRead { meta, start: range.start, written, size, ended })
    }
}

/// The words of a 206 whose `Content-Range` is not one range from `start`
/// or before it.
fn range_text(answer: &Response, start: u64) -> String {
    let got = answer.header("content-range").map(crate::error::far_text);
    format!("a 206 with the Content-Range {got:?} for a range from {start}")
}
