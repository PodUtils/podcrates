//! NAT-PMP (RFC 6886) wire codec. Uses its own protocol enum (`PmpProto`), distinct from
//! PCP's IANA protocol numbers.

use std::net::{IpAddr, Ipv4Addr};

/// Default NAT-PMP / PCP UDP port.
pub const PMP_PORT: u16 = 5351;
/// NAT-PMP version byte.
pub const PMP_VERSION: u8 = 0;
/// External-address opcode.
pub const OP_EXTERNAL_ADDR: u8 = 0;
/// Map UDP opcode.
pub const OP_MAP_UDP: u8 = 1;
/// Map TCP opcode.
pub const OP_MAP_TCP: u8 = 2;
/// Response bit set on reply opcodes (`opcode | 0x80`).
pub const RESPONSE_BIT: u8 = 0x80;

/// NAT-PMP transport protocol opcode (RFC 6886 section 3.3: UDP = 1, TCP = 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PmpProto {
    Udp,
    Tcp,
}

impl PmpProto {
    /// Wire opcode byte (`1` for UDP, `2` for TCP).
    pub fn opcode(self) -> u8 {
        match self {
            Self::Udp => OP_MAP_UDP,
            Self::Tcp => OP_MAP_TCP,
        }
    }
}

/// NAT-PMP result codes (RFC 6886 section 3.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PmpResult {
    Success,
    UnsupportedVersion,
    NotAuthorized,
    NetworkFailure,
    OutOfResources,
    UnsupportedOpcode,
    Other(u16),
}

impl PmpResult {
    pub fn from_code(code: u16) -> Self {
        match code {
            0 => Self::Success,
            1 => Self::UnsupportedVersion,
            2 => Self::NotAuthorized,
            3 => Self::NetworkFailure,
            4 => Self::OutOfResources,
            5 => Self::UnsupportedOpcode,
            other => Self::Other(other),
        }
    }

    pub fn code(self) -> u16 {
        match self {
            Self::Success => 0,
            Self::UnsupportedVersion => 1,
            Self::NotAuthorized => 2,
            Self::NetworkFailure => 3,
            Self::OutOfResources => 4,
            Self::UnsupportedOpcode => 5,
            Self::Other(c) => c,
        }
    }
}

/// Decoded NAT-PMP reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PmpReply {
    /// 12-byte response to an external-address request (op 0).
    ExternalAddr { result: PmpResult, epoch: u32, external_ip: Option<Ipv4Addr> },
    /// 8-byte unsupported-version response (RFC 6886 section 3.5), typically sent by a
    /// NAT-PMP-only gateway in answer to a PCP version-2 packet.
    UnsupportedVersion { opcode: u8, epoch: u32 },
    /// 16-byte response to a UDP/TCP mapping request (op 1 or 2).
    Mapping {
        proto: PmpProto,
        result: PmpResult,
        epoch: u32,
        internal_port: u16,
        external_port: u16,
        lifetime_secs: u32,
    },
}

/// Build the 2-byte read-only external-address request (`00 00`, RFC 6886 section 3.2).
pub fn external_addr_request() -> [u8; 2] {
    [PMP_VERSION, OP_EXTERNAL_ADDR]
}

/// Build a 12-byte mapping request (never sent in probe-only mode).
pub fn map_request(proto: PmpProto, internal_port: u16, external_port: u16, lifetime_secs: u32) -> [u8; 12] {
    let mut out = [0u8; 12];
    out[0] = PMP_VERSION;
    out[1] = proto.opcode();
    out[4..6].copy_from_slice(&internal_port.to_be_bytes());
    out[6..8].copy_from_slice(&external_port.to_be_bytes());
    out[8..12].copy_from_slice(&lifetime_secs.to_be_bytes());
    out
}

/// Parse a NAT-PMP reply after verifying that `src_ip == gateway_ip` (RFC 6886 section 3.2).
/// Accepts exact lengths 8 (unsupported-version), 12 (external address), and 16 (mapping).
pub fn parse_reply(bytes: &[u8], src_ip: IpAddr, gateway_ip: IpAddr) -> Option<PmpReply> {
    if src_ip != gateway_ip || bytes.len() < 8 || bytes[0] != PMP_VERSION {
        return None;
    }
    let op = bytes[1];
    let result = PmpResult::from_code(u16::from_be_bytes([bytes[2], bytes[3]]));
    let epoch = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);

    if bytes.len() == 8 && result == PmpResult::UnsupportedVersion {
        return Some(PmpReply::UnsupportedVersion { opcode: op, epoch });
    }
    if bytes.len() == 12 && op == (RESPONSE_BIT | OP_EXTERNAL_ADDR) {
        let ip = Ipv4Addr::new(bytes[8], bytes[9], bytes[10], bytes[11]);
        // 0.0.0.0 means "WAN pending" and non-zero result leaves the address field undefined.
        let external_ip = if result == PmpResult::Success && !ip.is_unspecified() { Some(ip) } else { None };
        return Some(PmpReply::ExternalAddr { result, epoch, external_ip });
    }
    if bytes.len() == 16 && (op == (RESPONSE_BIT | OP_MAP_UDP) || op == (RESPONSE_BIT | OP_MAP_TCP)) {
        let proto = if op == (RESPONSE_BIT | OP_MAP_UDP) { PmpProto::Udp } else { PmpProto::Tcp };
        let internal_port = u16::from_be_bytes([bytes[8], bytes[9]]);
        let external_port = u16::from_be_bytes([bytes[10], bytes[11]]);
        let lifetime_secs = u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        return Some(PmpReply::Mapping { proto, result, epoch, internal_port, external_port, lifetime_secs });
    }
    None
}
