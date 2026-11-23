//! Three-state observation types and unknown reasons. A timeout can never construct an
//! observed negative (`Obs::Absent`).

use std::time::Duration;

/// Microseconds since the start of the probe. The core never reads a clock.
pub type Tick = u64;

/// Index into [`crate::report::Report::observations`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub struct ObsId(pub u32);

/// A vector guaranteed to hold at least one item.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub struct NonEmpty<T> {
    first: T,
    rest: Vec<T>,
}

impl<T> NonEmpty<T> {
    /// Construct a non-empty list from a first element and tail.
    pub fn new(first: T, rest: Vec<T>) -> Self {
        Self { first, rest }
    }

    /// Construct from a vector, returning `None` when empty.
    pub fn from_vec(mut v: Vec<T>) -> Option<Self> {
        if v.is_empty() {
            return None;
        }
        let first = v.remove(0);
        Some(Self { first, rest: v })
    }

    /// Number of items (always at least 1).
    pub fn count(&self) -> usize {
        1 + self.rest.len()
    }

    /// First item.
    pub fn first(&self) -> &T {
        &self.first
    }

    /// Iterate over all items in order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        std::iter::once(&self.first).chain(self.rest.iter())
    }
}

/// Strength of the evidence supporting an observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum Certainty {
    /// All rows of the decision table were met, controls included.
    Exact,
    /// A mechanism-level inference with a named weaker premise.
    Suspected,
    /// Derived from a model or external mapping.
    Inferred,
}

/// Rows of `Report::observations` that justify a verdict. Never empty.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub struct Evidence {
    pub refs: NonEmpty<ObsId>,
    pub certainty: Certainty,
}

impl Evidence {
    /// Build exact evidence referencing a single observation.
    pub fn exact(id: ObsId) -> Self {
        Self { refs: NonEmpty::new(id, Vec::new()), certainty: Certainty::Exact }
    }
}

/// Proof of an observed negative. A timeout alone cannot construct this type.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum AbsenceProof {
    /// Protocol-level refusal (STUN 4xx/5xx, NAT-PMP/PCP result code, SOAP fault).
    Refusal { code: u32 },
    /// Operating-system error with a defined meaning (for example ENETUNREACH on UDP connect).
    OsError { errno: i32 },
    /// Silence after a positive control proved the path and peer for this test.
    SilenceAfterControl { control: ObsId },
}

/// Which positive control ran or failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum ControlKind {
    ChangeIpAndPort,
    ChangePort,
    ResponsePort,
    V6Positive,
    UdpPositive,
    GatewayReachable,
}

/// Why a field has no decided value.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum UnknownReason {
    NoT1,
    NoT2,
    NoT3,
    TooFewSamples,
    NeedTwoDistinctServerIps,
    InsufficientServers,
    NoCorroboration,
    NoV6Answer,
    NoReplyInWindow,
    Timeout { sends: u8, elapsed_ms: u32 },
    Pending,
    BudgetExhausted,
    Cancelled,
    NoOtherAddress,
    AlternateNotDistinct,
    ServerLacksChangeRequest,
    ServerIgnoresChangeRequest,
    SrcMismatch,
    ServerSilentOrControlFailed,
    ChangeIpUnverified,
    ServerIgnoresResponsePort,
    ResponsePortUnsupported,
    ServerFault { code: u16 },
    ControlFailed(ControlKind),
    UnstableMapping,
    UnstableFiltering,
    InconsistentMapping,
    PoolingConfounds,
    FilterConfounds,
    NonMonotone,
    NotPreserving,
    LayersUnknown,
    NoGateway,
    GatewayRange,
    UdpNotOpen,
}

/// A partial constraint that holds even when the exact class is unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum Constraint {
    None,
    /// Mapping is address-dependent or address-and-port-dependent, not endpoint-independent.
    MappingNotEndpointIndependent,
    /// Filtering is endpoint-independent or address-dependent, not address-and-port-dependent.
    FilteringNotAddressAndPortDependent,
}

/// An undecided observation paired with its reason and any partial constraint.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub struct Unknown {
    pub reason: UnknownReason,
    pub constraint: Constraint,
}

/// Why a probe does not apply on this host or network path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum NaReason {
    DirectPublic,
    PublicLocalAddress,
    NoFamily,
    UdpNotOpen,
    HairpinGuard,
}

/// Observation wrapper for every report field.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum Obs<T> {
    Observed { value: T, evidence: Evidence, flags: Vec<Flag> },
    Absent { proof: AbsenceProof },
    Unknown(Unknown),
    NotApplicable(NaReason),
}

impl<T> Obs<T> {
    /// Build an `Unknown` with no partial constraint.
    pub fn unknown(reason: UnknownReason) -> Self {
        Self::Unknown(Unknown { reason, constraint: Constraint::None })
    }

    /// Build an `Unknown` carrying a partial constraint.
    pub fn constrained(reason: UnknownReason, constraint: Constraint) -> Self {
        Self::Unknown(Unknown { reason, constraint })
    }

    /// Build an `Observed` with exact evidence and flags.
    pub fn observed(value: T, evidence: Evidence, flags: Vec<Flag>) -> Self {
        Self::Observed { value, evidence, flags }
    }

    /// Borrow the observed value when present.
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Observed { value, .. } => Some(value),
            _ => None,
        }
    }

    /// True when the observation is decided (`Observed`, `Absent`, or `NotApplicable`).
    pub fn is_decided(&self) -> bool {
        !matches!(self, Self::Unknown(_))
    }

    /// Partial constraint when `Unknown`, else [`Constraint::None`].
    pub fn constraint(&self) -> Constraint {
        match self {
            Self::Unknown(u) => u.constraint,
            _ => Constraint::None,
        }
    }
}

/// Non-verdict annotations attached to an observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "report", derive(serde::Serialize, serde::Deserialize))]
pub enum Flag {
    AlgSuspected,
    MappedOnly,
    XorAlt8020,
    OriginNeSrc,
    SrcMismatch,
    LateResponse,
    PreserveCollisions,
    NarrowPool,
    LowPorts,
    PoolSplit,
    ArbitraryIp,
    AfterPunch,
    HairpinRepliedFromInternal,
    ServerRedirects,
    DnsResolverIsDoh,
    ComposedLayers,
    LegacyDiverges,
}

/// Convert a duration to saturated milliseconds.
pub fn ms(d: Duration) -> u32 {
    d.as_millis().min(u128::from(u32::MAX)) as u32
}
