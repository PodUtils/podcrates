//! The copy of a body between the edge (futures I/O) and the origin (tokio
//! I/O), with the counters of the tunnel and the capture of the consumer.
//! The capture gets each part after its write to the peer: a slow sink
//! holds no byte back.

use crate::fail::{IoFail, OrFail, Result, bail};
use pod_core::{Class, Stage};

use crate::capture::{Direction, Tap};
use crate::metrics::Metrics;

/// The longest line of the trailer of a chunked answer of the origin.
pub(crate) const MAX_TRAILER_LINE: usize = 8192;

/// The largest trailer of a chunked answer of the origin.
pub(crate) const MAX_TRAILER_BYTES: usize = 64 * 1024;

// ── Byte pumps ──────────────────────────────────────────────────────────────

pub(crate) async fn copy_futures_to_tokio_n<R, W>(
    src: &mut R,
    dst: &mut W,
    mut remaining: u64,
    metrics: &Metrics,
    tap: &Tap,
) -> Result<()>
where
    R: futures::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buf = [0u8; 16 * 1024];
    while remaining > 0 {
        let want = buf.len().min(remaining as usize);
        let n = futures::io::AsyncReadExt::read(src, &mut buf[..want])
            .await
            .io_fail(Stage::EdgeStream, "reading from the edge")?;
        if n == 0 {
            bail!(Stage::Origin, Class::Protocol, "edge EOF with {remaining} request bytes still expected");
        }
        tokio::io::AsyncWriteExt::write_all(dst, &buf[..n]).await.io_fail(Stage::Origin, "writing to the origin")?;
        metrics.add_in(n as u64);
        tap.body(Direction::Request, &buf[..n]);
        remaining -= n as u64;
    }
    Ok(())
}

pub(crate) async fn copy_futures_to_tokio_eof<R, W>(
    src: &mut R,
    dst: &mut W,
    metrics: &Metrics,
    tap: &Tap,
) -> Result<u64>
where
    R: futures::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buf = [0u8; 16 * 1024];
    let mut total = 0u64;
    loop {
        let n =
            futures::io::AsyncReadExt::read(src, &mut buf).await.io_fail(Stage::EdgeStream, "reading from the edge")?;
        if n == 0 {
            break;
        }
        tokio::io::AsyncWriteExt::write_all(dst, &buf[..n]).await.io_fail(Stage::Origin, "writing to the origin")?;
        metrics.add_in(n as u64);
        tap.body(Direction::Request, &buf[..n]);
        total += n as u64;
    }
    Ok(total)
}

pub(crate) async fn copy_tokio_to_futures_n<R, W>(
    src: &mut R,
    dst: &mut W,
    mut remaining: u64,
    metrics: &Metrics,
    tap: &Tap,
) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: futures::io::AsyncWrite + Unpin,
{
    let mut buf = [0u8; 16 * 1024];
    while remaining > 0 {
        let want = buf.len().min(remaining as usize);
        let n = tokio::io::AsyncReadExt::read(src, &mut buf[..want])
            .await
            .io_fail(Stage::Origin, "reading from the origin")?;
        if n == 0 {
            bail!(Stage::Origin, Class::Protocol, "origin EOF with {remaining} response bytes still expected");
        }
        futures::io::AsyncWriteExt::write_all(dst, &buf[..n])
            .await
            .io_fail(Stage::EdgeStream, "writing to the edge")?;
        // Flush each read: a small write of the origin (an SSE event, the
        // start of a page) goes to the visitor at once.
        futures::io::AsyncWriteExt::flush(dst).await.io_fail(Stage::EdgeStream, "writing to the edge")?;
        metrics.add_out(n as u64);
        tap.body(Direction::Response, &buf[..n]);
        remaining -= n as u64;
    }
    Ok(())
}

pub(crate) async fn copy_tokio_to_futures_eof<R, W>(
    src: &mut R,
    dst: &mut W,
    metrics: &Metrics,
    tap: &Tap,
) -> Result<u64>
where
    R: tokio::io::AsyncRead + Unpin,
    W: futures::io::AsyncWrite + Unpin,
{
    let mut buf = [0u8; 16 * 1024];
    let mut total = 0u64;
    loop {
        let n = tokio::io::AsyncReadExt::read(src, &mut buf).await.io_fail(Stage::Origin, "reading from the origin")?;
        if n == 0 {
            break;
        }
        futures::io::AsyncWriteExt::write_all(dst, &buf[..n])
            .await
            .io_fail(Stage::EdgeStream, "writing to the edge")?;
        // Flush each read: a small write of the origin (an SSE event, the
        // start of a page) goes to the visitor at once.
        futures::io::AsyncWriteExt::flush(dst).await.io_fail(Stage::EdgeStream, "writing to the edge")?;
        metrics.add_out(n as u64);
        tap.body(Direction::Response, &buf[..n]);
        total += n as u64;
    }
    Ok(total)
}

/// Decode the chunked body of `src` and write its payload to `dst`. The
/// trailer is read and dropped. A size line of more than 4096 bytes, a bad
/// size or an early end is an error.
pub(crate) async fn copy_chunked_to_futures<R, W>(src: &mut R, dst: &mut W, metrics: &Metrics, tap: &Tap) -> Result<()>
where
    R: tokio::io::AsyncBufRead + Unpin,
    W: futures::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncBufReadExt;
    let mut line = Vec::new();
    loop {
        line.clear();
        let n = tokio::io::AsyncReadExt::take(&mut *src, 4096)
            .read_until(b'\n', &mut line)
            .await
            .io_fail(Stage::Origin, "reading from the origin")?;
        if n == 0 || !line.ends_with(b"\n") {
            bail!(Stage::Origin, Class::Protocol, "the chunked body of the origin ends early");
        }
        let text = std::str::from_utf8(&line).or_fail(Stage::Origin, Class::Protocol, "a chunk size is not text")?;
        let hex = text.split(';').next().unwrap_or("").trim();
        let size = u64::from_str_radix(hex, 16).or_fail(
            Stage::Origin,
            Class::Protocol,
            format!("a bad chunk size {hex:?} from the origin"),
        )?;
        if size == 0 {
            // The trailer: header lines up to an empty line, at most
            // MAX_TRAILER_LINE bytes each and MAX_TRAILER_BYTES in all. The
            // client does not send the trailer on.
            let mut total = 0;
            loop {
                line.clear();
                let n = tokio::io::AsyncReadExt::take(&mut *src, MAX_TRAILER_LINE as u64)
                    .read_until(b'\n', &mut line)
                    .await
                    .io_fail(Stage::Origin, "reading the trailer of the origin")?;
                if n == 0 || line == b"\r\n" || line == b"\n" {
                    return Ok(());
                }
                if !line.ends_with(b"\n") {
                    bail!(
                        Stage::Origin,
                        Class::Protocol,
                        "a trailer line of the origin is longer than {} KiB",
                        MAX_TRAILER_LINE / 1024
                    );
                }
                total += n;
                if total > MAX_TRAILER_BYTES {
                    bail!(
                        Stage::Origin,
                        Class::Protocol,
                        "the trailer of the origin is larger than {} KiB",
                        MAX_TRAILER_BYTES / 1024
                    );
                }
            }
        }
        copy_tokio_to_futures_n(src, dst, size, metrics, tap).await?;
        line.clear();
        tokio::io::AsyncReadExt::take(&mut *src, 2)
            .read_until(b'\n', &mut line)
            .await
            .io_fail(Stage::Origin, "reading from the origin")?;
        if line != b"\r\n" && line != b"\n" {
            bail!(Stage::Origin, Class::Protocol, "a chunk of the origin has no line end after its data");
        }
    }
}

pub(crate) async fn futures_write_all<W>(dst: &mut W, bytes: &[u8]) -> Result<()>
where
    W: futures::io::AsyncWrite + Unpin,
{
    futures::io::AsyncWriteExt::write_all(dst, bytes).await.io_fail(Stage::EdgeStream, "writing to the edge")?;
    Ok(())
}

pub(crate) async fn futures_close<W>(dst: &mut W) -> Result<()>
where
    W: futures::io::AsyncWrite + Unpin,
{
    futures::io::AsyncWriteExt::close(dst).await.io_fail(Stage::EdgeStream, "writing to the edge")?;
    Ok(())
}
