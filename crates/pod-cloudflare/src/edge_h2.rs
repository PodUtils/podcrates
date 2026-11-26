//! The HTTP/2 edge transport.
//!
//! This is the TCP transport of cloudflared (`--protocol http2`). Use it where
//! UDP is blocked and the QUIC transport cannot work.
//!
//! * The client opens TCP to the edge (port 7844) and starts TLS with the SNI
//!   `h2.cftunnel.com` and no ALPN.
//! * On that connection the edge is the HTTP/2 client and the client is the HTTP/2
//!   server. The edge opens one stream for each request.
//! * The first stream has the header
//!   `Cf-Cloudflared-Proxy-Connection-Upgrade: control-stream`. Its request
//!   body and its response body carry the capnp RPC that registers the
//!   connection.
//! * A visitor request has the visitor headers as plain HTTP/2 headers. The
//!   response headers go serialized in `Cf-Cloudflared-Response-Headers`,
//!   because HTTP/2 header rules do not apply to HTTP/1 header values.
//!
//! Each visitor stream is changed into the `ConnectRequest` form of the QUIC
//! transport and given to [`crate::proxy::serve_decoded`]. Thus both
//! transports use one service layer.

mod leg;
mod scan;
mod serve;
pub use leg::*;
use scan::*;
#[cfg(test)]
use serve::*;
pub use serve::{MAX_CONFIGURATION, body_reader, body_writer, serve_connection};

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::cloudflare::rpc;
use crate::cloudflare::stream::{
    ConnectRequest, ConnectionType, HTTP_HEADER_KEY, HTTP_HOST_KEY, HTTP_METHOD_KEY, HTTP_STATUS_KEY,
};
use crate::fail::{Context, IoFail, OrFail, Result, bail};
use bytes::Bytes;
use pod_core::{Class, Stage};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::task::JoinHandle;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::Host;
use crate::cloudflare::http2::{
    CONTROL_STREAM_HEADER, CONTROL_STREAM_VALUE, PROXY_SRC_HEADER, build_origin_response, canonical_header_name,
    edge_tls, is_control_header, is_hop_by_hop,
};
use crate::cloudflare::{EdgeAddr, h2_sni};
use crate::ingress::Ingress;
use crate::proxy::Runtime;
pub use pod_core::Metered;
use pod_core::{BoxedIo, Target};

/// The longest time for TCP and TLS to the edge.
pub const DIAL_TIMEOUT: Duration = Duration::from_secs(10);
/// The longest time for the edge to open the control stream and for the
/// registration to complete.
pub const REGISTER_TIMEOUT: Duration = Duration::from_secs(30);
/// The budget of one try of the leg on one edge address: TCP, TLS and the
/// HTTP/2 handshake ([`DIAL_TIMEOUT`] each), then the control stream and
/// the registration ([`REGISTER_TIMEOUT`] each).
pub const TRY_BUDGET: Duration = DIAL_TIMEOUT.saturating_mul(3).saturating_add(REGISTER_TIMEOUT.saturating_mul(2));
/// The time between two HTTP/2 PINGs to the edge.
pub const PING_INTERVAL: Duration = Duration::from_secs(30);
/// The longest wait for the answer of a PING. No answer is a lost
/// connection: a NAT or a proxy can drop it without a close.
pub const PING_TIMEOUT: Duration = Duration::from_secs(30);
/// The longest time that a write to the edge or to an origin may make no
/// progress. Then the stream fails, and its task and buffers are freed.
pub const WRITE_STALL: Duration = Duration::from_secs(60);
/// The longest time that a closing connection may keep its open streams.
pub const DRAIN_LIMIT: Duration = Duration::from_secs(90);
/// The size of the pipe buffers between HTTP/2 and the service layer.
const PIPE_BUFFER: usize = 64 * 1024;
/// The header value that marks a WebSocket stream.
const WEBSOCKET_VALUE: &str = "websocket";
/// The header value that marks a configuration push.
const CONFIGURATION_VALUE: &str = "update-configuration";
/// The meta header for a response that the client makes itself.
const META_CLOUDFLARED: &str = r#"{"src":"cloudflared"}"#;

/// What the edge opened a stream for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamType {
    /// The registration RPC.
    Control,
    /// A configuration push.
    Configuration,
    /// A WebSocket upgrade from a visitor.
    Websocket,
    /// A raw TCP stream (private network routing).
    Tcp,
    /// A plain HTTP request from a visitor.
    Http,
}

/// Classify a stream from its request headers, in the order cloudflared uses
/// (`connection/http2.go`, `determineHTTP2Type`).
pub fn stream_type(headers: &http::HeaderMap) -> StreamType {
    let upgrade = headers.get(CONTROL_STREAM_HEADER).and_then(|v| v.to_str().ok()).unwrap_or("");
    if upgrade == CONFIGURATION_VALUE {
        return StreamType::Configuration;
    }
    if upgrade == WEBSOCKET_VALUE {
        return StreamType::Websocket;
    }
    if headers.get(PROXY_SRC_HEADER).is_some_and(|v| !v.as_bytes().is_empty()) {
        return StreamType::Tcp;
    }
    if upgrade == CONTROL_STREAM_VALUE {
        return StreamType::Control;
    }
    StreamType::Http
}

/// Make the `ConnectRequest` of the QUIC transport from an HTTP/2 request.
pub fn connect_request(parts: &http::request::Parts, kind: StreamType) -> ConnectRequest {
    let authority = parts
        .uri
        .authority()
        .map(|a| a.to_string())
        .or_else(|| parts.headers.get(http::header::HOST).and_then(|v| v.to_str().ok()).map(str::to_string))
        .unwrap_or_default();
    if kind == StreamType::Tcp {
        return ConnectRequest { dest: authority, conn_type: ConnectionType::Tcp, metadata: Vec::new() };
    }
    let scheme = parts.uri.scheme_str().unwrap_or("http");
    let path = parts.uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
    let mut metadata =
        vec![(HTTP_METHOD_KEY.to_string(), parts.method.to_string()), (HTTP_HOST_KEY.to_string(), authority.clone())];
    for (name, value) in &parts.headers {
        let name = name.as_str();
        if is_control_header(name) || is_hop_by_hop(name) || name == "host" {
            continue;
        }
        let value = String::from_utf8_lossy(value.as_bytes());
        let value = value.as_ref();
        metadata.push((format!("{HTTP_HEADER_KEY}:{}", canonical_header_name(name)), value.to_string()));
    }
    let conn_type = if kind == StreamType::Websocket {
        // HTTP/2 has no hop-by-hop upgrade headers. The origin needs them.
        metadata.push((format!("{HTTP_HEADER_KEY}:Connection"), "Upgrade".into()));
        metadata.push((format!("{HTTP_HEADER_KEY}:Upgrade"), "websocket".into()));
        ConnectionType::Websocket
    } else {
        ConnectionType::Http
    };
    ConnectRequest { dest: format!("{scheme}://{authority}{path}"), conn_type, metadata }
}

/// Change the `ConnectResponse` metadata of the service layer into an
/// HTTP/2 response status and headers for the edge.
pub fn edge_response(error: &str, metadata: &[(String, String)]) -> (u16, Vec<(String, String)>) {
    if !error.is_empty() {
        return (502, vec![(crate::cloudflare::http2::RESPONSE_META_HEADER.to_string(), META_CLOUDFLARED.to_string())]);
    }
    let prefix = format!("{HTTP_HEADER_KEY}:");
    let mut status = 200u16;
    let mut headers = Vec::new();
    for (key, value) in metadata {
        if key == HTTP_STATUS_KEY {
            status = value.parse().unwrap_or(502);
        } else if let Some(name) = key.strip_prefix(&prefix) {
            headers.push((name.to_string(), value.clone()));
        }
    }
    build_origin_response(status, &headers)
}

/// One registered HTTP/2 connection to the edge.
pub struct H2Leg {
    /// The edge location that accepted the registration.
    pub location: String,
    /// The RPC session. Drop it, or shut it down, to unregister.
    pub control: rpc::ControlSession,
    /// The task that accepts streams. It ends when the connection ends.
    pub serve: JoinHandle<()>,
    /// Ready when the connection takes no new stream: the edge sent GOAWAY
    /// or closed it, or a PING got no answer. The open streams go on in
    /// `serve` until they end.
    pub draining: tokio::sync::oneshot::Receiver<String>,
    /// When the leg registered.
    pub started: std::time::Instant,
}

/// The keepalive of a connection.
#[derive(Clone, Copy, Debug)]
pub struct Keepalive {
    pub interval: Duration,
    pub timeout: Duration,
}

impl Default for Keepalive {
    fn default() -> Self {
        Self { interval: PING_INTERVAL, timeout: PING_TIMEOUT }
    }
}

/// The edge streams of one HTTP/2 connection.
pub struct Served {
    /// The control stream, when the edge opens it.
    pub control: tokio::sync::oneshot::Receiver<(http::Request<h2::RecvStream>, h2::server::SendResponse<Bytes>)>,
    pub serve: JoinHandle<()>,
    pub draining: tokio::sync::oneshot::Receiver<String>,
}

/// The window of each stream that the client gives the edge.
pub const STREAM_WINDOW: u32 = 1 << 20;
/// The window of the connection.
pub const CONNECTION_WINDOW: u32 = 1 << 24;
/// The smallest DATA frame of a visitor: a WebSocket frame with no
/// payload, a header of 2 bytes and a mask of 4 (RFC 6455, section 5.2).
const SMALLEST_FRAME: usize = 6;
/// The budget of h2 for small DATA frames that wait unread. For each frame
/// under 256 bytes, h2 counts 256 bytes less its size until the reader takes
/// it. Past the budget, h2 ends the connection with ENHANCE_YOUR_CALM, and
/// the stream of each visitor with it. The default of h2 (half the
/// connection window, 8 MiB) is about 43,000 unread frames of 60 bytes. With
/// it, a few visitors whose origin reads slowly end the connection of all
/// visitors. This budget holds the full window in the smallest frames, so
/// the windows stop the edge first.
pub const DATA_FRAME_BUDGET: usize = CONNECTION_WINDOW as usize / SMALLEST_FRAME * (256 - SMALLEST_FRAME);
const _: () = assert!(DATA_FRAME_BUDGET >= CONNECTION_WINDOW as usize / 60 * (256 - 60));

/// The HTTP/2 settings with which the client serves its connection to the
/// edge.
pub fn server_builder() -> h2::server::Builder {
    let mut builder = h2::server::Builder::new();
    builder
        .max_concurrent_streams(crate::cloudflare::MAX_EDGE_STREAMS)
        .initial_window_size(STREAM_WINDOW)
        .initial_connection_window_size(CONNECTION_WINDOW)
        .data_frame_budget(DATA_FRAME_BUDGET);
    builder
}

/// A limit of a connection: when it is spent, the connection takes no new
/// stream (the draining signal comes), and the open streams go on.
pub struct Budget {
    /// Notified when the bytes of the connection are spent (see `Metered`).
    pub spent: Arc<tokio::sync::Notify>,
    /// The age of the connection at which the budget is spent.
    pub age: Duration,
}

impl Budget {
    /// The budget of an edge connection through the relay of `context`:
    /// under the limits of the relay in use, as for a forward session
    /// (`pod_relay::relay::byte_budget`, `time_budget`). Gives the budget,
    /// its signal and its bytes.
    fn relay(context: &pod_relay::Context) -> (Self, Arc<tokio::sync::Notify>, u64) {
        let spent = Arc::new(tokio::sync::Notify::new());
        let budget = Self { spent: spent.clone(), age: pod_relay::relay::time_budget(&context.in_use()) };
        (budget, spent, pod_relay::relay::byte_budget(context, None))
    }
}

#[cfg(test)]
mod tests;
