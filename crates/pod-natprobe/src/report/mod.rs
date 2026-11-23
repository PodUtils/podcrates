//! Top-level NAT probe report, observation log, server records and formatting.

pub mod axes;
pub mod family;
pub mod legacy;
pub mod obs;
pub mod status;
pub mod wire;

use std::fmt;
use std::net::{IpAddr, SocketAddr};

pub use axes::{
    AlgDetection, Connectivity, Family, FilteringBehavior, HairpinBehavior, IpPooling, LifetimeBound, MappingBehavior,
    NatLifetime, PoolBand, PortAllocation, PublicEndpoint,
};
pub use family::{FamilyParts, FamilyReport, Profile, ReportError, SockId};
pub use obs::{
    AbsenceProof, Certainty, Constraint, ControlKind, Evidence, Flag, NaReason, NonEmpty, Obs, ObsId, Tick, Unknown,
    UnknownReason, ms,
};
pub use status::{
    EvidenceItem, ExternalIpSource, Ipv6Status, LayerCount, LayerKind, LimitKind, LocalV6Class, Nat64Status, NatLayers,
    PcpState, PmpState, PortMappingSupport, Pref64, UdpPathStatus, UpnpState,
};

/// Current report schema version.
pub const REPORT_SCHEMA: u8 = 1;

/// One timestamped wire event recorded during a probe run.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub struct Observation {
    pub id: ObsId,
    pub at: Tick,
    pub socket: SockId,
    pub kind: ObservationKind,
}

/// Kind of recorded observation.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum ObservationKind {
    Mapped { dest: SocketAddr, mapped: SocketAddr, rtt_us: u32 },
    Timeout { dest: SocketAddr, sends: u8, elapsed_ms: u32 },
    WrongSource { dest: SocketAddr, from: SocketAddr },
    Error { dest: SocketAddr, code: u16 },
    Dropped { reason: String },
    LocalFact { note: String },
    GatewayReply { proto: String, detail: String },
    DnsResult { detail: String },
}

/// Behaviour notes observed for one remote STUN server during the run.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub struct ServerRecord {
    pub name: String,
    pub ip: IpAddr,
    pub flags: Vec<ServerFlag>,
}

/// Server-side quirks or capabilities observed on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum ServerFlag {
    IgnoresChangeRequest,
    LacksChangeRequest,
    NoOtherAddress,
    OriginNeSrc,
    AlternateNotDistinct,
    Redirects,
    SendsXorOnly,
}

/// Identifiers of individual probe questions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum QuestionId {
    Local,
    Dns64,
    Portmap,
    UdpV4,
    UdpV6,
    Mapping,
    Filtering,
    Allocation,
    Pooling,
    Hairpin,
    UdpMatrix,
    Lifetime,
    Mtu,
}

/// Complete NAT probe report. IPv4 and IPv6 reports are kept separate.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub struct Report {
    pub schema: u8,
    pub profile: Profile,
    pub started: Tick,
    pub finished: Tick,
    pub v4: Option<FamilyReport>,
    pub v6: Option<FamilyReport>,
    pub ipv6: Obs<Ipv6Status>,
    pub nat64: Obs<Nat64Status>,
    pub portmap: PortMappingSupport,
    pub layers: Obs<NatLayers>,
    pub servers: Vec<ServerRecord>,
    pub observations: Vec<Observation>,
    /// Questions still undecided when the profile budget expired.
    pub pending: Vec<QuestionId>,
}

impl Report {
    /// Duration of the probe run in microseconds.
    pub fn elapsed_us(&self) -> u64 {
        self.finished.saturating_sub(self.started)
    }

    /// Encode this report as a compact 20-byte wire summary.
    pub fn compact_profile(&self) -> wire::CompactProfile {
        wire::CompactProfile::from_report(self)
    }

    /// Serialize the report as JSON (feature `report`).
    #[cfg(feature = "report")]
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "natprobe({:?}, {}ms)", self.profile, self.elapsed_us() / 1000)?;
        if let Some(ref v4) = self.v4 {
            write!(
                f,
                " v4[conn={:?} map={:?} filt={:?} alloc={:?}]",
                v4.connectivity.value(),
                v4.mapping.value(),
                v4.filtering.value(),
                v4.allocation.value()
            )?;
        }
        write!(f, " v6={:?} pending={}", self.ipv6.value(), self.pending.len())
    }
}
