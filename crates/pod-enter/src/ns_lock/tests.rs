use super::*;
use crate::ns_setup_step;

/// A map reads as its lines of three numbers; anything else does not.
#[test]
fn a_map_reads_as_its_lines() {
    let host = "         0          0 4294967295\n";
    assert_eq!(map_lines(host), Some(vec![(0, 0, u32::MAX)]));
    let range = "         0       1000          1\n         1     100000      65536\n";
    assert_eq!(map_lines(range), Some(vec![(0, 1000, 1), (1, 100000, 65536)]));
    for bad in ["", "0 1000\n", "0 x 1\n", "0 1000 1 2\n"] {
        assert_eq!(map_lines(bad), None, "{bad:?}");
    }
}

/// The identity keeps the ids inside and the count of each line.
#[test]
fn the_identity_maps_each_id_inside_to_itself() {
    assert_eq!(identity(&[(0, 0, u32::MAX)]), b"0 0 4294967295\n");
    assert_eq!(identity(&[(0, 1000, 1), (1, 100000, 65536)]), b"0 0 1\n1 1 65536\n");
}

/// One id that the caller holds: the child writes `0 0 1` itself. Host
/// root, a subordinate range, and a group that the caller does not hold:
/// the parent writes the identity of each line.
#[test]
fn the_child_writes_one_id_and_the_parent_each_other_map() {
    let one = lock((0, 0), "0 1000 1\n", "0 1000 1\n").unwrap();
    let maps = one.child.as_ref().expect("the child writes the map of one id");
    assert_eq!((maps.uid_line(), maps.gid_line()), (&b"0 0 1\n"[..], &b"0 0 1\n"[..]));
    assert!(one.parent_lines.is_none() && one.mount && !one.host_root);

    let host = lock((0, 0), "0 0 4294967295\n", "0 0 4294967295\n").unwrap();
    assert!(host.child.is_none() && host.host_root);
    assert_eq!(host.parent_lines, Some((b"0 0 4294967295\n".to_vec(), b"0 0 4294967295\n".to_vec())));

    let range = lock((0, 0), "0 1000 1\n1 100000 65536\n", "0 1000 1\n1 100000 65536\n").unwrap();
    assert!(range.child.is_none() && !range.host_root);
    assert_eq!(range.parent_lines.as_ref().map(|l| l.0.as_slice()), Some(&b"0 0 1\n1 1 65536\n"[..]));

    let other_group = lock((0, 5), "0 1000 1\n", "0 1000 1\n").unwrap();
    assert!(other_group.child.is_none() && other_group.parent_lines.is_some());

    let e = lock((0, 0), "", "0 1000 1\n").err().map(|e| e.to_string());
    assert!(e.is_some_and(|e| e.contains("/proc/self/uid_map does not read as a map")));
}

/// The refused unshare of host root is a setup step of the rung: the
/// entry falls back to chroot. Each other step of the lock stops the
/// payload.
#[test]
fn only_the_unshare_of_host_root_is_a_setup_step() {
    assert_eq!(
        ns_setup_step(NS_LOCK_USER_STEP),
        Some("unshare(CLONE_NEWUSER) of the nested user namespace of host root")
    );
    assert_eq!(ns_setup_step(NS_NESTED_NS_STEP), None);
    assert_eq!(nested_step(NS_LOCK_USER_STEP), Some(Step::Unshare));
    for b in [NS_LOCK_USER_STEP, NS_NESTED_NS_STEP] {
        assert!(b > crate::NS_TMPFS_LIMIT_STEP, "the byte {b} is the byte of another step");
        assert_ne!(b, crate::ns_join::NS_JOIN_OWNER_STEP);
    }
}

/// The pipes of the parent exist only where the parent writes the maps.
#[test]
fn the_pipes_exist_only_for_the_parent_writer() {
    let child = lock((0, 0), "0 1000 1\n", "0 1000 1\n").unwrap();
    let plan = crate::ns_user::tests::plan(None);
    assert!(Pipes::of(&plan, Some(&child)).unwrap().0.is_none());
    assert!(Pipes::of(&plan, None).unwrap().0.is_none());
    let host = lock((0, 0), "0 0 4294967295\n", "0 0 4294967295\n").unwrap();
    let pipes = Pipes::of(&plan, Some(&host)).unwrap();
    let fds = pipes.0.expect("the pipes of the parent writer");
    assert!(fds.iter().all(|fd| *fd >= 0));
    pipes.close_child_ends();
    pipes.close_parent_ends();
}
