//! Fast, accurate NAT behaviour probe for the PodUtils crates and binaries.
//!
//! `pod-natprobe` separates mapping, filtering, port allocation, IP pooling,
//! hairpinning, mapping lifetime, ALG rewriting, IPv6/NAT64 and gateway port
//! mapping (NAT-PMP, PCP, UPnP IGD) into typed [`Obs`] observations.
//!
//! * **Sans-IO core.** With `default-features = false`, [`engine::ProbeMachine`]
//!   compiles with no `tokio`, no sockets and no clock reads.
//! * **Async runner** (feature `run`, default): [`Prober`] drives the state
//!   machine over caller-supplied [`Seams`] (`pod_core::Datagrams`, `Clock`,
//!   `Entropy`, `Report`).
//! * **Deterministic simulator** (feature `sim`): pure-`std` virtual-time NAT
//!   and server simulator for exhaustive testing without root or network access.
//!
//! **No process state, no output.** The crate reads no environment variable,
//! keeps no global state and writes nothing to stdout or stderr.

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
