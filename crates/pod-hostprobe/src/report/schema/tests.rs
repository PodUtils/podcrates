//! The schema file of `schemas/` is the schema of the types, and each
//! example of `tests/examples/report/` gets the same verdict from
//! [`super::super::read`] and from a validator of JSON Schema (boon).
//!
//! After a change of a wire type, write the file again:
//! `POD_HOSTPROBE_WRITE_SCHEMAS=1 cargo test -p pod-hostprobe --all-features schema`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::*;
use crate::report::{read, write};

/// The schema file of the repository.
fn file() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/hostprobe-report.v1.schema.json")
}

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/examples")
}

/// The examples whose rule no schema holds: a row name that comes two
/// times is a rule of `read` only.
const CHECK_ONLY: [&str; 1] = ["report/invalid-duplicate-row.json"];

/// The schema, compiled by a validator of JSON Schema 2020-12.
fn validator() -> (boon::Schemas, boon::SchemaIndex) {
    let mut compiler = boon::Compiler::new();
    let mut schemas = boon::Schemas::new();
    compiler.add_resource(SCHEMA_ID, schema()).expect("a resource");
    let at = compiler.compile(SCHEMA_ID, &mut schemas).expect("the schema compiles");
    (schemas, at)
}

#[test]
fn the_schema_file_is_generated_from_the_types() {
    let text = serde_json::to_string_pretty(&schema()).expect("a schema is JSON") + "\n";
    if std::env::var_os("POD_HOSTPROBE_WRITE_SCHEMAS").is_some() {
        std::fs::write(file(), &text).expect("write the schema");
    }
    let have = std::fs::read_to_string(file()).unwrap_or_default();
    assert!(have == text, "{} differs from the types: write it again (see the head of this file)", file().display());
}

/// The `$id` names the file of the version that the report carries in its
/// field `schema`.
#[test]
fn the_id_names_the_version_of_the_report() {
    assert!(SCHEMA_ID.ends_with(&format!("/hostprobe-report.v{SCHEMA}.schema.json")), "{SCHEMA_ID}");
    assert_eq!(schema()["$id"], SCHEMA_ID);
    assert_eq!(schema()["properties"]["schema"]["const"], SCHEMA);
}

#[test]
fn each_example_passes_or_fails_for_its_reason() {
    let ex = examples();
    let expect: BTreeMap<String, String> =
        serde_json::from_str(&std::fs::read_to_string(ex.join("expect.json")).expect("expect.json")).expect("a map");
    let (schemas, at) = validator();
    let mut names: Vec<String> = std::fs::read_dir(ex.join("report"))
        .expect("the examples")
        .map(|e| e.expect("an entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let (mut valid, mut invalid) = (0, 0);
    for name in names {
        let key = format!("report/{name}");
        let text = std::fs::read_to_string(ex.join(&key)).expect("an example");
        let value: Value = serde_json::from_str(&text).expect("an example is JSON");
        let by_schema = schemas.validate(&value, at).map_err(|e| format!("{e:#}"));
        let got = read(&text).map_err(|e| e.to_string());
        if name.starts_with("valid-") {
            let Ok((f, s)) = got else { panic!("{key}: {}", got.err().unwrap_or_default()) };
            assert_eq!(by_schema, Ok(()), "{key}: the schema refuses it");
            // The text of `write` validates too.
            let again: Value = serde_json::from_str(&write(&f, &s)).unwrap();
            assert_eq!(again, value, "{key}: write does not give the example back");
            valid += 1;
        } else {
            let reason = expect.get(&key).unwrap_or_else(|| panic!("expect.json has no reason for {key}"));
            let Err(why) = got else { panic!("{key}: read takes it") };
            assert!(why.contains(reason.as_str()), "{key}: fails with {why:?}, not {reason:?}");
            if CHECK_ONLY.contains(&key.as_str()) {
                assert_eq!(by_schema, Ok(()), "{key}: a rule of read only");
            } else {
                assert!(by_schema.is_err(), "{key}: the schema takes it, read refuses it: {why}");
            }
            invalid += 1;
        }
    }
    assert_eq!((valid, invalid), (2, 10));
    assert_eq!(expect.len(), 10, "expect.json names a file that does not exist");
}

/// The object of `write` on the fixture of the module (each field with a
/// value that is not its default) validates.
#[test]
fn a_written_report_validates() {
    let (f, s) = crate::report::tests::fake();
    let value: Value = serde_json::from_str(&write(&f, &s)).unwrap();
    let (schemas, at) = validator();
    assert_eq!(schemas.validate(&value, at).map_err(|e| format!("{e:#}")), Ok(()));
}
