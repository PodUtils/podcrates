//! The conformance check that a program runs against its own server, from
//! a test: the frame, the version in each answer, the one answer of an
//! unknown method and of a method above the tier, the local methods and the
//! forms of `status` and `metrics`. It calls no method that changes the
//! program: not `reload`, `drain`, `stop` or an extra.
//!
//! ```no_run
//! # fn run(path: &std::path::Path) {
//! use pod_control::{Client, ClientSettings, conformance};
//! let mut connect = || Client::connect_unix(path, ClientSettings::default());
//! let faults = conformance::check(&mut connect, &conformance::Options::default());
//! assert!(faults.is_empty(), "{faults:#?}");
//! # }
//! ```

use serde_json::{Value, json};

use crate::catalogue::{Method, Tier};
use crate::client::{Client, Error};
use crate::host::Metrics;
use crate::wire::{Code, PROTOCOL};

/// What the check needs to know of the server.
#[derive(Debug, Clone)]
pub struct Options {
    /// The longest request line of the server ([`crate::Settings::line_max`]).
    pub line_max: usize,
}

impl Default for Options {
    fn default() -> Options {
        Options { line_max: crate::server::LINE_MAX }
    }
}

/// One fault of the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The name of the check.
    pub check: &'static str,
    pub fault: String,
}

struct Run<'a> {
    connect: &'a mut dyn FnMut() -> Result<Client, Error>,
    found: Vec<Finding>,
}

impl Run<'_> {
    fn fault(&mut self, check: &'static str, fault: impl Into<String>) {
        self.found.push(Finding { check, fault: fault.into() });
    }

    fn client(&mut self, check: &'static str) -> Option<Client> {
        match (self.connect)() {
            Ok(c) => Some(c),
            Err(e) => {
                self.fault(check, format!("no connection: {e}"));
                None
            }
        }
    }

    /// One raw line and its answer, which has `"v": 1` and the id `id`.
    fn exchange(&mut self, c: &mut Client, check: &'static str, line: &[u8], id: Value) -> Option<Value> {
        let answer = match c.raw(line) {
            Ok(Some(a)) => a,
            Ok(None) => {
                self.fault(check, "the server closed with no answer");
                return None;
            }
            Err(e) => {
                self.fault(check, format!("no answer: {e}"));
                return None;
            }
        };
        if answer["v"] != json!(PROTOCOL) {
            self.fault(check, format!("an answer with no \"v\": {PROTOCOL}"));
        }
        if answer["id"] != id {
            self.fault(check, format!("the answer has the id {}, not {id}", answer["id"]));
        }
        Some(answer)
    }

    fn expect_code(&mut self, check: &'static str, line: &[u8], id: Value, code: Code) -> Option<Value> {
        let mut c = self.client(check)?;
        let answer = self.exchange(&mut c, check, line, id)?;
        if answer["ok"] != json!(false) || answer["error"]["code"] != json!(code.name()) {
            self.fault(check, format!("the answer is not the error {code}"));
        }
        Some(answer["error"].clone())
    }
}

/// Check the server that `connect` reaches. Each connection is new; an
/// empty list is a server that conforms.
pub fn check(connect: &mut dyn FnMut() -> Result<Client, Error>, options: &Options) -> Vec<Finding> {
    let mut run = Run { connect, found: Vec::new() };
    local(&mut run);
    let unknown = run.expect_code(
        "unknown-method",
        br#"{"v":1,"id":1,"method":"no-such-method"}"#,
        1.into(),
        Code::UnknownMethod,
    );
    hidden(&mut run, unknown);
    frame(&mut run, options);
    delegated(&mut run);
    run.found
}

fn local(run: &mut Run<'_>) {
    let Some(mut c) = run.client("version") else { return };
    match c.version() {
        Ok(v) if v.program.is_empty() => run.fault("version", "no name of the program"),
        Ok(_) => {}
        Err(e) => run.fault("version", e.to_string()),
    }
    if let Some(a) = run.exchange(&mut c, "version", br#"{"v":1,"id":41,"method":"version"}"#, 41.into())
        && a["ok"] != json!(true)
    {
        run.fault("version", "a raw version request has no result");
    }
    if let Some(a) =
        run.exchange(&mut c, "params", br#"{"v":1,"id":42,"method":"version","params":{"x":1}}"#, 42.into())
        && a["error"]["code"] != json!(Code::BadParams.name())
    {
        run.fault("params", "a field that the method does not take is not bad-params");
    }
    match c.methods() {
        Err(e) => run.fault("methods", e.to_string()),
        Ok(rows) => {
            for m in Method::ALL.map(Method::spec).iter().filter(|s| s.tier == Tier::Read) {
                if !rows.iter().any(|r| r.name == m.name) {
                    run.fault("methods", format!("the read method {} is not listed", m.name));
                }
            }
            for r in &rows {
                if let Some(spec) = Method::from_name(&r.name).map(Method::spec)
                    && (r.tier != spec.tier.name() || r.route != spec.route.name())
                {
                    run.fault("methods", format!("{} has another tier or route than the catalogue", r.name));
                }
                if rows.iter().filter(|o| o.name == r.name).count() > 1 {
                    run.fault("methods", format!("{} is listed twice", r.name));
                }
            }
        }
    }
}

/// Each method of the catalogue that `methods` does not list answers as a
/// name that is not there, word for word.
fn hidden(run: &mut Run<'_>, unknown: Option<Value>) {
    let Some(unknown) = unknown else { return };
    let Some(mut c) = run.client("hidden") else { return };
    let rows = match c.methods() {
        Ok(rows) => rows,
        Err(e) => return run.fault("hidden", e.to_string()),
    };
    for spec in Method::ALL.map(Method::spec) {
        if rows.iter().any(|r| r.name == spec.name) {
            continue;
        }
        let line = format!(r#"{{"v":1,"id":5,"method":"{}","params":{{"bogus":1}}}}"#, spec.name);
        let Some(a) = run.exchange(&mut c, "hidden", line.as_bytes(), 5.into()) else { return };
        if a["error"] != unknown {
            run.fault("hidden", format!("{} above the tier does not answer as a name that is not there", spec.name));
        }
    }
}

fn frame(run: &mut Run<'_>, options: &Options) {
    run.expect_code("not-json", b"{not json", Value::Null, Code::BadRequest);
    run.expect_code("no-version", br#"{"id":2,"method":"version"}"#, 2.into(), Code::BadRequest);
    run.expect_code("other-version", br#"{"v":2,"id":3,"method":"version"}"#, 3.into(), Code::BadRequest);
    run.expect_code("no-id", br#"{"v":1,"method":"version"}"#, Value::Null, Code::BadRequest);
    run.expect_code("unknown-field", br#"{"v":1,"id":4,"method":"version","x":1}"#, 4.into(), Code::BadRequest);
    // A line that is not UTF-8 keeps the frame: the next request answers.
    if let Some(mut c) = run.client("not-utf8") {
        if let Some(a) = run.exchange(&mut c, "not-utf8", b"\xff\xfe", Value::Null)
            && a["error"]["code"] != json!(Code::NotUtf8.name())
        {
            run.fault("not-utf8", "a line that is not UTF-8 is not not-utf8");
        }
        if let Err(e) = c.version() {
            run.fault("not-utf8", format!("the connection does not go on: {e}"));
        }
    }
    // A line one byte over the cap is too long, and the server closes.
    if let Some(mut c) = run.client("too-long") {
        let line = vec![b' '; options.line_max + 1];
        if let Some(a) = run.exchange(&mut c, "too-long", &line, Value::Null)
            && a["error"]["code"] != json!(Code::TooLong.name())
        {
            run.fault("too-long", "a line over the cap is not too-long");
        }
        if c.version().is_ok() {
            run.fault("too-long", "the connection goes on after a line over the cap");
        }
    }
}

fn delegated(run: &mut Run<'_>) {
    let Some(mut c) = run.client("status") else { return };
    match c.status() {
        Ok(v) if !v.is_object() => run.fault("status", "the status is not a JSON object"),
        Ok(_) | Err(Error::Remote(_)) => {}
        Err(e) => run.fault("status", e.to_string()),
    }
    match c.call("metrics", json!({})) {
        Ok(v) => {
            if serde_json::from_value::<Metrics>(v).is_err() {
                run.fault("metrics", "the metrics are not of the form counters and gauges");
            }
        }
        Err(Error::Remote(_)) => {}
        Err(e) => run.fault("metrics", e.to_string()),
    }
}
