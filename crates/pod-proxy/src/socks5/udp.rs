//! UDP through a SOCKS5 proxy: the association of RFC 1928 (`UDP ASSOCIATE`,
//! command 0x03) and the command 0x83, "UDP in the stream". After the reply
//! of 0x83, each datagram in each direction is a frame on the control
//! connection: a length of 2 bytes (big endian), then a SOCKS5 UDP datagram
//! (RFC 1928, section 7: the header and the payload). No UDP socket is
//! necessary at either end, so a host that refuses datagram sockets keeps
//! UDP. A server that does not know 0x83 answers 0x07 ([`DialError::UdpRefused`]);
//! the caller can then use `UDP ASSOCIATE`. The caller dials the proxy and
//! gives the stream; the greeting and the user and password are those of
//! the TCP path. The pod-session crate keeps its own adapter for UDP in a
//! stream (T-020): these are the frames of SOCKS5 only.

use std::io;
use std::net::{IpAddr, SocketAddr};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::{IPV4, IPV6, NAME, Step, VERSION, greet, reply_text};
use crate::error::DialError;
use crate::proxy::Proxy;

/// The command of RFC 1928: a UDP association with a UDP socket at the proxy.
pub const CMD_UDP_ASSOCIATE: u8 = 0x03;
/// The command of a UDP association on the control connection itself.
pub const CMD_UDP_IN_STREAM: u8 = 0x83;
/// The reply of a server that does not know the command.
pub const REPLY_COMMAND_NOT_SUPPORTED: u8 = 0x07;

/// The greeting, then `command` with the address 0.0.0.0:0 (the client's
/// expected source is any), then the reply. The address of the reply is
/// returned.
async fn command<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    app: &str,
    proxy: &Proxy,
    command: u8,
) -> Result<SocketAddr, Step> {
    greet(stream, app, proxy).await?;
    stream.write_all(&[VERSION, command, 0x00, IPV4, 0, 0, 0, 0, 0, 0]).await.map_err(Step::Io)?;
    let mut head = [0u8; 4];
    stream.read_exact(&mut head).await.map_err(Step::Io)?;
    if head[0] != VERSION {
        return Err(Step::Protocol(format!("its reply has version {}, not 5", head[0])));
    }
    if head[1] != 0x00 {
        return Err(Step::Reply(head[1]));
    }
    bound(stream, head[3]).await
}

/// The bound address of a reply, read to exactly its end. A name must be an
/// address in text: the caller needs an address to send to.
async fn bound<S: AsyncRead + Unpin>(stream: &mut S, kind: u8) -> Result<SocketAddr, Step> {
    let ip = match kind {
        IPV4 => {
            let mut b = [0u8; 4];
            stream.read_exact(&mut b).await.map_err(Step::Io)?;
            IpAddr::from(b)
        }
        IPV6 => {
            let mut b = [0u8; 16];
            stream.read_exact(&mut b).await.map_err(Step::Io)?;
            IpAddr::from(b)
        }
        NAME => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await.map_err(Step::Io)?;
            let mut host = vec![0u8; usize::from(len[0])];
            stream.read_exact(&mut host).await.map_err(Step::Io)?;
            let host = String::from_utf8_lossy(&host).into_owned();
            host.parse().map_err(|_| Step::Protocol("its reply has a name, not an address, for the relay".into()))?
        }
        other => return Err(Step::Protocol(format!("its reply has the address type {other}"))),
    };
    let mut port = [0u8; 2];
    stream.read_exact(&mut port).await.map_err(Step::Io)?;
    Ok(SocketAddr::new(ip, u16::from_be_bytes(port)))
}

/// A refusal, in the terms of the TCP path; `unknown` is the reply that
/// means that the server does not know the command.
fn failure(proxy: &Proxy, step: Step, unknown: bool) -> DialError {
    let name = proxy.to_string();
    match step {
        Step::Reply(REPLY_COMMAND_NOT_SUPPORTED) if unknown => DialError::UdpRefused { proxy: name },
        Step::Reply(reply) => DialError::ProxyRefused {
            proxy: name,
            target: "UDP".into(),
            status: if reply == 0x02 { 403 } else { 502 },
            reason: format!("SOCKS5 reply {reply}, {}", reply_text(reply)),
        },
        Step::Auth(why) => {
            DialError::ProxyRefused { proxy: name, target: "UDP".into(), status: 407, reason: format!("SOCKS5: {why}") }
        }
        Step::Io(e) => DialError::ProxyProtocol { proxy: name, detail: format!("SOCKS5: {e}") },
        Step::Protocol(detail) => DialError::ProxyProtocol { proxy: name, detail: format!("SOCKS5: {detail}") },
        Step::Unreachable(e) | Step::Resolve(e) => e,
    }
}

/// Ask `proxy` for a UDP association in the stream (0x83) on `stream`, a
/// connection to it. The stream returned carries the frames of
/// [`write_frame`] and [`read_frame`]. The caller bounds the time.
///
/// # Errors
///
/// [`DialError::UdpRefused`] when the server answers 0x07: it does not know
/// the command, and the caller can use [`associate`]. A close or a break of
/// the protocol is a [`DialError::ProxyProtocol`].
pub async fn open_in_stream<S>(mut stream: S, app: &str, proxy: &Proxy) -> Result<S, DialError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    command(&mut stream, app, proxy, CMD_UDP_IN_STREAM).await.map_err(|step| failure(proxy, step, true))?;
    Ok(stream)
}

/// Ask `proxy` for a UDP association of RFC 1928 on `stream`, a connection
/// to it. The address that comes with the stream is the UDP relay of the
/// proxy: the caller sends the datagrams there ([`wrap`]), and keeps the
/// stream open as long as it needs the association.
///
/// # Errors
///
/// A refusal is a [`DialError::ProxyRefused`]; a close or a break of the
/// protocol is a [`DialError::ProxyProtocol`].
pub async fn associate<S>(mut stream: S, app: &str, proxy: &Proxy) -> Result<(S, SocketAddr), DialError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let relay =
        command(&mut stream, app, proxy, CMD_UDP_ASSOCIATE).await.map_err(|step| failure(proxy, step, false))?;
    Ok((stream, relay))
}

/// Write one frame: the length of `datagram` in 2 bytes, then `datagram`.
///
/// # Errors
///
/// `InvalidInput` for a datagram of more than 65535 bytes; else the error
/// of the write.
pub async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, datagram: &[u8]) -> io::Result<()> {
    let len = u16::try_from(datagram.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "the datagram is longer than 65535 bytes"))?;
    let mut frame = Vec::with_capacity(2 + datagram.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(datagram);
    w.write_all(&frame).await
}

/// Read one frame. `Ok(None)` when the stream ends before the length: the
/// proxy closed it between two frames. A stream that ends inside a frame is
/// an `UnexpectedEof` error.
///
/// # Errors
///
/// The error of the read.
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 2];
    match r.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let mut datagram = vec![0u8; usize::from(u16::from_be_bytes(len))];
    r.read_exact(&mut datagram).await?;
    Ok(Some(datagram))
}

/// The datagram of RFC 1928, section 7, with an address as the destination:
/// the header (RSV 2 bytes, FRAG 0, the address type, the address and the
/// port), then `payload`.
#[must_use]
pub fn wrap(payload: &[u8], ip: IpAddr, port: u16) -> Vec<u8> {
    let mut out = vec![0u8, 0, 0];
    match ip {
        IpAddr::V4(v4) => {
            out.push(IPV4);
            out.extend_from_slice(&v4.octets());
        }
        IpAddr::V6(v6) => {
            out.push(IPV6);
            out.extend_from_slice(&v6.octets());
        }
    }
    out.extend_from_slice(&port.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// Like [`wrap`], with a name as the destination (address type 3).
///
/// # Errors
///
/// [`DialError::InvalidTarget`] for a name of more than 255 bytes.
pub fn wrap_name(payload: &[u8], name: &str, port: u16) -> Result<Vec<u8>, DialError> {
    let len = u8::try_from(name.len())
        .map_err(|_| DialError::InvalidTarget(format!("{name:?} is longer than the 255 bytes of SOCKS5")))?;
    let mut out = vec![0u8, 0, 0, NAME, len];
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&port.to_be_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// The destination and the payload of a datagram of RFC 1928, section 7:
/// the reverse of [`wrap`] and [`wrap_name`], for a server of a UDP
/// association. `None` for a fragment (FRAG not 0), a datagram shorter than
/// its header, an empty name, a name that is not UTF-8 and an unknown
/// address type: a server drops such a datagram.
#[must_use]
pub fn unwrap(datagram: &[u8]) -> Option<(pod_core::Target, &[u8])> {
    if datagram.len() < 4 || datagram[2] != 0 {
        return None;
    }
    let (host, rest) = match datagram[3] {
        IPV4 => {
            let ip: [u8; 4] = datagram.get(4..8)?.try_into().ok()?;
            (IpAddr::from(ip).to_string(), &datagram[8..])
        }
        IPV6 => {
            let ip: [u8; 16] = datagram.get(4..20)?.try_into().ok()?;
            (IpAddr::from(ip).to_string(), &datagram[20..])
        }
        NAME => {
            let len = usize::from(*datagram.get(4)?);
            let name = std::str::from_utf8(datagram.get(5..5 + len)?).ok().filter(|n| !n.is_empty())?;
            (name.to_string(), &datagram[5 + len..])
        }
        _ => return None,
    };
    let port = u16::from_be_bytes(rest.get(..2)?.try_into().ok()?);
    Some((pod_core::Target::new(&host, port), &rest[2..]))
}
