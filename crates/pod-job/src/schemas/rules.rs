//! The rules of T-163 on the records and their schemas: what an offer
//! refuses, what a public report never holds, and one rule for the version
//! field of each schema of `schemas/`.

use serde_json::{Value, json};

use super::{ID, RECORDS, Validator, examples, root, validate};
use crate::{JobId, JobOffer, PublicReport, RunReport, SCHEMA};

/// An example as a JSON value.
fn example(key: &str) -> Value {
    let text = std::fs::read_to_string(examples().join(key)).unwrap_or_else(|e| panic!("{key}: {e}"));
    serde_json::from_str(&text).expect("an example is JSON")
}

/// The verdict of the types on an offer, as one of a `github-scaleset`
/// source or not.
fn offer_verdict(offer: &Value, scaleset: bool) -> Result<(), String> {
    let offer: JobOffer = serde_json::from_value(offer.clone()).map_err(|e| e.to_string())?;
    offer.check(scaleset)
}

/// Each name of a property at each depth of a schema.
fn property_names(schema: &Value, out: &mut Vec<String>) {
    match schema {
        Value::Object(map) => {
            if let Some(Value::Object(properties)) = map.get("properties") {
                out.extend(properties.keys().cloned());
            }
            map.values().for_each(|v| property_names(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| property_names(v, out)),
        _ => {}
    }
}

/// An offer refuses a payload from a `github-scaleset` source, a credential
/// in the environment, an image by tag and a tier from the source. The
/// types refuse each one; the schema refuses each one but the payload of a
/// `github-scaleset` source, whose kind only the configuration knows.
#[test]
fn the_offer_refuses_what_a_source_may_not_send() {
    let validator = Validator::new();
    let base = example("job-offer/valid-push.json");
    let with = |path: &[&str], key: &str, value: Value| {
        let mut offer = base.clone();
        let mut at = &mut offer;
        for p in path {
            at = &mut at[*p];
        }
        at[key] = value;
        offer
    };
    let digest = json!(format!("sha256:{}", "a".repeat(64)));

    // A payload from a github-scaleset source: refused by check(true) only.
    let payload = with(&[], "payload", digest);
    assert_eq!(offer_verdict(&payload, true), Err("a github-scaleset source gives no payload".into()));
    assert_eq!(offer_verdict(&payload, false), Ok(()));
    assert_eq!(validator.check("job-offer", &payload), Ok(()));

    // A credential in the environment, at each depth of the offer.
    let credential = json!({ "RUNNER_TOKEN": "made-up-not-a-credential" });
    for (path, key) in
        [(&[][..], "env"), (&[][..], "environment"), (&[][..], "credential"), (&["trust_inputs"][..], "env")]
    {
        let offer = with(path, key, credential.clone());
        let why = offer_verdict(&offer, false).expect_err(key);
        assert!(why.contains(&format!("unknown field `{key}`")), "{key}: {why}");
        assert!(validator.check("job-offer", &offer).is_err(), "{path:?} {key}: the schema takes it");
    }
    let need = with(&[], "unit_need", json!({ "env": credential }));
    assert!(offer_verdict(&need, false).expect_err("env").contains("unknown field `env`"));
    assert!(validator.check("job-offer", &need).is_err());

    // An image by tag, or a reference with a name: only a bare digest.
    for image in ["runner:latest", "ghcr.io/actions/runner:2", &format!("runner@sha256:{}", "a".repeat(64))] {
        let offer = with(&[], "payload", json!(image));
        assert_eq!(offer_verdict(&offer, false), Err("the payload is not sha256:HEX".into()), "{image}");
        assert!(validator.check("job-offer", &offer).is_err(), "{image}: the schema takes it");
    }

    // A tier from the source: the runner computes it.
    for path in [&[][..], &["trust_inputs"][..]] {
        let offer = with(path, "tier", json!("trusted"));
        assert!(offer_verdict(&offer, false).expect_err("tier").contains("unknown field `tier`"));
        assert!(validator.check("job-offer", &offer).is_err(), "{path:?}: the schema takes a tier");
    }

    // No property of the schema can hold one of them.
    let mut names = Vec::new();
    let schema = super::generated().into_iter().find(|(n, _)| *n == "job-offer").expect("the offer").1;
    property_names(schema.as_value(), &mut names);
    for word in ["tier", "env", "environment", "credential", "secret", "token"] {
        assert!(!names.iter().any(|n| n.contains(word)), "the offer schema has a property {word}");
    }
}

/// The fields of the public report, in the order of the schema. The CPU
/// time and the peak of resident memory are facts of the job, not of the
/// host (T-527).
const PUBLIC: [&str; 11] = [
    "boundary",
    "cpu_s",
    "exit_reason",
    "job_id",
    "peak_rss_mib",
    "refusal_code",
    "schema",
    "source",
    "tier",
    "unit_kind",
    "wall_s",
];

/// The public report holds no host name and no detail of the boundary:
/// only the class of the boundary and the kind of the unit.
#[test]
fn the_public_report_holds_no_host_and_no_boundary_detail() {
    let validator = Validator::new();
    let schema = super::generated().into_iter().find(|(n, _)| *n == "run-report-public").expect("the report").1;
    let schema = schema.as_value();
    let keys: Vec<&String> = schema["properties"].as_object().expect("properties").keys().collect();
    assert_eq!(keys, PUBLIC);
    assert_eq!(schema["additionalProperties"], json!(false));

    let mut reports = 0;
    for name in super::names("run-report").into_iter().filter(|n| n.starts_with("valid-")) {
        let full: RunReport = serde_json::from_value(example(&format!("run-report/{name}"))).expect("a report");
        let public = full.public();
        assert_eq!(public.check(), Ok(()), "{name}");
        let value = serde_json::to_value(&public).expect("a public report is JSON");
        assert_eq!(validator.check("run-report-public", &value), Ok(()), "{name}");
        let text = value.to_string();
        let mut details = vec![full.host.clone()];
        if let Some(unit) = &full.unit {
            details.extend([unit.unit_id.to_string(), unit.payload.clone()]);
        }
        for detail in details {
            assert!(!text.contains(&detail), "{name}: the public report holds {detail:?}: {text}");
        }
        for key in value.as_object().expect("an object").keys() {
            assert!(public_has(key), "{name}: {key}");
        }
        reports += 1;
    }
    assert_eq!(reports, 5);

    // A detail added to a public report is refused by the types and by the
    // schema.
    let base = example("run-report-public/valid-success.json");
    let details = [
        ("host", json!("host-a")),
        ("unit", json!({})),
        ("unit_id", json!("u-7")),
        ("boundary_proof", json!("guest-self-test")),
        ("principal", json!("shared")),
        ("egress_hold", json!("advisory")),
        ("payload", json!(format!("sha256:{}", "a".repeat(64)))),
        ("residual_processes", json!(0)),
        ("exit_code", json!(0)),
        ("claim_to_first_step_s", json!(12)),
        ("job_images", json!([format!("sha256:{}", "b".repeat(64))])),
    ];
    for (key, value) in details {
        let mut report = base.clone();
        report[key] = value;
        let read = serde_json::from_value::<PublicReport>(report.clone()).map(|_| ()).map_err(|e| e.to_string());
        assert!(read.expect_err(key).contains(&format!("unknown field `{key}`")));
        assert!(validator.check("run-report-public", &report).is_err(), "{key}: the schema takes it");
    }

    fn public_has(key: &str) -> bool {
        PUBLIC.contains(&key)
    }
}

/// The `policy` of an offer is an empty object or a `broker-policy.v1`
/// policy (T-178). The check reads the top level; the schema of the offer
/// holds the whole `broker-policy.v1` schema by its `$ref`, so it also
/// refuses a policy whose rule `pod-broker` refuses.
#[test]
fn the_policy_of_an_offer_is_a_broker_policy() {
    let validator = Validator::new();
    let base = example("job-offer/valid-limits-and-need.json");
    let with = |policy: Value| {
        let mut offer = base.clone();
        offer["policy"] = policy;
        (offer_verdict(&offer, false), validator.check("job-offer", &offer).map_err(|_| ()))
    };
    for good in [
        json!({}),
        json!({"schema": 1, "mode": "open"}),
        json!({"schema": 1, "mode": "allow", "allow": [{"host": "*.github.com", "ports": [443]}], "deny": ["10.0.0.0/8"]}),
    ] {
        assert_eq!(with(good.clone()), (Ok(()), Ok(())), "{good}");
    }
    for (bad, why) in [
        (json!({"allow": ["github.com:443"]}), "schema is not 1"),
        (json!({"schema": 1, "mode": "allow", "token": "x"}), "unknown key \"token\""),
        (json!({"schema": 2, "mode": "open"}), "schema is not 1"),
        (json!({"schema": 1, "mode": "closed"}), "mode is not allow or open"),
        (json!({"schema": 1, "mode": "allow", "allow": ["github.com:443"]}), "allow is not a list of rules"),
        (json!({"schema": 1, "mode": "allow", "deny": [1]}), "deny is not a list of strings"),
    ] {
        let (check, schema) = with(bad.clone());
        let check = check.expect_err(&bad.to_string());
        assert!(check.contains("the policy is not a broker-policy.v1 policy") && check.contains(why), "{bad}: {check}");
        assert!(schema.is_err(), "{bad}: the schema takes it");
    }
    // A rule that `pod-broker` refuses: the schema refuses it, and the
    // consumer refuses it with `egress-policy-unsupported`.
    for deep in [
        json!({"schema": 1, "mode": "allow", "allow": [{"host": ""}]}),
        json!({"schema": 1, "mode": "allow", "allow": [{"host": "a.example", "path": "v1"}]}),
        json!({"schema": 1, "mode": "allow", "allow": [{"host": "a.example", "extra": 1}]}),
    ] {
        assert_eq!(with(deep.clone()).1, Err(()), "{deep}: the schema takes it");
    }
}

/// One rule for the version of each schema of `schemas/` (T-177): the file
/// is `NAME.vN.schema.json`, its `$id` is the URL of that file on `main`, it
/// is JSON Schema 2020-12, and the object has a field `schema`, its first
/// required key, whose value is the constant N. The records of this crate,
/// the log line and the broker policy have the same rule. Each schema
/// compiles in a validator on Rust's regex.
#[test]
fn each_schema_names_its_version_one_way() {
    let mut files: Vec<String> = std::fs::read_dir(root())
        .expect("schemas/")
        .map(|e| e.expect("an entry").file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    let mut names = Vec::new();
    for file in &files {
        let (name, n) = file
            .strip_suffix(".schema.json")
            .and_then(|stem| stem.rsplit_once(".v"))
            .and_then(|(name, n)| Some((name, n.parse::<u64>().ok()?)))
            .unwrap_or_else(|| panic!("{file} is not NAME.vN.schema.json"));
        let text = std::fs::read_to_string(root().join(file)).expect("a schema");
        let schema: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(schema["$id"], json!(format!("{ID}{file}")), "{file}");
        assert_eq!(schema["$schema"], json!("https://json-schema.org/draft/2020-12/schema"), "{file}");
        assert_eq!(schema["additionalProperties"], json!(false), "{file}");
        let mut compiler = boon::Compiler::new();
        for each in &files {
            let value = serde_json::from_str(&std::fs::read_to_string(root().join(each)).expect("a schema"));
            compiler.add_resource(&format!("{ID}{each}"), value.expect("a schema is JSON")).expect("a resource");
        }
        compiler.compile(&format!("{ID}{file}"), &mut boon::Schemas::new()).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(schema["properties"]["schema"]["const"], json!(n), "{file}: schema is not the constant {n}");
        assert_eq!(schema["required"][0], json!("schema"), "{file}: schema is not the first required key");
        names.push(name.to_string());
    }
    for record in RECORDS {
        assert!(names.iter().any(|n| n == record), "{record}: no file in schemas/");
    }
    assert!(names.len() > RECORDS.len(), "the test reads the other schemas of schemas/ too");
    assert_eq!(SCHEMA, 1, "the files of the records are NAME.v1");
}

/// The pattern of a job id in the schema is the rule of [`JobId::new`]:
/// it has no look-around, so the three branches must take the same ids.
#[test]
fn the_job_id_pattern_is_the_rule_of_the_type() {
    let validator = Validator::new();
    let base = example("job-offer/valid-push.json");
    let long = "a".repeat(128);
    let longer = "a".repeat(129);
    let dots = format!("..{}", "a".repeat(126));
    let ids = [
        "",
        ".",
        "..",
        "...",
        ".a",
        "a.",
        "..a",
        "a..",
        "._",
        "-",
        "_",
        "a/b",
        "../x",
        "a b",
        "\u{e9}",
        "a\0",
        "j-1",
        &long,
        &longer,
        &dots,
        &format!("{dots}a"),
        &format!(".{}", "a".repeat(127)),
    ];
    for id in ids {
        let mut offer = base.clone();
        offer["job_id"] = json!(id);
        let types = JobId::new(id).is_ok();
        assert_eq!(validator.check("job-offer", &offer).is_ok(), types, "{id:?}");
        assert_eq!(validate("job-offer", &offer.to_string()).is_ok(), types, "{id:?}");
    }
}

/// Each rule of a report: for each exit reason, with and without a refusal
/// code and a unit, the `check` of the full and of the public report and
/// their schemas give one verdict, and the public report of a full report
/// that passes passes.
#[test]
fn the_rules_of_a_report_are_the_rules_of_its_schema() {
    use crate::{ExitReason, RefusalCode};
    let validator = Validator::new();
    let full: RunReport = serde_json::from_value(example("run-report/valid-success.json")).expect("a report");
    let unit = full.unit.clone().expect("a unit");
    let reasons = [
        ExitReason::Success,
        ExitReason::Failure,
        ExitReason::Cancelled,
        ExitReason::Timeout,
        ExitReason::Limit,
        ExitReason::Refused,
        ExitReason::Withdrawn,
        ExitReason::JobMismatch,
        ExitReason::Lost,
        ExitReason::UnitFailed,
    ];
    let (mut passed, mut refused) = (0, 0);
    for exit_reason in reasons {
        for code in [None, Some(RefusalCode::ImageStale)] {
            for ran in [None, Some(unit.clone())] {
                let report = RunReport { exit_reason, refusal_code: code, unit: ran, ..full.clone() };
                let value = serde_json::to_value(&report).expect("a report is JSON");
                let types = report.check();
                assert_eq!(validator.check("run-report", &value).is_ok(), types.is_ok(), "{value}");
                let public = report.public();
                let value = serde_json::to_value(&public).expect("a report is JSON");
                assert_eq!(public.check().is_ok(), types.is_ok(), "{value}");
                assert_eq!(validator.check("run-report-public", &value).is_ok(), types.is_ok(), "{value}");
                if types.is_ok() { passed += 1 } else { refused += 1 }
            }
        }
    }
    // Of 40 reports: 8 reasons with no code, with and without a unit; a
    // withdrawn report with no unit; a refused report with a code and no
    // unit.
    assert_eq!((passed, refused), (18, 22));
}
