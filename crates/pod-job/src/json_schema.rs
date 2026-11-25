//! The JSON Schema of the fields and of the rules that the derive of
//! schemars cannot read from a type: the version, the architecture, the
//! digest, the names of the limits, and each rule of a `check` that a
//! schema can hold. Thus a validator of the schema and the `check` of a
//! record give the same verdict. Only the rule of a `github-scaleset`
//! source stays out: the kind of a source is in the configuration, not in
//! the offer. The `policy` of an offer refers to `broker-policy.v1`: the
//! schema holds each of its rules, and the check reads its top level only
//! (the consumer reads the rest with `pod-broker`).

use schemars::{Schema, SchemaGenerator, json_schema};
use serde_json::{Value, json};

use crate::offer::LIMITS;
use crate::{ARCHES, SCHEMA};

/// `schema`: the version of the record, and no other number.
pub fn version(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "integer", "const": SCHEMA })
}

/// The `policy` of an offer: an empty object, or a policy of the
/// `broker-policy.v1` schema, by its `$id`. A validator needs that file as
/// a resource too.
pub fn policy(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "anyOf": [
            { "type": "object", "maxProperties": 0 },
            { "$ref": format!("{}broker-policy.v1.schema.json", crate::SCHEMAS) }
        ],
        "default": {}
    })
}

/// `arch`: one of [`ARCHES`].
pub fn arch(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "string", "enum": ARCHES })
}

/// The `arch` of a report: absent, or one of [`ARCHES`].
pub fn arch_or_none(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": ["string", "null"], "enum": [ARCHES[0], ARCHES[1], null] })
}

/// The `payload` of an offer: absent, or `sha256:` and 64 lower-case hex
/// digits (`offer::is_digest`); an image by tag is refused.
pub fn digest(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": ["string", "null"], "pattern": "^sha256:[0-9a-f]{64}$" })
}

/// The `forge` of the trust inputs: absent, or a host name in lower case
/// (`parts::is_host_name`).
pub fn host_name(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": ["string", "null"], "maxLength": 253, "pattern": "^[a-z0-9-]{1,63}(\\.[a-z0-9-]{1,63})*$" })
}

/// The `ref` of a real job (`source::ref_fault`): no control character
/// (Unicode class Cc). The type holds at most 256 bytes; `maxLength` counts
/// characters, so the schema takes a longer ref of characters outside ASCII.
pub fn git_ref(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": ["string", "null"], "maxLength": 256, "pattern": "^[^\\u0000-\\u001f\\u007f-\\u009f]*$" })
}

/// The `image` of a sandbox: `NAME@sha256:` and 64 lower-case hex digits
/// (`parts::is_pinned_image`).
pub fn pinned_image(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "string", "pattern": "^.+@sha256:[0-9a-f]{64}$" })
}

/// The `job_images` of a report: digests (`offer::is_digest`), each once
/// or more, in the order of the job.
pub fn digests(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "array", "items": { "type": "string", "pattern": "^sha256:[0-9a-f]{64}$" } })
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

/// The rules of [`crate::RunReport::check`]: those of each report, and
/// each key of `unknown_reasons` names a measured field that is `unknown`
/// or a time that is absent (a time of the unit is absent also when the
/// report has no unit). serde reads a `null` time as absent, so the
/// schema does too. The bound of `log_tail` in bytes stays in the check: a
/// schema counts characters.
pub fn run_report_rules(schema: &mut Schema) {
    use crate::report::{MEASURED, TIMES, UNIT_TIMES};
    report_rules(schema, &["unit"]);
    let when = |name: &str| json!({ "required": ["unknown_reasons"], "properties": { "unknown_reasons": { "required": [name] } } });
    let measured = MEASURED.iter().map(|name| {
        json!({ "if": when(name), "then": { "required": [name], "properties": { (*name): { "const": "unknown" } } } })
    });
    let times =
        TIMES.iter().map(|name| json!({ "if": when(name), "then": { "properties": { (*name): { "type": "null" } } } }));
    let unit_times = UNIT_TIMES.iter().map(|name| {
        let unit = json!({ "properties": { (*name): { "type": "null" } } });
        json!({ "if": when(name), "then": { "properties": { "unit": unit } } })
    });
    if let Some(Value::Array(rules)) = schema.get_mut("allOf") {
        rules.extend(measured.chain(times).chain(unit_times));
    }
    let names: Vec<&str> = MEASURED.iter().chain(TIMES.iter()).chain(UNIT_TIMES.iter()).copied().collect();
    if let Some(Value::Object(field)) = schema.get_mut("properties").and_then(|p| p.get_mut("unknown_reasons")) {
        field.insert("propertyNames".into(), json!({ "enum": names }));
    }
}

/// The version of each object of the source wire: `parse` reads `schema`
/// before serde, and `write` puts it first. Each branch of the union gets
/// the property `schema` as the constant 1, required first.
pub fn wire_version(schema: &mut Schema) {
    let Some(Value::Array(branches)) = schema.get_mut("oneOf") else { return };
    for branch in branches {
        let Value::Object(branch) = branch else { continue };
        if let Some(Value::Object(properties)) = branch.get_mut("properties") {
            properties.insert("schema".into(), json!({ "type": "integer", "const": SCHEMA }));
        }
        let mut required = vec![json!("schema")];
        if let Some(Value::Array(have)) = branch.get("required") {
            required.extend(have.iter().cloned());
        }
        branch.insert("required".into(), required.into());
    }
}

/// The rules of [`crate::PublicReport::check`]: its unit is `unit_kind`
/// and `boundary`.
pub fn public_report_rules(schema: &mut Schema) {
    report_rules(schema, &["unit_kind", "boundary"]);
}
