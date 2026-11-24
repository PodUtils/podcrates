//! A probe of the NAT on the path, for the crates and the binaries of PodUtils.
//!
//! The probe gives each property as a typed [`Obs`]: the mapping, the filtering, the port
//! allocation, the IP pooling, hairpinning, the lifetime of a mapping, ALG rewrites, IPv6 and
//! NAT64, and the port mapping of the gateway (NAT-PMP, PCP, UPnP IGD).
//!
//! * **No I/O in the core.** With `default-features = false`, [`engine::ProbeMachine`] builds
//!   with no `tokio`, no socket and no read of a clock.
//! * **The runner** (feature `run`, default): [`Prober`] drives the state machine on the
//!   [`Seams`] of the caller (`pod_core::Datagrams`, `Clock`, `Entropy`, `Report`).
//! * **The simulator** (feature `sim`): NATs and STUN servers in virtual time, std only, for
//!   tests with no root and no network.
//!
//! The crate reads no environment variable, keeps no global state and writes nothing to
//! stdout or stderr.

#![forbid(unsafe_code)]

pub mod classify;
pub mod engine;
pub mod error;
pub mod report;
pub mod seams;
pub mod settings;
pub mod strategy;
pub mod wire;

#[cfg(feature = "run")]
pub mod run;

#[cfg(feature = "sim")]
pub mod sim;

pub use error::NatError;
pub use report::{
    AbsenceProof, AlgDetection, Certainty, Connectivity, Constraint, ControlKind, Evidence, EvidenceItem,
    ExternalIpSource, Family, FamilyParts, FamilyReport, FilteringBehavior, Flag, HairpinBehavior, IpPooling,
    Ipv6Status, LayerCount, LayerKind, LifetimeBound, LimitKind, LocalV6Class, MappingBehavior, NaReason, Nat64Status,
    NatLayers, NatLifetime, NonEmpty, Obs, ObsId, Observation, ObservationKind, PcpState, PmpState, PoolBand,
    PortAllocation, PortMappingSupport, Pref64, Profile, PublicEndpoint, QuestionId, REPORT_SCHEMA,
    Report as NatReport, ReportError, ServerFlag, ServerRecord, SockId, Tick, UdpPathStatus, Unknown, UnknownReason,
    UpnpState,
};
pub use seams::{BindSpec, Gateway, LocalEnv};
pub use settings::{BucketConfig, LadderProfile, LifetimeSettings, ServerEndpoint, Settings};

#[cfg(feature = "run")]
pub use run::{Prober, Seams};
