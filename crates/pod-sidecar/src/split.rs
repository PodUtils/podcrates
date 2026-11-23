//! Split and tamper as one library with one configuration format (rule
//! SPLIT-1): split at an offset, disorder, a fake TTL, a tampered checksum
//! and a padded first segment.
//!
//! Each behaviour changes the first outbound data batch of the simulator.
//! The simulator then says what the receiving end did with it, and a path
//! that defeats the behaviour gives a named error (rule SPLIT-2):
//! reassembly defeats a split, a disorder and a padding, a valid-checksum
//! requirement defeats a tampered checksum, and a path that honours the TTL
//! defeats a fake TTL. [`crate::limits`] holds the condition that triggers
//! each limit and the check that detects it.

use std::fmt;

use crate::error::SplitError;
use crate::packet::Segment;
use crate::primitive::{Date, text_field};
use crate::simulator::Simulator;
use crate::state::TcpState;

/// One behaviour of the library.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Behaviour {
    /// One segment becomes two at `offset`.
    SplitAt { offset: usize },
    /// The first two segments swap their order.
    Disorder,
    /// The first segment carries a low TTL.
    FakeTtl { ttl: u8 },
    /// One bit of the first checksum changes.
    TamperChecksum { mask: u16 },
    /// The first segment carries `bytes` bytes of padding.
    PadFirst { bytes: usize },
}

impl Behaviour {
    pub const ALL: [Behaviour; 5] = [
        Behaviour::SplitAt { offset: 2 },
        Behaviour::Disorder,
        Behaviour::FakeTtl { ttl: 1 },
        Behaviour::TamperChecksum { mask: 0x0102 },
        Behaviour::PadFirst { bytes: 4 },
    ];

    /// The name in the limits page and in an error.
    pub fn name(self) -> &'static str {
        match self {
            Behaviour::SplitAt { .. } => "split-at-an-offset",
            Behaviour::Disorder => "disorder",
            Behaviour::FakeTtl { .. } => "fake-ttl",
            Behaviour::TamperChecksum { .. } => "tamper-a-checksum",
            Behaviour::PadFirst { .. } => "pad-the-first-segment",
        }
    }
}

impl fmt::Display for Behaviour {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One split file: its evidence and its behaviours.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitConfig {
    pub date: Date,
    pub network: String,
    pub found_by: String,
    pub behaviours: Vec<Behaviour>,
}

impl SplitConfig {
    pub fn new(
        date: Date,
        network: impl Into<String>,
        found_by: impl Into<String>,
        behaviours: Vec<Behaviour>,
    ) -> SplitConfig {
        SplitConfig { date, network: network.into(), found_by: found_by.into(), behaviours }
    }

    /// Read a split file. A file with no date is refused (rule SYN-4).
    pub fn from_toml(text: &str) -> Result<SplitConfig, SplitError> {
        let table = text.parse::<toml::Table>().map_err(|e| SplitError::Config(e.to_string()))?;
        let date = match table.get("date") {
            Some(toml::Value::Datetime(dt)) => {
                Date::parse(&dt.to_string()).map_err(|e| SplitError::Config(e.to_string()))?
            }
            Some(toml::Value::String(s)) if !s.trim().is_empty() => {
                Date::parse(s.trim()).map_err(|e| SplitError::Config(e.to_string()))?
            }
            _ => return Err(SplitError::NoDate),
        };
        let network = text_field(&table, "network").map_err(|e| SplitError::Config(e.to_string()))?;
        let found_by = text_field(&table, "found_by").map_err(|e| SplitError::Config(e.to_string()))?;
        let Some(toml::Value::Array(array)) = table.get("behaviours") else {
            return Err(SplitError::Config("behaviours is not an array".into()));
        };
        let mut behaviours = Vec::with_capacity(array.len());
        for value in array {
            let toml::Value::Table(entry) = value else {
                return Err(SplitError::Config("a behaviour is not a table".into()));
            };
            behaviours.push(parse_behaviour(entry)?);
        }
        if behaviours.is_empty() {
            return Err(SplitError::Config("behaviours is empty".into()));
        }
        Ok(SplitConfig { date, network, found_by, behaviours })
    }
}

fn parse_behaviour(table: &toml::Table) -> Result<Behaviour, SplitError> {
    let kind = text_field(table, "kind").map_err(|e| SplitError::Config(e.to_string()))?;
    let normal = kind
        .chars()
        .filter(|c| *c != '-' && *c != '_' && !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    let number = |key: &str, default: usize| -> Result<usize, SplitError> {
        match table.get(key) {
            None => Ok(default),
            Some(toml::Value::Integer(n)) if *n >= 0 => Ok(*n as usize),
            Some(_) => Err(SplitError::Config(format!("{key} is not a number"))),
        }
    };
    match normal.as_str() {
        "splitat" => Ok(Behaviour::SplitAt { offset: number("offset", 2)? }),
        "disorder" => Ok(Behaviour::Disorder),
        "fakettl" => {
            let ttl = number("ttl", 1)?;
            if !(1..=255).contains(&ttl) {
                return Err(SplitError::Config("ttl is not 1 to 255".into()));
            }
            Ok(Behaviour::FakeTtl { ttl: ttl as u8 })
        }
        "tamperchecksum" => {
            let mask = number("mask", 0x0102)?;
            if !(1..=u16::MAX as usize).contains(&mask) {
                return Err(SplitError::Config("mask is not 1 to 65535".into()));
            }
            Ok(Behaviour::TamperChecksum { mask: mask as u16 })
        }
        "padfirst" => Ok(Behaviour::PadFirst { bytes: number("bytes", 4)? }),
        other => Err(SplitError::Config(format!("unknown behaviour {other:?}"))),
    }
}

/// What one behaviour did to the first outbound data batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitRun {
    pub behaviour: Behaviour,
    /// The batch before the behaviour.
    pub baseline: Vec<Segment>,
    /// The batch after the behaviour.
    pub frames: Vec<Segment>,
    /// The TTL of the first segment, when the behaviour set one.
    pub ttl: Option<u8>,
    /// Whether the first segment still carries a valid checksum.
    pub checksum_valid: bool,
    /// The bytes of padding that the behaviour added.
    pub padding: usize,
    /// The bytes that the server delivered when the batch was carried in
    /// order.
    pub delivered: Vec<u8>,
    /// The state of the server after the delivery.
    pub state: TcpState,
}

impl Simulator {
    /// Apply one behaviour to the first outbound data batch. A path that
    /// defeats it gives its named error.
    pub fn split(&self, behaviour: Behaviour) -> Result<SplitRun, SplitError> {
        let baseline = self.outbound_data();
        let mut frames = baseline.clone();
        let mut ttl = None;
        let mut checksum_valid = true;
        let mut padding = 0;
        match behaviour {
            Behaviour::SplitAt { offset } => {
                let Some(first) = frames.first().cloned() else {
                    return Err(SplitError::NoPayload { behaviour: behaviour.name() });
                };
                if first.payload.len() < 2 {
                    return Err(SplitError::NoPayload { behaviour: behaviour.name() });
                }
                let at = 1 + offset % (first.payload.len() - 1);
                let mut head = first.clone();
                let tail = head.payload.split_off(at);
                let mut second = first;
                second.seq = second.seq.wrapping_add(at as u32);
                second.payload = tail;
                head.seal();
                second.seal();
                frames[0] = head;
                frames.insert(1, second);
            }
            Behaviour::Disorder => {
                if frames.len() < 2 {
                    return Err(SplitError::NoPayload { behaviour: behaviour.name() });
                }
                frames.swap(0, 1);
            }
            Behaviour::FakeTtl { ttl: value } => ttl = Some(value),
            Behaviour::TamperChecksum { mask } => {
                let Some(first) = frames.first_mut() else {
                    return Err(SplitError::NoPayload { behaviour: behaviour.name() });
                };
                first.tamper_checksum(mask);
                checksum_valid = false;
            }
            Behaviour::PadFirst { bytes } => {
                let Some(first) = frames.first_mut() else {
                    return Err(SplitError::NoPayload { behaviour: behaviour.name() });
                };
                if first.payload.is_empty() {
                    return Err(SplitError::NoPayload { behaviour: behaviour.name() });
                }
                first.payload.extend(std::iter::repeat_n(0, bytes));
                first.seal();
                padding = bytes;
                // The bytes after the first segment move by the padding, so
                // the stream stays contiguous.
                for frame in frames.iter_mut().skip(1) {
                    frame.seq = frame.seq.wrapping_add(bytes as u32);
                    frame.seal();
                }
            }
        }
        if self.policy().reassembles
            && matches!(behaviour, Behaviour::SplitAt { .. } | Behaviour::Disorder | Behaviour::PadFirst { .. })
        {
            return Err(SplitError::Reassembled { behaviour: behaviour.name() });
        }
        if self.policy().valid_checksum && !checksum_valid {
            return Err(SplitError::ChecksumRejected { behaviour: behaviour.name() });
        }
        if self.policy().honour_ttl && matches!(behaviour, Behaviour::FakeTtl { ttl } if ttl <= 1) {
            return Err(SplitError::TimeExceeded { behaviour: behaviour.name() });
        }
        let (delivered, state) = self.deliver(&frames);
        Ok(SplitRun { behaviour, baseline, frames, ttl, checksum_valid, padding, delivered, state })
    }

    /// Apply each behaviour of a split file. The first defeat stops the run.
    pub fn split_all(&self, config: &SplitConfig) -> Result<Vec<SplitRun>, SplitError> {
        config.behaviours.iter().map(|behaviour| self.split(*behaviour)).collect()
    }
}
