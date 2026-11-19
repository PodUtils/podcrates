//! The JSON Schema of the release index (feature `schema`), generated from
//! the types of [`super`]: `schemas/release-index.v1.schema.json` of the
//! repository is this schema, and a test holds the file to the types.

use schemars::{Schema, SchemaGenerator, json_schema};

use super::{Index, SCHEMA};

/// The `$id` of the schema: the file of version [`SCHEMA`] in the
/// directory `schemas` of the repository.
pub const SCHEMA_ID: &str = "https://github.com/PodUtils/podcrates/blob/main/schemas/release-index.v1.schema.json";

/// The JSON Schema (2020-12) of an index. A caller validates an index
/// against it with no copy of the file.
pub fn schema() -> serde_json::Value {
    let mut s = schemars::schema_for!(Index);
    s.insert("$id".into(), SCHEMA_ID.into());
    s.insert("title".into(), "The release index of a product of PodUtils".into());
    s.to_value()
}

/// The field `schema`: the version that [`super::read`] reads.
pub(super) fn version(_: &mut SchemaGenerator) -> Schema {
    json_schema!({ "type": "integer", "const": SCHEMA })
}
