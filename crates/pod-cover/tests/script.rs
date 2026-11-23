//! The cover script: its TOML, its named refusals, its determined replay and
//! its split.

use std::path::PathBuf;

use pod_cover::{Record, Script, ScriptError};

const SCRIPT: &str = r#"
date = "2026-10-30"
cover_version = "1.7.2"
seed = 42

[[record_size]]
min = 512
max = 640
count = 50

[[record_size]]
min = 641
max = 768
count = 30

[[record_size]]
min = 769
max = 1024
count = 20

[[timing]]
micros = 1200
jitter_micros = 80
count = 70

[[timing]]
micros = 2400
jitter_micros = 120
count = 30

[split]
min = 512
max = 1600
"#;

fn parsed() -> Script {
    Script::parse(SCRIPT).expect("a script")
}

#[test]
fn a_script_replays_a_determined_sequence() {
    let script = parsed();
    let first = script.replay(100_000);
    let second = script.replay(100_000);
    assert_eq!(first, second);
    assert_eq!(first.iter().map(|r| r.size).sum::<usize>(), 100_000);
    // A different length has its own last record but the same start.
    let short = script.replay(4096);
    assert_eq!(short.iter().map(|r| r.size).sum::<usize>(), 4096);
    assert_eq!(&short[..short.len() - 1], &first[..short.len() - 1]);
    // No record is empty and each delay is inside the timing range.
    for record in &first {
        assert!(record.size > 0);
        let micros = record.delay.as_micros() as u64;
        assert!((1120..=1280).contains(&micros) || (2280..=2520).contains(&micros), "{micros}");
    }
}

#[test]
fn a_replay_reproduces_the_recorded_histogram() {
    let script = parsed();
    // 0.3 percent of each bucket: the replay has about 150,000 records at a
    // mean of 680 bytes, so the sampling error is below 0.15 percent.
    let records: Vec<Record> = script.replay(100_000_000);
    let total = records.len() as f64;
    for bucket in &script.record_size {
        let seen = records.iter().filter(|r| (bucket.min..=bucket.max).contains(&r.size)).count();
        let want = bucket.count as f64 / 100.0;
        let got = seen as f64 / total;
        assert!((got - want).abs() < 0.003, "bucket {}..={}: {got} for {want}", bucket.min, bucket.max);
    }
}

#[test]
fn a_missing_script_fails_at_load() {
    let path = std::env::temp_dir().join(format!("pod-cover-no-script-{}.toml", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let error = Script::load(&path).expect_err("a missing script");
    assert!(matches!(error, ScriptError::MissingFile(_)), "{error}");
    assert!(error.to_string().contains("is not there"), "{error}");
}

#[test]
fn a_script_with_no_date_or_no_cover_version_is_refused_by_name() {
    let no_date = SCRIPT.replace("date = \"2026-10-30\"\n", "");
    assert!(matches!(Script::parse(&no_date), Err(ScriptError::MissingDate)));
    let no_version = SCRIPT.replace("cover_version = \"1.7.2\"\n", "");
    assert!(matches!(Script::parse(&no_version), Err(ScriptError::MissingCoverVersion)));
    let no_split = SCRIPT.replace("[split]\nmin = 512\nmax = 1600\n", "");
    assert!(matches!(Script::parse(&no_split), Err(ScriptError::MissingSplit)));
    let empty_date = SCRIPT.replace("date = \"2026-10-30\"", "date = \"\"");
    assert!(matches!(Script::parse(&empty_date), Err(ScriptError::MissingDate)));
    for (text, word) in [(no_date, "no date"), (no_version, "no cover_version"), (no_split, "no split policy")] {
        let error = Script::parse(&text).expect_err("a refusal");
        assert!(error.to_string().contains(word), "{error}");
    }
}

#[test]
fn a_value_that_is_not_a_measurement_is_refused() {
    for change in [
        ("date = \"2026-10-30\"", "date = \"30-10-2026\""),
        ("count = 50", "count = 0"),
        ("min = 512\nmax = 640", "min = 640\nmax = 512"),
        ("max = 1600", "max = 20000"),
        ("jitter_micros = 80", "jitter_micros = 2000000"),
    ] {
        let text = SCRIPT.replace(change.0, change.1);
        assert!(matches!(Script::parse(&text), Err(ScriptError::Invalid(_))), "{change:?}");
    }
    let not_toml = "date = [\n";
    assert!(matches!(Script::parse(not_toml), Err(ScriptError::Parse(_))));
}

#[test]
fn the_split_policy_splits_a_payload_within_its_range() {
    let script = parsed();
    let payload = vec![7u8; 20_000];
    let parts = script.split_payload(&payload);
    assert_eq!(parts.concat(), payload);
    assert!(parts.len() > 1);
    for part in &parts[..parts.len() - 1] {
        assert!((script.split.min..=script.split.max).contains(&part.len()), "{}", part.len());
    }
    assert!(script.split_payload(&[]).is_empty());
}

#[test]
fn the_loader_reads_the_file_and_its_values() {
    let dir = std::env::temp_dir().join(format!("pod-cover-script-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temporary directory");
    let path: PathBuf = dir.join("cover.toml");
    std::fs::write(&path, SCRIPT).expect("the script file");
    let script = Script::load(&path).expect("a script");
    assert_eq!(script.date, "2026-10-30");
    assert_eq!(script.cover_version, "1.7.2");
    assert_eq!(script.record_size.len(), 3);
    assert_eq!(script.split.min, 512);
    let _ = std::fs::remove_dir_all(&dir);
}
