//! Output-only projections to legacy and vendor taxonomies. These are pure views
//! derived from canonical fields and are never fed back into classifiers.

use std::net::SocketAddr;

use super::axes::{Connectivity, FilteringBehavior, IpPooling, MappingBehavior, PortAllocation};
use super::obs::{Constraint, Obs};

/// Reduced canonical axes for legacy projections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Axes {
    pub connectivity: Connectivity,
    pub mapping: Option<MappingBehavior>,
    /// True when mapping is `Unknown` with `Constraint::MappingNotEndpointIndependent`.
    pub mapping_not_ei: bool,
    pub filtering: Option<FilteringBehavior>,
    pub allocation: Option<PortAllocation>,
    /// True when an active gateway port mapping exists for the tested port.
    pub portmap_active: bool,
}

/// Classic RFC 3489 / pjproject labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum Rfc3489 {
    OpenInternet,
    FullCone,
    RestrictedCone,
    PortRestrictedCone,
    Symmetric,
    SymmetricUdpFirewall,
    Blocked,
    Unknown,
}

fn dependent(a: &Axes) -> bool {
    matches!(a.mapping, Some(MappingBehavior::AddressDependent | MappingBehavior::AddressAndPortDependent))
        || (a.mapping.is_none() && a.mapping_not_ei)
}

/// Project canonical axes to RFC 3489 labels (preserving RFC 3489's own conflations).
pub fn rfc3489(a: &Axes) -> Rfc3489 {
    use FilteringBehavior::{AddressAndPortDependent, AddressDependent, EndpointIndependent};
    match a.connectivity {
        Connectivity::UdpBlocked => return Rfc3489::Blocked,
        Connectivity::DirectPublic => {
            return match a.filtering {
                Some(EndpointIndependent) => Rfc3489::OpenInternet,
                Some(AddressDependent | AddressAndPortDependent) => Rfc3489::SymmetricUdpFirewall,
                _ => Rfc3489::Unknown,
            };
        }
        _ => {}
    }
    match a.filtering {
        Some(EndpointIndependent) => Rfc3489::FullCone,
        Some(AddressDependent | AddressAndPortDependent) => {
            if a.mapping.is_none() && !a.mapping_not_ei {
                Rfc3489::Unknown
            } else if dependent(a) {
                Rfc3489::Symmetric
            } else if a.filtering == Some(AddressDependent) {
                Rfc3489::RestrictedCone
            } else {
                Rfc3489::PortRestrictedCone
            }
        }
        _ => Rfc3489::Unknown,
    }
}

/// Tailscale `MappingVariesByDestIP` projection plus a divergence indicator when
/// `(EndpointIndependent, Arbitrary)` pooling makes Tailscale's full-endpoint check disagree.
pub fn tailscale_varies_by_dest_ip(mapping: &Obs<MappingBehavior>, pooling: &Obs<IpPooling>) -> (Option<bool>, bool) {
    let base = match mapping {
        Obs::Observed { value: MappingBehavior::EndpointIndependent, .. } => Some(false),
        Obs::Observed { .. } => Some(true),
        Obs::Unknown(u) if u.constraint == Constraint::MappingNotEndpointIndependent => Some(true),
        _ => None,
    };
    let arbitrary = matches!(pooling, Obs::Observed { value: IpPooling::Arbitrary, .. });
    let diverges = base == Some(false) && arbitrary;
    (base, diverges)
}

/// Tailscale's raw full-endpoint check: do mapped `(ip, port)` endpoints differ across destinations?
pub fn endpoint_varies_by_dest(mapped_per_dest: &[SocketAddr]) -> Option<bool> {
    let first = mapped_per_dest.first()?;
    if mapped_per_dest.len() < 2 {
        return None;
    }
    Some(mapped_per_dest.iter().any(|m| m != first))
}

/// EasyTier 9-state NAT taxonomy projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum EasyTier {
    Unknown,
    OpenInternet,
    NoPat,
    FullCone,
    Restricted,
    PortRestricted,
    Symmetric,
    SymUdpFirewall,
    SymmetricEasyInc,
    SymmetricEasyDec,
}

/// Project canonical axes to EasyTier's taxonomy.
pub fn easytier(a: &Axes) -> EasyTier {
    use FilteringBehavior::{AddressAndPortDependent, AddressDependent, EndpointIndependent};
    match a.connectivity {
        Connectivity::UdpBlocked => return EasyTier::Unknown,
        Connectivity::DirectPublic => {
            return match a.filtering {
                Some(EndpointIndependent) => EasyTier::OpenInternet,
                Some(AddressDependent | AddressAndPortDependent) => EasyTier::SymUdpFirewall,
                _ => EasyTier::Unknown,
            };
        }
        _ => {}
    }
    if dependent(a) {
        return match a.allocation {
            Some(PortAllocation::Sequential { delta, .. }) if delta > 0 && delta < 100 => EasyTier::SymmetricEasyInc,
            Some(PortAllocation::Sequential { delta, .. }) if delta < 0 && delta > -100 => EasyTier::SymmetricEasyDec,
            _ => EasyTier::Symmetric,
        };
    }
    match (a.mapping, a.filtering) {
        (Some(MappingBehavior::EndpointIndependent), Some(EndpointIndependent)) => {
            if matches!(a.allocation, Some(PortAllocation::Preserving)) { EasyTier::NoPat } else { EasyTier::FullCone }
        }
        (Some(MappingBehavior::EndpointIndependent), Some(AddressDependent)) => EasyTier::Restricted,
        (Some(MappingBehavior::EndpointIndependent), Some(AddressAndPortDependent)) => EasyTier::PortRestricted,
        _ => EasyTier::Unknown,
    }
}

/// Console / gaming NAT type labels (community-inferred projections).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum Gaming {
    Unmapped,
    NintendoA,
    NintendoB,
    NintendoC,
    NintendoD,
    NintendoF,
    XboxOpen,
    XboxModerate,
    XboxStrict,
    XboxUnavailable,
    Psn1,
    Psn2,
    Psn3,
}

/// Project canonical axes to Nintendo Switch NAT types (A/B/C/D/F).
pub fn nintendo(a: &Axes) -> Gaming {
    use FilteringBehavior::{AddressAndPortDependent, AddressDependent, EndpointIndependent};
    match a.connectivity {
        Connectivity::UdpBlocked => return Gaming::NintendoF,
        Connectivity::BehindCgnat | Connectivity::DoubleNat => return Gaming::Unmapped,
        Connectivity::DirectPublic => {
            return if a.filtering == Some(EndpointIndependent) { Gaming::NintendoA } else { Gaming::Unmapped };
        }
        Connectivity::BehindNat => {}
    }
    if dependent(a) {
        return Gaming::NintendoD;
    }
    match (a.mapping, a.filtering) {
        (Some(MappingBehavior::EndpointIndependent), Some(EndpointIndependent)) => Gaming::NintendoA,
        (Some(MappingBehavior::EndpointIndependent), Some(AddressDependent)) => Gaming::NintendoB,
        (Some(MappingBehavior::EndpointIndependent), Some(AddressAndPortDependent)) => Gaming::NintendoC,
        _ => Gaming::Unmapped,
    }
}

/// Project canonical axes to Xbox NAT labels (Open / Moderate / Strict / Unavailable).
pub fn xbox(a: &Axes) -> Gaming {
    use FilteringBehavior::{AddressAndPortDependent, AddressDependent, EndpointIndependent};
    match a.connectivity {
        Connectivity::UdpBlocked => return Gaming::XboxUnavailable,
        Connectivity::BehindCgnat | Connectivity::DoubleNat => return Gaming::Unmapped,
        Connectivity::DirectPublic => {
            return if a.filtering == Some(EndpointIndependent) { Gaming::XboxOpen } else { Gaming::Unmapped };
        }
        Connectivity::BehindNat => {}
    }
    if dependent(a) {
        return Gaming::XboxStrict;
    }
    match (a.mapping, a.filtering) {
        (Some(MappingBehavior::EndpointIndependent), Some(EndpointIndependent)) => Gaming::XboxOpen,
        (Some(MappingBehavior::EndpointIndependent), Some(AddressDependent | AddressAndPortDependent)) => {
            Gaming::XboxModerate
        }
        _ => Gaming::Unmapped,
    }
}

/// Project canonical axes to PlayStation NAT types (1 / 2 / 3).
pub fn psn(a: &Axes) -> Gaming {
    use FilteringBehavior::{AddressAndPortDependent, AddressDependent, EndpointIndependent};
    match a.connectivity {
        Connectivity::UdpBlocked | Connectivity::BehindCgnat | Connectivity::DoubleNat => return Gaming::Unmapped,
        Connectivity::DirectPublic => {
            return if a.filtering == Some(EndpointIndependent) { Gaming::Psn1 } else { Gaming::Unmapped };
        }
        Connectivity::BehindNat => {}
    }
    if dependent(a) {
        return Gaming::Psn3;
    }
    match (a.mapping, a.filtering) {
        (Some(MappingBehavior::EndpointIndependent), Some(EndpointIndependent | AddressDependent)) => Gaming::Psn2,
        (Some(MappingBehavior::EndpointIndependent), Some(AddressAndPortDependent)) => {
            if a.portmap_active {
                Gaming::Psn2
            } else {
                Gaming::Psn3
            }
        }
        _ => Gaming::Unmapped,
    }
}
