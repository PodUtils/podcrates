//! A MASQUE session over HTTP/3 on UDP 443 (PodUtils/podnet issue 157).
//!
//! Over HTTP/2 ([`crate::session`]) one TCP connection carries each packet,
//! so a lost segment stops each flow. Over HTTP/3, each IP packet is an HTTP
//! datagram (RFC 9297) in a QUIC datagram, and a lost one is lost alone.
//! The request is an extended CONNECT (RFC 9220) with `:protocol
//! cf-connect-ip`. The client certificate and the pinned key of the front
//! door are the same as for HTTP/2 ([`crate::tls`]).
//!
//! One HTTP/3 path carries three modes ([`Mode`], podnet#486 QUIC-2):
//!
//! * CONNECT-IP (RFC 9484): IP packets.
//! * CONNECT-UDP (RFC 9298): the payloads of one UDP target, with the URI
//!   template of the protocol and the DATAGRAM capsule of the context.
//! * A stream CONNECT (RFC 9220) with the protocol of the caller, as a
//!   WebTransport-like client uses it: one stream, no datagram.
//!
//! The HTTP/3 client is minimal: a control stream with SETTINGS, one
//! request stream, QPACK with the static table only ([`wire`]), and the
//! datagrams. A packet that does not fit in a datagram goes as a DATA
//! capsule on the request stream. QUIC is `pod-quic`'s endpoint over the
//! datagrams of the caller ([`crate::UdpWay`]).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use pod_core::{Class, Datagrams, Failure, Stage};
use pod_quic::noq;

use crate::capsule;

pub mod wire;

use wire::put_varint;
pub use wire::{DataFrames, FRAME_DATA, FRAME_HEADERS, FRAME_SETTINGS, Frames, frame, response_status};

mod mode;
pub mod server;
pub mod shape;

pub use mode::{Mode, UDP_TEMPLATE, connect_headers, connect_ip_headers, udp_path};
pub use server::{Listener, ShapeEvent, ShapeLog};
pub use shape::{Profile, Settings as H3Settings, Shape, ShapeReport};

/// The ALPN of HTTP/3.
pub const ALPN: &[u8] = b"h3";

/// The SETTINGS of the client: extended CONNECT (RFC 9220), HTTP datagrams
/// (RFC 9297), and the draft value of the datagram setting that the front
/// door can still read.
pub const SETTINGS: &[(u64, u64)] = &[(0x08, 1), (0x33, 1), (0x276, 1)];

/// A silence of the QUIC connection that ends the session.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// The interval of the keepalive of QUIC.
pub const KEEP_ALIVE: Duration = Duration::from_secs(10);
/// The first UDP payload size. 1242 bytes reach the front door on each path
/// that podnet knows; the discovery of the MTU then goes up.
pub const INITIAL_MTU: u16 = 1242;
/// The bytes of datagrams that wait to be read, and to be sent.
pub const DATAGRAM_BUFFER: usize = 4 << 20;
/// The most bytes of one read of a stream.
const CHUNK: usize = 64 * 1024;

fn failure(class: Class, text: impl Into<String>) -> Failure {
    Failure::new(Stage::Warp, class, text)
}

/// The class of a QUIC connection that failed or closed.
fn class_of(e: &noq::ConnectionError) -> Class {
    match e {
        noq::ConnectionError::TimedOut => Class::Timeout,
        noq::ConnectionError::VersionMismatch => Class::Unsupported,
        noq::ConnectionError::TransportError(_) => Class::Protocol,
        noq::ConnectionError::ConnectionClosed(_) | noq::ConnectionError::ApplicationClosed(_) => Class::Refused,
        _ => Class::Unavailable,
    }
}

/// The QUIC settings of the session: podnet's silence, keepalive, first
/// packets and buffers of datagrams, and the handshake within `wait`.
pub fn quic_settings(wait: Duration) -> pod_quic::Settings {
    pod_quic::Settings {
        handshake_wait: wait,
        silence: IDLE_TIMEOUT,
        keepalive: Some(KEEP_ALIVE),
        first_packet_v4: INITIAL_MTU,
        first_packet_v6: INITIAL_MTU,
        datagram_buffer: Some(DATAGRAM_BUFFER),
        ..pod_quic::Settings::default()
    }
}

/// The write half of an HTTP/3 session.
pub struct H3Tx {
    conn: noq::Connection,
    send: noq::SendStream,
    /// The control stream: it must stay open for the life of the session.
    _control: noq::SendStream,
    /// The endpoint over the datagrams of the caller.
    _endpoint: noq::Endpoint,
    /// The Quarter Stream ID and the Context ID 0 of each datagram.
    prefix: Vec<u8>,
    /// The Context ID of each payload; `None` for a stream with no
    /// datagram.
    context: Option<u64>,
}

/// The read half of an HTTP/3 session.
pub struct H3Rx {
    conn: noq::Connection,
    recv: noq::RecvStream,
    frames: Frames,
    quarter: u64,
}

/// What came from the front door.
pub enum Incoming {
    /// One IP packet (a datagram).
    Packet(Bytes),
    /// Bytes of capsules (DATA frames of the request stream).
    Capsules(Bytes),
}

/// A QUIC connection with HTTP/3: the control stream is open, and the
/// streams of the server are read and dropped.
pub struct H3Conn {
    pub conn: noq::Connection,
    pub control: noq::SendStream,
    pub endpoint: noq::Endpoint,
}

/// QUIC with HTTP/3 to `door` over `datagrams`, with the TLS configuration
/// `tls` (its ALPN must be [`ALPN`]), the name `sni` and the handshake
/// within `wait`.
pub async fn open(
    door: SocketAddr,
    datagrams: Arc<dyn Datagrams>,
    tls: Arc<rustls::ClientConfig>,
    sni: &str,
    wait: Duration,
) -> Result<H3Conn, Failure> {
    let endpoint = pod_quic::endpoint::over(datagrams, None)?;
    let settings = quic_settings(wait);
    let mut config = pod_quic::connect::client_config(tls, &settings, door.is_ipv6())?;
    let mut transport = settings.transport(door.is_ipv6());
    transport.datagram_send_buffer_size(DATAGRAM_BUFFER);
    config.transport_config(Arc::new(transport));
    let connecting = endpoint
        .connect_with(config, door, sni)
        .map_err(|e| Failure::caused(Stage::Quic, Class::Usage, format!("QUIC to the front door {door}: {e}"), e))?;
    let conn = match tokio::time::timeout(wait, connecting).await {
        Ok(Ok(conn)) => conn,
        Ok(Err(e)) => {
            let text = format!("QUIC to the front door {door} (UDP, {sni}): {e}");
            return Err(Failure::caused(Stage::Quic, class_of(&e), text, e));
        }
        Err(_) => {
            let text = format!("QUIC to the front door {door} (UDP, {sni}): no answer in {} s", wait.as_secs());
            return Err(Failure::new(Stage::Quic, Class::Timeout, text));
        }
    };
    let stream = |e: &dyn std::fmt::Display, what: &str| failure(Class::Unavailable, format!("{what}: {e}"));
    // The control stream: its type (0x00) and SETTINGS.
    let mut control = conn.open_uni().await.map_err(|e| stream(&e, "the HTTP/3 control stream"))?;
    let mut settings = Vec::new();
    for (id, value) in SETTINGS {
        put_varint(&mut settings, *id);
        put_varint(&mut settings, *value);
    }
    let mut first = vec![0x00];
    first.extend_from_slice(&frame(FRAME_SETTINGS, &settings));
    control.write_all(&first).await.map_err(|e| stream(&e, "the HTTP/3 SETTINGS"))?;
    // The streams of the server (control, QPACK) are read and dropped.
    let accept = conn.clone();
    tokio::spawn(async move {
        while let Ok(mut uni) = accept.accept_uni().await {
            tokio::spawn(async move { while let Ok(Some(_)) = uni.read_chunk(CHUNK).await {} });
        }
    });
    Ok(H3Conn { conn, control, endpoint })
}

/// Send the request `headers` on a new stream and read the status of the
/// answer: the stream, the frames after the answer, and the status.
pub async fn request(
    conn: &noq::Connection,
    headers: &[u8],
) -> Result<(noq::SendStream, noq::RecvStream, Frames, u16), Failure> {
    let (mut send, mut recv) =
        conn.open_bi().await.map_err(|e| failure(Class::Unavailable, format!("an HTTP/3 request stream: {e}")))?;
    send.write_all(&frame(FRAME_HEADERS, headers))
        .await
        .map_err(|e| failure(Class::Unavailable, format!("sending the HTTP/3 request: {e}")))?;
    let mut frames = Frames::default();
    let status = loop {
        if let Some((kind, payload)) = frames.next_frame()? {
            if kind == FRAME_HEADERS {
                break response_status(&payload)?;
            }
            continue;
        }
        match recv.read_chunk(CHUNK).await {
            Ok(Some(chunk)) => frames.push(&chunk),
            Ok(None) => {
                return Err(failure(Class::Protocol, "the front door closed the request stream with no answer"));
            }
            Err(e) => return Err(failure(Class::Unavailable, format!("reading the HTTP/3 answer: {e}"))),
        }
    };
    Ok((send, recv, frames, status))
}

/// Open a CONNECT-IP session over HTTP/3 to `door` over `datagrams`, with
/// the TLS configuration of the session (`tls` must have the ALPN
/// [`ALPN`]).
pub async fn connect(
    door: SocketAddr,
    datagrams: Arc<dyn Datagrams>,
    tls: Arc<rustls::ClientConfig>,
    sni: &str,
    authority: &str,
    wait: Duration,
) -> Result<(H3Tx, H3Rx), Failure> {
    connect_mode(door, datagrams, tls, sni, authority, wait, &Mode::Ip).await
}

/// Open a session of any [`Mode`] over HTTP/3: one code path for
/// CONNECT-IP, CONNECT-UDP and a stream CONNECT.
pub async fn connect_mode(
    door: SocketAddr,
    datagrams: Arc<dyn Datagrams>,
    tls: Arc<rustls::ClientConfig>,
    sni: &str,
    authority: &str,
    wait: Duration,
    mode: &Mode,
) -> Result<(H3Tx, H3Rx), Failure> {
    let H3Conn { conn, control, endpoint } = open(door, datagrams, tls, sni, wait).await?;
    let headers = connect_headers(authority, mode);
    let (send, recv, frames, status) = request(&conn, &headers).await?;
    if status != 200 {
        let (class, why) = match status {
            403 => (Class::Refused, " (the enrolled certificate was not accepted)"),
            _ => (Class::Protocol, ""),
        };
        return Err(failure(class, format!("{} over HTTP/3 refused with {status}{why}", mode.protocol())));
    }
    let quarter = u64::from(send.id()) / 4;
    let mut prefix = Vec::new();
    if mode.datagrams() {
        put_varint(&mut prefix, quarter);
        put_varint(&mut prefix, 0);
    }
    tracing::debug!(
        max_datagram = ?conn.max_datagram_size(),
        protocol = mode.protocol(),
        "the MASQUE session over HTTP/3 is open"
    );
    let context = mode.datagrams().then_some(0);
    let tx = H3Tx { conn: conn.clone(), send, _control: control, _endpoint: endpoint, prefix, context };
    Ok((tx, H3Rx { conn, recv, frames, quarter }))
}

impl H3Tx {
    /// Send each packet as a datagram; one that does not fit goes as a DATA
    /// capsule on the request stream.
    pub async fn send_packets<P: AsRef<[u8]>>(&mut self, packets: &[P]) -> Result<(), Failure> {
        self.send_all(packets, false).await
    }

    /// Send each UDP payload as a datagram of the context; one that does
    /// not fit goes as a DATAGRAM capsule on the request stream (RFC 9298,
    /// section 5).
    pub async fn send_udp<P: AsRef<[u8]>>(&mut self, payloads: &[P]) -> Result<(), Failure> {
        self.send_all(payloads, true).await
    }

    async fn send_all<P: AsRef<[u8]>>(&mut self, payloads: &[P], with_context: bool) -> Result<(), Failure> {
        let mut stream = BytesMut::new();
        for payload in payloads {
            let payload = payload.as_ref();
            if self.context.is_some() {
                let fits = self.conn.max_datagram_size().is_some_and(|max| self.prefix.len() + payload.len() <= max);
                if fits {
                    let mut d = BytesMut::with_capacity(self.prefix.len() + payload.len());
                    d.extend_from_slice(&self.prefix);
                    d.extend_from_slice(payload);
                    match self.conn.send_datagram_wait(d.freeze()).await {
                        Ok(()) => continue,
                        Err(noq::SendDatagramError::TooLarge) => {}
                        Err(e) => return Err(failure(Class::Unavailable, format!("an HTTP/3 datagram: {e}"))),
                    }
                }
            }
            // A payload that does not fit, or a stream with no datagram,
            // goes as a capsule of the request stream.
            let mut value = BytesMut::new();
            if with_context {
                put_varint(&mut value, self.context.unwrap_or(0));
            }
            value.extend_from_slice(payload);
            capsule::put_capsule(&mut stream, capsule::CAPSULE_DATA, &value);
        }
        if !stream.is_empty() {
            self.send
                .write_all(&frame(FRAME_DATA, &stream))
                .await
                .map_err(|e| failure(Class::Unavailable, format!("a DATA capsule on the MASQUE stream: {e}")))?;
        }
        Ok(())
    }

    /// Write bytes as a DATA frame of the request stream: the mode of a
    /// stream CONNECT (RFC 9220, section 4.4 of RFC 9114), where the caller
    /// carries its own protocol.
    pub async fn write(&mut self, bytes: &[u8]) -> Result<(), Failure> {
        self.send
            .write_all(&frame(FRAME_DATA, bytes))
            .await
            .map_err(|e| failure(Class::Unavailable, format!("a write of the stream CONNECT: {e}")))
    }

    /// Close the request stream and the connection.
    pub fn close(&mut self) {
        let _ = self.send.finish();
        self.conn.close(0u32.into(), b"");
    }

    /// The connection, for the liveness check.
    pub fn connection(&self) -> noq::Connection {
        self.conn.clone()
    }
}

impl H3Rx {
    /// The next packet or the next bytes of capsules; `None` at the end of
    /// the session.
    pub async fn next(&mut self) -> Result<Option<Incoming>, Failure> {
        loop {
            if let Some((kind, payload)) = self.frames.next_frame()? {
                if kind == FRAME_DATA {
                    return Ok(Some(Incoming::Capsules(payload.into())));
                }
                continue;
            }
            tokio::select! {
                datagram = self.conn.read_datagram() => match datagram {
                    Ok(d) => {
                        if let Some(packet) = self.packet_of(d) {
                            return Ok(Some(Incoming::Packet(packet)));
                        }
                    }
                    Err(noq::ConnectionError::LocallyClosed) => return Ok(None),
                    Err(e) => return Err(Failure::caused(Stage::Warp, class_of(&e), "the HTTP/3 session", e)),
                },
                chunk = self.recv.read_chunk(CHUNK) => match chunk {
                    Ok(Some(chunk)) => self.frames.push(&chunk),
                    Ok(None) => return Ok(None),
                    Err(e) => return Err(failure(Class::Unavailable, format!("reading the MASQUE stream: {e}"))),
                },
            }
        }
    }

    /// The next bytes of the request stream: the mode of a stream CONNECT
    /// (RFC 9220). `None` at the end of the stream.
    pub async fn read(&mut self) -> Result<Option<Vec<u8>>, Failure> {
        loop {
            if let Some((kind, payload)) = self.frames.next_frame()? {
                if kind == FRAME_DATA {
                    return Ok(Some(payload));
                }
                continue;
            }
            match self.recv.read_chunk(CHUNK).await {
                Ok(Some(chunk)) => self.frames.push(&chunk),
                Ok(None) => return Ok(None),
                Err(e) => return Err(failure(Class::Unavailable, format!("reading the stream CONNECT: {e}"))),
            }
        }
    }

    /// The IP packet of a datagram of this request with the Context ID 0.
    fn packet_of(&self, d: Bytes) -> Option<Bytes> {
        let (quarter, a) = wire::get_varint(&d)?;
        let (context, b) = wire::get_varint(&d[a..])?;
        (quarter == self.quarter && context == 0).then(|| d.slice(a + b..))
    }
}

/// A CONNECT-UDP session (RFC 9298) over HTTP/3: the payloads of one UDP
/// target, each in a datagram or a DATAGRAM capsule of the context.
pub struct H3Udp {
    tx: H3Tx,
    rx: H3Rx,
    /// The capsules of the stream that are not yet whole.
    capsules: capsule::CapsuleDecoder,
    /// The payloads of the capsules that are ready.
    ready: std::collections::VecDeque<Vec<u8>>,
}

impl H3Udp {
    /// Open a CONNECT-UDP session to the proxy `door` for the UDP target
    /// `host:port`.
    pub async fn connect(
        door: SocketAddr,
        datagrams: Arc<dyn Datagrams>,
        tls: Arc<rustls::ClientConfig>,
        sni: &str,
        authority: &str,
        target: (&str, u16),
        wait: Duration,
    ) -> Result<H3Udp, Failure> {
        let mode = Mode::Udp { host: target.0.to_string(), port: target.1 };
        let (tx, rx) = connect_mode(door, datagrams, tls, sni, authority, wait, &mode).await?;
        Ok(H3Udp { tx, rx, capsules: capsule::CapsuleDecoder::new(), ready: Default::default() })
    }

    /// The UDP payload of the next datagram or capsule; `None` at the end of
    /// the session.
    pub async fn recv(&mut self) -> Result<Option<Vec<u8>>, Failure> {
        loop {
            if let Some(payload) = self.ready.pop_front() {
                return Ok(Some(payload));
            }
            match self.rx.next().await? {
                Some(Incoming::Packet(packet)) => return Ok(Some(packet.to_vec())),
                Some(Incoming::Capsules(bytes)) => {
                    self.capsules.push(&bytes);
                    while let Some((kind, value)) = self.capsules.next_capsule() {
                        if kind != capsule::CAPSULE_DATA {
                            continue;
                        }
                        let Some((_, n)) = capsule::get_varint(&value) else { continue };
                        self.ready.push_back(value[n..].to_vec());
                    }
                    if let Some(error) = self.capsules.error() {
                        return Err(failure(Class::Protocol, format!("the capsules of the session: {error}")));
                    }
                }
                None => return Ok(None),
            }
        }
    }

    /// Send each UDP payload to the target.
    pub async fn send<P: AsRef<[u8]>>(&mut self, payloads: &[P]) -> Result<(), Failure> {
        self.tx.send_udp(payloads).await
    }

    pub fn close(&mut self) {
        self.tx.close();
    }
}

/// Ready with a failure when the QUIC connection closes: the keepalive and
/// the idle timeout of QUIC find a dead path.
pub async fn closed(conn: noq::Connection) -> Result<(), Failure> {
    let reason = conn.closed().await;
    let text = format!("the HTTP/3 session closed: {reason}");
    Err(Failure::caused(Stage::Warp, class_of(&reason), text, reason))
}

#[cfg(test)]
mod tests;
