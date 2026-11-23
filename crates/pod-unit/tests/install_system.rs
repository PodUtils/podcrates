//! The install and the removal of a unit of the system, with a host of the
//! test that records each program and gives each probe (root, systemd, the
//! accounts, the files of the secrets): the refusal of a caller with no
//! privilege before any write, each missing part refused with its words,
//! the steps of systemd, launchd and a Windows service in their order, a
//! unit that does not start and is not left behind, a removal that leaves
//! nothing, and a file or a service of another program that stays.

mod common;

use std::path::{Path, PathBuf};

use common::fake::{Fake, files};
use common::node;
use pod_core::Class;
use pod_unit::install::{self, FileMeta, Form, Removed, SystemPlaces};
use pod_unit::{Unit, windows};

fn places(dir: &Path) -> SystemPlaces {
    SystemPlaces { systemd: dir.join("etc/systemd/system"), launch_daemons: dir.join("Library/LaunchDaemons") }
}

/// The file of the secret of [`node`].
const TUNNEL: &str = "/etc/podnet/tunnel.token";

/// A file of `uid` with `mode`.
fn meta(uid: u32, mode: u32) -> FileMeta {
    FileMeta { uid: Some(uid), mode: Some(mode) }
}

/// A host where the caller is root and the secret of [`node`] is root's.
fn host() -> Fake {
    let mut host = Fake::root();
    host.files.insert(PathBuf::from(TUNNEL), meta(0, 0o600));
    host
}

/// A daemon of launchd that runs as alice, with a secret of hers.
fn daemon() -> (Unit, Fake) {
    let mut unit = Unit::new("podnet-relay-node", Path::new("/usr/local/bin/podnet"), "podnet unit print")
        .system(Some("alice"))
        .args(["relay", "node"])
        .secret_file("tunnel", Path::new("/Users/alice/.podnet/tunnel.token"), "TUNNEL_TOKEN_FILE");
    unit.label = Some("dev.podutils.podnet".into());
    let mut host = Fake::root();
    host.files.insert(PathBuf::from("/Users/alice/.podnet/tunnel.token"), meta(1000, 0o600));
    (unit, host)
}

/// A node of a relay as a Windows service of its virtual account.
fn service() -> (Unit, Fake) {
    let secret = "C:\\ProgramData\\podnet\\tunnel.token";
    let unit = Unit::new("podnet-relay-node", Path::new("C:\\Program Files\\PodUtils\\podnet.exe"), "podnet unit")
        .system(None)
        .args(["relay", "node"])
        .secret_file("tunnel", Path::new(secret), "TUNNEL_TOKEN_FILE");
    let mut host = Fake::root();
    host.files.insert(PathBuf::from(secret), FileMeta { uid: None, mode: None });
    (unit, host)
}

#[test]
fn a_unit_of_the_system_is_written_enabled_and_started_and_its_removal_leaves_nothing() {
    let dir = tempfile::tempdir().expect("a directory");
    let (host, places) = (host(), places(dir.path()));
    let done = install::install_system(&node(), Form::Systemd, &places, &host).expect("an install");
    assert!(done[0].contains("a unit of the system of systemd") && done[0].contains("enabled and started"), "{done:?}");
    let path = places.systemd.join("podnet-relay-node.service");
    assert_eq!(std::fs::read_to_string(&path).expect("the unit"), pod_unit::systemd::render(&node()).unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode & 0o022, 0, "a unit that others may write: {mode:o}");
    }
    assert_eq!(
        host.runs.take(),
        [
            "systemctl daemon-reload",
            "systemctl enable podnet-relay-node.service",
            "systemctl restart podnet-relay-node.service"
        ]
    );
    // A second install replaces its own file.
    install::install_system(&node(), Form::Systemd, &places, &host).expect("again");
    host.runs.take();
    let removed = install::remove_system(&node(), Form::Systemd, &places, &host).expect("a removal");
    assert!(matches!(&removed, Removed::Done(lines) if lines[0].contains("the unit of the system")), "{removed:?}");
    assert_eq!(
        host.runs.take(),
        [
            "systemctl disable --now podnet-relay-node.service",
            "systemctl daemon-reload",
            "systemctl reset-failed podnet-relay-node.service"
        ]
    );
    assert_eq!(files(dir.path()), Vec::<String>::new());
    assert_eq!(install::remove_system(&node(), Form::Systemd, &places, &host).expect("again"), Removed::Nothing);
}

/// The plant `unit-system-install-no-root-check` drops the check of the
/// privilege: the install then writes the unit as a caller with none.
#[test]
fn a_caller_with_no_privilege_is_refused_before_any_write() {
    let dir = tempfile::tempdir().expect("a directory");
    let places = places(dir.path());
    let user = Fake { admin: false, ..host() };
    let (daemon, _) = daemon();
    let (service, _) = service();
    for (unit, form) in [(node(), Form::Systemd), (daemon, Form::Launchd), (service, Form::Service)] {
        for fault in [
            install::install_system(&unit, form, &places, &user).expect_err("no privilege"),
            install::remove_system(&unit, form, &places, &user).expect_err("no privilege"),
        ] {
            assert_eq!(fault.class, Class::Refused, "{}: {}", form.name(), fault.text());
            assert!(fault.text().contains("run it as root"), "{}", fault.text());
        }
    }
    assert!(user.runs.take().is_empty());
    assert_eq!(files(dir.path()), Vec::<String>::new());
}

#[test]
fn each_missing_part_of_a_system_install_is_refused_with_its_words() {
    let alice = node().system(Some("alice"));
    let fine = tempfile::tempdir().expect("a directory");
    assert!(install::install_system(&alice, Form::Systemd, &places(fine.path()), &host()).is_ok());
    let dir = tempfile::tempdir().expect("a directory");
    let places = places(dir.path());
    let refused = |unit: &Unit, host: &Fake, class: Class, says: &str| {
        let fault = install::install_system(unit, Form::Systemd, &places, host).expect_err(says);
        assert_eq!(fault.class, class, "{says}: {}", fault.text());
        assert!(fault.text().contains(says), "{says}: {}", fault.text());
        assert!(!host.runs.borrow().iter().any(|r| r.starts_with("systemctl")), "{:?}", host.runs.borrow());
    };
    refused(&alice, &Fake { booted: false, ..host() }, Class::Unavailable, "/run/systemd/system");
    refused(&node().system(Some("bob")), &host(), Class::NotFound, "the account bob is not on this host");
    refused(&node().system(Some("toor")), &host(), Class::Usage, "the account toor is root");
    refused(&node().system(Some("al ice")), &host(), Class::Usage, "the account \"al ice\"");
    refused(&node(), &Fake::root(), Class::NotFound, "the secret tunnel, /etc/podnet/tunnel.token");
    for (uid, mode) in [(1000, 0o600), (0, 0o640), (0, 0o604)] {
        let mut theirs = host();
        theirs.files.insert(PathBuf::from(TUNNEL), meta(uid, mode));
        refused(&node(), &theirs, Class::Config, "chmod 0600");
    }
    let mut env_file = node();
    env_file.env_file = Some("/etc/podnet/node.env".into());
    refused(&env_file, &host(), Class::NotFound, "the file of variables, /etc/podnet/node.env");
    assert_eq!(files(dir.path()), Vec::<String>::new());
}

#[test]
fn a_unit_of_the_system_that_does_not_start_is_not_left_behind() {
    let dir = tempfile::tempdir().expect("a directory");
    let places = places(dir.path());
    let host = Fake { fail: vec!["systemctl restart"], ..host() };
    let fault = install::install_system(&node(), Form::Systemd, &places, &host).expect_err("no start");
    assert_eq!(fault.class, Class::Unavailable);
    assert!(fault.text().contains("refused by the test"), "{}", fault.text());
    let runs = host.runs.take();
    assert_eq!(runs[runs.len() - 2..], ["systemctl disable podnet-relay-node.service", "systemctl daemon-reload"]);
    let (unit, mut host) = daemon();
    host.fail = vec!["launchctl bootstrap"];
    assert!(install::install_system(&unit, Form::Launchd, &places, &host).is_err());
    assert_eq!(files(dir.path()), Vec::<String>::new());
}

#[test]
fn a_file_or_a_service_of_another_program_is_refused_and_stays() {
    let dir = tempfile::tempdir().expect("a directory");
    let (host, places) = (host(), places(dir.path()));
    let path = places.systemd.join("podnet-relay-node.service");
    std::fs::create_dir_all(&places.systemd).unwrap();
    std::fs::write(&path, "[Service]\nExecStart=/bin/mine\n").unwrap();
    for fault in [
        install::install_system(&node(), Form::Systemd, &places, &host).expect_err("not ours"),
        install::remove_system(&node(), Form::Systemd, &places, &host).expect_err("not ours"),
    ] {
        assert_eq!(fault.class, Class::Config, "{}", fault.text());
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[Service]\nExecStart=/bin/mine\n");
    assert!(host.runs.take().is_empty());
    let (unit, host) = service();
    *host.service.borrow_mut() = Some("the service of another program".into());
    for fault in [
        install::install_system(&unit, Form::Service, &places, &host).expect_err("not ours"),
        install::remove_system(&unit, Form::Service, &places, &host).expect_err("not ours"),
    ] {
        assert_eq!(fault.class, Class::Config, "{}", fault.text());
        assert!(fault.text().contains("the service podnet-relay-node"), "{}", fault.text());
    }
    assert_eq!(host.runs.take(), ["sc.exe qdescription podnet-relay-node 8192"; 2]);
    assert_eq!(host.service.borrow().as_deref(), Some("the service of another program"));
}

/// PodUtils/podssh#342 (b): a unit of the system that an older version of
/// the program wrote is its own: the file of systemd and the Windows service.
#[test]
fn an_older_mark_of_the_program_takes_its_unit_of_the_system() {
    let dir = tempfile::tempdir().expect("a directory");
    let (host, places) = (host(), places(dir.path()));
    let path = places.systemd.join("podnet-relay-node.service");
    std::fs::create_dir_all(&places.systemd).unwrap();
    std::fs::write(
        &path,
        "# Written by podnet relay node --install --system.
[Service]
",
    )
    .unwrap();
    let unit = node().older_mark("# Written by podnet relay node --install");
    install::install_system(&unit, Form::Systemd, &places, &host).expect("an upgrade");
    assert!(matches!(install::remove_system(&unit, Form::Systemd, &places, &host), Ok(Removed::Done(_))));
    assert_eq!(files(dir.path()), Vec::<String>::new());
    let (unit, host) = service();
    let unit = unit.older_mark("Written by podnet relay node --install");
    *host.service.borrow_mut() = Some("podnet relay node. Written by podnet relay node --install.".into());
    assert!(matches!(install::remove_system(&unit, Form::Service, &places, &host), Ok(Removed::Done(_))));
    assert!(host.service.borrow().is_none());
}

#[test]
fn a_launch_daemon_runs_as_its_user_in_the_domain_of_the_system() {
    let dir = tempfile::tempdir().expect("a directory");
    let places = places(dir.path());
    let (unit, host) = daemon();
    let done = install::install_system(&unit, Form::Launchd, &places, &host).expect("an install");
    assert!(done[0].starts_with("a LaunchDaemon") && done[0].contains("each boot"), "{done:?}");
    let path = places.launch_daemons.join("dev.podutils.podnet.plist");
    assert!(std::fs::read_to_string(&path).expect("the plist").contains("<string>alice</string>"));
    assert_eq!(
        host.runs.take(),
        [
            "id -u alice".to_string(),
            "launchctl bootout system/dev.podutils.podnet".to_string(),
            "launchctl enable system/dev.podutils.podnet".to_string(),
            format!("launchctl bootstrap system {}", path.display()),
        ]
    );
    assert!(matches!(install::remove_system(&unit, Form::Launchd, &places, &host), Ok(Removed::Done(_))));
    assert_eq!(host.runs.take(), ["launchctl bootout system/dev.podutils.podnet"]);
    assert_eq!(files(dir.path()), Vec::<String>::new());
    // The daemon reads its secret as alice: a file of root is refused.
    let mut roots = Fake::root();
    roots.files.insert(PathBuf::from("/Users/alice/.podnet/tunnel.token"), meta(0, 0o600));
    let fault = install::install_system(&unit, Form::Launchd, &places, &roots).expect_err("root's");
    assert_eq!(fault.class, Class::Config);
    assert!(fault.text().contains("a file of alice"), "{}", fault.text());
    let fault = install::install_system(&node(), Form::Launchd, &places, &host).expect_err("no account");
    assert_eq!(fault.class, Class::Unsupported);
    assert_eq!(files(dir.path()), Vec::<String>::new());
}

#[test]
fn a_windows_service_is_made_by_its_commands_started_and_deleted() {
    let dir = tempfile::tempdir().expect("a directory");
    let places = places(dir.path());
    let (unit, host) = service();
    let done = install::install_system(&unit, Form::Service, &places, &host).expect("an install");
    assert!(done[0].contains("made and started"), "{done:?}");
    let commands = windows::service_commands(&unit).expect("the commands");
    let line = |c: &windows::Command| format!("{} {}", c.program, c.args.join(" "));
    let mut want = vec!["sc.exe qdescription podnet-relay-node 8192".to_string()];
    want.extend(commands.iter().map(line));
    want.push("sc.exe start podnet-relay-node".to_string());
    assert_eq!(host.runs.take(), want);
    // A second install changes the service in place, with no old variable.
    let done = install::install_system(&unit, Form::Service, &places, &host).expect("again");
    assert!(done[0].contains("changed and started"), "{done:?}");
    let runs = host.runs.take();
    assert_eq!(runs[1], "sc.exe stop podnet-relay-node");
    assert!(runs[2].starts_with("reg.exe delete HKLM\\SYSTEM\\CurrentControlSet\\Services\\podnet-relay-node /v"));
    assert!(runs[3].starts_with("sc.exe config podnet-relay-node binPath="), "{runs:?}");
    // A service that does not start is deleted.
    let host = Fake { fail: vec!["sc.exe start"], ..service().1 };
    assert!(install::install_system(&unit, Form::Service, &places, &host).is_err());
    assert!(host.service.borrow().is_none());
    assert!(
        host.runs.take().ends_with(&["sc.exe stop podnet-relay-node".into(), "sc.exe delete podnet-relay-node".into()])
    );
    let (_, host) = service();
    install::install_system(&unit, Form::Service, &places, &host).expect("an install");
    host.runs.take();
    assert!(matches!(install::remove_system(&unit, Form::Service, &places, &host), Ok(Removed::Done(_))));
    assert_eq!(
        host.runs.take(),
        [
            "sc.exe qdescription podnet-relay-node 8192",
            "sc.exe stop podnet-relay-node",
            "sc.exe delete podnet-relay-node"
        ]
    );
    assert_eq!(install::remove_system(&unit, Form::Service, &places, &host).expect("again"), Removed::Nothing);
    assert_eq!(files(dir.path()), Vec::<String>::new());
}

#[test]
fn a_form_of_the_user_alone_is_refused_for_the_system_and_a_probe_finds_the_manager() {
    let dir = tempfile::tempdir().expect("a directory");
    let places = places(dir.path());
    for form in [Form::Task, Form::Cron] {
        let fault = install::install_system(&node(), form, &places, &host()).expect_err("of the user");
        assert_eq!(fault.class, Class::Unsupported, "{}", fault.text());
    }
    let fault = install::install_system(&common::podrunner(), Form::Systemd, &places, &host()).expect_err("user");
    assert_eq!(fault.class, Class::Usage);
    let user = install::Places::of_home(dir.path(), dir.path(), 1000, "HOST\\ci");
    let fault = install::install(&common::podrunner(), Form::Service, &user, &host()).expect_err("a service");
    assert_eq!(fault.class, Class::Unsupported);
    assert_eq!(files(dir.path()), Vec::<String>::new());
    if cfg!(windows) {
        assert_eq!(install::probe_system(&host()).expect("a form"), Form::Service);
        return;
    }
    assert_eq!(install::probe_system(&host()).expect("a form"), Form::Systemd);
    assert_eq!(install::probe_system(&Fake { booted: false, ..host() }).expect("a form"), Form::Launchd);
    let none = Fake { booted: false, fail: vec!["launchctl"], ..host() };
    let fault = install::probe_system(&none).expect_err("no manager");
    assert_eq!(fault.class, Class::Unavailable);
    assert!(fault.text().contains("/run/systemd/system"), "{}", fault.text());
}
