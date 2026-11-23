//! Per-family report with constructor-enforced invariants.

use std::fmt;
use std::net::IpAddr;

use super::axes::{
    AlgDetection, Connectivity, Family, FilteringBehavior, HairpinBehavior, IpPooling, MappingBehavior, NatLifetime,
    PortAllocation,
};
use super::obs::Obs;
use super::status::UdpPathStatus;

/// Probe depth and time budget profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum Profile {
    /// Bounded to 600 ms; reports positive observations only.
    Fast,
    /// Full RFC 5780 mapping, filtering with positive controls, allocation, pooling, hairpin, gateway.
    Standard,
    /// Adds opt-in mapping lifetime measurement and idle-gap counter verification.
    Deep,
}

/// Logical identifier for an isolated UDP socket owned by the probe engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub struct SockId(pub u16);

/// Why [`FamilyReport::new`] refused to construct an inconsistent report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReportError {
    /// A NAT field holds an observed value even though the UDP path is blocked.
    NatFieldOnBlockedPath(&'static str),
    /// `Connectivity::UdpBlocked` and `UdpPathStatus::Blocked` disagree.
    BlockedMismatch,
    /// A NAT-only field holds an observed value on a directly connected public host.
    FieldOnDirectPublic(&'static str),
    /// Mapping and filtering probes used the same socket.
    SharedSocket,
}

impl fmt::Display for ReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NatFieldOnBlockedPath(field) => write!(f, "NAT field `{field}` observed on a blocked UDP path"),
            Self::BlockedMismatch => f.write_str("Connectivity::UdpBlocked disagrees with UdpPathStatus::Blocked"),
            Self::FieldOnDirectPublic(field) => write!(f, "field `{field}` observed on a DirectPublic host"),
            Self::SharedSocket => f.write_str("mapping and filtering probes shared one socket"),
        }
    }
}

impl std::error::Error for ReportError {}

/// Unvalidated fields for [`FamilyReport::new`].
#[derive(Clone, Debug)]
pub struct FamilyParts {
    pub family: Family,
    pub udp_path: Obs<UdpPathStatus>,
    pub connectivity: Obs<Connectivity>,
    pub mapping: Obs<MappingBehavior>,
    pub filtering: Obs<FilteringBehavior>,
    pub allocation: Obs<PortAllocation>,
    pub pooling: Obs<IpPooling>,
    pub hairpin: Obs<HairpinBehavior>,
    pub lifetime: Obs<NatLifetime>,
    pub alg: Obs<AlgDetection>,
    pub public_ips: Vec<IpAddr>,
    pub mapping_socket: SockId,
    pub filtering_socket: SockId,
}

/// Validated per-family NAT probe report.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub struct FamilyReport {
    family: Family,
    pub udp_path: Obs<UdpPathStatus>,
    pub connectivity: Obs<Connectivity>,
    pub mapping: Obs<MappingBehavior>,
    pub filtering: Obs<FilteringBehavior>,
    pub allocation: Obs<PortAllocation>,
    pub pooling: Obs<IpPooling>,
    pub hairpin: Obs<HairpinBehavior>,
    pub lifetime: Obs<NatLifetime>,
    pub alg: Obs<AlgDetection>,
    pub public_ips: Vec<IpAddr>,
    /// Socket used for mapping probes (isolated from `filtering_socket`).
    pub mapping_socket: SockId,
    /// Socket used for filtering probes (isolated from `mapping_socket`).
    pub filtering_socket: SockId,
}

fn observed<T>(o: &Obs<T>) -> bool {
    matches!(o, Obs::Observed { .. })
}

impl FamilyReport {
    /// Validate invariants and construct a [`FamilyReport`].
    pub fn new(p: FamilyParts) -> Result<Self, ReportError> {
        if p.mapping_socket == p.filtering_socket {
            return Err(ReportError::SharedSocket);
        }
        let blocked = matches!(&p.udp_path, Obs::Observed { value: UdpPathStatus::Blocked, .. });
        let conn_blocked = matches!(&p.connectivity, Obs::Observed { value: Connectivity::UdpBlocked, .. });
        if blocked != conn_blocked {
            return Err(ReportError::BlockedMismatch);
        }
        if blocked {
            let named: [(&'static str, bool); 7] = [
                ("mapping", observed(&p.mapping)),
                ("filtering", observed(&p.filtering)),
                ("allocation", observed(&p.allocation)),
                ("pooling", observed(&p.pooling)),
                ("hairpin", observed(&p.hairpin)),
                ("lifetime", observed(&p.lifetime)),
                ("alg", observed(&p.alg)),
            ];
            if let Some((name, _)) = named.iter().find(|(_, set)| *set) {
                return Err(ReportError::NatFieldOnBlockedPath(name));
            }
        }
        let direct = matches!(&p.connectivity, Obs::Observed { value: Connectivity::DirectPublic, .. });
        if direct {
            let named: [(&'static str, bool); 4] = [
                ("hairpin", observed(&p.hairpin)),
                ("allocation", observed(&p.allocation)),
                ("pooling", observed(&p.pooling)),
                ("lifetime", observed(&p.lifetime)),
            ];
            if let Some((name, _)) = named.iter().find(|(_, set)| *set) {
                return Err(ReportError::FieldOnDirectPublic(name));
            }
        }
        Ok(Self {
            family: p.family,
            udp_path: p.udp_path,
            connectivity: p.connectivity,
            mapping: p.mapping,
            filtering: p.filtering,
            allocation: p.allocation,
            pooling: p.pooling,
            hairpin: p.hairpin,
            lifetime: p.lifetime,
            alg: p.alg,
            public_ips: p.public_ips,
            mapping_socket: p.mapping_socket,
            filtering_socket: p.filtering_socket,
        })
    }

    /// Address family of this report.
    pub fn family(&self) -> Family {
        self.family
    }
}
