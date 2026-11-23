//! The tests of the capture sink: redaction when a record is written, a
//! stable merge by time, and no secret in either file.

mod common;

use pod_offline::capture::{self, Capture, Direction};

fn holds(raw: &[u8], secret: &[u8]) -> bool {
    raw.windows(secret.len()).any(|window| window == secret)
}

#[test]
fn two_captures_merge_by_time_in_a_stable_order_and_no_secret_survives() {
    let scratch = common::Scratch::new("capture-merge");
    let secrets = vec![b"TOPSECRET-ONE".to_vec(), b"TOPSECRET-TWO".to_vec()];
    let one = scratch.join("one.cap");
    let two = scratch.join("two.cap");

    let mut first = Capture::create(&one, secrets.clone()).expect("a capture");
    assert!(!first.record(30, Direction::Out, b"plain thirty").expect("a record"));
    assert!(first.record(10, Direction::In, b"first TOPSECRET-ONE").expect("a record"));
    let mut second = Capture::create(&two, secrets.clone()).expect("a capture");
    assert!(second.record(20, Direction::Out, b"second TOPSECRET-TWO").expect("a record"));
    assert!(!second.record(10, Direction::In, b"other ten").expect("a record"));
    drop(first);
    drop(second);

    for path in [&one, &two] {
        let raw = std::fs::read(path).expect("the file");
        for secret in &secrets {
            assert!(!holds(&raw, secret), "{} holds a secret", path.display());
        }
    }

    let out = scratch.join("merged.cap");
    let merged = capture::merge(&[one.clone(), two.clone()], &out, secrets.clone()).expect("a merge");
    let times: Vec<u64> = merged.iter().map(|record| record.timestamp).collect();
    assert_eq!(times, vec![10, 10, 20, 30]);
    assert_eq!(merged[0].source, 0);
    assert_eq!(merged[1].source, 1);
    let marker: Vec<bool> = merged.iter().map(|record| record.redacted).collect();
    assert_eq!(marker, vec![true, false, true, false]);
    assert_eq!(merged[0].bytes, b"first <redacted>");
    assert_eq!(merged[1].bytes, b"other ten");

    let raw = std::fs::read(&out).expect("the merged file");
    for secret in &secrets {
        assert!(!holds(&raw, secret), "the merged file holds a secret");
    }

    let again = scratch.join("merged-again.cap");
    capture::merge(&[one, two], &again, secrets).expect("the second merge");
    assert_eq!(std::fs::read(&out).expect("one"), std::fs::read(&again).expect("two"));
}

#[test]
fn a_record_that_needs_no_redaction_says_so() {
    let scratch = common::Scratch::new("capture-plain");
    let path = scratch.join("plain.cap");
    let mut capture = Capture::create(&path, vec![b"secret".to_vec()]).expect("a capture");
    assert!(!capture.record(1, Direction::Out, b"nothing to hide").expect("a record"));
    let read = capture::records(&path).expect("the records");
    assert_eq!(read.len(), 1);
    assert!(!read[0].redacted);
    assert_eq!(read[0].bytes, b"nothing to hide");
}
