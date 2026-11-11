//! The JSON Schemas of `schemas/`, generated from the types, and the
//! examples of `schemas/examples/`: each valid example passes, and each
//! invalid example fails for the reason that `expect.json` names.
//!
//! After a change of a type, write the schemas again:
//! `PODRUNNER_WRITE_SCHEMAS=1 cargo test -p podrunner-schema schemas`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;

use crate::{Capability, JobOffer, PublicReport, RunReport};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")
}

/// Each record: the name of its schema file and directory of examples.
const RECORDS: [&str; 4] = ["job-offer", "unit-capability", "run-report", "run-report-public"];

fn schema_of<T: JsonSchema>(id: &str) -> String {
    let mut s = schemars::schema_for!(T);
    s.insert("$id".into(), format!("https://github.com/PodUtils/podrunner/schemas/{id}.v1.json").into());
    serde_json::to_string_pretty(&s).expect("a schema is JSON") + "\n"
}

fn generated() -> Vec<(&'static str, String)> {
    vec![
        ("job-offer", schema_of::<JobOffer>("job-offer")),
        ("unit-capability", schema_of::<Capability>("unit-capability")),
        ("run-report", schema_of::<RunReport>("run-report")),
        ("run-report-public", schema_of::<PublicReport>("run-report-public")),
    ]
}

/// Read one example as its record; the error is the reason.
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
        "run-report-public" => read::<PublicReport>(text).map(|_| ()),
        other => Err(format!("no record {other}")),
    }
}

#[test]
fn the_schemas_are_generated_from_the_types() {
    let write = std::env::var_os("PODRUNNER_WRITE_SCHEMAS").is_some();
    for (name, text) in generated() {
        let path = root().join(format!("{name}.v1.json"));
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
    let ex = root().join("examples");
    let expect: BTreeMap<String, String> =
        serde_json::from_str(&std::fs::read_to_string(ex.join("expect.json")).expect("expect.json")).expect("a map");
    let (mut valid, mut invalid) = (0, 0);
    for record in RECORDS {
        let mut names: Vec<String> = std::fs::read_dir(ex.join(record))
            .expect("a directory of examples")
            .map(|e| e.expect("an entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for name in names {
            let key = format!("{record}/{name}");
            let text = std::fs::read_to_string(ex.join(&key)).expect("an example");
            let got = validate(record, &text);
            if name.starts_with("valid-") {
                assert_eq!(got, Ok(()), "{key}");
                valid += 1;
            } else {
                let reason = expect.get(&key).unwrap_or_else(|| panic!("expect.json has no reason for {key}"));
                let why = got.expect_err(&key);
                assert!(why.contains(reason.as_str()), "{key}: fails with {why:?}, not {reason:?}");
                invalid += 1;
            }
        }
    }
    assert_eq!((valid, invalid), (11, 19));
    assert_eq!(expect.len(), 19, "expect.json names a file that does not exist");
}

#[test]
fn the_public_report_of_a_full_report_never_names_the_host() {
    let text = std::fs::read_to_string(root().join("examples/run-report/valid-success.json")).unwrap();
    let r: RunReport = serde_json::from_str(&text).unwrap();
    let public = serde_json::to_string(&r.public()).unwrap();
    assert!(!public.contains("host"), "{public}");
    let back: PublicReport = serde_json::from_str(&public).unwrap();
    assert_eq!(back.unit_kind.as_deref(), Some("podvm"));
}
