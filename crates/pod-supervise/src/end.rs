//! The end of a payload, as the launcher measured it: why it ended, how
//! long it ran, the use of its tree, and the processes of the tree that
//! stayed after the kill.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Why the payload ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reason {
    /// The payload exited with a code.
    Exit,
    /// A signal ended the payload (a `kill`, a `stop`, or its own fault).
    Signal,
    /// The launcher killed the payload at its wall bound
    /// (`Container::wall_ms`).
    Wall,
}

/// The processes of the tree that stayed after the end: a number, or
/// `unknown`. `unknown` is never written as 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Residual {
    Count(u64),
    /// The launcher cannot find each descendant: the host refused the child
    /// subreaper, or `/proc` cannot be read while a child is left.
    Unknown,
}

impl Serialize for Residual {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Residual::Count(n) => s.serialize_u64(*n),
            Residual::Unknown => s.serialize_str("unknown"),
        }
    }
}

impl<'de> Deserialize<'de> for Residual {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Count(u64),
            Word(String),
        }
        match Raw::deserialize(d)? {
            Raw::Count(n) => Ok(Residual::Count(n)),
            Raw::Word(w) if w == "unknown" => Ok(Residual::Unknown),
            Raw::Word(w) => {
                Err(serde::de::Error::custom(format!("a residual count is a number or \"unknown\", not {w:?}")))
            }
        }
    }
}

/// The end of one run of a payload. The launcher writes it into the record
/// before it answers a waiter, so a `wait` that returns finds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct End {
    pub reason: Reason,
    /// From the start of the payload to its end, in milliseconds.
    pub run_ms: u64,
    /// The CPU time of the tree (`getrusage(RUSAGE_CHILDREN)` of the
    /// launcher after the kill): each process of the tree that was reaped.
    pub user_ms: u64,
    pub system_ms: u64,
    /// The peak resident set of the largest one process of the tree, in
    /// KiB. It is not a sum.
    pub max_rss_kib: u64,
    /// The processes of the tree that stayed after the kill of the tree.
    pub residual_processes: Residual,
}

#[cfg(test)]
mod tests;
