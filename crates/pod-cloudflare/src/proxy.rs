//! Inbound tunnel streams: parse the edge framing and send each stream to a
//! service.
//!
//! One QUIC stream carries one request. After the `ConnectRequest` preamble,
//! the stream is HTTP or raw TCP. With HTTP, an HTTP/1.1 request goes to the
//! origin and the response comes back. With raw TCP, the bytes are copied.
//! The services of the consumer (static files, shares) get the request
//! through [`crate::ServiceDispatch`].

use std::sync::Arc;

use crate::fail::{Context, OrFail, Result};
use pod_core::{Class, Stage};

use crate::Host;
use crate::capture::Route;
use crate::cloudflare::stream as cqstream;
use crate::cloudflare::stream::{HTTP_HEADER_KEY, HTTP_HOST_KEY, HTTP_METHOD_KEY};
use crate::ingress::{Ingress, Service};
use crate::metrics::Metrics;
use crate::origin::Origins;

#[cfg(test)]
mod capture_route_tests;
#[cfg(test)]
mod capture_tests;
mod copy;
mod forward;
mod head;
#[cfg(test)]
mod tests;

pub(crate) use copy::MAX_TRAILER_LINE;
#[cfg(test)]
use copy::*;
pub use forward::{forward_http, forward_tcp, tcp_http_body};
pub use head::{
    build_request_head, content_length, header_value, is_websocket, write_response_meta, write_simple_response,
    write_simple_response_with,
};

/// The visitor side of one edge stream: the request body (after the
/// `ConnectRequest`) for reading, the `ConnectResponse` and body for writing.
/// A QUIC stream gives these directly. The HTTP/2 transport gives adapters.
pub type EdgeReader = std::pin::Pin<Box<dyn futures::io::AsyncRead + Send>>;
pub type EdgeWriter = std::pin::Pin<Box<dyn futures::io::AsyncWrite + Send>>;

/// What serves the streams of the edge for the rules of one ingress: the
/// host (its services), the dial of the origins and the counters.
pub struct Runtime {
    host: Arc<Host>,
    origins: Origins,
    metrics: Metrics,
}

impl Runtime {
    /// Prepare each rule of `ingress`. A service of the consumer that the
    /// consumer cannot build (a missing directory, a missing file) stops
    /// here. Thus the tunnel never starts with only a part of its services.
    pub fn build(host: Arc<Host>, ingress: &Ingress, metrics: Metrics) -> Result<Self> {
        for (index, rule) in ingress.rules.iter().enumerate() {
            if matches!(rule.service, Service::Static { .. } | Service::Custom(_)) {
                host.services.prepare(index, &rule.service)?;
            }
        }
        let origins = Origins::new(host.tls.clone())
            .with_report(host.report.clone())
            .with_words(host.settings.words.clone())
            .with_no_verify(host.origin_no_verify.clone())
            .with_capture(host.capture.clone());
        Ok(Self { host, origins, metrics })
    }

    pub fn host(&self) -> &Arc<Host> {
        &self.host
    }

    pub fn origins(&self) -> &Origins {
        &self.origins
    }

    pub fn metrics(&self) -> &Metrics {
        &self.metrics
    }
}

/// Handle one inbound stream. An error goes to the log, never to the edge.
pub async fn serve_stream(
    runtime: Arc<Runtime>,
    ingress: Arc<Ingress>,
    mut send: noq::SendStream,
    mut recv: noq::RecvStream,
) {
    let mut signature = [0u8; 6];
    if let Err(err) = recv.read_exact(&mut signature).await {
        tracing::debug!(error = %err, "the edge closed a stream before its signature");
        return;
    }
    if signature == crate::cloudflare::RPC_STREAM_SIGNATURE {
        // The edge calls the connector (a configuration push). The client serves
        // no RPC on such a stream: reset it, so the edge gets an answer at
        // once.
        tracing::debug!("the edge opened an RPC stream; it is reset");
        let _ = recv.stop(RPC_STREAM_REFUSED.into());
        let _ = send.reset(RPC_STREAM_REFUSED.into());
        return;
    }
    let metrics = runtime.metrics().clone();
    metrics.stream_started();
    if let Err(err) = serve_inner(&runtime, &ingress, signature, send, recv).await {
        metrics.error();
        tracing::warn!(error = %err.chain(), "stream failed");
    }
    metrics.stream_finished();
}

/// The QUIC error code of a reset RPC stream.
pub const RPC_STREAM_REFUSED: u32 = 0;

async fn serve_inner(
    runtime: &Runtime,
    ingress: &Ingress,
    signature: [u8; 6],
    send: noq::SendStream,
    recv: noq::RecvStream,
) -> Result<()> {
    use tokio::io::AsyncReadExt as _;
    use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
    let recv = std::io::Cursor::new(signature).chain(recv);
    let mut reader: EdgeReader = Box::pin(recv.compat());
    let writer: EdgeWriter = Box::pin(send.compat_write());
    let request = cqstream::read_connect_request(&mut reader).await?;
    serve_request(runtime, ingress, request, reader, writer).await
}

/// Handle one request that is already decoded. The HTTP/2 transport calls
/// this with adapters for the stream.
pub async fn serve_decoded(
    runtime: Arc<Runtime>,
    ingress: Arc<Ingress>,
    request: cqstream::ConnectRequest,
    reader: EdgeReader,
    writer: EdgeWriter,
) {
    let metrics = runtime.metrics().clone();
    metrics.stream_started();
    if let Err(err) = serve_request(&runtime, &ingress, request, reader, writer).await {
        metrics.error();
        tracing::warn!(error = %err.chain(), "stream failed");
    }
    metrics.stream_finished();
}

async fn serve_request(
    runtime: &Runtime,
    ingress: &Ingress,
    request: cqstream::ConnectRequest,
    reader: EdgeReader,
    writer: EdgeWriter,
) -> Result<()> {
    // A failure that a visitor sees names its request in the log.
    let what = format!("{} {}", request.meta(HTTP_METHOD_KEY).unwrap_or("-"), pod_core::text::one_line(&request.dest));
    serve_request_inner(runtime, ingress, request, reader, writer).await.with_context(|| what)
}

async fn serve_request_inner(
    runtime: &Runtime,
    ingress: &Ingress,
    mut request: cqstream::ConnectRequest,
    reader: EdgeReader,
    writer: EdgeWriter,
) -> Result<()> {
    // The credential in front of the tunnel. A request with no right
    // Authorization gets 401 and reaches no origin; the origin does not get
    // the credential of the tunnel.
    let words = &runtime.host().settings.words;
    if let Some(auth) = &ingress.auth {
        if !auth.check_basic(header_value(&request, "authorization")) {
            let mut writer = writer;
            return write_simple_response_with(
                &mut writer,
                401,
                "text/plain; charset=utf-8",
                auth_body(words).as_bytes(),
                &[("WWW-Authenticate".to_string(), format!("Basic realm=\"{}\", charset=\"UTF-8\"", words.name))],
            )
            .await;
        }
        let name = format!("{HTTP_HEADER_KEY}:authorization");
        request.metadata.retain(|(key, _)| !key.eq_ignore_ascii_case(&name));
    }
    let host =
        request.meta(HTTP_HOST_KEY).map(str::to_string).or_else(|| authority_of(&request.dest)).unwrap_or_default();
    let path = path_and_query(&request.dest);
    tracing::debug!(%host, %path, kind = ?request.conn_type, "inbound");

    let Some(index) = ingress.resolve_index(&host, &path) else {
        let mut writer = writer;
        return write_simple_response(&mut writer, 503, "text/plain; charset=utf-8", b"no ingress rule matched\n")
            .await;
    };
    let rule = ingress.rules.get(index).or_fail(Stage::Host, Class::Config, "runtime rule missing")?;
    let metrics = runtime.metrics();
    match &rule.service {
        service @ (Service::Http(_) | Service::Unix(_) | Service::UnixTls(_)) => {
            let route = Route { rule: index, hostname: rule.hostname.clone(), host: host.clone() };
            forward::forward_ruled(
                runtime.origins(),
                service,
                &rule.origin,
                &request,
                reader,
                writer,
                metrics,
                Some(route),
            )
            .await
        }
        service @ Service::Tcp(_) => {
            forward_tcp(runtime.origins(), service, &rule.origin, &request, reader, writer, metrics).await
        }
        Service::Forward(target) => {
            if !is_websocket(&request) {
                let mut writer = writer;
                return write_simple_response(
                    &mut writer,
                    426,
                    "text/plain; charset=utf-8",
                    b"this endpoint requires a WebSocket upgrade\n",
                )
                .await;
            }
            crate::ws::serve_websocket(runtime.origins(), target, &request, reader, writer, metrics).await
        }
        Service::HelloWorld => {
            let mut writer = writer;
            let text = format!("Hello, world! ({})\n", runtime.host().settings.words.name);
            write_simple_response(&mut writer, 200, "text/plain; charset=utf-8", text.as_bytes()).await
        }
        Service::Status(code) => {
            let mut writer = writer;
            write_simple_response(&mut writer, *code, "text/plain; charset=utf-8", b"").await
        }
        Service::Metrics => {
            let text = metrics.prometheus();
            let mut writer = writer;
            write_simple_response(&mut writer, 200, "text/plain; version=0.0.4; charset=utf-8", text.as_bytes()).await
        }
        Service::Static { .. } | Service::Custom(_) => {
            runtime.host().services.serve(index, &request, &host, reader, writer).await
        }
    }
}

/// The body of the 401 of the credential in front of the tunnel, in the
/// words of the consumer.
pub fn auth_body(words: &crate::Words) -> String {
    format!("{}: this tunnel needs a user and a password (HTTP Basic)\n", words.name)
}

/// The authority of a URL (`https://host:port/a` gives `host:port`).
fn authority_of(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() { None } else { Some(authority.to_string()) }
}

/// The path and the query of a destination URL (`https://host/a?b` gives
/// `/a?b`).
pub fn path_and_query(dest: &str) -> String {
    if let Some(scheme) = dest.find("://") {
        let rest = &dest[scheme + 3..];
        return match rest.find(['/', '?', '#']) {
            Some(index) if rest.as_bytes()[index] == b'/' => rest[index..].to_string(),
            Some(index) if rest.as_bytes()[index] == b'?' => format!("/{}", &rest[index..]),
            Some(_) => "/".to_string(),
            None => "/".to_string(),
        };
    }
    if dest.starts_with('/') { dest.to_string() } else { "/".to_string() }
}
