//! The L4 door of WARP (PodUtils/podnet issue 140, `warp proxy --l4`):
//! each TCP flow is one HTTP/3 CONNECT stream (RFC 9114, section 4.4) to
//! the door, with no IP stack in between. One QUIC connection carries each
//! flow, with the client certificate and the pinned key of the device, on
//! the UDP of the caller ([`crate::UdpWay`]); a new connection opens when
//! it closes. The door carries no UDP.
//!
//! [`L4`] is a [`pod_core::Dial`]: [`crate::proxy::Proxy::over_l4`] serves
//! it, with the rules decided with no lookup of a name
//! ([`crate::rules::Table::decide_static`]).

use std::net::{Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use pod_core::{BoxFuture, Class, Connected, Dial, Failure, Stage, Target};
use pod_quic::noq;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _, DuplexStream};
use tokio::time::Instant;

use crate::h3::wire::{put_prefixed, put_string};
use crate::h3::{self, DataFrames, FRAME_DATA, H3Conn, frame};
use crate::udp::{UdpWay, no_udp};
use crate::warp::Warp;

/// The longest time to open the QUIC connection of the door (podnet's).
pub const OPEN_WAIT: Duration = Duration::from_secs(15);
/// The buffer of the local pipe of a flow, each way.
pub const PIPE_BUFFER: usize = 256 * 1024;
/// The most bytes of one read of the pipe, sent as one DATA frame.
const UP_CHUNK: usize = 32 * 1024;
/// The most bytes of one read of the stream.
const DOWN_CHUNK: usize = 64 * 1024;

fn failure(class: Class, text: impl Into<String>) -> Failure {
    Failure::new(Stage::Warp, class, text)
}

/// The form of the CONNECT request (podnet's `PODNET_WARP_L4_FORM`, a test
/// of the form that the door takes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Form {
    /// A plain CONNECT (RFC 9114, section 4.4).
    #[default]
    Plain,
    /// An extended CONNECT with `:protocol cf-connect-tcp` (RFC 9220).
    Extended,
    /// A plain CONNECT with the field `cf-connect-proto: cf-connect-tcp`.
    Header,
}

impl Form {
    /// `plain` (or empty), `extended` or `header`; another text is
    /// `Config`.
    pub fn parse(text: &str) -> Result<Form, Failure> {
        match text.trim().to_ascii_lowercase().as_str() {
            "" | "plain" => Ok(Form::Plain),
            "extended" => Ok(Form::Extended),
            "header" => Ok(Form::Header),
            other => {
                let text = format!("{other:?} is not a form of the L4 CONNECT (plain, extended, header)");
                Err(Failure::new(Stage::Warp, Class::Config, text))
            }
        }
    }
}

/// The field section of a CONNECT to `authority` (`host:port`) in the form
/// `form`: QPACK with the static table only, no Huffman code.
pub fn connect_headers_form(authority: &str, form: Form) -> Vec<u8> {
    // Required Insert Count 0, Base 0; :method CONNECT (static 15).
    let mut out = vec![0x00, 0x00, 0xc0 | 15];
    if form == Form::Extended {
        // :scheme https and :path / (static 23 and 1).
        out.extend_from_slice(&[0xc0 | 23, 0xc0 | 1]);
    }
    // :authority (static name 0), a literal value.
    put_prefixed(&mut out, 0x50, 4, 0);
    put_string(&mut out, 0x00, 7, authority);
    let field = match form {
        Form::Plain => None,
        Form::Extended => Some(":protocol"),
        Form::Header => Some("cf-connect-proto"),
    };
    if let Some(name) = field {
        // A literal field line with a literal name (001NHxxx).
        put_string(&mut out, 0x20, 3, name);
        put_string(&mut out, 0x00, 7, "cf-connect-tcp");
    }
    out
}

/// `host:port`, with an IPv6 address in brackets.
pub fn authority(host: &str, port: u16) -> String {
    match host.parse::<Ipv6Addr>() {
        Ok(v6) => format!("[{v6}]:{port}"),
        Err(_) => format!("{host}:{port}"),
    }
}

/// The L4 door: one QUIC connection, opened again when it closes.
pub struct L4 {
    door: SocketAddr,
    udp: Arc<dyn UdpWay>,
    tls: Arc<rustls::ClientConfig>,
    sni: String,
    form: Form,
    wait: Duration,
    conn: tokio::sync::Mutex<Option<H3Conn>>,
}

impl std::fmt::Debug for L4 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let way = self.udp.label();
        f.debug_struct("L4")
            .field("door", &self.door)
            .field("way", &way)
            .field("sni", &self.sni)
            .finish_non_exhaustive()
    }
}

impl L4 {
    /// The door at `door` on `udp`, with the TLS configuration `tls` (the
    /// client certificate, the pinned key, the ALPN [`h3::ALPN`]), the name
    /// [`crate::service::l4_sni`], the [`Form::Plain`] and [`OPEN_WAIT`].
    pub fn new(door: SocketAddr, udp: Arc<dyn UdpWay>, tls: Arc<rustls::ClientConfig>) -> L4 {
        let sni = crate::service::l4_sni().to_string();
        L4 { door, udp, tls, sni, form: Form::Plain, wait: OPEN_WAIT, conn: tokio::sync::Mutex::new(None) }
    }

    /// The TLS name of the door (podnet's `--l4-sni`).
    pub fn with_sni(mut self, sni: &str) -> L4 {
        self.sni = sni.to_string();
        self
    }

    /// The form of each CONNECT.
    pub fn with_form(mut self, form: Form) -> L4 {
        self.form = form;
        self
    }

    /// The longest time to open the QUIC connection.
    pub fn with_wait(mut self, wait: Duration) -> L4 {
        self.wait = wait;
        self
    }

    /// The address of the door.
    pub fn door(&self) -> SocketAddr {
        self.door
    }

    /// The connection of now: a new one when there is none, or it closed.
    pub async fn connection(&self) -> Result<noq::Connection, Failure> {
        let mut slot = self.conn.lock().await;
        if let Some(open) = slot.as_ref()
            && open.conn.close_reason().is_none()
        {
            return Ok(open.conn.clone());
        }
        let datagrams = self.udp.datagrams(self.door).await?;
        let fresh = h3::open(self.door, datagrams, self.tls.clone(), &self.sni, self.wait).await?;
        let conn = fresh.conn.clone();
        *slot = Some(fresh);
        let way = self.udp.label();
        tracing::info!(door = %self.door, sni = %self.sni, %way, "the QUIC connection of the L4 door is open");
        Ok(conn)
    }

    /// A TCP flow to `target` through the door: the local end of a pipe
    /// that carries it.
    pub async fn open(&self, target: &Target) -> Result<DuplexStream, Failure> {
        let conn = self.connection().await?;
        let authority = authority(&target.host(), target.port());
        let (send, recv, frames, status) = h3::request(&conn, &connect_headers_form(&authority, self.form))
            .await
            .map_err(|e| Failure::caused(e.stage, e.class, format!("CONNECT {authority} through the L4 door"), e))?;
        if status != 200 {
            let class = match status {
                400..=499 => Class::Refused,
                500..=599 => Class::Unavailable,
                _ => Class::Protocol,
            };
            return Err(failure(class, format!("the L4 door refused CONNECT {authority} with {status}")));
        }
        let (local, remote) = tokio::io::duplex(PIPE_BUFFER);
        tokio::spawn(pump(send, recv, frames.into_rest(), remote));
        Ok(local)
    }
}

impl Dial for L4 {
    fn label(&self) -> String {
        format!("the L4 door of WARP {}", self.door)
    }

    fn dial<'a>(&'a self, target: &'a Target, deadline: Instant) -> BoxFuture<'a, Result<Connected, Failure>> {
        Box::pin(async move {
            let pipe = tokio::time::timeout_at(deadline, self.open(target)).await.map_err(|_| {
                failure(
                    Class::Timeout,
                    format!("CONNECT {target} through the L4 door {}: no answer in time", self.door),
                )
            })??;
            Ok(Connected::new(Box::new(pipe)))
        })
    }

    /// The door resolves a name.
    fn names_inside(&self) -> bool {
        true
    }
}

/// Copy the pipe to DATA frames of the stream, and the DATA frames of the
/// stream (after `rest`, the bytes that came with the answer) to the pipe.
async fn pump(mut send: noq::SendStream, mut recv: noq::RecvStream, rest: Vec<u8>, pipe: DuplexStream) {
    let (mut pipe_read, mut pipe_write) = tokio::io::split(pipe);
    let up = async {
        let mut buf = vec![0u8; UP_CHUNK];
        loop {
            match pipe_read.read(&mut buf).await {
                Ok(0) | Err(_) => {
                    let _ = send.finish();
                    break;
                }
                Ok(n) => {
                    if send.write_all(&frame(FRAME_DATA, &buf[..n])).await.is_err() {
                        break;
                    }
                }
            }
        }
    };
    let down = async {
        let mut data = DataFrames::default();
        let mut out = Vec::new();
        data.feed(&rest, &mut out);
        loop {
            if !out.is_empty() {
                if pipe_write.write_all(&out).await.is_err() {
                    // The local end is gone: the far end stops sending.
                    let _ = recv.stop(0u32.into());
                    return;
                }
                out.clear();
            }
            match recv.read_chunk(DOWN_CHUNK).await {
                Ok(Some(chunk)) => data.feed(&chunk, &mut out),
                _ => break,
            }
        }
        let _ = pipe_write.shutdown().await;
    };
    tokio::join!(up, down);
}

impl Warp {
    /// The L4 door of the device (podnet's `WarpClient::l4`): the pinned
    /// door of [`crate::Settings::door`] (an address), else the first door
    /// of the device record; the UDP of [`crate::Way::udp`], the client
    /// certificate and the pinned key of the record. A way with no UDP, or a
    /// pinned door that is a name, is `Usage`.
    pub fn l4(&self) -> Result<L4, Failure> {
        let way = &self.settings().way;
        let Some(udp) = way.udp.clone() else {
            return Err(no_udp("the L4 door", &way.dial.label()));
        };
        let door = match &self.settings().door {
            crate::Door::Pinned(Target::Addr(addr)) => *addr,
            crate::Door::Pinned(name) => {
                return Err(failure(Class::Usage, format!("the L4 door takes the address of a door, not {name}")));
            }
            crate::Door::Record => self
                .device()
                .masque_endpoint()
                .ok_or_else(|| failure(Class::Protocol, "the device record has no front door for the L4 door"))?,
        };
        let pinned = crate::open::pin_of(self.device())?;
        let tls = crate::tls::client_config(way.provider.clone(), self.identity(), pinned, h3::ALPN)?;
        Ok(L4::new(door, udp, tls))
    }
}

#[cfg(test)]
mod tests;
