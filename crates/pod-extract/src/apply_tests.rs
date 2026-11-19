//! The ownership decision of [`super::resolve_ids`].

use super::*;

/// The measured case: `etc/shadow` of alpine is uid 0, gid 42. An
/// extraction as uid 0 gid 0 keeps the uid and drops the gid, and the
/// reason says which.
#[test]
fn shadow_keeps_its_uid_and_reports_the_dropped_gid() {
    let ids = Ids { uid: 0, gid: 0 };
    let (u, g, why) = resolve_ids(0, 42, ids);
    assert_eq!((u, g), (0, 0));
    assert_eq!(why.as_deref(), Some("gid 42 unmapped"));
}

/// A file that the image gives to the extracting id has nothing dropped and
/// no reason: a sidecar in which each row claims a gap is not read.
#[test]
fn an_already_matching_owner_records_no_reason() {
    let ids = Ids { uid: 0, gid: 0 };
    assert_eq!(resolve_ids(0, 0, ids).2, None);
}

/// The ids are the ids of the PROCESS, not a fixed 0. An extraction as a
/// user records that user, and the reason names both gaps.
#[test]
fn extracting_as_a_non_root_id_records_that_id() {
    let ids = Ids { uid: 1000, gid: 1000 };
    let (u, g, why) = resolve_ids(0, 42, ids);
    assert_eq!((u, g), (1000, 1000));
    let why = why.unwrap();
    assert!(why.contains("uid 0 unmapped"), "{why}");
    assert!(why.contains("gid 42 unmapped"), "{why}");
}
