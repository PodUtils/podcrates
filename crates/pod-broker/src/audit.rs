//! The audit: one JSON line for each connection and each change of a job,
//! in the form of `schemas/log.v1.schema.json` (`schema`, `time`, `level`,
//! `event`, `fields`, and `stage` and `class` for a failure). A line holds the job
//! id, the host, the port, the address, the decision, the rule, the bytes
//! and the time. It never holds a secret value: no token, no header, no
//! path and no query (a path or a query can carry a credential).

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use pod_core::{Class, Stage};
use serde::Serialize;
use serde_json::{Map, Value};

/// One audit line. `schemas/log.v1.schema.json` comes from this type: it
/// is the one writer of JSON log lines in podcrates.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[cfg_attr(
    any(test, feature = "schema"),
    schemars(
        title = "One JSON log line of a tool of PodUtils",
        description = "One object on each line. The classes and their exit codes are the table of \
        crates/pod-core/README.md.",
        deny_unknown_fields
    )
)]
pub struct Line {
    /// The version of the log schema ([`pod_core::LOG_SCHEMA`]), the first
    /// key.
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "crate::json_schema::log_version"))]
    pub schema: u32,
    /// RFC 3339, UTC.
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "crate::json_schema::time"))]
    pub time: String,
    /// One of [`pod_core::LOG_LEVELS`].
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "crate::json_schema::level"))]
    pub level: &'static str,
    /// A dotted name: `broker.connection`, `broker.job.load`,
    /// `broker.job.drop`.
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "crate::json_schema::event"))]
    pub event: &'static str,
    /// The values of the event, never a secret: an object, where a missing
    /// key reads as `null`.
    #[cfg_attr(any(test, feature = "schema"), schemars(schema_with = "crate::json_schema::fields"))]
    pub fields: Value,
    /// The name of the `pod_core::Stage` of a failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<&'static str>,
    /// The name of the `pod_core::Class` of a failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(any(test, feature = "schema"), schemars(with = "Option<crate::json_schema::ClassName>"))]
    pub class: Option<&'static str>,
}

impl Line {
    pub(crate) fn new(level: &'static str, event: &'static str) -> Line {
        Line {
            schema: pod_core::LOG_SCHEMA,
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

/// The count of each kind of audit line, for the `metrics` of the control.
pub(crate) type Counts = Arc<Mutex<BTreeMap<String, u64>>>;

/// The sink of the host, and a count of each line on its way there.
pub(crate) struct Counted {
    pub inner: Arc<dyn Audit>,
    pub counts: Counts,
}

impl Audit for Counted {
    fn line(&self, line: &Line) {
        let mut counts = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        *counts.entry(count_name(line)).or_default() += 1;
        drop(counts);
        self.inner.line(line);
    }
}

/// The name of the count of a line: the event without `broker.`, with `_`
/// for `.`, then the decision of a connection or `failed` for a failure:
/// `connection_allow`, `connection_deny`, `job_load`, `job_load_failed`.
fn count_name(line: &Line) -> String {
    let mut name = line.event.strip_prefix("broker.").unwrap_or(line.event).replace('.', "_");
    if let Some(decision) = line.fields["decision"].as_str() {
        name.push('_');
        name.push_str(decision);
    } else if line.class.is_some() {
        name.push_str("_failed");
    }
    name
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
    fn each_line_is_counted_by_its_event_and_decision() {
        let counts = Counts::default();
        let sink = Counted { inner: Arc::new(Memory::new()), counts: counts.clone() };
        let mut allow = Line::new("info", "broker.connection");
        allow.set("decision", "allow");
        let mut failed = Line::new("warn", "broker.job.load");
        failed.failed(Stage::Proxy, Class::Usage);
        for line in [&allow, &allow, &failed, &Line::new("info", "broker.job.drop")] {
            sink.line(line);
        }
        let counts = counts.lock().unwrap().clone();
        let want = [("connection_allow", 2), ("job_drop", 1), ("job_load_failed", 1)];
        assert_eq!(counts, want.map(|(n, c)| (n.to_string(), c)).into_iter().collect());
    }

    #[test]
    fn the_time_is_rfc_3339_in_utc() {
        let at = UNIX_EPOCH + std::time::Duration::from_millis(1_794_268_800_123);
        assert_eq!(rfc3339(at), "2026-11-10T00:00:00.123Z");
    }
}
