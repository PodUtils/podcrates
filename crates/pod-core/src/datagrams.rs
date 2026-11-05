//! Datagrams through a way out: the system socket, WARP, UDP in a stream of
//! the relay. The shape is that of the `AsyncUdpSocket` of quinn and noq
//! (several transmits for each send, several buffers for each receive, the
//! source and the destination of each datagram), so that `pod-quic` can run
//! QUIC over any implementation.

use std::fmt;
use std::future::poll_fn;
use std::io::{self, IoSliceMut};
use std::net::{IpAddr, SocketAddr};
use std::task::{Context, Poll};

use tokio::io::ReadBuf;
use tokio::net::UdpSocket;

/// One datagram to send, or several of `segment_size` bytes each.
#[derive(Clone, Copy, Debug)]
pub struct Transmit<'a> {
    pub destination: SocketAddr,
    pub contents: &'a [u8],
    /// With `Some(n)`, `contents` is several datagrams of `n` bytes each
    /// (the last can be shorter).
    pub segment_size: Option<usize>,
    /// The source address, when the way has more than one.
    pub src_ip: Option<IpAddr>,
}

/// What [`Datagrams::poll_recv`] wrote into one buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecvMeta {
    /// The sender.
    pub addr: SocketAddr,
    /// The bytes written into the buffer.
    pub len: usize,
    /// The size of each datagram in the buffer (`len` for one datagram).
    pub stride: usize,
    /// The local address that the datagram came to, when known.
    pub dst_ip: Option<IpAddr>,
}

/// A source and a sink of datagrams.
pub trait Datagrams: Send + Sync + fmt::Debug {
    /// Ready when [`Datagrams::try_send`] can take a transmit.
    fn poll_writable(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>>;

    /// Send one transmit, or fail with `WouldBlock`.
    fn try_send(&self, transmit: &Transmit<'_>) -> io::Result<()>;

    /// Receive into `bufs`, one datagram (or one batch) for each buffer;
    /// the count of buffers written.
    fn poll_recv(
        &self,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
        meta: &mut [RecvMeta],
    ) -> Poll<io::Result<usize>>;

    fn local_addr(&self) -> io::Result<SocketAddr>;

    /// The most segments of one transmit (1 with no GSO).
    fn max_transmit_segments(&self) -> usize {
        1
    }

    /// The most datagrams of one receive buffer (1 with no GRO).
    fn max_receive_segments(&self) -> usize {
        1
    }

    /// True when a datagram can be fragmented on its way.
    fn may_fragment(&self) -> bool {
        true
    }
}

/// Send `buf` to `to`, and wait while the way is full.
pub async fn send_to(d: &dyn Datagrams, buf: &[u8], to: SocketAddr) -> io::Result<()> {
    let t = Transmit { destination: to, contents: buf, segment_size: None, src_ip: None };
    loop {
        poll_fn(|cx| d.poll_writable(cx)).await?;
        match d.try_send(&t) {
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
            other => return other,
        }
    }
}

/// Receive one datagram into `buf`: its length and its sender.
pub async fn recv_from(d: &dyn Datagrams, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
    let blank = RecvMeta { addr: SocketAddr::from(([0, 0, 0, 0], 0)), len: 0, stride: 0, dst_ip: None };
    let mut meta = [blank];
    poll_fn(|cx| {
        let mut bufs = [IoSliceMut::new(&mut buf[..])];
        d.poll_recv(cx, &mut bufs, &mut meta)
    })
    .await?;
    Ok((meta[0].len, meta[0].addr))
}

/// The UDP socket of the system.
#[derive(Debug)]
pub struct SystemDatagrams {
    socket: UdpSocket,
}

impl SystemDatagrams {
    pub fn new(socket: UdpSocket) -> SystemDatagrams {
        SystemDatagrams { socket }
    }

    /// A socket bound to `addr` (`0.0.0.0:0` for any port).
    pub async fn bind(addr: SocketAddr) -> io::Result<SystemDatagrams> {
        Ok(SystemDatagrams { socket: UdpSocket::bind(addr).await? })
    }

    pub fn socket(&self) -> &UdpSocket {
        &self.socket
    }
}

impl Datagrams for SystemDatagrams {
    fn poll_writable(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.socket.poll_send_ready(cx)
    }

    fn try_send(&self, t: &Transmit<'_>) -> io::Result<()> {
        let size = t.segment_size.filter(|s| *s > 0).unwrap_or(t.contents.len().max(1));
        for chunk in t.contents.chunks(size) {
            self.socket.try_send_to(chunk, t.destination)?;
        }
        Ok(())
    }

    fn poll_recv(
        &self,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
        meta: &mut [RecvMeta],
    ) -> Poll<io::Result<usize>> {
        let (Some(buf), Some(m)) = (bufs.first_mut(), meta.first_mut()) else {
            return Poll::Ready(Ok(0));
        };
        let mut read = ReadBuf::new(buf);
        match self.socket.poll_recv_from(cx, &mut read) {
            Poll::Ready(Ok(addr)) => {
                let len = read.filled().len();
                *m = RecvMeta { addr, len, stride: len, dst_ip: None };
                Poll::Ready(Ok(1))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_system_socket_carries_a_datagram_and_its_sender() {
        pod_testkit::need_loopback!();
        let a = SystemDatagrams::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
        let b = SystemDatagrams::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
        send_to(&a, b"one", b.local_addr().unwrap()).await.unwrap();
        let mut buf = [0u8; 16];
        let (n, from) = recv_from(&b, &mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"one");
        assert_eq!(from, a.local_addr().unwrap());
    }

    #[tokio::test]
    async fn a_transmit_of_segments_arrives_as_several_datagrams() {
        pod_testkit::need_loopback!();
        let a = SystemDatagrams::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
        let b = SystemDatagrams::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
        let t = Transmit {
            destination: b.local_addr().unwrap(),
            contents: b"aaabbbc",
            segment_size: Some(3),
            src_ip: None,
        };
        poll_fn(|cx| a.poll_writable(cx)).await.unwrap();
        a.try_send(&t).unwrap();
        let mut got = Vec::new();
        for _ in 0..3 {
            let mut buf = [0u8; 16];
            let (n, _) = recv_from(&b, &mut buf).await.unwrap();
            got.push(buf[..n].to_vec());
        }
        assert_eq!(got, [b"aaa".to_vec(), b"bbb".to_vec(), b"c".to_vec()]);
    }
}
