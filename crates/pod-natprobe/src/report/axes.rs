//! Canonical NAT behaviour axes (RFC 4787, RFC 5780).

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

/// IP address family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum Family {
    V4,
    V6,
}

/// Axis 1: high-level reachability and NAT layering class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum Connectivity {
    DirectPublic,
    BehindNat,
    BehindCgnat,
    DoubleNat,
    UdpBlocked,
}

/// Axis 2: RFC 4787 section 4.1 / RFC 5780 section 4.3 mapping behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum MappingBehavior {
    EndpointIndependent,
    AddressDependent,
    AddressAndPortDependent,
}

/// Axis 3: RFC 4787 section 5 / RFC 5780 section 4.4 filtering behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum FilteringBehavior {
    EndpointIndependent,
    AddressDependent,
    AddressAndPortDependent,
    /// A mapping exists, but even return packets from the contacted endpoint do not arrive.
    NoReturnTraffic,
}

/// Estimated port pool band for restricted random allocation (for example 49152..=65535).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub struct PoolBand {
    pub lo: u16,
    pub hi: u16,
    /// Unbiased range estimator `(max - min) * (n + 1) / (n - 1)`.
    pub width_est: u32,
}

/// External port allocation pattern across fresh sockets to one destination.
/// Deltas are computed on the 1024..=65535 ring (64512 ports) and stored as `i16`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum PortAllocation {
    /// Strict majority of fresh sockets kept their local source port.
    Preserving,
    Sequential {
        delta: i16,
        jitter: u16,
        tight: bool,
    },
    GlobalCounter {
        drift_per_s: i16,
        jitter: u16,
    },
    Random {
        pool: Option<PoolBand>,
    },
}

impl PortAllocation {
    /// True when the next mapped port can be predicted well enough to guide a focused spray.
    pub fn predictable(&self) -> bool {
        !matches!(self, Self::Random { .. })
    }
}

/// RFC 4787 section 4.1 IP pooling behaviour per internal host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum IpPooling {
    Paired,
    Arbitrary,
    /// Multi-WAN or ECMP: one IP to one destination, distinct IPs across destinations.
    PerDestinationEgress,
}

/// RFC 4787 section 6 hairpinning (NAT loopback) behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum HairpinBehavior {
    /// No datagram arrived under EIF or after a bidirectional punch of at least 3 rounds.
    None,
    /// Arrived with the sender's internal endpoint as source (RFC 4787 REQ-9a violation).
    InternalSource,
    /// Arrived with the sender's external endpoint as source.
    ExternalSource,
    /// Arrived from the external IP with a different port.
    ExternalIpOnly,
    /// Arrived from another source address.
    OtherSource,
}

/// Bracket for an observed UDP mapping lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum LifetimeBound {
    /// True lifetime lies in `(low, high]`; estimate is the geometric mean.
    Exact {
        low: Duration,
        high: Duration,
    },
    AtLeast(Duration),
    Below(Duration),
}

/// Measured UDP mapping lifetime (RFC 5780 section 4.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub struct NatLifetime {
    pub bound: LifetimeBound,
    pub estimate: Duration,
    /// Recommended keepalive interval: `clamp(estimate / 3, 5 s, 25 s)`.
    pub keepalive_hint: Duration,
    /// `Some(false)` when the bracket lies entirely below 120 s (RFC 4787 REQ-5).
    pub meets_req5: Option<bool>,
    pub repetitions: u8,
}

/// NAT ALG interference check comparing `MAPPED-ADDRESS` and `XOR-MAPPED-ADDRESS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum AlgDetection {
    /// Both attributes arrived and agreed.
    NoAlgObserved,
    /// Both attributes arrived and disagreed; the XOR value is used for all verdicts.
    Suspected { plain: SocketAddr, xor: SocketAddr },
    /// Only XOR-MAPPED-ADDRESS arrived; ALG check could not run.
    Unverifiable,
    /// Only MAPPED-ADDRESS arrived (RFC 3489 server).
    Undetectable,
}

/// Public IP and UDP port observed by a remote server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub struct PublicEndpoint {
    pub ip: IpAddr,
    pub port: u16,
}
