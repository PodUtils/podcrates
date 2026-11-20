//! The lines of the wire, version [`PROTOCOL`]. Each line is one JSON
//! object with `"v": 1`:
//!
//! ```text
//! {"v":1,"id":7,"method":"drain","params":{"exit":true}}       a request
//! {"v":1,"id":7,"ok":true,"result":{"draining":true}}          its answer
//! {"v":1,"id":7,"ok":false,"error":{"code":"busy","detail":"..."}}
//! {"v":1,"id":1,"for":7,"method":"confirm","params":{}}        a call back of the server
//! {"v":1,"id":1,"ok":true,"result":true}                       its answer by the client
//! ```
//!
//! A request has `method`; an answer has `ok`. Each side numbers its own
//! requests, and an answer has the id of its request (`null` when the line
//! had no id that can be read). The `detail` of an error holds no byte of
//! the request: a request can hold a token.

use std::fmt;

use serde_json::{Map, Value};

/// The version of the lines.
pub const PROTOCOL: u32 = 1;

/// The code of an error on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Code {
    /// The line is not a request of the protocol.
    BadRequest,
    /// The line is not UTF-8.
    NotUtf8,
    /// The line is longer than the cap; the connection closes.
    TooLong,
    /// No such method, or a method above the tier of the caller: the two
    /// give the same answer.
    UnknownMethod,
    /// The params do not fit the method.
    BadParams,
    /// The peer or the action is refused.
    Refused,
    /// The program does not answer now; a later try can work.
    Busy,
    /// The calls back of a request or of a connection are spent.
    Budget,
    /// A wait ended at its time limit.
    Timeout,
    /// The program does not do this.
    Unsupported,
    /// The program tried the action, and it failed.
    Failed,
    /// A fault of the program.
    Internal,
}

impl Code {
    pub const ALL: [Code; 12] = [
        Code::BadRequest,
        Code::NotUtf8,
        Code::TooLong,
        Code::UnknownMethod,
        Code::BadParams,
        Code::Refused,
        Code::Busy,
        Code::Budget,
        Code::Timeout,
        Code::Unsupported,
        Code::Failed,
        Code::Internal,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Code::BadRequest => "bad-request",
            Code::NotUtf8 => "not-utf8",
            Code::TooLong => "too-long",
            Code::UnknownMethod => "unknown-method",
            Code::BadParams => "bad-params",
            Code::Refused => "refused",
            Code::Busy => "busy",
            Code::Budget => "budget",
            Code::Timeout => "timeout",
            Code::Unsupported => "unsupported",
            Code::Failed => "failed",
            Code::Internal => "internal",
        }
    }

    pub fn from_name(name: &str) -> Option<Code> {
        Code::ALL.into_iter().find(|c| c.name() == name)
    }

    /// The class of the failure, for the exit code of a client.
    pub fn class(self) -> pod_core::Class {
        use pod_core::Class;
        match self {
            Code::BadRequest | Code::NotUtf8 | Code::TooLong | Code::BadParams => Class::Usage,
            Code::UnknownMethod | Code::Unsupported => Class::Unsupported,
            Code::Refused => Class::Refused,
            Code::Busy => Class::TempFail,
            Code::Budget => Class::Policy,
            Code::Timeout => Class::Timeout,
            Code::Failed => Class::Unavailable,
            Code::Internal => Class::Protocol,
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The error of an answer: a code and words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    pub code: Code,
    pub detail: String,
}

impl Fault {
    pub fn new(code: Code, detail: impl Into<String>) -> Fault {
        Fault { code, detail: detail.into() }
    }

    /// The one answer to a name that is not there and to a method above the
    /// tier of the caller.
    pub fn unknown_method() -> Fault {
        Fault::new(Code::UnknownMethod, "no such method")
    }

    fn to_value(&self) -> Value {
        serde_json::json!({ "code": self.code.name(), "detail": self.detail })
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for Fault {}

/// A request that was read.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub id: u64,
    pub method: String,
    /// An object; `{}` when the line had none.
    pub params: Value,
    /// The id of the request that a call back serves.
    pub for_id: Option<u64>,
}

/// A line that a client or a server reads.
#[derive(Debug, Clone, PartialEq)]
pub enum Inbound {
    Request(Request),
    Answer { id: Option<u64>, outcome: Result<Value, Fault> },
}

fn bad(id: Option<u64>, detail: &str) -> (Option<u64>, Fault) {
    (id, Fault::new(Code::BadRequest, detail))
}

fn take_object(line: &str) -> Result<Map<String, Value>, (Option<u64>, Fault)> {
    let value: Value = serde_json::from_str(line).map_err(|_| bad(None, "the line is not JSON"))?;
    let Value::Object(map) = value else { return Err(bad(None, "the line is not a JSON object")) };
    let id = map.get("id").and_then(Value::as_u64);
    if map.get("v").and_then(Value::as_u64) != Some(u64::from(PROTOCOL)) {
        return Err(bad(id, "the line has no \"v\": 1"));
    }
    Ok(map)
}

/// Read one line. An error names the id where the line has one that can be
/// read, and never holds a byte of the line.
pub fn parse(line: &str) -> Result<Inbound, (Option<u64>, Fault)> {
    let map = take_object(line)?;
    let id = map.get("id").and_then(Value::as_u64);
    if map.contains_key("method") {
        let known = ["v", "id", "method", "params", "for"];
        if map.keys().any(|k| !known.contains(&k.as_str())) {
            return Err(bad(id, "a request has an unknown field"));
        }
        let Some(id) = id else { return Err(bad(None, "a request has no id of 0 or more")) };
        let Some(method) = map.get("method").and_then(Value::as_str) else {
            return Err(bad(Some(id), "the method is not a string"));
        };
        let params = match map.get("params") {
            None => Value::Object(Map::new()),
            Some(p @ Value::Object(_)) => p.clone(),
            Some(_) => return Err(bad(Some(id), "the params are not an object")),
        };
        let for_id = match map.get("for") {
            None => None,
            Some(v) => Some(v.as_u64().ok_or_else(|| bad(Some(id), "the field for is not an id"))?),
        };
        return Ok(Inbound::Request(Request { id, method: method.to_string(), params, for_id }));
    }
    let known = ["v", "id", "ok", "result", "error"];
    if map.keys().any(|k| !known.contains(&k.as_str())) {
        return Err(bad(id, "an answer has an unknown field"));
    }
    if !(map.get("id").is_some_and(Value::is_null) || id.is_some()) {
        return Err(bad(None, "an answer has no id and no null"));
    }
    let outcome = match (map.get("ok"), map.get("result"), map.get("error")) {
        (Some(Value::Bool(true)), Some(r), None) => Ok(r.clone()),
        (Some(Value::Bool(false)), None, Some(e)) => Err(fault_of(e).ok_or_else(|| bad(id, "the error has no code"))?),
        _ => return Err(bad(id, "an answer has ok and one of result and error")),
    };
    Ok(Inbound::Answer { id, outcome })
}

fn fault_of(e: &Value) -> Option<Fault> {
    let map = e.as_object()?;
    if map.keys().any(|k| k != "code" && k != "detail") {
        return None;
    }
    let code = Code::from_name(map.get("code")?.as_str()?)?;
    Some(Fault::new(code, map.get("detail")?.as_str()?))
}

fn id_text(id: Option<u64>) -> String {
    id.map_or_else(|| "null".to_string(), |i| i.to_string())
}

/// The line of a request, with its line end. `v` comes first, so that a
/// reader of a log finds it.
pub fn request_line(id: u64, method: &str, params: &Value, for_id: Option<u64>) -> String {
    let for_text = for_id.map(|f| format!(",\"for\":{f}")).unwrap_or_default();
    let method = Value::from(method);
    format!("{{\"v\":{PROTOCOL},\"id\":{id}{for_text},\"method\":{method},\"params\":{params}}}\n")
}

/// The line of an answer, with its line end.
pub fn answer_line(id: Option<u64>, outcome: &Result<Value, Fault>) -> String {
    let id = id_text(id);
    match outcome {
        Ok(result) => format!("{{\"v\":{PROTOCOL},\"id\":{id},\"ok\":true,\"result\":{result}}}\n"),
        Err(fault) => format!("{{\"v\":{PROTOCOL},\"id\":{id},\"ok\":false,\"error\":{}}}\n", fault.to_value()),
    }
}
