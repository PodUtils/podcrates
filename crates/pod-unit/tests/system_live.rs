//! A unit of the system on the systemd of this host: an install as root
//! with `DynamicUser=yes` and a credential, its state by `systemctl show`,
//! and its removal, after which the directory of the units is as before.
//! The test needs root and systemd as the service manager, and skips with
//! the reason elsewhere; `scripts/ci/root-unit-tests.sh` runs it as root in
//! CI, where a skip fails.

use std::path::Path;
use std::process::Command;

use pod_unit::install::{self, Form, Host, Removed, SystemPlaces};
use pod_unit::{System, Unit};

/// The value of `property` of the unit `name`, by `systemctl show`.
fn show(name: &str, property: &str) -> String {
    let out = Command::new("systemctl").args(["show", "-p", property, "--value", name]).output().expect("systemctl");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The names in the directory of the units and in its `multi-user.target.wants`,
/// where `enable` puts a link.
fn names(dir: &Path) -> Vec<String> {
    let wants = dir.join("multi-user.target.wants");
    let mut names: Vec<String> = [dir, wants.as_path()]
        .into_iter()
        .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
        .map(|e| e.path().display().to_string())
        .collect();
    names.sort();
    names
}

#[test]
fn a_unit_of_the_system_runs_on_the_systemd_of_this_host_and_its_removal_leaves_nothing() {
    pod_testkit::need_root!();
    let host = System::default();
    if !host.systemd_booted() {
        pod_testkit::host::skip_line("systemd is not the service manager of this host");
        return;
    }
    let sleep = ["/usr/bin/sleep", "/bin/sleep"].into_iter().find(|p| Path::new(p).is_file()).expect("sleep");
    let dir = tempfile::Builder::new().prefix("pod-unit-").tempdir_in("/run").expect("a directory of root");
    let secret = dir.path().join("tunnel.token");
    pod_core::files::replace_private(&secret, b"a value of the test", 0o600, false).expect("the secret");
    let name = format!("pod-unit-test-{}", std::process::id());
    let mut unit = Unit::new(&name, Path::new(sleep), "pod-unit tests/system_live.rs")
        .system(None)
        .arg("600")
        .secret_file("tunnel", &secret, "TUNNEL_TOKEN_FILE");
    // A start that waits for the network would wait on a host with none.
    unit.network = false;
    let places = SystemPlaces::default();
    let before = names(&places.systemd);

    let done = install::install_system(&unit, Form::Systemd, &places, &host);
    // systemd starts no unit whose credential it cannot read: an active
    // unit read its secret.
    let state = (show(&name, "ActiveState"), show(&name, "DynamicUser"));
    let removed = install::remove_system(&unit, Form::Systemd, &places, &host);
    let done = done.expect("an install");
    assert!(done[0].contains("enabled and started"), "{done:?}");
    assert_eq!(state.0, "active", "{state:?}");
    assert_eq!(state.1, "yes", "{state:?}");
    assert!(matches!(removed, Ok(Removed::Done(_))), "{removed:?}");
    assert_eq!(show(&name, "LoadState"), "not-found");
    assert_eq!(names(&places.systemd), before);
}
