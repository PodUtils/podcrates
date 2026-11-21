//! The lines of the wire, version [`PROTOCOL`]. Each line is one JSON
//! object with `"schema": 1`:
//!
//! ```text
//! {"schema":1,"id":7,"method":"drain","params":{"exit":true}}       a request
//! {"schema":1,"id":7,"ok":true,"result":{"draining":true}}          its answer
//! {"schema":1,"id":7,"ok":false,"error":{"code":"busy","detail":"..."}}
//! {"schema":1,"id":1,"for":7,"method":"confirm","params":{}}        a call back of the server
//! {"schema":1,"id":1,"ok":true,"result":true}                       its answer by the client
//! ```
//!
//! A request has `method`; an answer has `ok`. Each side numbers its own
//! requests, and an answer has the id of its request (`null` when the line
//! had no id that can be read). The `detail` of an error holds no byte of
//! the request: a request can hold a token. For the same reason each value
//! that [`parse`] reads is wiped at its drop, and each line that
//! [`request_line`] and [`answer_line`] write is wiped at its drop.

use std::fmt::{self, Write as _};

use serde_json::{Map, Value};
use zeroize::{Zeroize, Zeroizing};

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

/// A request that was read. Its params are wiped at its drop ([`wipe`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub id: u64,
    pub method: String,
    /// An object; `{}` when the line had none.
    pub params: Value,
    /// The id of the request that a call back serves.
    pub for_id: Option<u64>,
}

impl Drop for Request {
    fn drop(&mut self) {
        wipe(&mut self.params);
    }
}

/// Wipe each string of `value`, the keys of each object too, and make it
/// `null`. A number keeps its bytes until they are written again.
pub fn wipe(value: &mut Value) {
    match value {
        Value::String(s) => s.zeroize(),
        Value::Array(items) => items.iter_mut().for_each(wipe),
        Value::Object(map) => {
            for (mut key, mut item) in std::mem::take(map) {
                key.zeroize();
                wipe(&mut item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    *value = Value::Null;
}

/// Wipe the result of an answer that is dropped with no use.
pub(crate) fn wipe_outcome(outcome: Result<Value, Fault>) {
    if let Ok(mut result) = outcome {
        wipe(&mut result);
    }
}

/// A value that is wiped at its drop, on each path of [`parse`].
struct Wiped(Value);

impl Drop for Wiped {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
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

fn take_object(line: &str) -> Result<Wiped, (Option<u64>, Fault)> {
    let value = Wiped(serde_json::from_str(line).map_err(|_| bad(None, "the line is not JSON"))?);
    let Value::Object(map) = &value.0 else { return Err(bad(None, "the line is not a JSON object")) };
    let id = map.get("id").and_then(Value::as_u64);
    if map.get("schema").and_then(Value::as_u64) != Some(u64::from(PROTOCOL)) {
        return Err(bad(id, "the line has no \"schema\": 1"));
    }
    Ok(value)
}

/// Read one line. An error names the id where the line has one that can be
/// read, and never holds a byte of the line. The params and the result move
/// out of the object that was read; the rest of it is wiped.
pub fn parse(line: &str) -> Result<Inbound, (Option<u64>, Fault)> {
    let mut value = take_object(line)?;
    let Value::Object(map) = &mut value.0 else { return Err(bad(None, "the line is not a JSON object")) };
    let id = map.get("id").and_then(Value::as_u64);
    if map.contains_key("method") {
        let known = ["schema", "id", "method", "params", "for"];
        if map.keys().any(|k| !known.contains(&k.as_str())) {
            return Err(bad(id, "a request has an unknown field"));
        }
        let Some(id) = id else { return Err(bad(None, "a request has no id of 0 or more")) };
        let Some(method) = map.get("method").and_then(Value::as_str) else {
            return Err(bad(Some(id), "the method is not a string"));
        };
        let method = method.to_string();
        let for_id = match map.get("for") {
            None => None,
            Some(v) => Some(v.as_u64().ok_or_else(|| bad(Some(id), "the field for is not an id"))?),
        };
        let params = match map.remove("params").map(Wiped) {
            None => Value::Object(Map::new()),
            Some(mut p @ Wiped(Value::Object(_))) => std::mem::take(&mut p.0),
            Some(_) => return Err(bad(Some(id), "the params are not an object")),
        };
        return Ok(Inbound::Request(Request { id, method, params, for_id }));
    }
    let known = ["schema", "id", "ok", "result", "error"];
    if map.keys().any(|k| !known.contains(&k.as_str())) {
        return Err(bad(id, "an answer has an unknown field"));
    }
    if !(map.get("id").is_some_and(Value::is_null) || id.is_some()) {
        return Err(bad(None, "an answer has no id and no null"));
    }
    let ok = map.get("ok").and_then(Value::as_bool);
    let outcome = match (ok, map.remove("result").map(Wiped), map.get("error")) {
        (Some(true), Some(mut r), None) => Ok(std::mem::take(&mut r.0)),
        (Some(false), None, Some(e)) => Err(fault_of(e).ok_or_else(|| bad(id, "the error has no code"))?),
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

/// A line of the exact length of `args`: a `String` that grows leaves a copy
/// of its old bytes in memory that no one wipes.
fn exact(args: fmt::Arguments<'_>) -> Zeroizing<String> {
    struct Count(usize);
    impl fmt::Write for Count {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            self.0 += s.len();
            Ok(())
        }
    }
    let mut count = Count(0);
    let _ = count.write_fmt(args);
    let mut line = Zeroizing::new(String::with_capacity(count.0));
    let _ = line.write_fmt(args);
    line
}

/// The line of a request, with its line end. `schema` comes first, so that a
/// reader of a log finds it. The line is wiped at its drop.
pub fn request_line(id: u64, method: &str, params: &Value, for_id: Option<u64>) -> Zeroizing<String> {
    let for_text = for_id.map(|f| format!(",\"for\":{f}")).unwrap_or_default();
    let method = Value::from(method);
    exact(format_args!("{{\"schema\":{PROTOCOL},\"id\":{id}{for_text},\"method\":{method},\"params\":{params}}}\n"))
}

/// The line of an answer, with its line end. The line is wiped at its drop.
pub fn answer_line(id: Option<u64>, outcome: &Result<Value, Fault>) -> Zeroizing<String> {
    let id = id_text(id);
    match outcome {
        Ok(result) => exact(format_args!("{{\"schema\":{PROTOCOL},\"id\":{id},\"ok\":true,\"result\":{result}}}\n")),
        Err(fault) => {
            exact(format_args!("{{\"schema\":{PROTOCOL},\"id\":{id},\"ok\":false,\"error\":{}}}\n", fault.to_value()))
        }
    }
}
