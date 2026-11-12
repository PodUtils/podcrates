//! The UDP of a session as [`Datagrams`]. The stack owns a documentation
//! address for a family that the device lacks (`NO_V4`, `NO_V6`), so a
//! datagram to that family would leave with it as its source and get no
//! answer: the socket refuses it (PodUtils/podnet issue 112).

use std::io::{self, IoSliceMut};
use std::net::SocketAddr;
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
    pub(crate) inner: UdpDatagrams,
    pub(crate) families: Families,
}

impl Datagrams for WarpDatagrams {
    fn poll_writable(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.inner.poll_writable(cx)
    }

    fn try_send(&self, transmit: &Transmit<'_>) -> io::Result<()> {
        self.families.check(transmit.destination)?;
        self.inner.try_send(transmit)
    }

    fn poll_recv(
        &self,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
        meta: &mut [RecvMeta],
    ) -> Poll<io::Result<usize>> {
        self.inner.poll_recv(cx, bufs, meta)
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
