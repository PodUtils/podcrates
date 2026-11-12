//! The named counters of `nft -j list counters`, read from the guest's
//! output. The text is untrusted: a size limit, and each counter checked.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::Value;

/// The most bytes of the text of one read.
pub const MAX_TEXT: usize = 1 << 20;

/// One counter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Count {
    pub packets: u64,
    pub bytes: u64,
}

/// The counters of one table, by name.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Counters {
    map: BTreeMap<String, Count>,
}

impl Counters {
    pub fn get(&self, name: &str) -> Option<Count> {
        self.map.get(name).copied()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.map.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Set a counter (for a consumer that reads counters another way, and
    /// for tests).
    pub fn insert(&mut self, name: &str, count: Count) {
        self.map.insert(name.to_string(), count);
    }
}

/// A text that is not the output of `nft -j list counters`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CounterError {
    /// More than `MAX_TEXT` bytes.
    TooLong(usize),
    /// Not JSON; the text of the parser's error.
    NotJson(String),
    /// No array `nftables` at the top.
    NoList,
    /// An item of the array that is not an object of one key, or a counter
    /// with a missing or wrong field.
    BadItem { index: usize, why: &'static str },
    /// A counter of the table that comes two times.
    Duplicate(String),
}

impl fmt::Display for CounterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CounterError::TooLong(n) => write!(f, "the counters are {n} bytes, more than {MAX_TEXT}"),
            CounterError::NotJson(e) => write!(f, "the counters are not JSON: {e}"),
            CounterError::NoList => f.write_str("the counters have no array \"nftables\""),
            CounterError::BadItem { index, why } => write!(f, "item {index} of the counters: {why}"),
            CounterError::Duplicate(name) => write!(f, "the counter {name:?} comes two times"),
        }
    }
}

impl std::error::Error for CounterError {}

impl From<CounterError> for pod_core::Failure {
    fn from(e: CounterError) -> pod_core::Failure {
        pod_core::Failure::caused(pod_core::Stage::Host, pod_core::Class::Protocol, e.to_string(), e)
    }
}

fn field<'a>(object: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a Value> {
    object.get(key)
}

/// The counters of the `inet` table `table` in the output of
/// `nft -j list counters`. The items of other tables and other kinds
/// (`metainfo`) are not read.
pub fn parse_counters(text: &str, table: &str) -> Result<Counters, CounterError> {
    if text.len() > MAX_TEXT {
        return Err(CounterError::TooLong(text.len()));
    }
    let value: Value = serde_json::from_str(text).map_err(|e| CounterError::NotJson(e.to_string()))?;
    let items = value.get("nftables").and_then(Value::as_array).ok_or(CounterError::NoList)?;
    let mut out = Counters::default();
    for (index, item) in items.iter().enumerate() {
        let bad = |why| CounterError::BadItem { index, why };
        let object = item.as_object().filter(|o| o.len() == 1).ok_or(bad("not an object of one key"))?;
        let Some(counter) = object.get("counter") else { continue };
        let counter = counter.as_object().ok_or(bad("the counter is not an object"))?;
        let text_of = |key| field(counter, key).and_then(Value::as_str);
        let number_of = |key| field(counter, key).and_then(Value::as_u64);
        let family = text_of("family").ok_or(bad("the counter has no family"))?;
        let owner = text_of("table").ok_or(bad("the counter has no table"))?;
        let name = text_of("name").ok_or(bad("the counter has no name"))?;
        let packets = number_of("packets").ok_or(bad("the counter has no packets"))?;
        let bytes = number_of("bytes").ok_or(bad("the counter has no bytes"))?;
        if family != "inet" || owner != table {
            continue;
        }
        if out.map.insert(name.to_string(), Count { packets, bytes }).is_some() {
            return Err(CounterError::Duplicate(name.to_string()));
        }
    }
    Ok(out)
}
