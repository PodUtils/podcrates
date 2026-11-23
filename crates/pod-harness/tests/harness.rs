//! The full harness: the fixtures, the matrix, the benign sample and the
//! binary. Every classifier computes its score on a capture.

mod common;

use pod_harness::fixture::Fixtures;
use pod_harness::matrix::{Outcome, Profile};
use std::path::PathBuf;
use std::process::Command;

const DATE: &str = "2026-11-20";

fn fixtures() -> Fixtures {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    Fixtures::load(&dir).expect("the fixture directory loads")
}

#[test]
fn the_full_matrix_reports_a_table_with_each_road_and_profile() {
    let fixtures = fixtures();
    let run = fixtures.run(DATE).expect("the run completes");
    assert_eq!(run.table.rows.len(), fixtures.cases.len());
    assert_eq!(run.table.rows.len(), 33, "11 roads by 3 profiles");
    assert_eq!(run.table.date, DATE);
    assert!(run.elapsed.as_secs_f64() < pod_harness::BUDGET_SECS as f64);

    let text = run.table.render();
    assert!(text.contains("road"), "{text}");
    assert!(text.contains("classifier"), "{text}");
    assert!(text.contains("score"), "{text}");
    assert!(text.contains(DATE), "{text}");
    assert!(text.contains("false-positive rate"), "{text}");
    assert!(!text.contains("pass count"), "{text}");

    for row in &run.table.rows {
        let Some(scores) = row.scores() else { continue };
        assert_eq!(scores.len(), 3, "{} / {}", row.road, row.profile.as_str());
        for score in scores {
            assert!((0.0..=1.0).contains(&score.score), "{}: {score:?}", row.road);
            assert!(!score.reason.is_empty(), "{}: {score:?}", row.road);
        }
    }
    assert_eq!(run.table.rows.iter().filter(|r| r.scores().is_some()).count(), 31);
    assert_eq!(run.table.rows.iter().filter(|r| r.skip_reason().is_some()).count(), 2);

    let json: serde_json::Value = serde_json::from_str(&run.table.json()).expect("the table is JSON");
    assert_eq!(json["schema"], "pod-harness.table/1");
    assert_eq!(json["rows"].as_array().unwrap().len(), 33);
    assert_eq!(json["benign"]["samples"], 1);
}

#[test]
fn an_unpadded_road_scores_worse_than_a_padded_one() {
    let fixtures = fixtures();
    let run = fixtures.run(DATE).unwrap();
    let find = |road: &str, profile: Profile| {
        run.table
            .rows
            .iter()
            .find(|row| row.road == road && row.profile == profile)
            .unwrap_or_else(|| panic!("no row {road} / {}", profile.as_str()))
    };
    let plain = find("unix", Profile::Direct);
    let padded = find("unix", Profile::Proxy);
    let plain_score = plain.score("field").expect("a scored row");
    let padded_score = padded.score("field").expect("a scored row");
    assert!(plain_score > padded_score, "the unpadded road is {plain_score}, the padded {padded_score}");
    assert!(plain_score > 0.9, "{plain_score}");
    assert!(padded_score < 0.9, "{padded_score}");
    // The padding changed the record sizes, and the reason names the rules.
    assert!(padded.scores().unwrap().iter().any(|s| s.classifier == "field" && !s.reason.contains("small-records")));
}

#[test]
fn the_benign_cover_site_keeps_a_bounded_false_positive_rate() {
    let run = fixtures().run(DATE).unwrap();
    let benign = run.benign;
    assert_eq!(benign.samples, 1);
    assert_eq!(benign.flagged, 0);
    assert!(benign.rate() <= 0.1, "{}", benign.rate());
    assert!(run.table.render().contains("false-positive rate 0.000"));
}

#[test]
fn a_row_that_did_not_run_is_visible_as_skipped() {
    let run = fixtures().run(DATE).unwrap();
    let gone = run
        .table
        .rows
        .iter()
        .find(|row| row.road == "icmp" && row.profile == Profile::Restricted)
        .expect("the icmp restricted row is present");
    assert!(matches!(&gone.outcome, Outcome::Skipped(reason) if reason.contains("raw socket")));
    let text = run.table.render();
    let line = text.lines().find(|line| line.starts_with("icmp") && line.contains("restricted")).unwrap();
    assert!(line.contains("skip"), "{line}");
    assert!(line.contains("raw socket"), "{line}");
    let json: serde_json::Value = serde_json::from_str(&run.table.json()).unwrap();
    let row = json["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["road"] == "icmp" && row["profile"] == "restricted")
        .unwrap();
    assert_eq!(row["state"], "skipped");
    assert!(row["reason"].as_str().unwrap().contains("raw socket"));
}

#[test]
fn the_binary_prints_the_table_and_the_exit_table() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let output =
        Command::new(env!("CARGO_BIN_EXE_pod-harness")).arg("--data").arg(&dir).output().expect("the binary runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("unix") && text.contains("skip"), "{text}");
    assert!(text.contains("false-positive rate"), "{text}");
    let errors = String::from_utf8(output.stderr).unwrap();
    assert!(errors.contains("budget"), "{errors}");

    let help = Command::new(env!("CARGO_BIN_EXE_pod-harness")).arg("--help").output().unwrap();
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    for code in ["64", "66", "69", "71", "77", "78"] {
        assert!(text.contains(code), "the help lacks {code}: {text}");
    }
}

#[test]
fn a_missing_argument_is_a_usage_failure() {
    let output = Command::new(env!("CARGO_BIN_EXE_pod-harness")).output().unwrap();
    assert_eq!(output.status.code(), Some(64));
}

#[test]
fn the_capture_of_a_simulated_run_round_trips() {
    let fixtures = fixtures();
    let case = fixtures.cases.iter().find(|c| !c.faults.is_empty()).unwrap();
    let capture = pod_harness::Capture::load(&fixtures.dir.join(case.capture.as_deref().unwrap())).unwrap();
    let mut simulator = pod_harness::Middlebox::new(case.faults.clone());
    let output = simulator.apply(&capture);
    let temp = common::TempDir::new("capture");
    let path = temp.path().join("out.cap");
    output.save(&path).unwrap();
    let loaded = pod_harness::Capture::load(&path).unwrap();
    assert_eq!(loaded, output);
    assert!(!simulator.decisions().is_empty());
}

#[test]
fn the_readme_passes_the_claim_check() {
    let findings = pod_harness::check_claims(include_str!("../README.md"));
    assert!(findings.is_empty(), "{findings:?}");
}
