//! Two emulated links at 10:1 carry a session within 20 percent of the sum,
//! and a flow does not move mid-stream.

use pod_sidecar::{Bond, BondError};

#[test]
fn two_links_at_ten_to_one_carry_one_session_within_twenty_percent() {
    let mut bond = Bond::new(&[(1, 10), (2, 1)]).unwrap();
    assert_eq!(bond.assign(1), Some(1), "the first flow takes the larger link");
    let capacity = bond.estimate(1).unwrap();
    let sum = bond.sum();
    assert_eq!(sum, 11);
    assert!(capacity * 100 >= sum * 80, "{capacity} of {sum} is not within 20 percent of the sum");
    // The session sends a thousand packets; its link does not change.
    for _ in 0..1000 {
        assert_eq!(bond.send(1, 1200).unwrap(), 1);
    }
    assert_eq!(bond.moves(), 0);
    assert_eq!(bond.link(1).unwrap().bytes, 1000 * 1200);
}

#[test]
fn a_second_flow_takes_the_smaller_link() {
    let mut bond = Bond::new(&[(1, 10), (2, 1)]).unwrap();
    bond.assign(1);
    assert_eq!(bond.assign(2), Some(2));
    assert_eq!(bond.assign(3), Some(1), "the load, not the count alone, decides");
    for _ in 0..10 {
        bond.send(2, 100).unwrap();
    }
    assert_eq!(bond.link(2).unwrap().bytes, 1000);
}

#[test]
fn a_flow_moves_only_when_its_link_fails() {
    let mut bond = Bond::new(&[(1, 10), (2, 1)]).unwrap();
    bond.assign(1);
    bond.assign(2);
    for _ in 0..10 {
        bond.send(1, 100).unwrap();
        bond.send(2, 100).unwrap();
    }
    assert_eq!(bond.moves(), 0);
    bond.fail(1);
    assert_eq!(bond.moves(), 1);
    assert_eq!(bond.link_of(1), Some(2));
    assert_eq!(bond.link_of(2), Some(2), "a flow on a live link does not move");
    assert_eq!(bond.send(1, 100).unwrap(), 2);
    assert_eq!(bond.moves(), 1, "a flow moves once");
}

#[test]
fn a_failed_link_leaves_the_sum_and_a_named_error_when_no_link_is_left() {
    let mut bond = Bond::new(&[(1, 10), (2, 1)]).unwrap();
    bond.assign(7);
    bond.fail(1);
    assert_eq!(bond.sum(), 1);
    assert_eq!(bond.estimate(7), Some(1));
    bond.fail(2);
    assert_eq!(bond.sum(), 0);
    assert_eq!(bond.send(7, 1), Err(BondError::NoLink));
    assert_eq!(bond.estimate(7), None);
}

#[test]
fn a_bad_link_is_refused() {
    assert_eq!(Bond::new(&[(1, 0)]).unwrap_err(), BondError::NoCapacity(1));
    assert_eq!(Bond::new(&[(1, 1), (1, 2)]).unwrap_err(), BondError::DuplicateLink(1));
    assert_eq!(Bond::new(&[]).unwrap_err(), BondError::NoLink);
    let mut bond = Bond::new(&[(1, 1)]).unwrap();
    assert_eq!(bond.send(9, 1), Err(BondError::UnknownFlow(9)));
    assert_eq!(bond.assign(9), Some(1));
    assert_eq!(bond.assign(9), Some(1), "an assigned flow keeps its link");
}
