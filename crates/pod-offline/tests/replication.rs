//! The tests of the replication log: two replicas diverge, then converge,
//! and the rule resolves and logs the conflict.

mod common;

use pod_identity::Identity;
use pod_offline::replication::Replica;

#[test]
fn two_replicas_diverge_then_converge_and_the_conflict_is_logged() {
    let scratch = common::Scratch::new("replication");
    let left_id = Identity::from_seed(&[61; 32]);
    let right_id = Identity::from_seed(&[62; 32]);
    let mut left = Replica::new(&left_id);
    let mut right = Replica::new(&right_id);

    left.set_session("term", b"vi");
    right.set_session("term", b"emacs");
    left.set_peer("node-1", b"10.0.0.1");
    right.set_peer("node-2", b"10.0.0.2");
    assert_ne!(left.state(), right.state(), "the replicas did not diverge");

    let merged_left = left.merge(right.log());
    let merged_right = right.merge(left.log());
    assert_eq!(merged_left.appended, 2);
    assert_eq!(merged_right.appended, 2);
    assert_eq!(left.state(), right.state(), "the replicas did not converge");
    assert_eq!(left.conflicts(), right.conflicts());
    assert_eq!(left.conflicts().len(), 1);
    assert_eq!(left.conflicts()[0].key, "session:term");

    // The tie break is the greater hex of the actor's key at an equal count.
    let expected: &[u8] = if left_id.public().hex() > right_id.public().hex() { b"vi" } else { b"emacs" };
    assert_eq!(left.get("session:term").expect("a value"), expected);
    assert_eq!(left.get("peer:node-1").expect("a value"), b"10.0.0.1");
    assert_eq!(left.get("peer:node-2").expect("a value"), b"10.0.0.2");
    assert_eq!(left.merge(right.log()).appended, 0, "a second merge appended a record");

    // The log is the state: a third replica reads it and agrees.
    let path = scratch.join("log");
    left.save(&path).expect("the log");
    let reader = Replica::load(&path, &Identity::from_seed(&[63; 32])).expect("the log back");
    assert_eq!(reader.state(), left.state());
    assert_eq!(reader.conflicts().len(), 1);
}

#[test]
fn a_write_after_a_merge_wins_without_a_conflict() {
    let left_id = Identity::from_seed(&[64; 32]);
    let right_id = Identity::from_seed(&[65; 32]);
    let mut left = Replica::new(&left_id);
    let mut right = Replica::new(&right_id);
    left.set_session("term", b"vi");
    right.merge(left.log());
    right.set_session("term", b"nano");
    assert!(right.conflicts().is_empty());
    left.merge(right.log());
    assert_eq!(left.state(), right.state());
    assert_eq!(left.get("session:term").expect("a value"), b"nano");
    assert!(left.conflicts().is_empty());
}
