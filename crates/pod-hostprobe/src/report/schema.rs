//! The JSON Schema of the report (feature `schema`), generated from the
//! wire types of [`super`]: `schemas/hostprobe-report.v1.schema.json` of the
//! repository is this schema, and a test holds the file to the types. A
//! word of the crate (a verdict, a rung, the name of a row) is an `enum` of
//! the words of the crate, so a reader in another language refuses what
//! [`super::read`] refuses.

use schemars::{Schema, SchemaGenerator, json_schema};

use super::{Document, RUNGS, SCHEMA, VERDICTS};
use crate::probes::PROBES;

/// The `$id` of the schema: the file of version [`SCHEMA`] in the
/// directory `schemas` of the repository.
pub const SCHEMA_ID: &str = "https://github.com/PodUtils/podcrates/blob/main/schemas/hostprobe-report.v1.schema.json";

/// The JSON Schema (2020-12) of the object of [`super::write`]. A caller
/// validates a report against it with no copy of the file.
pub fn schema() -> serde_json::Value {
    let mut s = schemars::schema_for!(Document);
    s.insert("$id".into(), SCHEMA_ID.into());
    s.insert("title".into(), "The report of pod-hostprobe".into());
    s.to_value()
}

/// The field `schema`: the version that [`super::read`] reads.
pub(super) fn version(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "integer", "const": SCHEMA })
}

/// A verdict: a word of [`crate::verdict::Verdict::word`].
pub(super) fn verdict(_: &mut SchemaGenerator) -> Schema {
    let words: Vec<&str> = VERDICTS.iter().map(|v| v.word()).collect();
    json_schema!({ "type": "string", "enum": words })
}

/// A rung: a word of [`crate::select::Rung::word`].
pub(super) fn rung(_: &mut SchemaGenerator) -> Schema {
    let words: Vec<&str> = RUNGS.iter().map(|r| r.word()).collect();
    json_schema!({ "type": "string", "enum": words })
}

/// The name of a row: a name of [`PROBES`].
pub(super) fn row_name(_: &mut SchemaGenerator) -> Schema {
    let names: Vec<&str> = PROBES.iter().map(|p| p.name).collect();
    json_schema!({ "type": "string", "enum": names })
}

#[cfg(test)]
mod tests;
