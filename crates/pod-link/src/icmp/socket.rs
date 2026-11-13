//! A raw ICMPv4 socket (`SOCK_RAW`, `IPPROTO_ICMP`), which needs
//! `CAP_NET_RAW` or uid 0. It sends and takes whole ICMP messages; a read of
//! a raw IPv4 socket gives the IP header, which the socket strips. For the
//! real path of the ICMP carrier; the tests without a privilege use a memory
//! pair instead (call C276). Unix only.

use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use tokio::io::unix::AsyncFd;

/// The largest IP packet that a read takes.
const MAX_PACKET: usize = 65535;

/// A raw ICMPv4 socket.
pub(super) struct RawIcmp {
    fd: AsyncFd<OwnedFd>,
    mtu: usize,
}

impl RawIcmp {
    /// Open a non-blocking raw ICMPv4 socket. A host with no privilege for it
    /// fails here.
    pub(super) fn open(mtu: usize) -> io::Result<RawIcmp> {
        // SAFETY: a call of `socket` with constant arguments.
        let raw = unsafe { libc::socket(libc::AF_INET, libc::SOCK_RAW | libc::SOCK_CLOEXEC, libc::IPPROTO_ICMP) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `raw` is a fresh, owned descriptor.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        set_nonblocking(&fd)?;
        // SAFETY: `fd` owns the descriptor and keeps it open for the AsyncFd.
        let fd = unsafe { AsyncFd::register(fd) }?;
        Ok(RawIcmp { fd, mtu })
    }

    pub(super) fn mtu(&self) -> usize {
        self.mtu
    }

    /// Bind the socket to `ip`, so a server takes the ICMP of that address.
    pub(super) fn bind(&self, ip: Ipv4Addr) -> io::Result<()> {
        let addr = sockaddr(SocketAddrV4::new(ip, 0));
        // SAFETY: `addr` is a valid `sockaddr_in` of its given length.
        let r = unsafe {
            libc::bind(self.fd.as_raw_fd(), (&raw const addr).cast(), std::mem::size_of::<libc::sockaddr_in>() as u32)
        };
        if r < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }

    /// The local address of the socket.
    pub(super) fn local_ip(&self) -> io::Result<Ipv4Addr> {
        let mut addr: libc::sockaddr_in = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::sockaddr_in>() as u32;
        // SAFETY: `addr` and `len` are a valid buffer and its length.
        let r = unsafe { libc::getsockname(self.fd.as_raw_fd(), (&raw mut addr).cast(), &mut len) };
        if r < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr)))
    }

    /// Send a whole ICMP message to `addr`.
    pub(super) async fn send_to(&self, bytes: &[u8], addr: SocketAddr) -> io::Result<()> {
        let SocketAddr::V4(v4) = addr else {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "the ICMP carrier is IPv4 only"));
        };
        let sa = sockaddr(v4);
        loop {
            let mut guard = self.fd.writable().await?;
            let send = guard.try_io(|_| {
                // SAFETY: `bytes` and `sa` are valid buffers of their lengths.
                let r = unsafe {
                    libc::sendto(
                        self.fd.as_raw_fd(),
                        bytes.as_ptr().cast(),
                        bytes.len(),
                        0,
                        (&raw const sa).cast(),
                        std::mem::size_of::<libc::sockaddr_in>() as u32,
                    )
                };
                if r < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
            });
            match send {
                Ok(result) => return result,
                Err(_would_block) => continue,
            }
        }
    }

    /// The next whole ICMP message (the IP header stripped) and its sender.
    pub(super) async fn recv_message(&self) -> io::Result<(Vec<u8>, SocketAddr)> {
        let mut buf = vec![0u8; MAX_PACKET];
        loop {
            let mut guard = self.fd.readable().await?;
            let mut from: libc::sockaddr_in = unsafe { std::mem::zeroed() };
            let mut len = std::mem::size_of::<libc::sockaddr_in>() as u32;
            let recv = guard.try_io(|_| {
                // SAFETY: `buf` and `from` are valid buffers of their lengths.
                let r = unsafe {
                    libc::recvfrom(
                        self.fd.as_raw_fd(),
                        buf.as_mut_ptr().cast(),
                        buf.len(),
                        0,
                        (&raw mut from).cast(),
                        &mut len,
                    )
                };
                if r < 0 { Err(io::Error::last_os_error()) } else { Ok(r as usize) }
            });
            match recv {
                Ok(Ok(n)) => {
                    let ip = Ipv4Addr::from(u32::from_be(from.sin_addr.s_addr));
                    let Some(icmp) = strip_ip_header(&buf[..n]) else { continue };
                    return Ok((icmp.to_vec(), SocketAddr::new(IpAddr::V4(ip), 0)));
                }
                Ok(Err(e)) => return Err(e),
                Err(_would_block) => continue,
            }
        }
    }
}

/// The ICMP message of an IPv4 packet: the bytes after its header. `None`
/// when the packet is not a well-formed IPv4 header.
fn strip_ip_header(packet: &[u8]) -> Option<&[u8]> {
    let first = *packet.first()?;
    if first >> 4 != 4 {
        return None;
    }
    let ihl = usize::from(first & 0x0f) * 4;
    packet.get(ihl..)
}

/// A `sockaddr_in` for `addr`.
fn sockaddr(addr: SocketAddrV4) -> libc::sockaddr_in {
    let mut sa: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    sa.sin_family = libc::AF_INET as libc::sa_family_t;
    sa.sin_port = addr.port().to_be();
    sa.sin_addr.s_addr = u32::from(*addr.ip()).to_be();
    sa
}

/// Set `O_NONBLOCK` on `fd`.
fn set_nonblocking(fd: &OwnedFd) -> io::Result<()> {
    // SAFETY: `F_GETFL` and `F_SETFL` on a valid descriptor.
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    let r = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if r < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}
