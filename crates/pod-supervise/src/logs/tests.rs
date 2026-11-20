//! The reads of a log through the stream: the tail of a large log, the
//! bound of a follow, a link at the log.

use super::*;
use crate::tests::{Fields, scratch};
use pod_core::Failure;

fn made(s: &Settings, name: &str) -> Container<Fields> {
    crate::create(s, Some(name), vec!["true".into()], Vec::new(), "/".into(), Fields::default()).unwrap()
}

/// Set the state of the record `name`: an ended run ends a follow.
fn set_state(s: &Settings, name: &str, state: State) {
    table::update(s, |t: &mut table::Table<Fields>| {
        let c = t.containers.iter_mut().find(|c| c.name == name).expect("the record");
        c.state = state;
        Ok::<_, Failure>(())
    })
    .unwrap();
}

/// A log of lines of 100 bytes, larger than three pieces, then `end`.
fn large_log(end: &[u8]) -> Vec<u8> {
    let mut log = Vec::new();
    while log.len() < 3 * stream::CHUNK + 1000 {
        log.extend_from_slice(&[b'y'; 99]);
        log.push(b'\n');
    }
    log.extend_from_slice(end);
    log
}

/// `follow_tail` of a log larger than a piece writes the tail of the whole
/// read, then ends with the run.
#[test]
fn follow_tail_of_a_log_larger_than_a_piece_writes_the_tail_of_the_whole_read() {
    let s = scratch("tail-large");
    let c = made(&s, "t1");
    let log = large_log(b"last-but-one\nlast\n");
    std::fs::write(table::log_path(&s, &c.id), &log).unwrap();
    set_state(&s, "t1", State::Exited);
    for n in [0u64, 1, 2, 3, 700, 10_000] {
        let mut out = Vec::new();
        follow_tail::<Fields>(&s, "t1", &mut out, n).unwrap();
        assert_eq!(out, log[tail_offset(&log, n) as usize..], "n={n}");
    }
    let mut out = Vec::new();
    follow_from::<Fields>(&s, "t1", &mut out, 5).unwrap();
    assert_eq!(out, log[5..]);
    let _ = std::fs::remove_dir_all(&s.root);
}

/// `follow_from_limit` stops at its bound while the container runs, and
/// gives the count of a log shorter than the bound after the run ended.
#[test]
fn follow_from_limit_ends_at_the_bound_while_the_container_runs() {
    let s = scratch("follow-limit");
    let c = made(&s, "l1");
    std::fs::write(table::log_path(&s, &c.id), b"0123456789").unwrap();
    let mut out = Vec::new();
    // A created container is not terminal: with no bound, this follow
    // would not end.
    assert_eq!(follow_from_limit::<Fields>(&s, "l1", &mut out, 2, 4).unwrap(), 4);
    assert_eq!(out, b"2345");
    let mut out = Vec::new();
    assert_eq!(follow_from_limit::<Fields>(&s, "l1", &mut out, 0, 0).unwrap(), 0);
    assert!(out.is_empty());
    set_state(&s, "l1", State::Exited);
    let mut out = Vec::new();
    assert_eq!(follow_from_limit::<Fields>(&s, "l1", &mut out, 0, 100).unwrap(), 10);
    assert_eq!(out, b"0123456789");
    let _ = std::fs::remove_dir_all(&s.root);
}

/// A link at the log is refused by each read: `logs`, `follow_tail` and
/// `open_log` read no byte of its target.
#[test]
fn a_link_at_the_log_is_refused_by_each_read() {
    let s = scratch("logs-link");
    let c = made(&s, "k1");
    let target = s.root.join("outside");
    std::fs::write(&target, b"a secret of the user\n").unwrap();
    std::os::unix::fs::symlink(&target, table::log_path(&s, &c.id)).unwrap();
    set_state(&s, "k1", State::Exited);
    assert!(logs::<Fields>(&s, "k1").is_err(), "logs read through a link");
    let mut out = Vec::new();
    assert!(follow_tail::<Fields>(&s, "k1", &mut out, 1).is_err(), "follow_tail read through a link");
    assert!(out.is_empty());
    assert!(stream::open_log::<Fields>(&s, "k1").is_err(), "open_log opened a link");
    let _ = std::fs::remove_dir_all(&s.root);
}
