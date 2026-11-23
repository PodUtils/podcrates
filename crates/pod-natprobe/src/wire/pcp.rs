//! PCP (RFC 6887) wire codec. Uses IANA protocol numbers (`PcpProto`), distinct from NAT-PMP.

use std::net::{IpAddr, Ipv6Addr};

/// PCP version byte (RFC 6887 section 7.1).
pub const PCP_VERSION: u8 = 2;
/// ANNOUNCE opcode (0).
pub const OP_ANNOUNCE: u8 = 0;
/// MAP opcode (1).
pub const OP_MAP: u8 = 1;
/// Minimum PCP packet length (common header).
pub const HEADER_LEN: usize = 24;
/// Maximum PCP UDP payload (RFC 6887 section 7).
pub const MAX_LEN: usize = 1100;

/// PCP transport protocol (IANA protocol numbers: UDP = 17, TCP = 6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcpProto {
    Udp,
    Tcp,
}

impl PcpProto {
    /// IANA protocol byte (`17` for UDP, `6` for TCP).
    pub fn iana(self) -> u8 {
        match self {
            Self::Udp => 17,
            Self::Tcp => 6,
        }
    }
}

/// PCP result codes (RFC 6887 section 7.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcpResult {
    Success,
    UnsuppVersion,
    NotAuthorized,
    MalformedRequest,
    UnsuppOpcode,
    UnsuppOption,
    MalformedOption,
    NetworkFailure,
    NoResources,
    UnsuppProtocol,
    UserExQuota,
    CannotProvideExternal,
    AddressMismatch,
    ExcessiveRemotePeers,
    Other(u8),
}

impl PcpResult {
    pub fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Success,
            1 => Self::UnsuppVersion,
            2 => Self::NotAuthorized,
            3 => Self::MalformedRequest,
            4 => Self::UnsuppOpcode,
            5 => Self::UnsuppOption,
            6 => Self::MalformedOption,
            7 => Self::NetworkFailure,
            8 => Self::NoResources,
            9 => Self::UnsuppProtocol,
            10 => Self::UserExQuota,
            11 => Self::CannotProvideExternal,
            12 => Self::AddressMismatch,
            13 => Self::ExcessiveRemotePeers,
            other => Self::Other(other),
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::UnsuppVersion => 1,
            Self::NotAuthorized => 2,
            Self::MalformedRequest => 3,
            Self::UnsuppOpcode => 4,
            Self::UnsuppOption => 5,
            Self::MalformedOption => 6,
            Self::NetworkFailure => 7,
            Self::NoResources => 8,
            Self::UnsuppProtocol => 9,
            Self::UserExQuota => 10,
            Self::CannotProvideExternal => 11,
            Self::AddressMismatch => 12,
            Self::ExcessiveRemotePeers => 13,
            Self::Other(c) => c,
        }
    }
}

/// Decoded PCP response header and optional MAP payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcpReply {
    pub opcode: u8,
    pub result: PcpResult,
    pub lifetime_secs: u32,
    pub epoch: u32,
    pub mapped: Option<PcpMapPayload>,
}

/// Decoded 36-byte PCP MAP response payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcpMapPayload {
    pub nonce: [u8; 12],
    pub protocol: u8,
    pub internal_port: u16,
    pub external_port: u16,
    pub external_ip: IpAddr,
}

fn to_pcp_addr(ip: IpAddr) -> [u8; 16] {
    match ip {
        IpAddr::V4(v4) => v4.to_ipv6_mapped().octets(),
        IpAddr::V6(v6) => v6.octets(),
    }
}

fn from_pcp_addr(bytes: [u8; 16]) -> IpAddr {
    let v6 = Ipv6Addr::from(bytes);
    v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4)
}

/// Build a read-only 24-byte PCP ANNOUNCE request (opcode 0, lifetime 0).
pub fn announce_request(client_ip: IpAddr) -> [u8; HEADER_LEN] {
    let mut out = [0u8; HEADER_LEN];
    out[0] = PCP_VERSION;
    out[1] = OP_ANNOUNCE;
    out[8..24].copy_from_slice(&to_pcp_addr(client_ip));
    out
}

/// Build a 60-byte PCP MAP request (only sent when `allow_portmap_side_effects` is true).
pub fn map_request(
    client_ip: IpAddr,
    nonce: [u8; 12],
    proto: PcpProto,
    internal_port: u16,
    suggested_port: u16,
    suggested_ip: IpAddr,
    lifetime_secs: u32,
) -> [u8; 60] {
    let mut out = [0u8; 60];
    out[0] = PCP_VERSION;
    out[1] = OP_MAP;
    out[4..8].copy_from_slice(&lifetime_secs.to_be_bytes());
    out[8..24].copy_from_slice(&to_pcp_addr(client_ip));
    out[24..36].copy_from_slice(&nonce);
    out[36] = proto.iana();
    out[40..42].copy_from_slice(&internal_port.to_be_bytes());
    out[42..44].copy_from_slice(&suggested_port.to_be_bytes());
    out[44..60].copy_from_slice(&to_pcp_addr(suggested_ip));
    out
}

/// Parse and validate a PCP response per RFC 6887 section 8.3:
/// source IP must match gateway, length in 24..=1100 and multiple of 4, version == 2, R bit set.
pub fn parse_reply(bytes: &[u8], src_ip: IpAddr, gateway_ip: IpAddr) -> Option<PcpReply> {
    if src_ip != gateway_ip {
        return None;
    }
    if bytes.len() < HEADER_LEN || bytes.len() > MAX_LEN || !bytes.len().is_multiple_of(4) {
        return None;
    }
    if bytes[0] != PCP_VERSION || (bytes[1] & 0x80) == 0 {
        return None;
    }
    let opcode = bytes[1] & 0x7f;
    let result = PcpResult::from_code(bytes[3]);
    let lifetime_secs = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let epoch = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    let mapped = if opcode == OP_MAP && bytes.len() >= 60 {
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&bytes[24..36]);
        let mut addr = [0u8; 16];
        addr.copy_from_slice(&bytes[44..60]);
        Some(PcpMapPayload {
            nonce,
            protocol: bytes[36],
            internal_port: u16::from_be_bytes([bytes[40], bytes[41]]),
            external_port: u16::from_be_bytes([bytes[42], bytes[43]]),
            external_ip: from_pcp_addr(addr),
        })
    } else {
        None
    };
    Some(PcpReply { opcode, result, lifetime_secs, epoch, mapped })
}
