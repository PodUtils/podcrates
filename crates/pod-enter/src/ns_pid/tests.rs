use super::*;
use crate::ns_setup_step;

fn plan() -> Plan {
    crate::tests::plain_plan()
}

/// The mask names the signals by their bits: signal n is bit n - 1. The
/// init gives the list of the waiter and SIGWINCH (PodUtils/podvm#608).
#[test]
fn the_forwarded_signals_are_the_bits_of_their_numbers() {
    let proxied =
        [sys::SIGHUP, sys::SIGINT, sys::SIGQUIT, sys::SIGUSR1, sys::SIGUSR2, sys::SIGTERM, sys::SIGALRM, sys::SIGPWR];
    for s in proxied {
        assert_ne!(PROXIED & (1 << (s - 1)), 0, "signal {s}");
    }
    assert_eq!(PROXIED.count_ones(), 8);
    assert_eq!(FORWARDED, PROXIED | (1 << (sys::SIGWINCH - 1)));
    assert_eq!(FORWARDED.count_ones(), 9);
    assert_eq!(FORWARDED & sys::sigbit(sys::SIGCHLD), 0);
    assert_eq!(MASK, FORWARDED | (1 << (sys::SIGCHLD - 1)));
}

/// Each step byte of the PID namespace has a name, and a refusal of the
/// clone names the flags of the clone.
#[test]
fn each_step_of_the_pid_namespace_has_a_name() {
    assert_eq!(ns_setup_step(NS_PID_STEP), Some("clone(CLONE_NEWPID)"));
    assert_eq!(ns_setup_step(NS_PID_USER_STEP), Some("clone(CLONE_NEWUSER | CLONE_NEWPID)"));
    assert_eq!(ns_setup_step(NS_PID_FORK_STEP), Some("forking the payload under the init of the PID namespace"));
    let own = Pid { flags: sys::CLONE_NEWPID, own: true, relay: true, parent: None };
    assert_eq!(own.clone_step(), NS_PID_STEP);
    let user = Pid { flags: sys::CLONE_NEWPID | sys::CLONE_NEWUSER, ..own };
    assert_eq!(user.clone_step(), NS_PID_USER_STEP);
}

/// `EPERM` of the clone with a PID namespace calls for the form with a
/// user namespace, as `EPERM` of `unshare(CLONE_NEWNS)` does.
#[test]
fn eperm_of_the_pid_namespace_calls_for_a_user_namespace() {
    let step = ns_setup_step(NS_PID_STEP).unwrap();
    assert!(crate::ns_user::calls_for_user_namespace(step, sys::EPERM));
    assert!(!crate::ns_user::calls_for_user_namespace(step, sys::EINVAL));
    let user = ns_setup_step(NS_PID_USER_STEP).unwrap();
    assert!(!crate::ns_user::calls_for_user_namespace(user, sys::EPERM));
}

/// A plan with no PID namespace forks once; a join follows the run; an
/// entry with no mount namespace of its own refuses a plan with one.
#[test]
fn the_pid_namespace_of_each_entry() {
    let mut plan = plan();
    assert_eq!(Pid::of(&plan, None, Some("the chroot rung")).unwrap(), Pid::default());
    let join = crate::ns_join::Join { pidfd: -1, overlay: false, user: false, pid: true };
    let joined = Pid::of(&plan, Some(&NsEntry::Join(join, None)), None).unwrap();
    assert_eq!(joined, Pid { relay: true, ..Pid::default() });
    plan.pid_namespace = true;
    let e = Pid::of(&plan, None, Some("the chroot rung")).unwrap_err().to_string();
    assert!(e.contains("the PID namespace needs the namespace rung; the chroot rung has none"), "{e}");
    let still = Pid::of(&plan, Some(&NsEntry::Join(join, None)), None).unwrap();
    assert_eq!(still, joined);
    let not_in = crate::ns_join::Join { pid: false, ..join };
    assert_eq!(Pid::of(&plan, Some(&NsEntry::Join(not_in, None)), None).unwrap(), Pid::default());
}

/// The pidfd of the parent opens only for a child that is pid 1 and must
/// end with its parent, and the check reads it: a living parent has not
/// ended.
#[test]
fn the_parent_check_of_pid_1_reads_a_pidfd() {
    let own = Pid { flags: sys::CLONE_NEWPID, own: true, relay: true, parent: None };
    assert_eq!(own.with_parent(false).unwrap(), own);
    assert_eq!(Pid::default().with_parent(true).unwrap(), Pid::default());
    let with = match own.with_parent(true) {
        Ok(p) => p,
        Err(e) => return pod_testkit::host::skip_line(&format!("no pidfd_open on this kernel: {e}")),
    };
    assert!(with.parent.is_some());
    assert!(!with.parent_ended(sys::getpid()));
    // The check closed the descriptor.
    assert!(sys::close(with.parent.unwrap()).is_err());
    // With no pidfd, the check compares the parent.
    assert!(!Pid::default().parent_ended(sys::getppid()));
    assert!(Pid::default().parent_ended(sys::getppid() + 1));
}

/// A signal that a process sent goes to the payload; SIGCHLD and a signal
/// of the terminal (`SI_KERNEL`) do not: the payload got that one already.
#[test]
fn the_init_forwards_the_signals_of_a_process() {
    let (term, int, chld) = (sys::SIGTERM as u32, sys::SIGINT as u32, sys::SIGCHLD as u32);
    assert!(forwards(term, 0));
    assert!(forwards(int, -6));
    assert!(!forwards(int, SI_KERNEL));
    assert!(!forwards(chld, 1));
}
