//! An HTTP/3 server of a [`Shape`] (PodUtils/podnet#486, QUIC-6): the `h3`
//! ALPN, a control stream with SETTINGS, and a request stream that answers
//! a GET with a body. The listener takes a `pod-quic` endpoint with the
//! TLS configuration of the caller, so the crate runs no server of its own
//! stack.
//!
//! The record of a run is a [`ShapeLog`]: a qlog-like list of the events of
//! the shape, which [`super::ShapeReport`] reads for `doctor`. This is what a
//! conformance test asserts, because a stock HTTP/3 client is not in the
//! workspace.
//!
//! The QPACK decoder of the request takes the static table and a literal
//! of no Huffman code; a request that uses the dynamic table is refused
//! (see [`crate::h3::wire::request_fields`]). [`crate::h3::wire`] names the
//! limit.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};

use pod_core::{Class, Failure, Stage};
use pod_quic::noq;

use super::frame;
use super::shape::Shape;
use super::wire::{self, FRAME_DATA, FRAME_HEADERS, FRAME_SETTINGS, Frames, put_varint};

/// The body of a GET of the shape.
pub const BODY: &[u8] = b"pod-warp h3 shape\n";
/// The most bytes of one read of a stream.
const CHUNK: usize = 64 * 1024;
/// The stream type of a control stream (RFC 9114, section 6.2.1).
pub const CONTROL: u64 = 0x00;

fn failure(class: Class, text: impl Into<String>) -> Failure {
    Failure::new(Stage::Warp, class, text)
}

/// One event of the shape, in the order that the server saw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapeEvent {
    /// A connection with the ALPN `protocol`.
    Alpn { protocol: String },
    /// A control stream of `kind` was opened.
    ControlStream { kind: u64 },
    /// The server sent its SETTINGS frame.
    SettingsSent(Vec<(u64, u64)>),
    /// The client opened its control stream.
    ClientControlStream,
    /// The client sent its SETTINGS frame.
    ClientSettings(Vec<(u64, u64)>),
    /// A request, and the status of the answer.
    Request { method: String, path: String, status: u16 },
    /// The connection ended.
    Closed,
}

/// The events of a listener, kept for the report of `doctor` and for a
/// test.
#[derive(Clone, Debug, Default)]
pub struct ShapeLog(Arc<Mutex<Vec<ShapeEvent>>>);

impl ShapeLog {
    /// The events, in order.
    pub fn events(&self) -> Vec<ShapeEvent> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// The number of events.
    pub fn len(&self) -> usize {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn push(&self, event: ShapeEvent) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).push(event);
    }
}

/// A listener of the shape: a QUIC endpoint with the ALPN `h3` and the
/// transport of the shape.
pub struct Listener {
    endpoint: noq::Endpoint,
    shape: Arc<Shape>,
    log: ShapeLog,
}

impl Listener {
    /// A listener on `addr` with the TLS configuration `crypto` of the
    /// caller, the `shape`, and the timings of `base`.
    pub fn bind(
        addr: SocketAddr,
        crypto: Arc<dyn noq::crypto::ServerConfig>,
        shape: Shape,
        base: &pod_quic::Settings,
    ) -> Result<Listener, Failure> {
        let mut config = pod_quic::endpoint::server_config(crypto)?;
        config.transport_config(Arc::new(shape.transport(base)));
        let endpoint = pod_quic::endpoint::server(config, addr)?;
        Ok(Listener { endpoint, shape: Arc::new(shape), log: ShapeLog::default() })
    }

    /// The address of the listener.
    pub fn local_addr(&self) -> Result<SocketAddr, Failure> {
        self.endpoint
            .local_addr()
            .map_err(|e| Failure::caused(Stage::Quic, Class::Unavailable, format!("the listener: {e}"), e))
    }

    pub fn shape(&self) -> &Shape {
        &self.shape
    }

    /// The events of each connection served so far.
    pub fn log(&self) -> ShapeLog {
        self.log.clone()
    }

    /// Serve one connection until it closes: its control stream and each of
    /// its requests. `None` when the endpoint stops.
    pub async fn accept(&self) -> Result<(), Failure> {
        let Some(incoming) = self.endpoint.accept().await else {
            return Err(failure(Class::Refused, "the HTTP/3 listener ended"));
        };
        let conn = incoming.await.map_err(|e| {
            Failure::caused(Stage::Quic, Class::Protocol, format!("a connection at the HTTP/3 listener: {e}"), e)
        })?;
        self.serve(conn).await
    }

    /// Serve each connection until the endpoint stops.
    pub async fn serve_forever(self: Arc<Self>) {
        while self.accept().await.is_ok() {}
    }

    async fn serve(&self, conn: noq::Connection) -> Result<(), Failure> {
        if let Some(data) = conn.handshake_data().and_then(|d| d.downcast::<noq::crypto::rustls::HandshakeData>().ok())
        {
            self.log.push(ShapeEvent::Alpn {
                protocol: data.protocol.as_deref().map(String::from_utf8_lossy).unwrap_or_default().into_owned(),
            });
        }
        // RFC 9114, section 6.2.1: the sender must not close the control
        // stream. Keep its send side alive for the connection's lifetime;
        // the drop at the end of this function is after `closed()`.
        let _control = if self.shape.control_stream { Some(self.control(&conn).await?) } else { None };
        let uni = tokio::spawn(self.clone_detached().client_control(conn.clone()));
        let bi = tokio::spawn(self.clone_detached().requests(conn.clone()));
        let _ = conn.closed().await;
        uni.abort();
        bi.abort();
        self.log.push(ShapeEvent::Closed);
        Ok(())
    }

    /// A handle of the listener for a task of one connection: the fields
    /// that a task reads are shared, and the endpoint is not.
    fn clone_detached(&self) -> Handle {
        Handle { log: self.log.clone() }
    }

    /// The control stream of the server: its type, then SETTINGS, before
    /// any answer (RFC 9114, section 6.2.1). The send side is returned so
    /// that the caller keeps it open; dropping it sends FIN.
    async fn control(&self, conn: &noq::Connection) -> Result<noq::SendStream, Failure> {
        let mut stream = conn
            .open_uni()
            .await
            .map_err(|e| failure(Class::Unavailable, format!("the HTTP/3 control stream: {e}")))?;
        let mut first = Vec::new();
        put_varint(&mut first, CONTROL);
        first.extend_from_slice(&frame(FRAME_SETTINGS, &self.shape.settings_payload()));
        stream.write_all(&first).await.map_err(|e| failure(Class::Unavailable, format!("the HTTP/3 SETTINGS: {e}")))?;
        self.log.push(ShapeEvent::ControlStream { kind: CONTROL });
        self.log.push(ShapeEvent::SettingsSent(self.shape.settings.pairs()));
        Ok(stream)
    }
}

/// The parts of the listener that a task of a connection needs.
struct Handle {
    log: ShapeLog,
}

impl Handle {
    /// Read the control stream of the client, and its SETTINGS.
    async fn client_control(self, conn: noq::Connection) {
        while let Ok(mut stream) = conn.accept_uni().await {
            let log = self.log.clone();
            tokio::spawn(async move {
                let mut frames = Frames::default();
                let mut buf = Vec::new();
                while let Ok(Some(chunk)) = stream.read_chunk(CHUNK).await {
                    buf.extend_from_slice(&chunk);
                    if !buf.is_empty() {
                        break;
                    }
                }
                if buf.is_empty() {
                    return;
                }
                let Some((kind, used)) = wire::get_varint(&buf) else { return };
                if kind != CONTROL {
                    return;
                }
                log.push(ShapeEvent::ClientControlStream);
                frames.push(&buf[used..]);
                loop {
                    while let Ok(Some((id, payload))) = frames.next_frame() {
                        if id == FRAME_SETTINGS
                            && let Ok(pairs) = wire::settings_pairs(&payload)
                        {
                            log.push(ShapeEvent::ClientSettings(pairs));
                        }
                    }
                    match stream.read_chunk(CHUNK).await {
                        Ok(Some(chunk)) => frames.push(&chunk),
                        _ => return,
                    }
                }
            });
        }
    }

    /// Answer each request stream.
    async fn requests(self, conn: noq::Connection) {
        while let Ok((mut send, mut recv)) = conn.accept_bi().await {
            let log = self.log.clone();
            tokio::spawn(async move {
                let mut frames = Frames::default();
                let fields = loop {
                    if let Ok(Some((id, payload))) = frames.next_frame() {
                        if id == FRAME_HEADERS {
                            break match wire::request_fields(&payload) {
                                Ok(fields) => fields,
                                Err(e) => {
                                    let _ = send.finish();
                                    tracing::debug!(error = %e.chain(), "an HTTP/3 request of the shape was refused");
                                    return;
                                }
                            };
                        }
                        continue;
                    }
                    match recv.read_chunk(CHUNK).await {
                        Ok(Some(chunk)) => frames.push(&chunk),
                        _ => return,
                    }
                };
                let method = field(&fields, ":method").unwrap_or_default();
                let path = field(&fields, ":path").unwrap_or_default();
                let status: u16 = match method.as_str() {
                    "GET" | "HEAD" => 200,
                    _ => 405,
                };
                if status == 200 {
                    let _ = send.write_all(&frame(FRAME_HEADERS, &[0x00, 0x00, 0xc0 | 25])).await;
                    if method == "GET" {
                        let _ = send.write_all(&frame(FRAME_DATA, BODY)).await;
                    }
                } else {
                    // :status 405 is not in the static table: a literal
                    // with a name reference to :status (static 24..).
                    let mut section = vec![0x00, 0x00];
                    wire::put_prefixed(&mut section, 0x50, 4, 24);
                    wire::put_string(&mut section, 0x00, 7, "405");
                    let _ = send.write_all(&frame(FRAME_HEADERS, &section)).await;
                }
                let _ = send.finish();
                log.push(ShapeEvent::Request { method, path, status });
            });
        }
    }
}

fn field(fields: &[(String, String)], name: &str) -> Option<String> {
    fields.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::shape::Shape;

    /// The log keeps each event in order, and a report of the shape reads
    /// the control stream and the SETTINGS of it.
    #[test]
    fn the_log_keeps_the_shape() {
        let log = ShapeLog::default();
        let shape = Shape::common();
        log.push(ShapeEvent::Alpn { protocol: "h3".into() });
        log.push(ShapeEvent::ControlStream { kind: CONTROL });
        log.push(ShapeEvent::SettingsSent(shape.settings.pairs()));
        log.push(ShapeEvent::Request { method: "GET".into(), path: "/".into(), status: 200 });
        assert_eq!(log.len(), 4);
        let report = super::super::shape::ShapeReport::of(&shape, &log);
        assert!(report.control_stream && report.settings);
        assert_eq!(report.setting_ids, vec![0x01, 0x06, 0x08, 0x33]);
        assert!(report.line().contains("congestion=cubic"));
    }
}
