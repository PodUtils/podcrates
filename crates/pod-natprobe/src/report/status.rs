//! Auxiliary status types: gateway port mapping, IPv6, NAT64, UDP path, and NAT layers.

use std::net::{Ipv4Addr, Ipv6Addr};

use super::obs::Obs;

/// NAT-PMP (RFC 6886) gateway state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum PmpState {
    Present,
    /// Result code 2: implemented, switched off.
    PresentDisabled,
    /// Gateway answered a PCP packet with an 8-byte version-0 unsupported-version reply.
    SpeaksNatPmpOnly,
}

/// PCP (RFC 6887) gateway state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum PcpState {
    Present,
    PresentDisabled,
    /// Result code 12 (`ADDRESS_MISMATCH`): a NAT sits between host and PCP server.
    BehindNat,
}

/// UPnP IGD gateway state.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub struct UpnpState {
    pub igd_count: u8,
    pub control_url_seen: bool,
}

/// Protocol that reported a gateway external IPv4 address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum ExternalIpSource {
    NatPmp,
    Upnp,
    PcpMap,
}

/// Combined port-mapping protocol discovery result.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub struct PortMappingSupport {
    pub nat_pmp: Obs<PmpState>,
    pub pcp: Obs<PcpState>,
    pub upnp: Obs<UpnpState>,
    /// Reported gateway external IPv4 addresses (`0.0.0.0` is never stored).
    pub external_v4: Vec<(ExternalIpSource, Ipv4Addr)>,
}

/// Address class of the local IPv6 source selected for global destinations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum LocalV6Class {
    Gua,
    Ula,
    LinkLocal,
    Teredo,
    SixToFour,
    Nat64Synth,
}

/// IPv6 stack, route and UDP reachability status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum Ipv6Status {
    NoStack,
    /// UDP connect to a global IPv6 address failed with ENETUNREACH.
    NoRoute,
    LinkLocalOnly,
    /// Packets sent with silence while a positive IPv6 control succeeded elsewhere.
    Filtered,
    Ok {
        nat66: Option<bool>,
        local: LocalV6Class,
    },
}

/// Discovered NAT64 prefix (RFC 6052 / RFC 7050).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub struct Pref64 {
    pub prefix: Ipv6Addr,
    /// Prefix length: 32, 40, 48, 56, 64, or 96.
    pub len: u8,
}

/// NAT64 / DNS64 detection outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum Nat64Status {
    Pref64 {
        prefixes: Vec<Pref64>,
        route_to_wkp: Option<bool>,
    },
    /// AAAA answer lacked the 192.0.0.170/171 marker at every RFC 6052 offset.
    DnsHijackSuspect,
    /// Resolver did not synthesize an AAAA record.
    NoDns64,
}

/// Observed UDP datagram size or rate restriction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum LimitKind {
    Size { bytes_ok: u16 },
    Rate { per_s: u16 },
}

/// UDP transport reachability class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum UdpPathStatus {
    Open,
    PortFiltered {
        blocked_port: u16,
    },
    OnlyPort {
        port: u16,
    },
    /// Needs at least two distinct server IPs, the full ladder, and non-UDP corroboration.
    Blocked,
    Limited(LimitKind),
}

/// Estimated count of NAT translation layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum LayerCount {
    Zero,
    One,
    AtLeastTwo,
}

/// Classification of the NAT layer topology.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum LayerKind {
    None,
    SingleNat,
    Cgnat,
    DoubleNat,
    UpstreamNatLikely,
    SingleNatAssumed,
}

/// Individual signals fed into the CGNAT / double-NAT combiner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum EvidenceItem {
    E1RouteSourceSharedSpace,
    E2DsLiteAftr,
    E3PortmapIpNotGlobal,
    E4PortmapIpDiffersFromStun,
    E5PortmapIpEqualsStun,
    E6StunIpNotGlobal,
    E7ArbitraryPooling,
    E8CounterOrNarrowPool,
    E9ShortLifetime,
    E10PcpAddressMismatch,
    E11MappedEqualsLocal,
    E12ReachabilityEcho,
}

/// Output of the CGNAT and double-NAT evidence combiner.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub struct NatLayers {
    pub layers: Option<LayerCount>,
    pub kind: LayerKind,
    /// Noisy-OR of positive evidence weights (0.0..=1.0).
    pub confidence: f32,
    pub evidence: Vec<EvidenceItem>,
    pub contradictions: Vec<EvidenceItem>,
}
