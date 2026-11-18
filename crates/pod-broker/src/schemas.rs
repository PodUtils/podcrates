//! The schemas of `schemas/` that come from the types of this crate:
//! `broker-policy.v1` from [`Policy`] and `log.v1` from the audit [`Line`].
//! A test writes each and compares it with the file; a validator of JSON
//! Schema (boon) gives each policy and each line the verdict of the types.
//!
//! After a change of a type, write the schemas again:
//! `POD_BROKER_WRITE_SCHEMAS=1 cargo test -p pod-broker --lib schemas`.

use std::path::{Path, PathBuf};

use pod_core::{Class, Stage};
use schemars::{JsonSchema, Schema};
use serde_json::{Value, json};

use crate::{Line, Policy};

/// The start of the `$id` of each schema of `schemas/`.
const ID: &str = "https://github.com/PodUtils/podcrates/blob/main/schemas/";

fn file(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas").join(format!("{name}.v1.schema.json"))
}

fn schema_of<T: JsonSchema>(name: &str) -> Schema {
    let mut s = schemars::schema_for!(T);
    s.insert("$id".into(), format!("{ID}{name}.v1.schema.json").into());
    s
}

fn generated() -> [(&'static str, Schema); 2] {
    [("broker-policy", schema_of::<Policy>("broker-policy")), ("log", schema_of::<Line>("log"))]
}

/// The verdict of the generated schema `name` on `value`.
fn verdict(name: &str, value: &Value) -> Result<(), String> {
    let (_, schema) = generated().into_iter().find(|(n, _)| *n == name).expect("a schema");
    let id = format!("{ID}{name}.v1.schema.json");
    let mut compiler = boon::Compiler::new();
    let mut schemas = boon::Schemas::new();
    compiler.add_resource(&id, schema.to_value()).expect("a resource");
    let at = compiler.compile(&id, &mut schemas).expect("the schema compiles");
    schemas.validate(value, at).map_err(|e| format!("{e:#}"))
}

#[test]
fn the_schemas_are_generated_from_the_types() {
    let write = std::env::var_os("POD_BROKER_WRITE_SCHEMAS").is_some();
    for (name, schema) in generated() {
        let text = serde_json::to_string_pretty(&schema).expect("a schema is JSON") + "\n";
        if write {
            std::fs::write(file(name), &text).expect("write a schema");
        }
        let have = std::fs::read_to_string(file(name)).unwrap_or_default();
        assert!(
            have == text,
            "{} differs from the types: write it again (see the head of this file)",
            file(name).display()
        );
    }
}

/// Each policy gets the same verdict from the reader and from the schema:
/// the examples of the schema, an open policy with a rule, port 0, an
/// unknown key, another version and a bad path.
#[test]
fn the_schema_and_the_reader_agree_on_each_policy() {
    let schema = schema_of::<Policy>("broker-policy").to_value();
    let mut cases: Vec<(Value, bool)> =
        schema["examples"].as_array().unwrap().iter().map(|e| (e.clone(), true)).collect();
    assert_eq!(cases.len(), 2);
    let rule = json!({ "host": "a.example" });
    cases.extend([
        (json!({ "schema": 1, "mode": "open", "allow": [rule] }), false),
        (json!({ "schema": 1, "mode": "allow", "allow": [{ "host": "a.example", "ports": [0] }] }), false),
        (json!({ "schema": 1, "mode": "allow", "allow": [{ "host": "" }] }), false),
        (json!({ "schema": 1, "mode": "allow", "allow": [{ "host": "a.example", "path": "x" }] }), false),
        (json!({ "schema": 1, "mode": "open", "extra": true }), false),
        (json!({ "schema": 2, "mode": "open" }), false),
        (json!({ "schema": 1, "mode": "deny" }), false),
        (json!({ "schema": 1, "mode": "allow", "allow": [rule], "deny": ["*.ads.example"] }), true),
    ]);
    for (value, good) in cases {
        let reader = Policy::from_value(value.clone()).and_then(|p| p.compile().map(|_| ()));
        assert_eq!(reader.is_ok(), good, "the reader on {value}: {reader:?}");
        let by_schema = verdict("broker-policy", &value);
        assert_eq!(by_schema.is_ok(), good, "the schema on {value}: {by_schema:?}");
    }
}

/// A line of each form passes the log schema: a connection, and a failure
/// with its stage and class; a line with another level or class fails.
#[test]
fn each_audit_line_passes_the_log_schema() {
    let mut ok = Line::new("info", "broker.connection");
    ok.set("job_id", "job1").set("port", 443);
    let mut failed = Line::new("warn", "broker.connection");
    failed.failed(Stage::Proxy, Class::Policy);
    for line in [&ok, &failed] {
        let value: Value = serde_json::from_str(&line.to_json()).unwrap();
        assert_eq!(verdict("log", &value), Ok(()), "{}", line.to_json());
    }
    for (key, bad) in [("level", "loud"), ("class", "bogus"), ("event", "Not.Dotted")] {
        let mut value: Value = serde_json::from_str(&failed.to_json()).unwrap();
        value[key] = bad.into();
        assert!(verdict("log", &value).is_err(), "{value}");
    }
}
