use super::*;
use crate::ns_setup_step;

/// The pid of a pidfd comes from its `Pid:` line; 0 (a process outside
/// the PID namespace) and -1 (a process that ended) give none.
#[test]
fn the_pid_of_a_pidfd_is_read_from_its_fdinfo() {
    let text = "pos:\t0\nflags:\t02000002\nmnt_id:\t15\nino:\t1057\nPid:\t4242\nNSpid:\t4242\n";
    assert_eq!(fdinfo_pid(text), Some(4242));
    assert_eq!(fdinfo_pid("Pid:\t0\n"), None);
    assert_eq!(fdinfo_pid("Pid:\t-1\n"), None);
    assert_eq!(fdinfo_pid("pos:\t0\n"), None);
    assert_eq!(fdinfo_pid("Pid:\tx\n"), None);
}

/// A join of a payload in another user namespace joins it with the mount
/// namespace, in one call with its own step; a join in the user namespace
/// of the caller joins the mount namespace alone.
#[test]
fn the_join_takes_the_user_namespace_only_where_it_differs() {
    let own = Join { pidfd: 9, overlay: false, user: false, pid: false, owner: None };
    assert_eq!(own.flags(), sys::CLONE_NEWNS);
    assert_eq!(own.step(), NS_JOIN_STEP);
    let other = Join { user: true, ..own };
    assert_eq!(other.flags(), sys::CLONE_NEWUSER | sys::CLONE_NEWNS);
    assert_eq!(other.step(), NS_JOIN_USER_STEP);
    assert_eq!(ns_setup_step(NS_JOIN_USER_STEP), Some("setns(pidfd, CLONE_NEWUSER | CLONE_NEWNS)"));
    assert_eq!(ns_setup_step(NS_JOIN_STEP), Some("setns(pidfd, CLONE_NEWNS)"));
}

/// A join of a payload in another PID namespace adds `CLONE_NEWPID` to the
/// one call, with a step of its own.
#[test]
fn the_join_takes_the_pid_namespace_only_where_it_differs() {
    let own = Join { pidfd: 9, overlay: false, user: false, pid: true, owner: None };
    assert_eq!(own.flags(), sys::CLONE_NEWNS | sys::CLONE_NEWPID);
    assert_eq!(own.step(), NS_JOIN_PID_STEP);
    let other = Join { user: true, ..own };
    assert_eq!(other.flags(), sys::CLONE_NEWUSER | sys::CLONE_NEWNS | sys::CLONE_NEWPID);
    assert_eq!(other.step(), NS_JOIN_USER_PID_STEP);
    assert_eq!(ns_setup_step(NS_JOIN_PID_STEP), Some("setns(pidfd, CLONE_NEWNS | CLONE_NEWPID)"));
    assert_eq!(ns_setup_step(NS_JOIN_USER_PID_STEP), Some("setns(pidfd, CLONE_NEWUSER | CLONE_NEWNS | CLONE_NEWPID)"));
}

/// A process in the user namespace of the test is not in another one:
/// the join of a run as root stays `CLONE_NEWNS` alone. A descriptor that
/// is not a pidfd tells nothing, and gives false.
#[test]
fn a_child_in_the_own_user_namespace_is_not_in_another() {
    let mut child = std::process::Command::new("sleep").arg("5").spawn().unwrap();
    let pidfd = match sys::pidfd_open(i64::from(child.id())) {
        Ok(fd) => fd,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return pod_testkit::host::skip_line(&format!("no pidfd_open on this kernel: {}", e.name()));
        }
    };
    assert!(!other_user_namespace(pidfd));
    assert!(!other_namespace(pidfd, "pid"));
    assert_eq!(Join::of(pidfd, true), Join { pidfd, overlay: true, user: false, pid: false, owner: None });
    let _ = sys::close(pidfd);
    let _ = child.kill();
    let _ = child.wait();
    assert!(!other_user_namespace(0));
}

/// The numbers of a line of `/proc`: the pids of each PID namespace, the
/// parent; a line that is absent, empty or not a number gives none.
#[test]
fn the_ids_of_a_line_of_proc_are_read() {
    let text = "Name:\tsleep\nPid:\t4242\nPPid:\t4241\nNSpid:\t4242\t2\nNSpgid:\t4241\t1\n";
    assert_eq!(ids_of(text, "NSpid"), Some(vec![4242, 2]));
    assert_eq!(ids_of(text, "PPid"), Some(vec![4241]));
    assert_eq!(ids_of(text, "NSpgid"), Some(vec![4241, 1]));
    assert_eq!(ids_of(text, "NSsid"), None);
    assert_eq!(ids_of("NSpid:\t\n", "NSpid"), None);
    assert_eq!(ids_of("NSpid:\t7\tx\n", "NSpid"), None);
    assert_eq!(ids_of("NSpidx:\t7\n", "NSpid"), None);
}

/// A process that is no init of a PID namespace under the one of the test
/// is joined itself: `joinable` gives none, and a descriptor that is not
/// a pidfd gives none.
#[test]
fn a_process_that_is_no_init_is_joined_itself() {
    let mut child = std::process::Command::new("sleep").arg("5").spawn().unwrap();
    let pidfd = match sys::pidfd_open(i64::from(child.id())) {
        Ok(fd) => fd,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return pod_testkit::host::skip_line(&format!("no pidfd_open on this kernel: {}", e.name()));
        }
    };
    assert!(joinable(pidfd).is_none());
    let _ = sys::close(pidfd);
    let _ = child.kill();
    let _ = child.wait();
    assert!(joinable(0).is_none());
}

/// The mount namespace of this process belongs to its own user namespace:
/// a join of it enters no owner first, and its step has a name.
#[test]
fn a_join_of_this_process_enters_no_owner_first() {
    let pidfd = sys::pidfd_open(sys::getpid()).expect("pidfd_open of this process");
    assert!(mount_owner(pidfd).is_none());
    let _ = sys::close(pidfd);
    assert!(mount_owner(-1).is_none(), "a descriptor that names no process");
    let step = ns_setup_step(NS_JOIN_OWNER_STEP);
    assert_eq!(step, Some("setns(the owner of the mount namespace of the run, CLONE_NEWUSER)"));
    let with = Join { owner: Some(-1), ..Join { pidfd: -1, overlay: false, user: true, pid: false, owner: None } };
    assert_eq!(with.enter(), Err((NS_JOIN_OWNER_STEP, sys::EBADF)), "the owner comes first");
}
