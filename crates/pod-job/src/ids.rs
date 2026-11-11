//! The id of a job (from the source) and the id of a unit (from the
//! runner). A job id is data from a forge: it is never a path component,
//! and a unit id never derives from it.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Deserializer, Serialize};

/// The longest job id that a source may give.
pub const JOB_ID_MAX: usize = 128;

/// A refused id, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdError(pub String);

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for IdError {}

/// The pattern of a job id in a JSON Schema: 1 to 128 characters of
/// `[A-Za-z0-9._-]`, and not `.` or `..`. The three branches start with a
/// character that is not a dot, with one dot, and with two dots and more.
#[cfg(any(test, feature = "schema"))]
const JOB_ID_PATTERN: &str =
    "^([A-Za-z0-9_-][A-Za-z0-9._-]{0,127}|\\.[A-Za-z0-9_-][A-Za-z0-9._-]{0,126}|\\.\\.[A-Za-z0-9._-]{1,126})$";

/// A job id: `[A-Za-z0-9._-]{1,128}`, and not `.` or `..`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct JobId(String);

impl JobId {
    pub fn new(s: &str) -> Result<JobId, IdError> {
        if s.is_empty() || s.len() > JOB_ID_MAX {
            return Err(IdError(format!("a job id has 1 to {JOB_ID_MAX} characters, not {}", s.len())));
        }
        if let Some(c) = s.chars().find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))) {
            return Err(IdError(format!("a job id has only A-Z, a-z, 0-9, '.', '_' and '-', not {c:?}")));
        }
        // Each character is allowed, but these two name directories.
        if s == "." || s == ".." {
            return Err(IdError(format!("a job id is not {s:?}")));
        }
        Ok(JobId(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for JobId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<JobId, D::Error> {
        let s = String::deserialize(d)?;
        JobId::new(&s).map_err(serde::de::Error::custom)
    }
}

/// A unit id: `u-` and a decimal counter. Only [`UnitIds`] makes one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct UnitId(String);

impl UnitId {
    /// Read a unit id of the journal or of a wire.
    pub fn parse(s: &str) -> Result<UnitId, IdError> {
        match s.strip_prefix("u-") {
            Some(n) if !n.is_empty() && n.len() <= 20 && n.bytes().all(|b| b.is_ascii_digit()) => Ok(UnitId(s.into())),
            _ => Err(IdError(format!("a unit id is u- and a decimal number, not {s:?}"))),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UnitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for UnitId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<UnitId, D::Error> {
        let s = String::deserialize(d)?;
        UnitId::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// The counter of unit ids of one process. The journal gives the first
/// value after a restart, so that no id comes twice.
#[derive(Debug, Default)]
pub struct UnitIds(AtomicU64);

impl UnitIds {
    pub fn starting_at(n: u64) -> UnitIds {
        UnitIds(AtomicU64::new(n))
    }

    pub fn next(&self) -> UnitId {
        UnitId(format!("u-{}", self.0.fetch_add(1, Ordering::Relaxed)))
    }
}

/// The JSON Schema of the ids, with their patterns. A pattern has no
/// look-around: Rust's regex and Go's RE2 do not take one, and a validator
/// on them must read each schema of `schemas/`.
#[cfg(any(test, feature = "schema"))]
mod schema {
    use std::borrow::Cow;

    use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};

    impl JsonSchema for super::JobId {
        fn schema_name() -> Cow<'static, str> {
            "JobId".into()
        }
        fn json_schema(_: &mut SchemaGenerator) -> Schema {
            json_schema!({ "type": "string", "pattern": super::JOB_ID_PATTERN })
        }
    }

    impl JsonSchema for super::UnitId {
        fn schema_name() -> Cow<'static, str> {
            "UnitId".into()
        }
        fn json_schema(_: &mut SchemaGenerator) -> Schema {
            json_schema!({ "type": "string", "pattern": "^u-[0-9]{1,20}$" })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_id_is_never_a_path() {
        for bad in ["", "..", ".", "../x", "a/b", "a\\b", "a b", "\u{e9}", "a\0"] {
            assert!(JobId::new(bad).is_err(), "{bad:?}");
        }
        assert!(JobId::new(&"a".repeat(129)).is_err());
        for good in ["a", "1234", "job.1_x-2", "...", &"a".repeat(128)] {
            assert!(JobId::new(good).is_ok(), "{good:?}");
        }
    }

    #[test]
    fn a_job_id_is_checked_when_read() {
        assert!(serde_json::from_str::<JobId>("\"../x\"").is_err());
        assert_eq!(serde_json::from_str::<JobId>("\"j-1\"").unwrap().as_str(), "j-1");
    }

    #[test]
    fn unit_ids_count_and_parse() {
        let ids = UnitIds::starting_at(7);
        assert_eq!(ids.next().as_str(), "u-7");
        assert_eq!(ids.next().as_str(), "u-8");
        assert!(UnitId::parse("u-12").is_ok());
        for bad in ["u-", "u-x", "x-1", "u-1/2", "job-1"] {
            assert!(UnitId::parse(bad).is_err(), "{bad:?}");
        }
    }
}
