//! The tests of the spool: a process that dies between `receive` and
//! `consume` loses nothing, and a corrupt entry is reported with its file
//! and its offset.

mod common;

use pod_identity::Identity;
use pod_offline::spool::Spool;

const CHILD: &str = "POD_OFFLINE_SPOOL_CHILD";

#[test]
fn a_kill_between_receive_and_consume_loses_nothing() {
    if std::env::var_os(CHILD).is_some() {
        return;
    }
    let scratch = common::Scratch::new("spool-kill");
    let dir = scratch.join("spool");
    let sender = Identity::from_seed(&[21; 32]);
    {
        let spool = Spool::open(&dir).expect("a spool");
        spool.put(&sender, 1, b"one").expect("the first entry");
        spool.put(&sender, 2, b"two").expect("the second entry");
    }

    // The child receives the first entry and dies before it consumes it.
    let status = std::process::Command::new(std::env::current_exe().expect("the test binary"))
        .args(["--exact", "receive_then_abort", "--nocapture"])
        .env(CHILD, &dir)
        .status()
        .expect("a child process");
    assert!(!status.success(), "the child did not die as the test expects");

    let spool = Spool::open(&dir).expect("the spool again");
    let entries = spool.entries().expect("the entries");
    assert_eq!(entries.len(), 2, "an entry was lost with the process");
    let first = spool.receive().expect("a read").expect("the first entry");
    assert_eq!(first.sequence, 1);
    assert_eq!(first.body, b"one");
    spool.consume(&first).expect("the consume");
    assert_eq!(spool.entries().expect("the rest").len(), 1);
}

/// The child test. It runs only with the variable of the parent, and it
/// aborts between the read and the consume.
#[test]
fn receive_then_abort() {
    let Some(dir) = std::env::var_os(CHILD) else { return };
    let spool = Spool::open(dir).expect("the spool");
    let entry = spool.receive().expect("a read").expect("an entry");
    assert_eq!(entry.body, b"one");
    // A kill: no destructor of the test and no consume run.
    std::process::abort();
}

#[test]
fn a_corrupt_entry_is_reported_with_its_file_and_offset() {
    let scratch = common::Scratch::new("spool-corrupt");
    let spool = Spool::open(scratch.join("spool")).expect("a spool");
    let sender = Identity::from_seed(&[22; 32]);
    let entry = spool.put(&sender, 7, b"repair me").expect("an entry");
    let mut bytes = std::fs::read(entry.path()).expect("the file");
    let body_start = bytes.len() - entry.body.len();
    bytes[body_start + 1] ^= 0x01;
    std::fs::write(entry.path(), &bytes).expect("the change");

    let faults = spool.check().expect("the check");
    assert_eq!(faults.len(), 1);
    assert_eq!(faults[0].file, entry.path());
    assert_eq!(faults[0].offset, body_start as u64);
    assert!(faults[0].why.contains("checksum"), "{}", faults[0].why);
    assert!(spool.receive().expect("a read").is_none(), "a corrupt entry was taken");
}

#[test]
fn a_stored_entry_carries_its_sequence_and_its_sender() {
    let scratch = common::Scratch::new("spool-header");
    let spool = Spool::open(scratch.join("spool")).expect("a spool");
    let sender = Identity::from_seed(&[23; 32]);
    let entry = spool.put(&sender, 9, b"body").expect("an entry");
    let text = std::fs::read_to_string(entry.path()).expect("the file as text");
    assert!(text.starts_with("pod-offline spool entry 1\n"), "{text}");
    assert!(text.contains("sequence: 9\n"));
    assert!(text.contains(&format!("sender: {}\n", sender.public().hex())));
    assert!(text.contains("length: 4\n"));
    assert!(text.contains("checksum: "));
}
