//! The parts of the HTTP routes that the fakes of a consumer's CI share
//! (the features `github` and `githome`): a JSON answer, and the JSON body
//! of a request.

use serde_json::Value;

use crate::stand::{Answer, Request};

/// A JSON answer.
pub(crate) fn json_answer(status: u16, value: &Value) -> Answer {
    Answer::new(status, value.to_string()).header("Content-Type", "application/json; charset=utf-8")
}

/// The JSON body of a request; `Null` when it is not JSON.
pub(crate) fn body(req: &Request) -> Value {
    serde_json::from_slice(&req.body).unwrap_or(Value::Null)
}
