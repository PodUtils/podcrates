use super::*;
use crate::ns_setup_step;

/// The bind, the pivot and the detach are setup steps of the rung, each
/// with its name; the read-only root and its `/dev` are not: they stop
/// the payload with a refusal that names the read-only root.
#[test]
fn the_steps_of_the_root_have_names_and_kinds() {
    assert_eq!(ns_setup_step(NS_ROOT_BIND_STEP), Some("binding the root onto itself"));
    assert_eq!(ns_setup_step(NS_PIVOT_STEP), Some("pivot_root into the root"));
    assert_eq!(ns_setup_step(NS_OLD_ROOT_STEP), Some("detaching the old root"));
    for b in [NS_READ_ONLY_STEP, NS_DEV_KEEP_STEP, NS_TMP_POINT_STEP, crate::ns_mask::NS_PROC_MASK_STEP] {
        assert_eq!(ns_setup_step(b), None, "the byte {b}");
    }
    let e = failure(NS_READ_ONLY_STEP, sys::EPERM, 0).unwrap().to_string();
    assert!(e.contains("the read-only root of the plan: remounting the root read-only failed with EPERM (1)"), "{e}");
    assert!(e.ends_with("the payload did not run"), "{e}");
    let e = failure(NS_READ_ONLY_STEP, sys::EROFS, 1).unwrap().to_string();
    assert!(e.contains("the root does not read as read-only after its read-only remount"), "{e}");
    let e = failure(NS_DEV_KEEP_STEP, sys::ELOOP, 1).unwrap().to_string();
    assert!(e.contains("opening /dev of the root with no link followed failed with ELOOP"), "{e}");
    let e = failure(NS_TMP_POINT_STEP, sys::ELOOP, 0).unwrap().to_string();
    assert!(e.contains("the /tmp of the image is not a directory, and the entry does not fall back"), "{e}");
    let e = failure(crate::ns_mask::NS_PROC_MASK_STEP, sys::EPERM, 3).unwrap().to_string();
    assert!(e.contains("the mask of /proc/sys for a payload of uid 0 of the host failed with EPERM"), "{e}");
    assert!(failure(NS_PIVOT_STEP, sys::EINVAL, 0).is_none());
    let mut all = vec![NS_ROOT_BIND_STEP, NS_PIVOT_STEP, NS_OLD_ROOT_STEP, NS_READ_ONLY_STEP, NS_DEV_KEEP_STEP];
    all.extend([NS_TMP_POINT_STEP, crate::ns_mask::NS_PROC_MASK_STEP]);
    all.extend([crate::NS_LOCK_USER_STEP, crate::NS_NESTED_NS_STEP, crate::ns_join::NS_JOIN_OWNER_STEP]);
    let mut d = all.clone();
    d.sort_unstable();
    d.dedup();
    assert_eq!(d.len(), all.len(), "each step has a byte of its own");
    assert!(all.iter().all(|b| *b > crate::NS_TMPFS_LIMIT_STEP));
}

/// The tmpfs of `/dev` only for a read-only root, whatever the tree holds
/// at `/dev`: the child opens it with no link followed (T-1639).
#[test]
fn dev_is_a_tmpfs_only_for_a_read_only_root() {
    let tree = std::env::temp_dir().join(format!("pod-enter-ns-root-{}", std::process::id()));
    std::fs::create_dir_all(tree.join("dev")).unwrap();
    let root = RootDir::open(tree.to_str().unwrap()).unwrap();
    let mut plan = crate::ns_user::tests::plan(None);
    assert!(RootMount::of(&plan, &root).unwrap().dev.is_none());
    plan.read_only_root = true;
    let m = RootMount::of(&plan, &root).unwrap();
    assert!(m.read_only && m.dev.is_some());
    std::fs::remove_dir(tree.join("dev")).unwrap();
    std::os::unix::fs::symlink("/dev", tree.join("dev")).unwrap();
    assert!(RootMount::of(&plan, &root).unwrap().dev.is_some(), "the child refuses the link");
    let _ = std::fs::remove_dir_all(&tree);
}
