//! Topology-gated STUN request builder over `pod_turn::stun`.
//!
//! A `CHANGE-REQUEST` packet can only be built when the caller holds a validated [`Topology`]
//! pinned from a Test I response (Q3 / INV-03 / INV-10).

use std::net::SocketAddr;

use pod_turn::stun::{CHANGE_IP, CHANGE_PORT, NatBuildError, NatRequestBuilder, TransactionId, binding_request};

use super::addr::{AddrPolicy, canonical_addr, classify_ip};

/// Validated RFC 5780 server 4-socket square pinned from Test I:
/// `pp = (A1, P1)`, `pa = (A1, P2)`, `ap = (A2, P1)`, `aa = (A2, P2)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Topology {
    pub pp: SocketAddr,
    pub pa: SocketAddr,
    pub ap: SocketAddr,
    pub aa: SocketAddr,
}

impl Topology {
    /// Pin a [`Topology`] from the primary address `pp` and a Test I `OTHER-ADDRESS` (`other`).
    /// Returns `None` when `other` is not usable ([`usable_other`]).
    pub fn pin(pp: SocketAddr, other: SocketAddr, policy: AddrPolicy) -> Option<Self> {
        let pp = canonical_addr(pp);
        let other = canonical_addr(other);
        if !usable_other(pp, other, policy) {
            return None;
        }
        Some(Self {
            pp,
            pa: SocketAddr::new(pp.ip(), other.port()),
            ap: SocketAddr::new(other.ip(), pp.port()),
            aa: other,
        })
    }

    /// Expected UDP reply source for a request sent to `sent_to` with `flags`.
    /// This is the single place where `CHANGE_IP` and `CHANGE_PORT` bits are interpreted.
    pub fn expected_src(&self, sent_to: SocketAddr, flags: u8) -> SocketAddr {
        let sent_to = canonical_addr(sent_to);
        let alt_ip = if sent_to.ip() == self.pp.ip() { self.aa.ip() } else { self.pp.ip() };
        let alt_port = if sent_to.port() == self.pp.port() { self.aa.port() } else { self.pp.port() };
        let ip = if (flags & CHANGE_IP) != 0 { alt_ip } else { sent_to.ip() };
        let port = if (flags & CHANGE_PORT) != 0 { alt_port } else { sent_to.port() };
        SocketAddr::new(ip, port)
    }
}

/// True when `other` is an alternate address of the server: its IP and its port differ from
/// those of `pp`, it is not unspecified, multicast, broadcast or loopback (unless `pp` is
/// loopback), and its class (`classify_ip`) is the class of `pp`. The server gives `other`: a
/// public server must not send the requests of the probe into the LAN.
pub fn usable_other(pp: SocketAddr, other: SocketAddr, policy: AddrPolicy) -> bool {
    let pp = canonical_addr(pp);
    let other = canonical_addr(other);
    if pp.is_ipv4() != other.is_ipv4() {
        return false;
    }
    if other.ip().is_unspecified() || other.ip().is_multicast() || other.port() == 0 {
        return false;
    }
    if let std::net::IpAddr::V4(v4) = other.ip()
        && v4.is_broadcast()
    {
        return false;
    }
    if other.ip().is_loopback() && !pp.ip().is_loopback() {
        return false;
    }
    other.ip() != pp.ip()
        && other.port() != pp.port()
        && classify_ip(other.ip(), policy) == classify_ip(pp.ip(), policy)
}

/// Build a plain STUN Binding request (no CHANGE-REQUEST, RESPONSE-PORT or PADDING).
pub fn plain_binding(id: TransactionId, software: Option<&str>) -> Result<Vec<u8>, NatBuildError> {
    match software {
        None => Ok(binding_request(&id).to_vec()),
        Some(sw) => NatRequestBuilder::binding(id).software(sw).build(),
    }
}

/// Build a STUN Binding request with `CHANGE-REQUEST`, returning the encoded bytes and the
/// exact expected UDP source address derived from `topology`.
pub fn change_request(
    id: TransactionId,
    topology: &Topology,
    sent_to: SocketAddr,
    flags: u8,
    software: Option<&str>,
) -> Result<(Vec<u8>, SocketAddr), NatBuildError> {
    let mut builder = NatRequestBuilder::binding(id).change_request(flags);
    if let Some(sw) = software {
        builder = builder.software(sw);
    }
    let bytes = builder.build()?;
    let expected = topology.expected_src(sent_to, flags);
    Ok((bytes, expected))
}

/// Build a STUN Binding request carrying `RESPONSE-PORT` (RFC 5780 section 7.5).
pub fn response_port_request(id: TransactionId, port: u16, software: Option<&str>) -> Result<Vec<u8>, NatBuildError> {
    let mut builder = NatRequestBuilder::binding(id).response_port(port);
    if let Some(sw) = software {
        builder = builder.software(sw);
    }
    builder.build()
}
