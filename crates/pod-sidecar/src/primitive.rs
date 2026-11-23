//! The five primitives, the trigger of a step, and the strategy file.
//!
//! A strategy is an ordered list of steps (rule SYN-2). Each step carries a
//! trigger and a primitive. A strategy file is a TOML table with `date`,
//! `network`, `found_by` and `steps` (rule SYN-4), and a file with no date is
//! refused at load.

use std::fmt;

use crate::error::StrategyError;

/// A calendar date, `YYYY-MM-DD`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl Date {
    /// Read `YYYY-MM-DD`. A date that is not a day of the calendar is
    /// refused, so a typo cannot become evidence.
    pub fn parse(text: &str) -> Result<Date, StrategyError> {
        let bad = || StrategyError::BadDate(text.to_string());
        if text.len() != 10 {
            return Err(bad());
        }
        let bytes = text.as_bytes();
        if bytes[4] != b'-' || bytes[7] != b'-' {
            return Err(bad());
        }
        let digits = |range: std::ops::Range<usize>| -> Result<u32, StrategyError> {
            let part = &text[range];
            if !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad());
            }
            part.parse::<u32>().map_err(|_| bad())
        };
        let year = digits(0..4)?;
        let month = digits(5..7)?;
        let day = digits(8..10)?;
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
            2 => 28,
            _ => return Err(bad()),
        };
        if day == 0 || day > days || year == 0 {
            return Err(bad());
        }
        Ok(Date { year: year as u16, month: month as u8, day: day as u8 })
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// One of the five primitives of rule SYN-2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Primitive {
    /// The segment never arrives.
    Drop,
    /// One payload byte changes. A middlebox that repairs the checksum
    /// carries the change; a path that requires a valid checksum drops it.
    Tamper,
    /// The segment arrives more than once.
    Duplicate,
    /// The payload becomes two segments at an offset.
    Fragment,
    /// The segment and the next contiguous one become one segment.
    Coalesce,
}

impl Primitive {
    pub const ALL: [Primitive; 5] =
        [Primitive::Drop, Primitive::Tamper, Primitive::Duplicate, Primitive::Fragment, Primitive::Coalesce];

    pub fn as_str(self) -> &'static str {
        match self {
            Primitive::Drop => "drop",
            Primitive::Tamper => "tamper",
            Primitive::Duplicate => "duplicate",
            Primitive::Fragment => "fragment",
            Primitive::Coalesce => "coalesce",
        }
    }

    pub fn parse(text: &str) -> Result<Primitive, StrategyError> {
        match normalise(text).as_str() {
            "drop" => Ok(Primitive::Drop),
            "tamper" => Ok(Primitive::Tamper),
            "duplicate" => Ok(Primitive::Duplicate),
            "fragment" => Ok(Primitive::Fragment),
            "coalesce" => Ok(Primitive::Coalesce),
            _ => Err(StrategyError::UnknownPrimitive(text.to_string())),
        }
    }
}

impl fmt::Display for Primitive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The event that fires a step (rule SYN-2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Trigger {
    /// The first segment out of the protected host that carries payload.
    FirstOutboundData,
    /// The SYN-ACK of the server, inbound.
    SynAck,
    /// The first RST of the server, inbound.
    FirstInboundRst,
    /// Every outbound segment.
    Always,
}

impl Trigger {
    pub const ALL: [Trigger; 4] =
        [Trigger::FirstOutboundData, Trigger::SynAck, Trigger::FirstInboundRst, Trigger::Always];

    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::FirstOutboundData => "FirstOutboundData",
            Trigger::SynAck => "SynAck",
            Trigger::FirstInboundRst => "FirstInboundRst",
            Trigger::Always => "Always",
        }
    }

    pub fn parse(text: &str) -> Result<Trigger, StrategyError> {
        match normalise(text).as_str() {
            "firstoutbounddata" => Ok(Trigger::FirstOutboundData),
            "synack" => Ok(Trigger::SynAck),
            "firstinboundrst" => Ok(Trigger::FirstInboundRst),
            "always" => Ok(Trigger::Always),
            _ => Err(StrategyError::UnknownTrigger(text.to_string())),
        }
    }
}

impl fmt::Display for Trigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The numbers of a step. Each primitive reads the ones it needs, and a
/// value outside its range is refused at load.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Params {
    /// The offset of a tamper in the payload, or the split point of a
    /// fragment.
    pub offset: Option<usize>,
    /// The mask of a tamper, by exclusive or.
    pub byte: Option<u8>,
    /// The extra copies of a duplicate, from 1 to 8.
    pub count: Option<u32>,
}

/// One step of a strategy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub trigger: Trigger,
    pub primitive: Primitive,
    pub params: Params,
}

impl Step {
    pub fn new(trigger: Trigger, primitive: Primitive) -> Step {
        Step { trigger, primitive, params: Params::default() }
    }

    pub fn with_offset(mut self, offset: usize) -> Step {
        self.params.offset = Some(offset);
        self
    }

    pub fn with_byte(mut self, byte: u8) -> Step {
        self.params.byte = Some(byte);
        self
    }

    pub fn with_count(mut self, count: u32) -> Step {
        self.params.count = Some(count);
        self
    }
}

/// A strategy: its evidence and its ordered steps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Strategy {
    pub date: Date,
    pub network: String,
    pub found_by: String,
    pub steps: Vec<Step>,
}

impl Strategy {
    pub fn new(date: Date, network: impl Into<String>, found_by: impl Into<String>, steps: Vec<Step>) -> Strategy {
        Strategy { date, network: network.into(), found_by: found_by.into(), steps }
    }

    /// Read a strategy file. A file with no date is refused (rule SYN-4).
    pub fn from_toml(text: &str) -> Result<Strategy, StrategyError> {
        let table = text.parse::<toml::Table>().map_err(|e| StrategyError::Toml(e.to_string()))?;
        let date = match table.get("date") {
            Some(toml::Value::Datetime(dt)) => Date::parse(&dt.to_string())?,
            Some(toml::Value::String(s)) if !s.trim().is_empty() => Date::parse(s.trim())?,
            _ => return Err(StrategyError::NoDate),
        };
        let network = text_field(&table, "network")?;
        let found_by = text_field(&table, "found_by")?;
        let array = match table.get("steps") {
            Some(toml::Value::Array(a)) => a,
            Some(_) => return Err(StrategyError::Toml("steps is not an array".into())),
            None => return Err(StrategyError::NoSteps),
        };
        let mut steps = Vec::with_capacity(array.len());
        for value in array {
            let toml::Value::Table(step) = value else {
                return Err(StrategyError::Toml("a step is not a table".into()));
            };
            let trigger = Trigger::parse(&text_field(step, "trigger")?)?;
            let primitive = Primitive::parse(&text_field(step, "primitive")?)?;
            let params = Params { offset: number(step, "offset")?, byte: byte(step, "byte")?, count: count(step)? };
            steps.push(Step { trigger, primitive, params });
        }
        if steps.is_empty() {
            return Err(StrategyError::NoSteps);
        }
        Ok(Strategy { date, network, found_by, steps })
    }
}

/// A text field of a table, refused when it is absent or empty.
pub(crate) fn text_field(table: &toml::Table, key: &'static str) -> Result<String, StrategyError> {
    match table.get(key) {
        Some(toml::Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
        _ => Err(StrategyError::Missing(key)),
    }
}

fn number(table: &toml::Table, key: &str) -> Result<Option<usize>, StrategyError> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::Integer(n)) if *n >= 0 => Ok(Some(*n as usize)),
        Some(toml::Value::Integer(_)) => Err(StrategyError::Toml(format!("{key} is negative"))),
        Some(_) => Err(StrategyError::Toml(format!("{key} is not a number"))),
    }
}

fn byte(table: &toml::Table, key: &str) -> Result<Option<u8>, StrategyError> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::Integer(n)) if (0..=255).contains(n) => Ok(Some(*n as u8)),
        Some(_) => Err(StrategyError::Toml(format!("{key} is not a byte"))),
    }
}

fn count(table: &toml::Table) -> Result<Option<u32>, StrategyError> {
    match table.get("count") {
        None => Ok(None),
        Some(toml::Value::Integer(n)) if (1..=8).contains(n) => Ok(Some(*n as u32)),
        Some(_) => Err(StrategyError::Toml("count is not 1 to 8".into())),
    }
}

/// A trigger or a primitive as one word, lower case, with no separator.
fn normalise(text: &str) -> String {
    text.chars().filter(|c| *c != '-' && *c != '_' && !c.is_whitespace()).flat_map(char::to_lowercase).collect()
}
