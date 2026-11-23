//! The raw-socket path of the probe. It exists only with the feature
//! `sidecar`, and it checks the host before it opens a socket (rules SYN-1
//! and LIM-2). A host that refuses it gives `Unknown` with the reason, never
//! a failure.
//!
//! The path sends a SYN, waits for a SYN-ACK, sends the ACK, and then sends
//! the unusual segment. A kernel that owns no socket for the four-tuple may
//! answer with a RST; the probe reports that answer as it is. Use the path
//! against a host that accepts the connection.

use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use crate::error::Refusal;
use crate::packet::{CLIENT_PORT, Segment, WINDOW};
use crate::probe::{Observation, Path, ProbeKind};

/// The wait of one read, in microseconds.
const WAIT_US: libc::suseconds_t = 250_000;
/// The most replies that one probe collects.
const MAX_REPLIES: usize = 8;

/// Whether this host lets the process open a raw TCP socket. It opens one
/// and closes it at once; it sends nothing.
pub fn capability() -> Result<(), Refusal> {
    let fd = socket()?;
    drop(fd);
    Ok(())
}

/// The raw-socket path to one target.
pub struct RawPath {
    fd: OwnedFd,
    local: Ipv4Addr,
    target: SocketAddrV4,
    isn: u32,
}

impl RawPath {
    /// Open the path. It calls [`capability`] first, so a host without the
    /// privilege gives a named refusal.
    pub fn open(target: SocketAddrV4, seed: u32) -> Result<RawPath, Refusal> {
        capability()?;
        let local = local_address(target)?;
        let fd = socket()?;
        Ok(RawPath { fd, local, target, isn: 0x4000_0000u32.wrapping_add(seed) })
    }

    pub fn target(&self) -> SocketAddrV4 {
        self.target
    }

    /// One handshake and one unusual segment.
    pub fn exchange(&mut self, kind: ProbeKind) -> Result<Observation, Refusal> {
        self.send(&Segment::syn(self.isn))?;
        let Some(syn_ack) = self.wait_for_syn_ack() else {
            return Ok(Observation::silent());
        };
        let peer = syn_ack.seq.wrapping_add(1);
        self.send(&Segment::ack(self.isn.wrapping_add(1), peer))?;
        let mut expected_ack = None;
        match kind {
            ProbeKind::OutOfWindow => {
                expected_ack = Some(self.isn.wrapping_add(1));
                let mut segment = Segment::data(
                    self.isn.wrapping_add(1).wrapping_add(u32::from(WINDOW) + 10),
                    peer,
                    b"out-of-window".to_vec(),
                );
                segment.seal();
                self.send(&segment)?;
            }
            ProbeKind::Retransmit => {
                for payload in [b"probe".as_slice(), b"PROBE".as_slice()] {
                    let mut segment = Segment::data(self.isn.wrapping_add(1), peer, payload.to_vec());
                    segment.seal();
                    self.send(&segment)?;
                }
            }
            ProbeKind::FinBeforeAck => {
                let mut data = Segment::data(self.isn.wrapping_add(1), peer, b"fin".to_vec());
                data.seal();
                self.send(&data)?;
                let mut fin = Segment::fin(self.isn.wrapping_add(4), peer);
                fin.seal();
                self.send(&fin)?;
            }
        }
        let mut responses = Vec::new();
        for _ in 0..MAX_REPLIES {
            match self.receive() {
                Some(segment) => responses.push(segment),
                None => break,
            }
        }
        Ok(Observation { responses, stream: Vec::new(), expected_ack, expected_stream: None, fault: None })
    }

    fn wait_for_syn_ack(&self) -> Option<Segment> {
        for _ in 0..MAX_REPLIES {
            let segment = self.receive()?;
            if segment.is_syn() && segment.is_ack() && segment.ack == self.isn.wrapping_add(1) {
                return Some(segment);
            }
        }
        None
    }

    fn send(&self, segment: &Segment) -> Result<(), Refusal> {
        let mut segment = segment.clone();
        segment.src_port = CLIENT_PORT;
        segment.dst_port = self.target.port();
        let packet = self.packet(&segment);
        let mut address = sockaddr(self.target);
        // SAFETY: the packet and the address are valid buffers of their
        // lengths, and the descriptor is open for the life of `self`.
        let sent = unsafe {
            libc::sendto(
                self.fd.as_raw_fd(),
                packet.as_ptr().cast(),
                packet.len(),
                0,
                (&raw mut address).cast(),
                std::mem::size_of::<libc::sockaddr_in>() as u32,
            )
        };
        if sent < 0 {
            return Err(refusal("raw socket send", io::Error::last_os_error()));
        }
        Ok(())
    }

    /// One complete IPv4 packet with the TCP segment inside.
    fn packet(&self, segment: &Segment) -> Vec<u8> {
        let mut tcp = segment.to_bytes();
        // The checksum field is zero in the sum, and these addresses are the
        // real ones of the path, not the fixed pair of the simulator.
        tcp[16..18].fill(0);
        let sum =
            crate::packet::checksum_with(&self.local.octets(), &self.target.ip().octets(), &tcp[..20], &tcp[20..]);
        tcp[16..18].copy_from_slice(&sum.to_be_bytes());
        let mut ip = vec![0u8; 20];
        ip[0] = 0x45;
        let total = (20 + tcp.len()) as u16;
        ip[2..4].copy_from_slice(&total.to_be_bytes());
        ip[4..6].copy_from_slice(&(self.isn as u16).to_be_bytes());
        ip[6..8].copy_from_slice(&0x4000u16.to_be_bytes());
        ip[8] = 64;
        ip[9] = 6;
        ip[12..16].copy_from_slice(&self.local.octets());
        ip[16..20].copy_from_slice(&self.target.ip().octets());
        let header_sum = internet_checksum(&ip);
        ip[10..12].copy_from_slice(&header_sum.to_be_bytes());
        ip.extend_from_slice(&tcp);
        ip
    }

    fn receive(&self) -> Option<Segment> {
        let mut buffer = vec![0u8; 65_535];
        for _ in 0..MAX_REPLIES {
            // SAFETY: the buffer is valid for its length, and the descriptor
            // is open for the life of `self`.
            let read = unsafe {
                libc::recvfrom(
                    self.fd.as_raw_fd(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if read <= 0 {
                return None;
            }
            let packet = &buffer[..read as usize];
            if packet.first().map(|b| b >> 4) != Some(4) {
                continue;
            }
            let header = usize::from(packet.first()? & 0x0f) * 4;
            if packet.get(9) != Some(&6) {
                continue;
            }
            let Some(segment) = Segment::from_bytes(packet.get(header..)?).ok() else { continue };
            if segment.src_port == self.target.port() && segment.dst_port == CLIENT_PORT {
                return Some(segment);
            }
        }
        None
    }
}

impl Path for RawPath {
    fn exchange(&mut self, kind: ProbeKind) -> Result<Observation, Refusal> {
        RawPath::exchange(self, kind)
    }
}

/// Open a raw TCP socket with a receive timeout.
fn socket() -> Result<OwnedFd, Refusal> {
    #[cfg(target_os = "linux")]
    // SAFETY: a call of `socket` with constant arguments.
    let raw = unsafe { libc::socket(libc::AF_INET, libc::SOCK_RAW | libc::SOCK_CLOEXEC, libc::IPPROTO_TCP) };
    #[cfg(not(target_os = "linux"))]
    // SAFETY: a call of `socket` with constant arguments.
    let raw = unsafe { libc::socket(libc::AF_INET, libc::SOCK_RAW, libc::IPPROTO_TCP) };
    if raw < 0 {
        return Err(refusal("raw socket", io::Error::last_os_error()));
    }
    // SAFETY: `raw` is a fresh, owned descriptor.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    #[cfg(not(target_os = "linux"))]
    {
        // SAFETY: `fd` is valid and `FD_CLOEXEC` is a constant flag.
        if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(refusal("raw socket cloexec", io::Error::last_os_error()));
        }
    }
    let one: libc::c_int = 1;
    // SAFETY: `fd` is valid and `one` is a `c_int` of its size.
    let r = unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            libc::IPPROTO_IP,
            libc::IP_HDRINCL,
            (&raw const one).cast(),
            std::mem::size_of::<libc::c_int>() as u32,
        )
    };
    if r < 0 {
        return Err(refusal("raw socket option", io::Error::last_os_error()));
    }
    let wait = libc::timeval { tv_sec: 0, tv_usec: WAIT_US };
    // SAFETY: `fd` is valid and `wait` is a `timeval` of its size.
    let r = unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            (&raw const wait).cast(),
            std::mem::size_of::<libc::timeval>() as u32,
        )
    };
    if r < 0 {
        return Err(refusal("raw socket timeout", io::Error::last_os_error()));
    }
    Ok(fd)
}

fn local_address(target: SocketAddrV4) -> Result<Ipv4Addr, Refusal> {
    let probe = UdpSocket::bind("0.0.0.0:0").map_err(|e| refusal("udp local address", e))?;
    probe.connect(target).map_err(|e| refusal("udp local address", e))?;
    let address = probe.local_addr().map_err(|e| refusal("udp local address", e))?;
    match address.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_unspecified() => Ok(ip),
        _ => Err(Refusal::new("raw socket unavailable: this host has no IPv4 address to the target")),
    }
}

fn refusal(context: &str, error: io::Error) -> Refusal {
    let code = error.raw_os_error().map(|n| n.to_string()).unwrap_or_else(|| "none".to_string());
    Refusal::new(format!("{context} unavailable: {error} (errno {code})"))
}

fn sockaddr(target: SocketAddrV4) -> libc::sockaddr_in {
    let mut address: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    {
        address.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
    }
    address.sin_family = libc::AF_INET as libc::sa_family_t;
    address.sin_port = target.port().to_be();
    address.sin_addr.s_addr = u32::from(*target.ip()).to_be();
    address
}

/// The one's-complement checksum of an IPv4 header.
fn internet_checksum(header: &[u8]) -> u16 {
    let mut total = 0u32;
    for pair in header.chunks(2) {
        let word = if pair.len() == 2 { u16::from_be_bytes([pair[0], pair[1]]) } else { u16::from(pair[0]) << 8 };
        total += u32::from(word);
    }
    while total >> 16 != 0 {
        total = (total & 0xffff) + (total >> 16);
    }
    !(total as u16)
}
