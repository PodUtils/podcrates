//! The schema file of `schemas/` is the schema of the types; the index
//! that `scripts/release-index.py --self-test` writes is valid for the
//! schema and for [`read`], and [`Index::to_text`] gives its bytes back.
//!
//! After a change of a type, write the schema file again:
//! `POD_RELEASE_WRITE_SCHEMAS=1 cargo test -p pod-release --all-features index`.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::schema::{SCHEMA_ID, schema};
use super::*;
use crate::{hex, sha256};

fn file() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/release-index.v1.schema.json")
}

const WRITTEN: &str = include_str!("../../tests/fixtures/index/release-index.json");

fn validator() -> (boon::Schemas, boon::SchemaIndex) {
    pod_testkit::schema::compiled(SCHEMA_ID, schema())
}

#[test]
fn the_schema_file_is_generated_from_the_types() {
    pod_testkit::schema::holds(&file(), &schema(), "POD_RELEASE_WRITE_SCHEMAS");
}

#[test]
fn the_id_names_the_version_of_the_index() {
    assert!(SCHEMA_ID.ends_with(&format!("/release-index.v{SCHEMA}.schema.json")), "{SCHEMA_ID}");
    assert_eq!(schema()["properties"]["schema"]["const"], SCHEMA);
}

/// The writer of CI and the types of the crate agree, byte for byte.
#[test]
fn the_index_of_the_script_is_valid_and_written_back_the_same() {
    let (schemas, at) = validator();
    let value: Value = serde_json::from_str(WRITTEN).unwrap();
    assert!(schemas.validate(&value, at).is_ok(), "the schema refuses the index of the script");
    let index = read(WRITTEN).expect("the index of the script reads");
    assert_eq!(index.to_text(), WRITTEN);
    assert_eq!(index.files.len(), 4);
    assert_eq!(index.files[0].target.as_deref(), Some("aarch64-pc-windows-msvc"));
}

/// `SHA256SUMS` of the files of the script's self-test (each file is its
/// name and a line end), with the index's own line.
fn sums_of(names: &[&str]) -> Sums {
    let mut text = String::new();
    for name in names {
        text.push_str(&format!("{}  {name}\n", hex(&sha256(format!("{name}\n").as_bytes()))));
    }
    text.push_str(&format!("{}  {FILE}\n", hex(&sha256(WRITTEN.as_bytes()))));
    Sums::parse(text.as_bytes()).unwrap()
}

const NAMES: [&str; 4] =
    ["podlab-aarch64-pc-windows-msvc.exe", "podlab-install.sh", "podlab-x86_64-unknown-linux-musl", "sbom.spdx.json"];

#[test]
fn the_index_agrees_with_its_sums_and_not_with_others() {
    let index = read(WRITTEN).unwrap();
    assert_eq!(index.agrees(&sums_of(&NAMES)), Ok(()));
    let why = index.agrees(&sums_of(&NAMES[1..])).unwrap_err();
    assert!(why.contains("names no podlab-aarch64-pc-windows-msvc.exe"), "{why}");
    let more = [&NAMES[..], &["podlab-riscv64gc-unknown-linux-musl"]].concat();
    let why = index.agrees(&sums_of(&more)).unwrap_err();
    assert!(why.contains("which the index does not"), "{why}");
    let mut changed = index.clone();
    changed.files[1].sha256 = "0".repeat(64);
    let why = changed.agrees(&sums_of(&NAMES)).unwrap_err();
    assert!(why.contains("two SHA-256 sums"), "{why}");
}

/// Each change: refused by [`read`], and by the schema where a schema can
/// say it.
#[test]
fn each_broken_index_is_refused() {
    let (schemas, at) = validator();
    let base: Value = serde_json::from_str(WRITTEN).unwrap();
    let cases: [(&str, Value, bool); 12] = [
        ("schema", 2.into(), true),
        ("schema", "1".into(), true),
        ("project", "PodUtils".into(), true),
        ("version", "v1/../x".into(), true),
        ("date", "2026-11-19 09:27:57".into(), true),
        ("date", "2026-13-19T09:27:57Z".into(), false),
        ("commit", "ABC".into(), true),
        ("sbom", "x.json".into(), false),
        ("acceptance", "http://example.org/a".into(), false),
        ("files", serde_json::json!([{"name": "../x", "sha256": "0".repeat(64), "size": 1}]), false),
        ("files", serde_json::json!([{"name": "x", "sha256": "A".repeat(64), "size": 1}]), true),
        ("files", serde_json::json!([{"name": "x", "sha256": "0".repeat(64)}]), true),
    ];
    for (field, value, by_schema) in cases {
        let mut doc = base.clone();
        doc[field] = value.clone();
        if field == "files" {
            doc["sbom"] = Value::Null;
            doc.as_object_mut().unwrap().remove("sbom");
        }
        let text = doc.to_string();
        assert!(read(&text).is_err(), "{field} = {value}: read takes it");
        assert_eq!(schemas.validate(&doc, at).is_err(), by_schema, "{field} = {value}: the schema");
    }
    let mut doc = base.clone();
    doc["files"][1]["name"] = "SHA256SUMS".into();
    assert!(read(&doc.to_string()).is_err(), "the sums as a file of the index");
    let mut doc = base;
    doc["files"][1]["name"] = doc["files"][0]["name"].clone();
    assert!(read(&doc.to_string()).is_err(), "a name two times");
}

/// A field that a later index adds is read, and the schema takes it.
#[test]
fn a_field_of_a_later_index_is_taken() {
    let (schemas, at) = validator();
    let mut doc: Value = serde_json::from_str(WRITTEN).unwrap();
    doc["provenance"] = "https://example.org/provenance".into();
    assert!(read(&doc.to_string()).is_ok());
    assert!(schemas.validate(&doc, at).is_ok());
}
