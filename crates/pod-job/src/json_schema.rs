//! The JSON Schema of the fields and of the rules that the derive of
//! schemars cannot read from a type: the version, the architecture, the
//! digest, the names of the limits, and each rule of a `check` that a
//! schema can hold. Thus a validator of the schema and the `check` of a
//! record give the same verdict. Only the rule of a `github-scaleset`
//! source stays out: the kind of a source is in the configuration, not in
//! the offer.

use schemars::{Schema, SchemaGenerator, json_schema};
use serde_json::{Value, json};

use crate::offer::LIMITS;
use crate::{ARCHES, SCHEMA};

/// `schema`: the version of the record, and no other number.
pub fn version(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "integer", "const": SCHEMA })
}

/// `arch`: one of [`ARCHES`].
pub fn arch(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "string", "enum": ARCHES })
}

/// The `payload` of an offer: absent, or `sha256:` and 64 lower-case hex
/// digits (`offer::is_digest`); an image by tag is refused.
pub fn digest(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": ["string", "null"], "pattern": "^sha256:[0-9a-f]{64}$" })
}

/// The `limits` of an offer: a value for each name of [`LIMITS`].
pub fn limit_values(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "object",
        "propertyNames": { "enum": LIMITS },
        "additionalProperties": { "type": "integer", "format": "uint64", "minimum": 0 }
    })
}

/// The `limits_required` of an offer: names of [`LIMITS`].
pub fn limit_names(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "array", "items": { "type": "string", "enum": LIMITS } })
}

/// The rule of an offer that serde does not read: each required limit has
/// a value. One `if` for each name, as a schema cannot compare an item of
/// one list with a key of another.
pub fn offer_rules(schema: &mut Schema) {
    let rules: Vec<Value> = LIMITS
        .iter()
        .map(|name| {
            json!({
                "if": {
                    "required": ["limits_required"],
                    "properties": { "limits_required": { "contains": { "const": name } } }
                },
                "then": { "required": ["limits"], "properties": { "limits": { "required": [name] } } }
            })
        })
        .collect();
    schema.insert("allOf".into(), rules.into());
}

/// The rules of `report::rules` on a report whose unit is in `unit`: a
/// refused report has a refusal code and no unit; a refusal code needs
/// `refused`; a withdrawn report has no unit. serde reads a `null` as an
/// absent field, so the schema does too.
fn report_rules(schema: &mut Schema, unit: &[&str]) {
    let no_unit: serde_json::Map<String, Value> =
        unit.iter().map(|f| (f.to_string(), json!({ "type": "null" }))).collect();
    let mut refused = no_unit.clone();
    refused.insert("refusal_code".into(), json!({ "not": { "type": "null" } }));
    let when =
        |reason: &str| json!({ "required": ["exit_reason"], "properties": { "exit_reason": { "const": reason } } });
    let rules = json!([
        {
            "if": when("refused"),
            "then": { "required": ["refusal_code"], "properties": refused },
            "else": { "properties": { "refusal_code": { "type": "null" } } }
        },
        { "if": when("withdrawn"), "then": { "properties": no_unit } }
    ]);
    schema.insert("allOf".into(), rules);
}

/// The rules of [`crate::RunReport::check`].
pub fn run_report_rules(schema: &mut Schema) {
    report_rules(schema, &["unit"]);
}

/// The rules of [`crate::PublicReport::check`]: its unit is `unit_kind`
/// and `boundary`.
pub fn public_report_rules(schema: &mut Schema) {
    report_rules(schema, &["unit_kind", "boundary"]);
}
