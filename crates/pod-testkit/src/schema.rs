//! The tests of a JSON Schema that a crate generates from its types
//! (feature `schema`): the file of `schemas/` holds the schema of the
//! types, and a validator of JSON Schema 2020-12 (boon) checks a document.

use std::path::Path;

use serde_json::Value;

/// `schema`, with the `$id` `id`, compiled by a validator of JSON Schema
/// 2020-12.
pub fn compiled(id: &str, schema: Value) -> (boon::Schemas, boon::SchemaIndex) {
    let mut compiler = boon::Compiler::new();
    let mut schemas = boon::Schemas::new();
    compiler.add_resource(id, schema).expect("a resource");
    let at = compiler.compile(id, &mut schemas).expect("the schema compiles");
    (schemas, at)
}

/// The file `path` holds `schema` as pretty JSON with a line end. When the
/// variable `write` is set, the test writes the file first: the way to make
/// it again after a change of a type.
pub fn holds(path: &Path, schema: &Value, write: &str) {
    let text = serde_json::to_string_pretty(schema).expect("a schema is JSON") + "\n";
    if std::env::var_os(write).is_some() {
        std::fs::write(path, &text).expect("write the schema");
    }
    let have = std::fs::read_to_string(path).unwrap_or_default();
    assert!(have == text, "{} differs from the types: write it again with {write}=1", path.display());
}
