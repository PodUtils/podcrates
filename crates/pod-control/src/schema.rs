//! The JSON Schema of one line of the wire (`control.v1`), made from the
//! catalogue and the codes: `schemas/control.v1.schema.json` of the
//! repository is this schema, and a test holds the file to it.

use serde_json::{Map, Value, json};

use crate::catalogue::{Method, Route, Tier};
use crate::wire::{Code, PROTOCOL};

/// The `$id` of the schema: the file of version [`PROTOCOL`] in the
/// directory `schemas` of the repository.
pub const SCHEMA_ID: &str = "https://github.com/PodUtils/podcrates/blob/main/schemas/control.v1.schema.json";

fn closed(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "additionalProperties": false, "properties": properties, "required": required })
}

/// The params of each method of the catalogue.
fn params(method: Method) -> Value {
    match method {
        Method::Auth => closed(json!({ "token": { "type": "string", "minLength": 1 } }), &["token"]),
        Method::Drain => closed(
            json!({ "exit": {
                "type": "boolean",
                "description": "Exit after the last work in flight ends; true when it is missing."
            } }),
            &[],
        ),
        Method::Version | Method::Methods | Method::Status | Method::Metrics | Method::Reload | Method::Stop => {
            closed(json!({}), &[])
        }
    }
}

/// The result of each method whose result this crate makes or types; the
/// host gives the others.
fn result(method: Method) -> Value {
    let counts = |kind: &str, minimum: Option<i64>| {
        let mut item = json!({ "type": "integer" });
        if let Some(m) = minimum {
            item["minimum"] = m.into();
        }
        json!({ "type": "object", "description": kind, "additionalProperties": item })
    };
    match method {
        Method::Version => closed(
            json!({
                "program": { "type": "string" },
                "version": { "type": "string" },
                "protocol": { "const": PROTOCOL }
            }),
            &["program", "version", "protocol"],
        ),
        Method::Methods => closed(
            json!({
                "tier": { "$ref": "#/$defs/tier" },
                "methods": { "type": "array", "items": closed(json!({
                    "name": { "$ref": "#/$defs/method" },
                    "tier": { "$ref": "#/$defs/tier" },
                    "route": { "enum": [Route::Local.name(), Route::Delegated.name()] },
                    "summary": { "type": "string" }
                }), &["name", "tier", "route", "summary"]) }
            }),
            &["tier", "methods"],
        ),
        Method::Auth => closed(json!({ "tier": { "const": Tier::Master.name() } }), &["tier"]),
        Method::Metrics => closed(
            json!({
                "counters": counts("Counters that only grow.", Some(0)),
                "gauges": counts("Values that go up and down.", None)
            }),
            &["counters", "gauges"],
        ),
        Method::Status => json!({ "type": "object", "description": "The status of the program, in its own schema." }),
        Method::Reload | Method::Drain | Method::Stop => json!({ "description": "The words of the program." }),
    }
}

/// The JSON Schema (2020-12) of one line, in each direction.
pub fn schema() -> Value {
    let mut params_defs = Map::new();
    let mut result_defs = Map::new();
    let mut by_method = Vec::new();
    let mut catalogue = Vec::new();
    for m in Method::ALL {
        let spec = m.spec();
        params_defs.insert(spec.name.into(), params(m));
        result_defs.insert(spec.name.into(), result(m));
        by_method.push(json!({
            "if": { "properties": { "method": { "const": spec.name } }, "required": ["method"] },
            "then": { "properties": { "params": { "$ref": format!("#/$defs/params/{}", spec.name) } } }
        }));
        catalogue.push(json!({
            "name": spec.name,
            "route": spec.route.name(),
            "tier": spec.tier.name(),
            "outlives": spec.outlives,
            "summary": spec.summary
        }));
    }
    let id = json!({ "type": "integer", "minimum": 0 });
    // Each branch holds the rule of `schemas/`: an object with no other
    // key, and `schema`, the constant of the protocol, as its first
    // required key.
    let request = json!({
        "description": "A request of the client, or a call back of the server with for.",
        "type": "object",
        "additionalProperties": false,
        "required": ["schema", "id", "method"],
        "properties": {
            "schema": { "const": PROTOCOL },
            "id": { "$ref": "#/$defs/id" },
            "for": { "$ref": "#/$defs/id" },
            "method": { "$ref": "#/$defs/method" },
            "params": { "type": "object" }
        },
        "allOf": by_method
    });
    let answer = json!({
        "description": "The answer to a request: ok with result, or not ok with error.",
        "type": "object",
        "additionalProperties": false,
        "required": ["schema", "id", "ok"],
        "properties": {
            "schema": { "const": PROTOCOL },
            "id": { "oneOf": [{ "$ref": "#/$defs/id" }, { "type": "null" }] },
            "ok": { "type": "boolean" },
            "result": true,
            "error": closed(json!({
                "code": { "$ref": "#/$defs/code" },
                "detail": { "type": "string" }
            }), &["code", "detail"])
        },
        "oneOf": [
            { "properties": { "ok": { "const": true } }, "required": ["result"], "not": { "required": ["error"] } },
            { "properties": { "ok": { "const": false } }, "required": ["error"], "not": { "required": ["result"] } }
        ]
    });
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": SCHEMA_ID,
        "title": "One line of the control API of a program of PodUtils",
        "description": "Each line is one JSON object with \"schema\": 1. A request has a method; an answer has ok. The \
            client sends requests, and the server sends answers and calls back; a call back names its request \
            with for. A method above the tier of the caller gets the answer of a method that is not there.",
        "oneOf": [request, answer],
        "$defs": {
            "id": id,
            "method": { "type": "string", "pattern": "^[a-z][a-z0-9-]{0,62}$" },
            "tier": { "enum": Tier::ALL.map(Tier::name) },
            "code": { "enum": Code::ALL.map(Code::name) },
            "params": params_defs,
            "results": result_defs,
            "catalogue": {
                "description": "The methods of each program; a program can add methods of its own.",
                "const": catalogue
            }
        }
    })
}
