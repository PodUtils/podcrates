//! The standard answer of incus (`doc/rest-api.md`, "Return values"): a
//! JSON object with `type` `sync` (the `metadata` is the result), `async`
//! (a background operation: `operation` is its URL and `metadata` the
//! operation) or `error` (`error_code` and `error`). The type decides, not
//! the HTTP status: the wait of an operation sends 200 before its body, and
//! an error after that comes as a body of type `error`.

use serde::Deserialize;
use serde::de::{DeserializeOwned, Deserializer};
use serde_json::Value;

use crate::error::{Error, ErrorKind, Result, far_text};
use crate::operation::Operation;

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    operation: String,
    #[serde(default)]
    error_code: u16,
    #[serde(default)]
    error: String,
    #[serde(default)]
    metadata: Value,
}

/// A good answer.
#[derive(Debug)]
pub(crate) enum Reply {
    Sync(Value),
    Async(Box<Operation>),
}

/// The envelope of an answer with `status` and `body`. An error envelope,
/// and a body that is not JSON with a status that is not 2xx, are the error
/// of incus.
pub(crate) fn read(call: &str, status: u16, body: &[u8]) -> Result<Reply> {
    let envelope: Envelope = match serde_json::from_slice(body) {
        Ok(envelope) => envelope,
        Err(_) if !(200..300).contains(&status) => {
            let error = far_text(&String::from_utf8_lossy(body));
            return Err(Error::new(call, ErrorKind::Incus { status, code: status, error }));
        }
        Err(e) => return Err(Error::protocol(call, format!("the body is not a JSON envelope: {e}"))),
    };
    match envelope.kind.as_str() {
        "sync" => Ok(Reply::Sync(envelope.metadata)),
        "async" => {
            let mut operation: Operation = serde_json::from_value(envelope.metadata)
                .map_err(|e| Error::protocol(call, format!("the metadata of an async answer: {e}")))?;
            if operation.id.is_empty() {
                operation.id = envelope.operation.rsplit('/').next().unwrap_or_default().to_string();
            }
            if operation.id.is_empty() {
                return Err(Error::protocol(call, "an async answer names no operation"));
            }
            Ok(Reply::Async(Box::new(operation)))
        }
        "error" => {
            let code = if envelope.error_code == 0 { status } else { envelope.error_code };
            let status = if (200..300).contains(&status) { code } else { status };
            Err(Error::new(call, ErrorKind::Incus { status, code, error: far_text(&envelope.error) }))
        }
        other => Err(Error::protocol(call, format!("an envelope of type {:?}", far_text(other)))),
    }
}

/// The metadata of a sync answer as `T`.
pub(crate) fn sync<T: DeserializeOwned>(call: &str, reply: Reply) -> Result<T> {
    match reply {
        Reply::Sync(metadata) => serde_json::from_value(metadata)
            .map_err(|e| Error::protocol(call, format!("the metadata of the answer: {e}"))),
        Reply::Async(operation) => {
            Err(Error::protocol(call, format!("a background operation {} where a result was due", operation.id)))
        }
    }
}

/// The operation of an async answer.
pub(crate) fn operation(call: &str, reply: Reply) -> Result<Operation> {
    match reply {
        Reply::Async(operation) => Ok(*operation),
        Reply::Sync(_) => Err(Error::protocol(call, "a result where a background operation was due")),
    }
}

/// A field that Go writes as `null` for an empty map or list: the default.
pub(crate) fn nullable<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Option::unwrap_or_default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_type_of_envelope_is_read() {
        let sync = br#"{"type":"sync","status":"Success","status_code":200,"metadata":{"a":1}}"#;
        assert!(matches!(read("GET /1.0", 200, sync), Ok(Reply::Sync(v)) if v["a"] == 1));

        let operation = br#"{"type":"async","status":"Operation created","status_code":100,
            "operation":"/1.0/operations/abc","metadata":{"id":"abc","status_code":103,"resources":null,"metadata":null}}"#;
        let Ok(Reply::Async(op)) = read("POST /1.0/images", 202, operation) else { panic!("an operation") };
        assert_eq!((op.id.as_str(), op.status_code, op.metadata.len()), ("abc", 103, 0));

        let unnamed = br#"{"type":"async","operation":"/1.0/operations/def","metadata":{}}"#;
        assert!(matches!(read("POST /1.0/images", 202, unnamed), Ok(Reply::Async(op)) if op.id == "def"));

        let error = br#"{"type":"error","error":"Instance not found","error_code":404,"metadata":null}"#;
        let e = read("GET /1.0/instances/x", 404, error).unwrap_err();
        assert_eq!((e.status(), e.incus_error()), (Some(404), Some("Instance not found")));
        assert_eq!(
            e.to_string(),
            "GET /1.0/instances/x: incus refused the call (404, error_code 404): Instance not found"
        );
    }

    #[test]
    fn an_error_after_a_status_of_200_keeps_its_code() {
        let error = br#"{"type":"error","error":"Operation not found","error_code":404}"#;
        let e = read("GET /1.0/operations/a/wait", 200, error).unwrap_err();
        assert_eq!(e.status(), Some(404));
    }

    #[test]
    fn a_body_that_is_not_an_envelope_is_named() {
        let e = read("GET /1.0", 500, b"panic: \x1b[31mboom").unwrap_err();
        assert_eq!(e.incus_error(), Some("panic: [31mboom"));
        let e = read("GET /1.0", 200, b"<html>").unwrap_err();
        assert!(matches!(e.kind(), ErrorKind::Protocol(_)), "{e}");
        let e = read("GET /1.0", 200, br#"{"type":"other"}"#).unwrap_err();
        assert!(e.to_string().ends_with("an envelope of type \"other\""), "{e}");
        let e = read("GET /1.0", 202, br#"{"type":"async","metadata":{}}"#).unwrap_err();
        assert!(e.to_string().ends_with("an async answer names no operation"), "{e}");
    }
}
