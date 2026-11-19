//! The JSON Schema of the fields and of the rules that the derive of
//! schemars cannot read from a type: the version of each schema, the time,
//! the level, the event and the class of a log line, and the rule of an
//! open policy. `schemas/broker-policy.v1.schema.json` and
//! `schemas/log.v1.schema.json` come from [`crate::Policy`] and
//! [`crate::Line`] with these.

use pod_core::{Class, LOG_LEVELS, LOG_SCHEMA};
use schemars::{Schema, SchemaGenerator, json_schema};
use serde_json::json;

/// `schema` of a policy: [`crate::policy::SCHEMA`] and no other number.
pub fn policy_version(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "integer", "const": crate::policy::SCHEMA })
}

/// `schema` of a log line: [`LOG_SCHEMA`] and no other number.
pub fn log_version(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "integer", "const": LOG_SCHEMA })
}

/// `time` of a log line: RFC 3339.
pub fn time(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "string", "format": "date-time" })
}

/// `level` of a log line: one of [`LOG_LEVELS`].
pub fn level(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "string", "enum": LOG_LEVELS })
}

/// `event` of a log line: a dotted name, for example `relay.session.open`.
pub fn event(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "string", "pattern": "^[a-z0-9_]+(\\.[a-z0-9_]+)*$" })
}

/// `fields` of a log line: an object of any values.
pub fn fields(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "object" })
}

/// `class` of a log line: the name of a [`Class`]. A type, not a function,
/// so that the field stays optional in the schema.
pub struct ClassName;

impl schemars::JsonSchema for ClassName {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ClassName".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let names: Vec<&str> = Class::ALL.iter().map(|c| c.name()).collect();
        json_schema!({ "type": "string", "enum": names })
    }
}

/// The rule of [`crate::Policy::compile`] that serde does not read: an open
/// policy has no allow rule. The examples of the schema are policies (a
/// test compiles each).
pub fn policy_rules(schema: &mut Schema) {
    schema.insert("if".into(), json!({ "properties": { "mode": { "const": "open" } } }));
    schema.insert("then".into(), json!({ "properties": { "allow": { "maxItems": 0 } } }));
    let examples = json!([
        { "schema": 1, "mode": "open", "deny": ["203.0.113.0/24"] },
        {
            "schema": 1,
            "mode": "allow",
            "allow": [
                { "host": "api.github.com", "path": "/repos/owner", "ports": [443] },
                { "host": "*.githubusercontent.com" }
            ]
        },
        {
            "schema": 1,
            "mode": "open",
            "credentials": [
                { "placeholder": "PODCRED_GITHUB_TOKEN_0001", "secret": "github", "hosts": ["api.github.com"] }
            ]
        }
    ]);
    schema.insert("examples".into(), examples);
}
