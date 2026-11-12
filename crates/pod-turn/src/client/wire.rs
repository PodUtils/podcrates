//! The transport of a TURN client: the datagrams of UDP, or the byte stream
//! of TLS, and the two kinds of message on it (RFC 8656, 12.5): a STUN
//! message (the first two bits 00) or ChannelData (01). On a stream each
//! message is cut by its own length, and ChannelData is padded to a
//! multiple of 4.

use std::net::SocketAddr;
use std::sync::Arc;

use pod_core::{BoxedIo, Datagrams};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::Error;
use crate::stun::HEADER;

/// The first and the last channel number (RFC 8656, 12).
pub const FIRST_CHANNEL: u16 = 0x4000;
pub const LAST_CHANNEL: u16 = 0x4fff;
/// The header of ChannelData: the number and the length.
const CHANNEL_HEADER: usize = 4;
/// The largest datagram that the client reads: an IP packet's limit.
const DATAGRAM: usize = 65_536;

/// One message that the server sent.
#[derive(Debug, PartialEq, Eq)]
pub enum Frame {
    /// A STUN message, whole, not read yet.
    Stun(Vec<u8>),
    /// ChannelData: the number and the data, with no padding.
    Channel { number: u16, data: Vec<u8> },
}

/// The connection to the server.
pub enum Conn {
    Udp { socket: Arc<dyn Datagrams>, server: SocketAddr, buf: Vec<u8> },
    Stream { io: BoxedIo, buf: Vec<u8> },
}

impl Conn {
    pub fn udp(socket: Arc<dyn Datagrams>, server: SocketAddr) -> Conn {
        Conn::Udp { socket, server, buf: vec![0; DATAGRAM] }
    }

    pub fn stream(io: BoxedIo) -> Conn {
        Conn::Stream { io, buf: Vec::new() }
    }

    /// True on a stream, where ChannelData takes its padding.
    pub fn padded(&self) -> bool {
        matches!(self, Conn::Stream { .. })
    }

    pub async fn send(&mut self, bytes: &[u8]) -> Result<(), Error> {
        match self {
            Conn::Udp { socket, server, .. } => {
                pod_core::datagrams::send_to(socket.as_ref(), bytes, *server).await.map_err(Error::Io)
            }
            Conn::Stream { io, .. } => {
                io.write_all(bytes).await.map_err(Error::Io)?;
                io.flush().await.map_err(Error::Io)
            }
        }
    }

    /// The next message of the server. A datagram of another sender, or
    /// one that is neither kind, is dropped. Cancel safe: a part of a
    /// message on a stream stays in the buffer for the next call.
    pub async fn recv(&mut self) -> Result<Frame, Error> {
        match self {
            Conn::Udp { socket, server, buf } => loop {
                let (len, from) = pod_core::datagrams::recv_from(socket.as_ref(), buf).await.map_err(Error::Io)?;
                if from != *server {
                    tracing::debug!(%from, "TURN: a datagram of another sender is dropped");
                    continue;
                }
                if let Some(frame) = datagram(&buf[..len]) {
                    return Ok(frame);
                }
                tracing::debug!(len, "TURN: a datagram that is neither STUN nor ChannelData is dropped");
            },
            Conn::Stream { io, buf } => loop {
                if let Some(whole) = frame_len(buf)? {
                    let rest = buf.split_off(whole);
                    let bytes = std::mem::replace(buf, rest);
                    return Ok(stream_frame(bytes));
                }
                let mut chunk = [0u8; 16 * 1024];
                let n = io.read(&mut chunk).await.map_err(Error::Io)?;
                if n == 0 {
                    return Err(Error::Closed);
                }
                buf.extend_from_slice(&chunk[..n]);
            },
        }
    }

    /// The end of the stream; nothing for UDP.
    pub async fn shutdown(&mut self) {
        if let Conn::Stream { io, .. } = self {
            let _ = io.shutdown().await;
        }
    }
}

/// ChannelData of `number` with `data`, padded to a multiple of 4 when
/// `padded` (a stream; RFC 8656, 12.5).
pub fn channel_data(number: u16, data: &[u8], padded: bool) -> Result<Vec<u8>, Error> {
    let len = u16::try_from(data.len()).map_err(|_| Error::TooLarge(data.len()))?;
    let mut out = Vec::with_capacity(CHANNEL_HEADER + data.len() + 3);
    out.extend_from_slice(&number.to_be_bytes());
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(data);
    if padded {
        out.resize(out.len() + (4 - data.len() % 4) % 4, 0);
    }
    Ok(out)
}

/// The message of one datagram: a STUN message is the whole datagram;
/// ChannelData says its length, and the padding after it (which UDP does
/// not need) is dropped. `None` for anything else.
pub fn datagram(bytes: &[u8]) -> Option<Frame> {
    match bytes.first()? >> 6 {
        0b00 => Some(Frame::Stun(bytes.to_vec())),
        0b01 if bytes.len() >= CHANNEL_HEADER => {
            let number = u16::from_be_bytes([bytes[0], bytes[1]]);
            let len = usize::from(u16::from_be_bytes([bytes[2], bytes[3]]));
            let data = bytes.get(CHANNEL_HEADER..CHANNEL_HEADER + len)?;
            Some(Frame::Channel { number, data: data.to_vec() })
        }
        _ => None,
    }
}

/// The length of the first message in `buf`, a stream: `None` until the
/// whole message is there. Bytes that are neither kind cut the stream.
pub fn frame_len(buf: &[u8]) -> Result<Option<usize>, Error> {
    let Some(first) = buf.first() else { return Ok(None) };
    if buf.len() < CHANNEL_HEADER {
        return Ok(None);
    }
    let len = usize::from(u16::from_be_bytes([buf[2], buf[3]]));
    let whole = match first >> 6 {
        0b00 => HEADER + len,
        0b01 => CHANNEL_HEADER + len + (4 - len % 4) % 4,
        _ => {
            return Err(Error::Frame {
                offset: 0,
                why: format!("a first byte of 0x{first:02x}, neither STUN nor ChannelData"),
            });
        }
    };
    Ok((buf.len() >= whole).then_some(whole))
}

/// A whole message of a stream, as [`frame_len`] cut it.
fn stream_frame(mut bytes: Vec<u8>) -> Frame {
    if bytes[0] >> 6 == 0b00 {
        return Frame::Stun(bytes);
    }
    let number = u16::from_be_bytes([bytes[0], bytes[1]]);
    let len = usize::from(u16::from_be_bytes([bytes[2], bytes[3]]));
    bytes.truncate(CHANNEL_HEADER + len);
    bytes.drain(..CHANNEL_HEADER);
    Frame::Channel { number, data: bytes }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_data_is_padded_on_a_stream_alone() {
        assert_eq!(channel_data(0x4001, b"abcde", false).unwrap(), b"\x40\x01\x00\x05abcde");
        let padded = channel_data(0x4001, b"abcde", true).unwrap();
        assert_eq!(padded, b"\x40\x01\x00\x05abcde\0\0\0");
        assert_eq!(frame_len(&padded).unwrap(), Some(12));
        assert_eq!(frame_len(&padded[..11]).unwrap(), None);
        assert_eq!(stream_frame(padded), Frame::Channel { number: 0x4001, data: b"abcde".to_vec() });
        assert!(matches!(channel_data(0x4001, &vec![0; 65_536], true), Err(Error::TooLarge(65_536))));
    }

    #[test]
    fn a_datagram_is_stun_or_channel_data_or_nothing() {
        let stun = crate::stun::binding_request(&[1; 12]);
        assert_eq!(datagram(&stun), Some(Frame::Stun(stun.to_vec())));
        assert_eq!(datagram(b"\x40\x02\x00\x02hi\0\0"), Some(Frame::Channel { number: 0x4002, data: b"hi".to_vec() }));
        assert_eq!(datagram(b"\x40\x02\x00\x09hi"), None, "a length past the datagram");
        assert_eq!(datagram(b"\xc0\x00\x00\x00"), None, "QUIC's long header");
        assert_eq!(datagram(b""), None);
    }

    #[test]
    fn a_stream_is_cut_by_each_length_and_refuses_other_bytes() {
        let stun = crate::stun::binding_request(&[2; 12]);
        assert_eq!(frame_len(&stun[..19]).unwrap(), None);
        assert_eq!(frame_len(&stun).unwrap(), Some(20));
        assert!(matches!(frame_len(b"\x80\x00\x00\x00"), Err(Error::Frame { offset: 0, .. })));
        assert_eq!(frame_len(b"\x40").unwrap(), None);
    }
}
