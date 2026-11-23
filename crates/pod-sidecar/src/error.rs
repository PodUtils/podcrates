//! The named errors of the sidecar. Each error has a variant, and each
//! message names the rule that it holds.

use std::fmt;

/// A path or a device that cannot run a measurement. It carries the reason,
/// so a caller can print a named skip and never a failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    reason: String,
}

impl Refusal {
    pub fn new(reason: impl Into<String>) -> Refusal {
        Refusal { reason: reason.into() }
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for Refusal {}

/// A strategy file that cannot load. Rule SYN-4: a strategy with no date has
/// no evidence behind it, and the load refuses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StrategyError {
    /// The field `date` is absent or empty.
    NoDate,
    /// The field `date` is present but is not a calendar date.
    BadDate(String),
    /// A required field is absent or empty.
    Missing(&'static str),
    /// The field `primitive` names no primitive.
    UnknownPrimitive(String),
    /// The field `trigger` names no trigger.
    UnknownTrigger(String),
    /// The table has no step.
    NoSteps,
    /// The text is not a TOML table of the expected shape.
    Toml(String),
}

impl fmt::Display for StrategyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StrategyError::NoDate => f.write_str("strategy has no date; a strategy with no date has no evidence"),
            StrategyError::BadDate(d) => write!(f, "strategy date {d:?} is not YYYY-MM-DD"),
            StrategyError::Missing(field) => write!(f, "strategy field {field:?} is absent"),
            StrategyError::UnknownPrimitive(p) => {
                write!(f, "unknown primitive {p:?}; use drop, tamper, duplicate, fragment or coalesce")
            }
            StrategyError::UnknownTrigger(t) => {
                write!(f, "unknown trigger {t:?}; use FirstOutboundData, SynAck, FirstInboundRst or Always")
            }
            StrategyError::NoSteps => f.write_str("strategy has no step; a strategy is an ordered list"),
            StrategyError::Toml(m) => write!(f, "strategy is not a TOML table: {m}"),
        }
    }
}

impl std::error::Error for StrategyError {}

/// A split behaviour that the simulator defeated, or a split file that
/// cannot load. Each variant names the behaviour and the cause.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SplitError {
    /// The receiving side reassembled the segments, so the split or the
    /// disorder had no effect (rule SPLIT-2).
    Reassembled { behaviour: &'static str },
    /// The receiving side required a valid checksum, so the tampered segment
    /// was dropped (rule SPLIT-2).
    ChecksumRejected { behaviour: &'static str },
    /// The path honoured the fake TTL and dropped the segment.
    TimeExceeded { behaviour: &'static str },
    /// The segment that the behaviour acts on carries no payload.
    NoPayload { behaviour: &'static str },
    /// The split file is not valid.
    Config(String),
    /// The split file has no date.
    NoDate,
}

impl SplitError {
    pub fn behaviour(&self) -> Option<&'static str> {
        match self {
            SplitError::Reassembled { behaviour }
            | SplitError::ChecksumRejected { behaviour }
            | SplitError::TimeExceeded { behaviour }
            | SplitError::NoPayload { behaviour } => Some(behaviour),
            SplitError::Config(_) | SplitError::NoDate => None,
        }
    }
}

impl fmt::Display for SplitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SplitError::Reassembled { behaviour } => {
                write!(f, "{behaviour}: the receiver reassembled the segments, so the split is defeated")
            }
            SplitError::ChecksumRejected { behaviour } => {
                write!(f, "{behaviour}: the receiver requires a valid checksum, so the tamper is defeated")
            }
            SplitError::TimeExceeded { behaviour } => {
                write!(f, "{behaviour}: the path honoured the TTL and dropped the segment")
            }
            SplitError::NoPayload { behaviour } => write!(f, "{behaviour}: the segment carries no payload"),
            SplitError::Config(m) => write!(f, "split file: {m}"),
            SplitError::NoDate => f.write_str("split file has no date; a behaviour with no date has no evidence"),
        }
    }
}

impl std::error::Error for SplitError {}
