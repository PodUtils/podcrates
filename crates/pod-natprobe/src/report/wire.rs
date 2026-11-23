//! Compact 20-byte versioned binary wire profile for exchanging NAT capability summaries
//! between peers over a signalling or relay channel.

use std::fmt;

use super::Report;
use super::axes::{
    Connectivity, FilteringBehavior, HairpinBehavior, IpPooling, LifetimeBound, MappingBehavior, PortAllocation,
};
use super::obs::Obs;
use super::status::{Ipv6Status, PortMappingSupport, UdpPathStatus};

/// Wire version byte for [`CompactProfile`].
pub const WIRE_VERSION: u8 = 1;
/// Exact encoded length of [`CompactProfile`].
pub const WIRE_LEN: usize = 20;

/// Compact peer-exchange summary of a [`Report`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompactProfile {
    pub version: u8,
    pub udp_code: u8,
    pub mapping_code: u8,
    pub filtering_code: u8,
    pub alloc_code: u8,
    pub delta: i16,
    pub jitter: u16,
    pub lifetime_secs: u16,
    pub lifetime_bound_code: u8,
    pub pooling_code: u8,
    pub hairpin_code: u8,
    pub ipv6_code: u8,
    pub portmap_bits: u8,
    pub connectivity_code: u8,
    pub observers: u8,
    pub reserved: [u8; 2],
}

/// Error returned when decoding or validating a [`CompactProfile`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireError {
    BadLength(usize),
    UnsupportedVersion(u8),
    NonZeroDeltaWithoutSequential(i16),
    BadEnumCode(&'static str, u8),
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadLength(n) => write!(f, "compact profile length {n} is not {WIRE_LEN}"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported compact profile version {v}"),
            Self::NonZeroDeltaWithoutSequential(d) => {
                write!(f, "non-zero port delta {d} requires sequential or counter allocation")
            }
            Self::BadEnumCode(field, code) => write!(f, "invalid compact profile code {code} for {field}"),
        }
    }
}

impl std::error::Error for WireError {}

impl CompactProfile {
    /// Project a full [`Report`] into its compact wire summary.
    pub fn from_report(r: &Report) -> Self {
        let v4 = r.v4.as_ref();
        let udp_code = match v4.and_then(|f| f.udp_path.value()) {
            None => 0,
            Some(UdpPathStatus::Open) => 1,
            Some(UdpPathStatus::PortFiltered { .. }) => 2,
            Some(UdpPathStatus::OnlyPort { .. }) => 3,
            Some(UdpPathStatus::Blocked) => 4,
            Some(UdpPathStatus::Limited(_)) => 5,
        };
        let connectivity_code = match v4.and_then(|f| f.connectivity.value()) {
            None => 0,
            Some(Connectivity::DirectPublic) => 1,
            Some(Connectivity::BehindNat) => 2,
            Some(Connectivity::BehindCgnat) => 3,
            Some(Connectivity::DoubleNat) => 4,
            Some(Connectivity::UdpBlocked) => 5,
        };
        let mapping_code = match v4.and_then(|f| f.mapping.value()) {
            None => 0,
            Some(MappingBehavior::EndpointIndependent) => 1,
            Some(MappingBehavior::AddressDependent) => 2,
            Some(MappingBehavior::AddressAndPortDependent) => 3,
        };
        let filtering_code = match v4.and_then(|f| f.filtering.value()) {
            None => 0,
            Some(FilteringBehavior::EndpointIndependent) => 1,
            Some(FilteringBehavior::AddressDependent) => 2,
            Some(FilteringBehavior::AddressAndPortDependent) => 3,
            Some(FilteringBehavior::NoReturnTraffic) => 4,
        };
        let (alloc_code, delta, jitter) = match v4.and_then(|f| f.allocation.value()) {
            None => (0, 0, 0),
            Some(PortAllocation::Preserving) => (1, 0, 0),
            Some(PortAllocation::Sequential { delta, jitter, .. }) => (2, *delta, *jitter),
            Some(PortAllocation::GlobalCounter { drift_per_s, jitter }) => (3, *drift_per_s, *jitter),
            Some(PortAllocation::Random { .. }) => (4, 0, 0),
        };
        let (lifetime_secs, lifetime_bound_code) = match v4.and_then(|f| f.lifetime.value()) {
            None => (0, 0),
            Some(l) => {
                let s = l.estimate.as_secs().min(u64::from(u16::MAX)) as u16;
                let bc = match l.bound {
                    LifetimeBound::Exact { .. } => 1,
                    LifetimeBound::AtLeast(_) => 2,
                    LifetimeBound::Below(_) => 3,
                };
                (s, bc)
            }
        };
        let pooling_code = match v4.and_then(|f| f.pooling.value()) {
            None => 0,
            Some(IpPooling::Paired) => 1,
            Some(IpPooling::Arbitrary) => 2,
            Some(IpPooling::PerDestinationEgress) => 3,
        };
        let hairpin_code = match v4.and_then(|f| f.hairpin.value()) {
            None => 0,
            Some(HairpinBehavior::None) => 1,
            Some(HairpinBehavior::ExternalSource) => 2,
            Some(HairpinBehavior::InternalSource) => 3,
            Some(HairpinBehavior::ExternalIpOnly) => 4,
            Some(HairpinBehavior::OtherSource) => 5,
        };
        let ipv6_code = match r.ipv6.value() {
            None => 0,
            Some(Ipv6Status::NoStack) => 1,
            Some(Ipv6Status::NoRoute) => 2,
            Some(Ipv6Status::LinkLocalOnly) => 3,
            Some(Ipv6Status::Filtered) => 4,
            Some(Ipv6Status::Ok { nat66: Some(true), .. }) => 6,
            Some(Ipv6Status::Ok { .. }) => 5,
        };
        let portmap_bits = encode_portmap_bits(&r.portmap);
        let observers = r.servers.len().min(usize::from(u8::MAX)) as u8;
        Self {
            version: WIRE_VERSION,
            udp_code,
            mapping_code,
            filtering_code,
            alloc_code,
            delta,
            jitter,
            lifetime_secs,
            lifetime_bound_code,
            pooling_code,
            hairpin_code,
            ipv6_code,
            portmap_bits,
            connectivity_code,
            observers,
            reserved: [0, 0],
        }
    }

    /// Encode into 20 big-endian bytes.
    pub fn encode(&self) -> [u8; WIRE_LEN] {
        let mut out = [0u8; WIRE_LEN];
        out[0] = self.version;
        out[1] = self.udp_code;
        out[2] = self.mapping_code;
        out[3] = self.filtering_code;
        out[4] = self.alloc_code;
        out[5..7].copy_from_slice(&self.delta.to_be_bytes());
        out[7..9].copy_from_slice(&self.jitter.to_be_bytes());
        out[9..11].copy_from_slice(&self.lifetime_secs.to_be_bytes());
        out[11] = self.lifetime_bound_code;
        out[12] = self.pooling_code;
        out[13] = self.hairpin_code;
        out[14] = self.ipv6_code;
        out[15] = self.portmap_bits;
        out[16] = self.connectivity_code;
        out[17] = self.observers;
        out[18..20].copy_from_slice(&self.reserved);
        out
    }

    /// Decode and validate a 20-byte compact profile.
    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        if bytes.len() != WIRE_LEN {
            return Err(WireError::BadLength(bytes.len()));
        }
        let p = Self {
            version: bytes[0],
            udp_code: bytes[1],
            mapping_code: bytes[2],
            filtering_code: bytes[3],
            alloc_code: bytes[4],
            delta: i16::from_be_bytes([bytes[5], bytes[6]]),
            jitter: u16::from_be_bytes([bytes[7], bytes[8]]),
            lifetime_secs: u16::from_be_bytes([bytes[9], bytes[10]]),
            lifetime_bound_code: bytes[11],
            pooling_code: bytes[12],
            hairpin_code: bytes[13],
            ipv6_code: bytes[14],
            portmap_bits: bytes[15],
            connectivity_code: bytes[16],
            observers: bytes[17],
            reserved: [bytes[18], bytes[19]],
        };
        p.validate()?;
        Ok(p)
    }

    /// Check all structural invariants of the decoded profile.
    pub fn validate(&self) -> Result<(), WireError> {
        if self.version != WIRE_VERSION {
            return Err(WireError::UnsupportedVersion(self.version));
        }
        if self.udp_code > 5 {
            return Err(WireError::BadEnumCode("udp", self.udp_code));
        }
        if self.mapping_code > 3 {
            return Err(WireError::BadEnumCode("mapping", self.mapping_code));
        }
        if self.filtering_code > 4 {
            return Err(WireError::BadEnumCode("filtering", self.filtering_code));
        }
        if self.alloc_code > 4 {
            return Err(WireError::BadEnumCode("allocation", self.alloc_code));
        }
        if !matches!(self.alloc_code, 2 | 3) && self.delta != 0 {
            return Err(WireError::NonZeroDeltaWithoutSequential(self.delta));
        }
        if self.lifetime_bound_code > 3 {
            return Err(WireError::BadEnumCode("lifetime_bound", self.lifetime_bound_code));
        }
        if self.pooling_code > 3 {
            return Err(WireError::BadEnumCode("pooling", self.pooling_code));
        }
        if self.hairpin_code > 5 {
            return Err(WireError::BadEnumCode("hairpin", self.hairpin_code));
        }
        if self.ipv6_code > 6 {
            return Err(WireError::BadEnumCode("ipv6", self.ipv6_code));
        }
        if self.connectivity_code > 5 {
            return Err(WireError::BadEnumCode("connectivity", self.connectivity_code));
        }
        Ok(())
    }
}

fn encode_portmap_bits(pm: &PortMappingSupport) -> u8 {
    let mut bits = 0u8;
    if matches!(pm.nat_pmp, Obs::Observed { .. }) {
        bits |= 1 << 0;
    }
    if matches!(pm.pcp, Obs::Observed { .. }) {
        bits |= 1 << 1;
    }
    if matches!(pm.upnp, Obs::Observed { .. }) {
        bits |= 1 << 2;
    }
    bits
}
