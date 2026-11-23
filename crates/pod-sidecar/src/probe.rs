//! The disagreement probe and its three values (rules PROBE-1 and PROBE-2).
//!
//! The probe sends legal but unusual segments: an out-of-window segment, a
//! retransmission with a different payload, and a FIN before the data is
//! acknowledged. It reports the disagreement that it found, or that it found
//! none. A probe that cannot run is `Unknown`, and it carries the reason.
//!
//! A path with no reply is not a disagreement and not a measurement of one:
//! the probe reports `Agrees` because a silent path gave no evidence of
//! interference. A device that refuses to run gives `Unknown`.

use std::fmt;

use crate::error::Refusal;
use crate::middlebox::Fault;
use crate::packet::Segment;
use crate::simulator::Simulator;

/// The three values of a probe (PROBE-2). A boolean cannot carry "not
/// measured".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The path answered in a way that a correct TCP does not.
    Disagrees,
    /// The path gave no wrong answer, or gave no answer at all.
    Agrees,
    /// The probe did not run. Never rendered as a disagreement.
    Unknown,
}

impl Verdict {
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Disagrees => "disagrees",
            Verdict::Agrees => "agrees",
            Verdict::Unknown => "unknown",
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// The three unusual segments of rule PROBE-1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProbeKind {
    /// A segment that starts beyond the receive window.
    OutOfWindow,
    /// A retransmission of a sequence number with a different payload.
    Retransmit,
    /// A FIN sent before the data is acknowledged.
    FinBeforeAck,
}

impl ProbeKind {
    pub const ALL: [ProbeKind; 3] = [ProbeKind::OutOfWindow, ProbeKind::Retransmit, ProbeKind::FinBeforeAck];

    pub fn as_str(self) -> &'static str {
        match self {
            ProbeKind::OutOfWindow => "out-of-window",
            ProbeKind::Retransmit => "retransmit-different-payload",
            ProbeKind::FinBeforeAck => "fin-before-ack",
        }
    }
}

impl fmt::Display for ProbeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What one probe of a path saw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    /// The segments that came back.
    pub responses: Vec<Segment>,
    /// The bytes that the far application received during the probe.
    pub stream: Vec<u8>,
    /// The acknowledgement that a correct TCP would send, when the probe
    /// knows it.
    pub expected_ack: Option<u32>,
    /// The bytes that a correct path would deliver to the far application,
    /// when the probe knows them.
    pub expected_stream: Option<Vec<u8>>,
    /// A named fault of the path.
    pub fault: Option<Fault>,
}

impl Observation {
    /// A path that gave no answer.
    pub fn silent() -> Observation {
        Observation {
            responses: Vec::new(),
            stream: Vec::new(),
            expected_ack: None,
            expected_stream: None,
            fault: None,
        }
    }

    fn is_silent(&self) -> bool {
        self.responses.is_empty() && self.fault.is_none() && self.expected_stream.is_none()
    }
}

/// A path that the probe can measure: a simulator, a silent path, a device
/// that cannot run, or a raw socket.
pub trait Path {
    fn exchange(&mut self, kind: ProbeKind) -> Result<Observation, Refusal>;
}

impl Path for Simulator {
    fn exchange(&mut self, kind: ProbeKind) -> Result<Observation, Refusal> {
        Ok(self.probe(kind))
    }
}

/// A path that never answers.
#[derive(Clone, Copy, Debug, Default)]
pub struct Silent;

impl Path for Silent {
    fn exchange(&mut self, _kind: ProbeKind) -> Result<Observation, Refusal> {
        Ok(Observation::silent())
    }
}

/// A path that cannot run, with its reason.
#[derive(Clone, Debug)]
pub struct Unavailable(pub Refusal);

impl Unavailable {
    pub fn new(reason: impl Into<String>) -> Unavailable {
        Unavailable(Refusal::new(reason))
    }
}

impl Path for Unavailable {
    fn exchange(&mut self, _kind: ProbeKind) -> Result<Observation, Refusal> {
        Err(self.0.clone())
    }
}

/// One probe's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    pub kind: ProbeKind,
    pub outcome: CheckOutcome,
}

/// What one probe found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckOutcome {
    /// The path answered as a correct TCP does, or did not answer.
    Agrees,
    /// The path answered in a way that a correct TCP does not.
    Disagrees(String),
    /// The path gave no answer.
    Silent,
    /// The probe did not run.
    Unavailable(String),
}

/// The probe and its verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub verdict: Verdict,
    /// A short reason: the first disagreement, or the reason that no probe
    /// could run.
    pub reason: String,
    pub checks: Vec<Check>,
}

impl Finding {
    /// Run the three probes of rule PROBE-1 against `path`.
    pub fn of(path: &mut impl Path) -> Finding {
        let mut checks = Vec::with_capacity(ProbeKind::ALL.len());
        let mut first_refusal: Option<String> = None;
        for kind in ProbeKind::ALL {
            let outcome = match path.exchange(kind) {
                Ok(observation) => judge(&observation),
                Err(refusal) => {
                    first_refusal.get_or_insert_with(|| refusal.reason().to_string());
                    CheckOutcome::Unavailable(refusal.reason().to_string())
                }
            };
            checks.push(Check { kind, outcome });
        }
        let disagree = checks.iter().find_map(|check| match &check.outcome {
            CheckOutcome::Disagrees(reason) => Some(reason.clone()),
            _ => None,
        });
        let ran = checks.iter().any(|check| !matches!(check.outcome, CheckOutcome::Unavailable(_)));
        let (verdict, reason) = if let Some(reason) = disagree {
            (Verdict::Disagrees, reason)
        } else if ran {
            (Verdict::Agrees, "no path disagreed with a correct TCP".to_string())
        } else {
            (Verdict::Unknown, first_refusal.unwrap_or_else(|| "the probe did not run".to_string()))
        };
        Finding { verdict, reason, checks }
    }
}

/// The probe. It holds no state, so one value serves each path.
#[derive(Clone, Copy, Debug, Default)]
pub struct Probe;

impl Probe {
    pub fn run(path: &mut impl Path) -> Finding {
        Finding::of(path)
    }
}

fn judge(observation: &Observation) -> CheckOutcome {
    if let Some(fault) = observation.fault {
        return CheckOutcome::Disagrees(format!("the path reported {}", fault.as_str()));
    }
    if observation.responses.iter().any(Segment::is_rst) {
        return CheckOutcome::Disagrees("the path reset a legal segment".to_string());
    }
    if let Some(expected) = observation.expected_ack
        && let Some(ack) = observation.responses.iter().rev().find(|s| s.is_ack() && !s.is_syn())
        && ack.ack != expected
    {
        return CheckOutcome::Disagrees(format!("the path acknowledged {} where {} is correct", ack.ack, expected));
    }
    if let Some(expected) = &observation.expected_stream
        && &observation.stream != expected
    {
        return CheckOutcome::Disagrees(format!(
            "the path delivered {:?} where {:?} is correct",
            String::from_utf8_lossy(&observation.stream),
            String::from_utf8_lossy(expected)
        ));
    }
    if observation.is_silent() { CheckOutcome::Silent } else { CheckOutcome::Agrees }
}

#[cfg(unix)]
mod raw;
#[cfg(unix)]
pub use raw::{RawPath, capability as raw_capability};

/// A host that has no unix raw socket cannot run the raw path.
#[cfg(not(unix))]
pub fn raw_capability() -> Result<(), Refusal> {
    Err(Refusal::new("raw socket unavailable: this target has no unix raw socket"))
}
