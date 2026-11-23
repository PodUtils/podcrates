//! The relocatable archive: build it, unzip it into a temporary directory,
//! run its manifest, and read one expected score for each row.

mod common;

use pod_harness::Archive;
use pod_harness::archive::expected_name;
use pod_harness::fixture::Fixtures;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
#[cfg(unix)]
use std::process::Command;

const DATE: &str = "2026-11-20";

fn fixtures() -> Fixtures {
    Fixtures::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")).unwrap()
}

#[test]
fn the_archive_unzips_and_its_manifest_lists_each_file() {
    let fixtures = fixtures();
    let run = fixtures.run(DATE).unwrap();
    let archive = Archive::build(&fixtures, &run).unwrap();
    let temp = common::TempDir::new("archive");
    let zip_path = temp.path().join("harness.zip");
    archive.write(&zip_path).unwrap();

    let read = Archive::open(&zip_path).unwrap();
    let out = temp.path().join("out");
    read.extract(&out).unwrap();
    let manifest = read.manifest().unwrap();
    let mut listed = 0;
    for line in manifest.lines() {
        let mut parts = line.split_whitespace();
        let digest = parts.next().unwrap();
        let size: usize = parts.next().unwrap().parse().unwrap();
        let name = parts.next().unwrap();
        let path = out.join(name);
        let data = std::fs::read(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(data.len(), size, "{name}");
        assert_eq!(pod_harness::capture::encode_hex(&Sha256::digest(&data)), digest, "{name}");
        listed += 1;
    }
    assert_eq!(listed + 1, read.zip().entries().len(), "the manifest lists each file but itself");
    assert!(out.join("run.sh").exists());
    assert!(out.join("simulator/middlebox.rs").exists());
    assert!(out.join("simulator/decisions.json").exists());
    assert!(out.join("expected/table.txt").exists());
}

#[test]
fn each_row_has_an_expected_score_file() {
    let fixtures = fixtures();
    let run = fixtures.run(DATE).unwrap();
    let archive = Archive::build(&fixtures, &run).unwrap();
    for row in &run.table.rows {
        let name = expected_name(&row.road, row.profile);
        let data = archive
            .zip()
            .get(&name)
            .unwrap_or_else(|| panic!("no expected file {name} for {} / {}", row.road, row.profile.as_str()));
        let value: serde_json::Value = serde_json::from_slice(data).unwrap();
        assert_eq!(value["road"], row.road);
        assert_eq!(value["profile"], row.profile.as_str());
        assert_eq!(value["date"], DATE);
        match row.scores() {
            Some(scores) => {
                assert_eq!(value["state"], "scored");
                assert_eq!(value["scores"].as_array().unwrap().len(), scores.len());
            }
            None => assert_eq!(value["state"], "skipped"),
        }
    }
    // Every expected file belongs to a row, and a skipped row has one too.
    assert!(archive.zip().get(&expected_name("icmp", pod_harness::Profile::Restricted)).is_some());
}

#[cfg(unix)]
#[test]
fn run_sh_lists_the_manifest_and_names_a_skip_with_no_binary() {
    let fixtures = fixtures();
    let run = fixtures.run(DATE).unwrap();
    let archive = Archive::build(&fixtures, &run).unwrap();
    let temp = common::TempDir::new("runsh");
    let out = temp.path().join("out");
    archive.extract(&out).unwrap();
    let manifest = archive.manifest().unwrap();

    let list = Command::new("sh").arg(out.join("run.sh")).arg("--list").output().unwrap();
    assert!(list.status.success(), "{}", String::from_utf8_lossy(&list.stderr));
    assert_eq!(String::from_utf8(list.stdout).unwrap(), manifest);

    let skip = Command::new("sh").arg(out.join("run.sh")).output().unwrap();
    assert!(skip.status.success(), "{}", String::from_utf8_lossy(&skip.stderr));
    let text = String::from_utf8(skip.stdout).unwrap();
    assert!(text.contains("skip: no prebuilt pod-harness binary given"), "{text}");
    assert!(text.contains("manifest:"), "{text}");
    assert!(text.contains("expected/table.txt"), "{text}");
}

#[cfg(unix)]
#[test]
fn run_sh_runs_a_binary_that_the_caller_gives() {
    use std::os::unix::fs::PermissionsExt as _;
    let fixtures = fixtures();
    let run = fixtures.run(DATE).unwrap();
    let archive = Archive::build(&fixtures, &run).unwrap();
    let temp = common::TempDir::new("runbin");
    let out = temp.path().join("out");
    archive.extract(&out).unwrap();
    let binary = temp.path().join("stub.sh");
    std::fs::write(&binary, "#!/bin/sh\necho \"stub $@\"\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new("sh").arg(out.join("run.sh")).arg(&binary).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("stub --archive "), "{text}");
    assert!(text.contains(out.to_str().unwrap()), "{text}");
}

#[test]
fn the_extracted_table_is_the_table_of_the_run() {
    let fixtures = fixtures();
    let run = fixtures.run(DATE).unwrap();
    let archive = Archive::build(&fixtures, &run).unwrap();
    let table = archive.zip().get("expected/table.txt").unwrap();
    assert_eq!(String::from_utf8(table.to_vec()).unwrap(), run.table.render());
    let json: serde_json::Value = serde_json::from_slice(archive.zip().get("expected/table.json").unwrap()).unwrap();
    assert_eq!(json["rows"].as_array().unwrap().len(), run.table.rows.len());
}
