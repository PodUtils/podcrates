//! Network labs inside a guest that is root: the part of sand-net that
//! PodUtils/podvm#260 asks for (podvm's issue 178).
//!
//! * [`Knobs`]: each knob of sand-net with its range, its flags, and the
//!   refusal of each contradiction ([`Knobs::check`]).
//! * [`Layout`]: the namespaces `PREFIX-NAME-g|r|w` (and `-c` for cgnat),
//!   their veth pairs and their addresses.
//! * [`scenario`]: the 27 offline scenarios, each a knob set.
//! * [`Lab::up`] and [`Lab::down`]: the commands, as argument lists, that
//!   build and remove the lab in the guest ([`plan`]).
//! * [`Lab::expect`]: from the knobs alone, the expected verdict of each
//!   probe and the named counters that must move and must stay.
//! * [`judge`]: the measured verdicts and the counters of
//!   `nft -j list counters` before and after ([`parse_counters`]) against
//!   the expectations, as PASS and FAIL rows.
//!
//! The crate runs no command, opens no socket and reads no file: the
//! consumer runs the steps in the guest, the probes of the guest measure,
//! and the consumer gives the output back. A failure converts to a
//! [`pod_core::Failure`] of the stage `Host`.

pub mod counters;
pub mod expect;
pub mod judge;
pub mod knobs;
pub mod lab;
pub mod layout;
pub mod plan;
pub mod probe;
pub mod scenario;

pub use counters::{Count, CounterError, Counters, parse_counters};
pub use expect::{Bound, Expect, Expectations, Must, Seen, Verdict};
pub use judge::{Measured, Report, Row, judge};
pub use knobs::{Broken, Contradiction, Family, Icmp, KnobError, Knobs, Nat, NatFlags, Ping, Policy, Proto, Stack};
pub use lab::{Lab, LabError, Settings};
pub use layout::{Layout, Role};
pub use plan::Step;
pub use plan::ruleset::Counter;
pub use probe::{Probe, Side, Target, probes};
pub use scenario::{Scenario, scenario, scenarios};
