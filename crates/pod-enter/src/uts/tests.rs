use super::*;
use crate::{Fds, Names};

fn plan(hostname: Option<&str>) -> Plan {
    Plan {
        argv: vec!["/bin/true".to_string()],
        env: Vec::new(),
        working_dir: "/".to_string(),
        fds: Fds::default(),
        banner: String::new(),
        path_dirs: vec!["/bin".to_string()],
        names: Names::DEFAULT,
        limits: Vec::new(),
        no_new_privs: false,
        die_with_parent: false,
        confine: None,
        hostname: hostname.map(str::to_string),
        volumes: Vec::new(),
        userns_ids: None,
        pid_namespace: false,
        tmpfs: Vec::new(),
        relay_title: None,
        read_only_root: false,
    }
}

/// A name of 1 to 64 bytes with no NUL passes; each other is refused by
/// name before the fork.
#[test]
fn the_name_is_checked_before_the_fork() {
    assert!(checked(&plan(None)).unwrap().is_none());
    assert!(checked(&plan(Some("unit1"))).unwrap().is_some());
    assert!(checked(&plan(Some(&"a".repeat(MOST)))).unwrap().is_some());
    for (bad, why) in [("", "is empty"), ("a\0b", "NUL"), (&"a".repeat(MOST + 1) as &str, "at most 64")] {
        let e = checked(&plan(Some(bad))).err().expect("refused").to_string();
        assert!(e.contains(why), "{bad:?}: {e}");
    }
}

/// An entry with no UTS namespace refuses a plan with a name, and names
/// itself; a plan with no name passes.
#[test]
fn an_entry_with_no_uts_namespace_refuses_a_name() {
    assert!(refuse_without_namespace(&plan(None), "the chroot rung").is_ok());
    let e = refuse_without_namespace(&plan(Some("unit1")), "the chroot rung").expect_err("refused");
    assert!(e.to_string().contains("needs the namespace rung; the chroot rung"), "{e}");
}

/// The steps of the two calls have names for the fallback line.
#[test]
fn each_step_has_a_name() {
    assert_eq!(crate::ns_setup_step(NS_UTS_STEP), Some("unshare(CLONE_NEWUTS)"));
    assert_eq!(crate::ns_setup_step(NS_HOSTNAME_STEP), Some("sethostname"));
}
