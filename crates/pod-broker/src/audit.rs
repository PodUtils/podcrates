//! The audit: one JSON line for each connection and each change of a job,
//! in the form of `schemas/log.v1.schema.json` (`time`, `level`, `event`,
//! `fields`, and `stage` and `class` for a failure). A line holds the job
//! id, the host, the port, the address, the decision, the rule, the bytes
//! and the time. It never holds a secret value: no token, no header, no
//! path and no query (a path or a query can carry a credential).

use std::io::Write;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use pod_core::{Class, Stage};
use serde::Serialize;
use serde_json::{Map, Value};

/// One audit line.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Line {
    /// RFC 3339, UTC.
    pub time: String,
    pub level: &'static str,
    /// `broker.connection`, `broker.job.load`, `broker.job.drop`.
    pub event: &'static str,
    /// An object: a missing key reads as `null`.
    pub fields: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<&'static str>,
}

impl Line {
    pub(crate) fn new(level: &'static str, event: &'static str) -> Line {
        Line {
            time: rfc3339(SystemTime::now()),
            level,
            event,
            fields: Value::Object(Map::new()),
            stage: None,
            class: None,
        }
    }

    pub(crate) fn set(&mut self, key: &str, value: impl Into<Value>) -> &mut Line {
        if let Value::Object(fields) = &mut self.fields {
            fields.insert(key.to_string(), value.into());
        }
        self
    }

    pub(crate) fn failed(&mut self, stage: Stage, class: Class) -> &mut Line {
        self.stage = Some(stage.name());
        self.class = Some(class.name());
        self
    }

    /// The line as one line of JSON, with no line break.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| String::from("{}"))
    }
}

/// Where the audit lines go. The consumer gives one; the default is
/// [`Tracing`].
pub trait Audit: Send + Sync {
    fn line(&self, line: &Line);
}

/// Each line as an event of `tracing` (target `pod_broker::audit`, the line
/// in the field `line`): `info` for an allowed connection, `warn` for a
/// refusal.
#[derive(Debug, Default, Clone, Copy)]
pub struct Tracing;

impl Audit for Tracing {
    fn line(&self, line: &Line) {
        let json = line.to_json();
        if line.level == "warn" {
            tracing::warn!(target: "pod_broker::audit", line = %json);
        } else {
            tracing::info!(target: "pod_broker::audit", line = %json);
        }
    }
}

/// Each line, and a line break, to a writer (a file, stderr, a pipe). A
/// write that fails goes to `tracing`; the connection goes on.
pub struct JsonLines<W: Write + Send> {
    out: Mutex<W>,
}

impl<W: Write + Send> JsonLines<W> {
    pub fn new(out: W) -> JsonLines<W> {
        JsonLines { out: Mutex::new(out) }
    }

    pub fn into_inner(self) -> W {
        self.out.into_inner().unwrap_or_else(|e| e.into_inner())
    }
}

impl<W: Write + Send> Audit for JsonLines<W> {
    fn line(&self, line: &Line) {
        let mut text = line.to_json();
        text.push('\n');
        let mut out = self.out.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
            tracing::warn!(error = %e, "an audit line was not written");
        }
    }
}

/// The lines in memory, for a test or for a host that sends them on.
#[derive(Debug, Default)]
pub struct Memory {
    lines: Mutex<Vec<Line>>,
}

impl Memory {
    pub fn new() -> Memory {
        Memory::default()
    }

    /// Each line so far.
    pub fn lines(&self) -> Vec<Line> {
        self.lines.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl Audit for Memory {
    fn line(&self, line: &Line) {
        self.lines.lock().unwrap_or_else(|e| e.into_inner()).push(line.clone());
    }
}

/// `now` in RFC 3339, UTC, to the millisecond.
pub fn rfc3339(now: SystemTime) -> String {
    let since = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs() as i64;
    let (y, m, d) = pod_core::clock::civil_from_days(secs.div_euclid(86_400));
    let rest = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        since.subsec_millis()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_time_is_rfc_3339_in_utc() {
        let at = UNIX_EPOCH + std::time::Duration::from_millis(1_794_268_800_123);
        assert_eq!(rfc3339(at), "2026-11-10T00:00:00.123Z");
    }
}
