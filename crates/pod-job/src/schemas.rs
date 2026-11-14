//! The JSON Schemas of `schemas/`, generated from the types, and the
//! examples of `tests/examples/`: each valid example passes, and each
//! invalid example fails for the reason that `expect.json` names. A
//! validator of JSON Schema (boon) gives each example the verdict of the
//! types, so that a reader in another language reads the records as this
//! crate does.
//!
//! After a change of a type, write the schemas again:
//! `POD_JOB_WRITE_SCHEMAS=1 cargo test -p pod-job schemas`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::{JsonSchema, Schema};
use serde_json::Value;

use crate::{Capability, JobOffer, PublicReport, RunReport};

mod rules;

/// The directory of the schema files of the repository.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")
}

/// The directory of the examples, with `expect.json`.
fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/examples")
}

/// The start of the `$id` of each schema of `schemas/`.
const ID: &str = crate::SCHEMAS;

/// The schemas of `schemas/` that the records refer to by `$ref`, and that
/// no type of this crate writes.
const REFERRED: [&str; 1] = ["broker-policy.v1.schema.json"];

/// Each record: the name of its schema file and of its directory of
/// examples.
const RECORDS: [&str; 4] = ["job-offer", "unit-capability", "run-report", "run-report-public"];

/// The examples whose rule no schema can hold: the kind of a source is in
/// the configuration of the runner, not in the offer.
const CHECK_ONLY: [&str; 1] = ["job-offer/invalid-scaleset-payload.json"];

fn schema_of<T: JsonSchema>(name: &str) -> Schema {
    let mut s = schemars::schema_for!(T);
    s.insert("$id".into(), format!("{ID}{name}.v1.schema.json").into());
    s
}

fn generated() -> Vec<(&'static str, Schema)> {
    vec![
        ("job-offer", schema_of::<JobOffer>("job-offer")),
        ("unit-capability", schema_of::<Capability>("unit-capability")),
        ("run-report", schema_of::<RunReport>("run-report")),
        ("run-report-public", schema_of::<PublicReport>("run-report-public")),
    ]
}

/// The text of a schema file, in the order of keys of schemars.
fn text_of(schema: &Schema) -> String {
    serde_json::to_string_pretty(schema).expect("a schema is JSON") + "\n"
}

/// Read one example as its record and check it; the error is the reason.
/// An offer of a source whose name starts with `github-scaleset` is checked
/// as one of a `github-scaleset` source.
fn validate(record: &str, text: &str) -> Result<(), String> {
    fn read<T: for<'de> serde::Deserialize<'de>>(text: &str) -> Result<T, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }
    match record {
        "job-offer" => {
            let o: JobOffer = read(text)?;
            o.check(o.source.starts_with("github-scaleset"))
        }
        "unit-capability" => read::<Capability>(text)?.check(),
        "run-report" => read::<RunReport>(text)?.check(),
        "run-report-public" => read::<PublicReport>(text)?.check(),
        other => Err(format!("no record {other}")),
    }
}

/// The generated schemas, compiled by a validator of JSON Schema.
struct Validator {
    schemas: boon::Schemas,
    index: BTreeMap<&'static str, boon::SchemaIndex>,
}

impl Validator {
    fn new() -> Validator {
        let mut compiler = boon::Compiler::new();
        let mut schemas = boon::Schemas::new();
        let mut index = BTreeMap::new();
        for file in REFERRED {
            let text = std::fs::read_to_string(root().join(file)).expect("a schema");
            let value: Value = serde_json::from_str(&text).expect("a schema is JSON");
            compiler.add_resource(&format!("{ID}{file}"), value).expect("a resource");
        }
        for (name, schema) in generated() {
            let id = format!("{ID}{name}.v1.schema.json");
            compiler.add_resource(&id, schema.to_value()).expect("a resource");
            let at = compiler.compile(&id, &mut schemas).unwrap_or_else(|e| panic!("{name}: {e}"));
            index.insert(name, at);
        }
        Validator { schemas, index }
    }

    /// The verdict of the schema of `record` on `value`.
    fn check(&self, record: &str, value: &Value) -> Result<(), String> {
        let at = *self.index.get(record).unwrap_or_else(|| panic!("no schema {record}"));
        self.schemas.validate(value, at).map_err(|e| format!("{e:#}"))
    }
}

/// The names of the examples of one record, sorted.
fn names(record: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(examples().join(record))
        .expect("a directory of examples")
        .map(|e| e.expect("an entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn the_schemas_are_generated_from_the_types() {
    let write = std::env::var_os("POD_JOB_WRITE_SCHEMAS").is_some();
    for (name, schema) in generated() {
        let text = text_of(&schema);
        let path = root().join(format!("{name}.v1.schema.json"));
        if write {
            std::fs::write(&path, &text).expect("write a schema");
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            have == text,
            "{} differs from the types: write the schemas again (see the head of this file)",
            path.display()
        );
    }
}

#[test]
fn each_example_passes_or_fails_for_its_reason() {
    let ex = examples();
    let expect: BTreeMap<String, String> =
        serde_json::from_str(&std::fs::read_to_string(ex.join("expect.json")).expect("expect.json")).expect("a map");
    let validator = Validator::new();
    let (mut valid, mut invalid) = (0, 0);
    for record in RECORDS {
        for name in names(record) {
            let key = format!("{record}/{name}");
            let text = std::fs::read_to_string(ex.join(&key)).expect("an example");
            let got = validate(record, &text);
            let value: Value = serde_json::from_str(&text).expect("an example is JSON");
            let schema = validator.check(record, &value);
            if name.starts_with("valid-") {
                assert_eq!(got, Ok(()), "{key}");
                assert_eq!(schema, Ok(()), "{key}: the schema refuses it");
                valid += 1;
            } else {
                let reason = expect.get(&key).unwrap_or_else(|| panic!("expect.json has no reason for {key}"));
                let why = got.expect_err(&key);
                assert!(why.contains(reason.as_str()), "{key}: fails with {why:?}, not {reason:?}");
                if CHECK_ONLY.contains(&key.as_str()) {
                    assert_eq!(schema, Ok(()), "{key}: a rule of check only");
                } else {
                    assert!(schema.is_err(), "{key}: the schema takes it, the types refuse it: {why}");
                }
                invalid += 1;
            }
        }
    }
    assert_eq!((valid, invalid), (18, 22));
    assert_eq!(expect.len(), 22, "expect.json names a file that does not exist");
}

#[test]
fn the_public_report_of_a_full_report_never_names_the_host() {
    let text = std::fs::read_to_string(examples().join("run-report/valid-success.json")).unwrap();
    let r: RunReport = serde_json::from_str(&text).unwrap();
    let public = serde_json::to_string(&r.public()).unwrap();
    assert!(!public.contains("host"), "{public}");
    let back: PublicReport = serde_json::from_str(&public).unwrap();
    assert_eq!(back.unit_kind.as_deref(), Some("podvm"));
}
