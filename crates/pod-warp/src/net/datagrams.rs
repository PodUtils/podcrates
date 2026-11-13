//! The UDP of a session as [`Datagrams`]. The stack owns a documentation
//! address for a family that the device lacks (`NO_V4`, `NO_V6`), so a
//! datagram to that family would leave with it as its source and get no
//! answer: the socket refuses it (PodUtils/podnet issue 112).
//!
//! The socket binds `[::]`, so a QUIC endpoint on it (noq) sends to the
//! IPv4-mapped form of an IPv4 peer. The stack would send that as an IPv6
//! packet to `::ffff:a.b.c.d`, which WARP's network drops: the socket sends
//! it as IPv4, and gives the answers of that peer back in the mapped form
//! (PodUtils/podnet#524).

use std::collections::HashSet;
use std::io::{self, IoSliceMut};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Mutex, PoisonError};
use std::task::{Context, Poll};

use pod_core::{Datagrams, RecvMeta, Transmit};
use pod_netstack::UdpDatagrams;

/// The families of the addresses of a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Families {
    pub(crate) v4: bool,
    pub(crate) v6: bool,
}

impl Families {
    /// Refuse a datagram to `to` when the device has no address of its
    /// family: `InvalidInput`, with podnet's words.
    pub(crate) fn check(self, to: SocketAddr) -> io::Result<()> {
        let (has, family) = if to.is_ipv4() { (self.v4, "IPv4") } else { (self.v6, "IPv6") };
        if has {
            return Ok(());
        }
        Err(io::Error::new(io::ErrorKind::InvalidInput, format!("the tunnel has no {family} address for {to}")))
    }
}

/// A UDP socket of a session on each address of the device.
#[derive(Debug)]
pub(crate) struct WarpDatagrams {
    inner: UdpDatagrams,
    families: Families,
    /// The IPv4 peers that the caller names in the IPv4-mapped form.
    mapped: Mutex<HashSet<SocketAddr>>,
}

impl WarpDatagrams {
    pub(crate) fn new(inner: UdpDatagrams, families: Families) -> WarpDatagrams {
        WarpDatagrams { inner, families, mapped: Mutex::new(HashSet::new()) }
    }
}

/// The IPv4 form of an IPv4-mapped IPv6 address, else the address.
fn canonical(ip: IpAddr) -> IpAddr {
    ip.to_canonical()
}

/// The IPv4-mapped form of an IPv4 address, else the address.
fn mapped(addr: SocketAddr) -> SocketAddr {
    match addr {
        SocketAddr::V4(v4) => SocketAddr::new(IpAddr::V6(v4.ip().to_ipv6_mapped()), v4.port()),
        other => other,
    }
}

impl Datagrams for WarpDatagrams {
    fn poll_writable(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.inner.poll_writable(cx)
    }

    fn try_send(&self, transmit: &Transmit<'_>) -> io::Result<()> {
        let destination = SocketAddr::new(canonical(transmit.destination.ip()), transmit.destination.port());
        self.families.check(destination)?;
        if destination == transmit.destination {
            return self.inner.try_send(transmit);
        }
        self.mapped.lock().unwrap_or_else(PoisonError::into_inner).insert(destination);
        let src_ip = transmit.src_ip.map(canonical);
        self.inner.try_send(&Transmit { destination, src_ip, ..*transmit })
    }

    fn poll_recv(
        &self,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
        meta: &mut [RecvMeta],
    ) -> Poll<io::Result<usize>> {
        let got = self.inner.poll_recv(cx, bufs, meta);
        if let Poll::Ready(Ok(n)) = got {
            let peers = self.mapped.lock().unwrap_or_else(PoisonError::into_inner);
            if !peers.is_empty() {
                for m in meta.iter_mut().take(n).filter(|m| peers.contains(&m.addr)) {
                    m.addr = mapped(m.addr);
                }
            }
        }
        got
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.local_addr()
    }

    fn max_transmit_segments(&self) -> usize {
        self.inner.max_transmit_segments()
    }

    fn max_receive_segments(&self) -> usize {
        self.inner.max_receive_segments()
    }

    fn may_fragment(&self) -> bool {
        self.inner.may_fragment()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_forms_of_an_ipv4_address() {
        let v4: SocketAddr = "198.41.192.67:7844".parse().unwrap();
        let in_v6: SocketAddr = "[::ffff:198.41.192.67]:7844".parse().unwrap();
        assert_eq!(mapped(v4), in_v6);
        assert_eq!(SocketAddr::new(canonical(in_v6.ip()), in_v6.port()), v4);
        let v6: SocketAddr = "[2606:4700::1]:443".parse().unwrap();
        assert_eq!(mapped(v6), v6);
        assert_eq!(canonical(v6.ip()), v6.ip());
        // A mapped IPv4 destination is checked as IPv4.
        let v4_only = Families { v4: true, v6: false };
        assert!(v4_only.check(SocketAddr::new(canonical(in_v6.ip()), 7844)).is_ok());
    }

    #[test]
    fn a_family_that_the_device_lacks_is_refused() {
        let v4_only = Families { v4: true, v6: false };
        let to_v6: SocketAddr = "[2606:4700:4700::1111]:53".parse().unwrap();
        let err = v4_only.check(to_v6).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(err.to_string(), "the tunnel has no IPv6 address for [2606:4700:4700::1111]:53");
        assert!(v4_only.check("1.1.1.1:53".parse().unwrap()).is_ok());
        let v6_only = Families { v4: false, v6: true };
        assert!(v6_only.check("1.1.1.1:53".parse().unwrap()).unwrap_err().to_string().contains("no IPv4 address"));
        assert!(v6_only.check(to_v6).is_ok());
    }
}
