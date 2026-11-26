//! The forward of a request to an origin: HTTP/1.1 with its body and its
//! answer, an upgrade as a byte pump, and raw TCP.

use crate::fail::{IoFail, Result};
use pod_core::BoxedIo;
use pod_core::Stage;

use super::copy::*;
use super::head::*;
use super::{EdgeReader, EdgeWriter, cqstream};
use crate::capture::{Direction, Head, Tap};
use crate::ingress::{OriginOptions, Service};
use crate::metrics::Metrics;
use crate::origin::Origins;

// ── HTTP forwarding ─────────────────────────────────────────────────────────

/// Forward one HTTP request to its origin and its answer to the edge. The
/// capture of [`Origins::with_capture`] gets the exchange.
pub async fn forward_http(
    origins: &Origins,
    service: &Service,
    options: &OriginOptions,
    request: &cqstream::ConnectRequest,
    reader: EdgeReader,
    writer: EdgeWriter,
    metrics: &Metrics,
) -> Result<()> {
    let tap = origins.tap();
    let done = exchange(origins, service, options, request, reader, writer, metrics, &tap).await;
    if done.is_ok() {
        tap.complete();
    }
    done
}

#[allow(clippy::too_many_arguments)]
async fn exchange(
    origins: &Origins,
    service: &Service,
    options: &OriginOptions,
    request: &cqstream::ConnectRequest,
    mut reader: EdgeReader,
    mut writer: EdgeWriter,
    metrics: &Metrics,
    tap: &Tap,
) -> Result<()> {
    // The QUIC leg marks a WebSocket by the connection type and may keep no
    // Upgrade header: is_websocket checks both.
    let upgrade = is_websocket(request);
    let head = match build_request_head(request, options, upgrade) {
        Ok(head) => head,
        Err(err) => {
            write_simple_response(
                &mut writer,
                400,
                "text/plain; charset=utf-8",
                b"the request is not valid HTTP/1.1\n",
            )
            .await?;
            return Err(err);
        }
    };
    let mut io = origins.dial(service, options).await?;
    tokio::io::AsyncWriteExt::write_all(&mut io, head.as_bytes())
        .await
        .io_fail(Stage::Origin, "writing the request head")?;
    tap.request(|| Head::of_request(&head));

    if upgrade || is_chunked(request) {
        if upgrade {
            tap.heads_only();
        } else {
            tap.chunked_request();
        }
        return proxy_bidi(&mut reader, &mut writer, io, !upgrade, metrics, tap).await;
    }

    if let Some(len) = content_length(request)
        && len > 0
    {
        copy_futures_to_tokio_n(&mut reader, &mut io, len, metrics, tap).await?;
    }
    answer(&mut io, &mut writer, true, metrics, tap).await
}

/// Read the answer of the origin and send it to the edge. With `framed`,
/// the framing of the answer gives the end of its body, and a chunked body
/// goes as its payload: the edge frames the body itself. With no `framed`
/// (an upgrade), the bytes go as they are until the origin closes.
async fn answer<R>(io: &mut R, writer: &mut EdgeWriter, framed: bool, metrics: &Metrics, tap: &Tap) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let (status, headers, leftover) = read_response_head(io).await?;
    let resp_len = if framed { response_length(&headers)? } else { None };
    write_response_meta(writer, status, &headers).await?;
    tap.response(status, &headers);
    // The last chunk ends the body, also when the origin keeps the
    // connection open.
    let chunked = framed
        && headers_value(&headers, "transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    if chunked {
        let mut source = tokio::io::BufReader::new(tokio::io::AsyncReadExt::chain(&leftover[..], io));
        copy_chunked_to_futures(&mut source, writer, metrics, tap).await?;
        return futures_close(writer).await;
    }
    if !leftover.is_empty() {
        futures_write_all(writer, &leftover).await?;
        futures::io::AsyncWriteExt::flush(writer).await.io_fail(Stage::EdgeStream, "writing to the edge")?;
        metrics.add_out(leftover.len() as u64);
        tap.body(Direction::Response, &leftover);
    }
    match resp_len {
        Some(total) => {
            let remaining = total.saturating_sub(leftover.len() as u64);
            if remaining > 0 {
                copy_tokio_to_futures_n(io, writer, remaining, metrics, tap).await?;
            }
        }
        None => {
            copy_tokio_to_futures_eof(io, writer, metrics, tap).await?;
        }
    }
    futures_close(writer).await
}

/// Copy both directions at the same time: an upgrade, or a request body
/// with no length (chunked), which the origin may answer before its end.
async fn proxy_bidi(
    reader: &mut EdgeReader,
    writer: &mut EdgeWriter,
    io: BoxedIo,
    framed: bool,
    metrics: &Metrics,
    tap: &Tap,
) -> Result<()> {
    let (mut io_r, mut io_w) = tokio::io::split(io);
    let request_pump = async {
        let _ = copy_futures_to_tokio_eof(reader, &mut io_w, metrics, tap).await;
        let _ = tokio::io::AsyncWriteExt::shutdown(&mut io_w).await;
    };
    let response_pump = answer(&mut io_r, writer, framed, metrics, tap);
    let (_, response) = tokio::join!(request_pump, response_pump);
    response
}

pub async fn forward_tcp(
    origins: &Origins,
    service: &Service,
    options: &OriginOptions,
    request: &cqstream::ConnectRequest,
    mut reader: EdgeReader,
    mut writer: EdgeWriter,
    metrics: &Metrics,
) -> Result<()> {
    // A quick tunnel gets HTTP requests. A raw byte pipe gives the edge no
    // HTTP status, and the edge answers 502: the tunnel answers 501.
    if request.conn_type == cqstream::ConnectionType::Http && !is_websocket(request) {
        origins.note_http_to_tcp(service);
        let body = tcp_http_body(origins.words());
        return write_simple_response(&mut writer, 501, "text/plain; charset=utf-8", body.as_bytes()).await;
    }
    let io = origins.dial(service, options).await?;
    cqstream::write_connect_response(&mut writer, "", &[]).await?;
    let (mut io_r, mut io_w) = tokio::io::split(io);
    let in_metrics = metrics.clone();
    let out_metrics = metrics.clone();
    let to_origin = async {
        let _ = copy_futures_to_tokio_eof(&mut reader, &mut io_w, &in_metrics, &Tap::off()).await;
        let _ = tokio::io::AsyncWriteExt::shutdown(&mut io_w).await;
    };
    let from_origin = async {
        let _ = copy_tokio_to_futures_eof(&mut io_r, &mut writer, &out_metrics, &Tap::off()).await;
        let _ = futures_close(&mut writer).await;
    };
    tokio::join!(to_origin, from_origin);
    Ok(())
}

/// The answer of a raw TCP origin to an HTTP request, in the words of the
/// consumer.
pub fn tcp_http_body(words: &crate::Words) -> String {
    format!("{}: a raw TCP origin carries TCP or WebSocket streams, not HTTP. {}\n", words.name, words.tcp_origin)
}
