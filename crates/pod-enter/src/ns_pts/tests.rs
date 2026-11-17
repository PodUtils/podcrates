use super::*;

/// The options make a new instance whose `ptmx` each user opens, and give
/// no `gid=`: the group of the host has no id in the user namespace.
#[test]
fn the_options_make_a_new_instance_with_an_open_ptmx() {
    let opts: Vec<&str> = DEVPTS_OPTIONS.split(',').collect();
    for o in ["newinstance", "ptmxmode=0666", "mode=0620"] {
        assert!(opts.contains(&o), "{o}: {DEVPTS_OPTIONS}");
    }
    assert!(!opts.iter().any(|o| o.starts_with("gid=")), "{DEVPTS_OPTIONS}");
    assert_eq!(DEVPTS_FLAGS & (sys::MS_NOSUID | sys::MS_NOEXEC), sys::MS_NOSUID | sys::MS_NOEXEC);
    assert_eq!(DEVPTS_FLAGS & (sys::MS_NODEV | sys::MS_BIND | sys::MS_RDONLY), 0, "the terminals must work");
}

/// The steps have bytes of their own and names, and the line of a refused
/// instance names the fallback and the step.
#[test]
fn the_steps_and_the_refused_line() {
    let taken: Vec<u8> = (0..NS_DEVPTS_STEP).filter(|b| ns_setup_step(*b).is_some()).collect();
    for b in [NS_DEVPTS_STEP, NS_PTMX_BIND_STEP] {
        assert!(!taken.contains(&b), "{b}");
        assert!(ns_setup_step(b).is_some(), "{b}");
    }
    assert_ne!(NS_DEVPTS_STEP, NS_PTMX_BIND_STEP);
    assert!(crate::ns_pid::step_name(NS_PTMX_BIND_STEP).is_none());
    let line = devpts_refused(&Names::DEFAULT);
    assert!(
        line.ends_with(
            ": the namespace rung binds the host's /dev/pts and /dev/ptmx: mounting a new devpts instance on \
             /dev/pts failed with errno "
        ),
        "{line}"
    );
}

/// The source of the bind of `/dev/ptmx` is in the `/dev/pts` target.
#[test]
fn the_ptmx_of_a_target() {
    assert_eq!(ptmx_of("/img/dev/pts"), "/img/dev/pts/ptmx");
    assert_eq!(ptmx_of("/img/dev/pts/"), "/img/dev/pts/ptmx");
}
