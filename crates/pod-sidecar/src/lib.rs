//! The packet sidecar of PodUtils/podnet#495, at library level.
//!
//! A network can rewrite a packet, and a network can refuse a datagram.
//! This crate holds the component that sits below the stream and above the
//! link, in five parts:
//!
//! * [`primitive`]: the five primitives (drop, tamper, duplicate, fragment,
//!   coalesce), a trigger, and a strategy file with its date and its
//!   network ([`Strategy::from_toml`]).
//! * [`simulator`]: a deterministic local TCP-like middlebox with a
//!   three-way handshake, a data phase, a window, retransmission and FIN
//!   rules, and a report of the divergence that a strategy caused. It opens
//!   no socket and needs no privilege.
//! * [`search`]: a genetic search over strategies, scored by the simulated
//!   disagreement with the fewest changes. It uses the simulator alone.
//! * [`probe`]: the disagreement probe and its third value ([`Verdict`]).
//!   It sends legal but unusual segments, and a path that cannot run gives
//!   `Unknown` with a named reason.
//! * [`split`], [`fake_tcp`] and [`bond`]: split and tamper with its limits
//!   ([`limits`]), message framing over a datagram channel, and a per-flow
//!   scheduler of two links.
//!
//! # The feature gate
//!
//! Every part above is behind the cargo feature `sidecar`, and that feature
//! is off by default. A default build of this crate is empty: it holds no
//! packet path, no raw socket and no TUN device. The sidecar is never a
//! built-in path of a default binary (rules SYN-1 and LIM-2).
//!
//! The raw socket of `probe::RawPath` exists only with the feature, and
//! it checks the host before it runs. A host that refuses it gives
//! [`Verdict::Unknown`] with the reason, never a failure.
//!
//! The crate reads no environment variable, keeps no global state and
//! prints nothing.

pub const FEATURE: &str = "sidecar";
pub const ENABLED: bool = cfg!(feature = "sidecar");

#[cfg(feature = "sidecar")]
mod error;
#[cfg(feature = "sidecar")]
mod middlebox;
#[cfg(feature = "sidecar")]
mod packet;
#[cfg(feature = "sidecar")]
mod rng;
#[cfg(feature = "sidecar")]
mod state;

#[cfg(feature = "sidecar")]
pub mod bond;
#[cfg(feature = "sidecar")]
pub mod endpoint;
#[cfg(feature = "sidecar")]
pub mod fake_tcp;
#[cfg(feature = "sidecar")]
pub mod limits;
#[cfg(feature = "sidecar")]
pub mod primitive;
#[cfg(feature = "sidecar")]
pub mod probe;
#[cfg(feature = "sidecar")]
pub mod search;
#[cfg(feature = "sidecar")]
pub mod simulator;
#[cfg(feature = "sidecar")]
pub mod split;

#[cfg(feature = "sidecar")]
pub use bond::{Bond, BondError, FlowId, LinkId};
#[cfg(feature = "sidecar")]
pub use error::{Refusal, SplitError, StrategyError};
#[cfg(feature = "sidecar")]
pub use fake_tcp::{FakeTcp, FakeTcpError};
#[cfg(feature = "sidecar")]
pub use middlebox::{Direction, Fault};
#[cfg(feature = "sidecar")]
pub use packet::Segment;
#[cfg(feature = "sidecar")]
pub use primitive::{Date, Params, Primitive, Step, Strategy, Trigger};
#[cfg(feature = "sidecar")]
pub use probe::{Finding, Path, Probe, Verdict};
#[cfg(feature = "sidecar")]
pub use search::{Outcome, Search, SearchError};
#[cfg(feature = "sidecar")]
pub use simulator::{Divergence, Policy, Run, Scenario, Simulator};
#[cfg(feature = "sidecar")]
pub use split::{Behaviour, SplitConfig, SplitRun};
#[cfg(feature = "sidecar")]
pub use state::{Side, TcpState};
