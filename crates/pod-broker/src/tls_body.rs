//! The bodies of the TLS end (T-100): a request body to the far end, with
//! each placeholder swapped within the bounds, and an answer body to the
//! job, with each value scrubbed. Each byte is counted in the tunnel.

use std::sync::atomic::Ordering;

use pod_core::{Failure, Stage};
use pod_proxy::http::{Framing, Request, Response};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use zeroize::Zeroizing;

use crate::inner::{Out, answer_head, encoded};
use crate::replace::Replace;
use crate::tls_end::{Ends, Exchange, body_of, read_whole, replace_of};

fn up(e: std::io::Error) -> Failure {
    Failure::io(Stage::Origin, e)
}

fn job(e: std::io::Error) -> Failure {
    Failure::io(Stage::Proxy, e)
}

/// Write `data` as one chunk; nothing for no data.
async fn chunk<W: AsyncWrite + Unpin>(to: &mut W, data: &[u8]) -> std::io::Result<u64> {
    if data.is_empty() {
        return Ok(0);
    }
    let size = format!("{:x}\r\n", data.len());
    to.write_all(size.as_bytes()).await?;
    to.write_all(data).await?;
    to.write_all(b"\r\n").await?;
    Ok((size.len() + data.len() + 2) as u64)
}

/// Each piece of `from` through `replace` to `to`, as chunks or as it is;
/// the bytes written.
async fn pump<R, W>(from: &mut R, to: &mut W, replace: &mut Replace, chunked: bool) -> std::io::Result<u64>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; 16 * 1024];
    let mut sent = 0;
    loop {
        let n = from.read(&mut buf).await?;
        let piece = Zeroizing::new(if n == 0 { replace.finish() } else { replace.push(&buf[..n]) });
        if chunked {
            sent += chunk(to, &piece).await?;
        } else if !piece.is_empty() {
            to.write_all(&piece).await?;
            sent += piece.len() as u64;
        }
        if n == 0 {
            if chunked {
                to.write_all(b"0\r\n\r\n").await?;
                sent += 5;
            }
            to.flush().await?;
            return Ok(sent);
        }
        to.flush().await?;
    }
}

/// Send `head` and the body of `request` of `framing` to the far end.
/// Whether the stream of the job can carry a next request.
pub(crate) async fn send_body(
    ex: &Exchange<'_, '_>,
    ends: &mut Ends,
    request: &Request,
    framing: Framing,
    head: &str,
) -> Result<bool, Failure> {
    let pairs = &ex.out.body;
    let coded = encoded(&request.headers);
    let note = |word| {
        if !pairs.is_empty() {
            ex.tunnel.report.note(word);
        }
    };
    let counts = &ex.tunnel.counts.up;
    let mut reusable = true;
    let sent = match framing {
        Framing::Length(0) => {
            let length = if request.has("content-length") { "content-length: 0\r\n" } else { "" };
            let whole = Zeroizing::new(format!("{head}{length}\r\n"));
            ends.upstream.write_all(whole.as_bytes()).await.map_err(up)?;
            whole.len() as u64
        }
        Framing::Length(n) if !pairs.is_empty() && !coded && n <= ex.bounds.max_body => {
            let data = Zeroizing::new(read_whole(&mut ends.client, n).await.map_err(job)?);
            let mut replace = replace_of(pairs);
            let data = Zeroizing::new(replace.all(&data));
            ex.tunnel.report.swaps.fetch_add(replace.count(), Ordering::Relaxed);
            let whole = Zeroizing::new(format!("{head}content-length: {}\r\n\r\n", data.len()));
            ends.upstream.write_all(whole.as_bytes()).await.map_err(up)?;
            ends.upstream.write_all(&data).await.map_err(up)?;
            (whole.len() + data.len()) as u64
        }
        Framing::Length(n) => {
            note(if coded { "body-encoded" } else { "body-large" });
            let whole = Zeroizing::new(format!("{head}content-length: {n}\r\n\r\n"));
            ends.upstream.write_all(whole.as_bytes()).await.map_err(up)?;
            let copied = tokio::io::copy(&mut (&mut ends.client).take(n), &mut ends.upstream).await.map_err(job)?;
            if copied < n {
                return Err(job(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "a body ended before its length",
                )));
            }
            whole.len() as u64 + n
        }
        _ => {
            if coded {
                note("body-encoded");
            }
            let whole = Zeroizing::new(format!("{head}transfer-encoding: chunked\r\n\r\n"));
            ends.upstream.write_all(whole.as_bytes()).await.map_err(up)?;
            let mut replace = if coded { Replace::default() } else { replace_of(pairs) };
            let mut body = body_of(&mut ends.client, Vec::new(), Framing::Chunked).map_err(job)?;
            let sent = pump(&mut body, &mut ends.upstream, &mut replace, true).await.map_err(up)?;
            ex.tunnel.report.swaps.fetch_add(replace.count(), Ordering::Relaxed);
            reusable = body.into_reusable().is_some();
            whole.len() as u64 + sent
        }
    };
    ends.upstream.flush().await.map_err(up)?;
    counts.fetch_add(sent, Ordering::Relaxed);
    Ok(reusable)
}

/// Send `answer` and its body of `framing` to the job in the way `out`.
/// Whether the stream of the far end can carry a next request.
pub(crate) async fn send_answer(
    ex: &Exchange<'_, '_>,
    ends: &mut Ends,
    answer: &Response,
    framing: Framing,
    out: Out,
    keep: bool,
) -> Result<bool, Failure> {
    let rest = std::mem::take(&mut ends.up_buf);
    let mut body = body_of(&mut ends.upstream, rest, framing).map_err(up)?;
    let mut replace = replace_of(ex.scrub);
    let (sent, head_scrubs) = match out {
        Out::Whole(n) => {
            let data = read_whole(&mut body, n).await.map_err(up)?;
            let data = replace.all(&data);
            let (head, scrubs) = answer_head(answer, ex.scrub, out, data.len() as u64, keep);
            ends.client.write_all(head.as_bytes()).await.map_err(job)?;
            ends.client.write_all(&data).await.map_err(job)?;
            ((head.len() + data.len()) as u64, scrubs)
        }
        _ => {
            let (head, scrubs) = answer_head(answer, ex.scrub, out, 0, keep);
            ends.client.write_all(head.as_bytes()).await.map_err(job)?;
            let sent = match out {
                Out::Empty => 0,
                Out::Length(_) => tokio::io::copy(&mut body, &mut ends.client).await.map_err(up)?,
                _ => pump(&mut body, &mut ends.client, &mut replace, out == Out::Chunked).await.map_err(up)?,
            };
            (head.len() as u64 + sent, scrubs)
        }
    };
    ends.client.flush().await.map_err(job)?;
    let report = ex.tunnel.report;
    report.scrubs.fetch_add(replace.count() + head_scrubs, Ordering::Relaxed);
    ex.tunnel.counts.down.fetch_add(sent, Ordering::Relaxed);
    let reusable = body.is_done() && body.into_reusable().is_some();
    if out == Out::ToClose {
        let _ = ends.client.shutdown().await;
    }
    Ok(reusable)
}

/// After 101: the bytes of the job go to the far end as they are, and the
/// bytes of the far end come back scrubbed, until one side ends.
pub(crate) async fn switch(ex: &Exchange<'_, '_>, ends: &mut Ends, answer: &Response) -> Result<(), Failure> {
    let (head, scrubs) = answer_head(answer, ex.scrub, Out::Empty, 0, true);
    ends.client.write_all(head.as_bytes()).await.map_err(job)?;
    ex.tunnel.report.note("upgrade");
    let rest = std::mem::take(&mut ends.up_buf);
    let (mut client_read, mut client_write) = tokio::io::split(&mut ends.client);
    let (upstream_read, mut upstream_write) = tokio::io::split(&mut ends.upstream);
    let mut replace = replace_of(ex.scrub);
    let mut down = body_of(upstream_read, rest, Framing::ToEnd).map_err(up)?;
    let counts = ex.tunnel.counts;
    let (a, b) = tokio::join!(
        crate::tunnel::pipe(&mut client_read, &mut upstream_write, &counts.up),
        pump(&mut down, &mut client_write, &mut replace, false),
    );
    counts.down.fetch_add(head.len() as u64 + b.as_ref().copied().unwrap_or(0), Ordering::Relaxed);
    ex.tunnel.report.scrubs.fetch_add(replace.count() + scrubs, Ordering::Relaxed);
    a.map_err(job)?;
    b.map(|_| ()).map_err(up)
}
