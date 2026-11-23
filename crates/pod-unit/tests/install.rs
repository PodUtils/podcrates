//! The install and the removal of a unit of the user, with a host of the
//! test that records each program and gives its answers: the files that
//! each form writes, the steps in their order, a failed start that leaves no
//! file, a removal that leaves nothing, and a file of another program that
//! stays.

mod common;

use std::path::Path;

use common::fake::{Fake, files};
use common::podrunner;
use pod_core::Class;
use pod_unit::install::{self, Form, Places, Removed};

fn places(home: &Path) -> Places {
    Places::of_home(home, &home.join("scratch"), 1000, "HOST\\ci")
}

#[test]
fn a_user_unit_is_written_enabled_and_started_and_its_removal_leaves_nothing() {
    let home = tempfile::tempdir().expect("a directory");
    let (host, places) = (Fake::default(), places(home.path()));
    let done = install::install(&podrunner(), Form::Systemd, &places, &host).expect("an install");
    let path = places.systemd_user.join("podrunner.service");
    assert!(done[0].contains("enabled and started"), "{done:?}");
    assert_eq!(std::fs::read_to_string(&path).expect("the unit"), pod_unit::systemd::render(&podrunner()).unwrap());
    let runs = host.runs.take();
    assert_eq!(
        runs,
        [
            "systemctl --user daemon-reload",
            "systemctl --user enable podrunner.service",
            "systemctl --user restart podrunner.service"
        ]
    );
    // A second install replaces its own file.
    install::install(&podrunner(), Form::Systemd, &places, &host).expect("again");
    host.runs.take();
    let removed = install::remove(&podrunner(), Form::Systemd, &places, &host).expect("a removal");
    assert!(matches!(removed, Removed::Done(_)), "{removed:?}");
    assert_eq!(host.runs.take()[0], "systemctl --user disable --now podrunner.service");
    assert_eq!(files(home.path()), Vec::<String>::new());
    assert_eq!(install::remove(&podrunner(), Form::Systemd, &places, &host).expect("again"), Removed::Nothing);
}

#[test]
fn a_unit_that_does_not_start_is_not_left_behind() {
    let home = tempfile::tempdir().expect("a directory");
    let host = Fake { fail: vec!["systemctl --user restart"], ..Fake::default() };
    let places = places(home.path());
    let fault = install::install(&podrunner(), Form::Systemd, &places, &host).expect_err("no start");
    assert_eq!(fault.class, Class::Unavailable);
    assert!(fault.text().contains("refused by the test"), "{}", fault.text());
    assert_eq!(files(home.path()), Vec::<String>::new());
    let host = Fake { fail: vec!["launchctl bootstrap"], ..Fake::default() };
    assert!(install::install(&podrunner(), Form::Launchd, &places, &host).is_err());
    assert_eq!(files(home.path()), Vec::<String>::new());
}

#[test]
fn a_file_of_another_program_is_refused_and_stays() {
    let home = tempfile::tempdir().expect("a directory");
    let (host, places) = (Fake::default(), places(home.path()));
    let path = places.systemd_user.join("podrunner.service");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "[Service]\nExecStart=/bin/mine\n").unwrap();
    for fault in [
        install::install(&podrunner(), Form::Systemd, &places, &host).expect_err("not ours"),
        install::remove(&podrunner(), Form::Systemd, &places, &host).expect_err("not ours"),
    ] {
        assert_eq!(fault.class, Class::Config);
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[Service]\nExecStart=/bin/mine\n");
    assert!(host.runs.take().is_empty());
}

#[test]
fn a_launch_agent_is_loaded_in_the_session_of_the_user_and_removed() {
    let home = tempfile::tempdir().expect("a directory");
    let (host, places) = (Fake::default(), places(home.path()));
    let mut unit = podrunner();
    unit.label = Some("dev.podutils.podrunner".into());
    install::install(&unit, Form::Launchd, &places, &host).expect("an install");
    let path = places.launch_agents.join("dev.podutils.podrunner.plist");
    assert!(path.is_file());
    assert_eq!(host.runs.take().last().unwrap(), &format!("launchctl bootstrap gui/1000 {}", path.display()));
    assert!(matches!(install::remove(&unit, Form::Launchd, &places, &host), Ok(Removed::Done(_))));
    assert_eq!(host.runs.take(), ["launchctl bootout gui/1000/dev.podutils.podrunner"]);
    assert_eq!(files(home.path()), Vec::<String>::new());
}

#[test]
fn a_task_is_made_from_its_xml_which_does_not_stay() {
    let home = tempfile::tempdir().expect("a directory");
    let (host, places) = (Fake::default(), places(home.path()));
    install::install(&podrunner(), Form::Task, &places, &host).expect("an install");
    assert!(host.task.borrow().as_deref().is_some_and(|x| x.contains("<UserId>HOST\\ci</UserId>")));
    assert_eq!(files(home.path()), Vec::<String>::new(), "the XML stays");
    // A task of another program with the same name is refused, and stays.
    let mut other = podrunner();
    other.mark = "another program".into();
    assert_eq!(install::remove(&other, Form::Task, &places, &host).expect_err("not ours").class, Class::Config);
    assert!(matches!(install::remove(&podrunner(), Form::Task, &places, &host), Ok(Removed::Done(_))));
    assert!(host.task.borrow().is_none());
}

#[test]
fn the_lines_of_the_crontab_are_added_and_removed_and_the_user_s_lines_stay() {
    let home = tempfile::tempdir().expect("a directory");
    let (host, places) = (Fake::default(), places(home.path()));
    install::install(&podrunner(), Form::Cron, &places, &host).expect("an install with no crontab");
    host.table.borrow_mut().as_mut().unwrap().push_str("0 1 * * * mine\n");
    install::install(&podrunner(), Form::Cron, &places, &host).expect("again");
    let table = host.table.borrow().clone().unwrap();
    assert_eq!(table.matches("@reboot").count(), 1, "{table}");
    assert!(matches!(install::remove(&podrunner(), Form::Cron, &places, &host), Ok(Removed::Done(_))));
    assert_eq!(host.table.borrow().as_deref(), Some("0 1 * * * mine\n"));
    assert_eq!(install::remove(&podrunner(), Form::Cron, &places, &host).unwrap(), Removed::Nothing);
}

#[test]
fn a_unit_of_the_system_is_not_installed_here_and_a_probe_finds_the_first_form() {
    let home = tempfile::tempdir().expect("a directory");
    let places = places(home.path());
    let fault = install::install(&common::node(), Form::Systemd, &places, &Fake::default()).expect_err("system");
    assert_eq!(fault.class, Class::Usage);
    if cfg!(windows) {
        return;
    }
    let host = Fake { fail: vec!["systemctl", "launchctl"], ..Fake::default() };
    assert_eq!(install::probe(&host, &places).expect("a form"), Form::Cron);
    let host = Fake { fail: vec!["systemctl", "launchctl", "crontab"], ..Fake::default() };
    let fault = install::probe(&host, &places).expect_err("no form");
    assert_eq!(fault.class, Class::Unavailable);
    assert_eq!(install::probe(&Fake::default(), &places).expect("a form"), Form::Systemd);
}

/// PodUtils/podssh#342 (b): what an older version of the program wrote, with
/// a mark that may hold `--`, is the program's own: an install replaces it
/// and a removal removes it. A mark of an older version that is not in its
/// place marks nothing, and that file stays.
#[test]
fn an_older_mark_of_the_program_lets_an_upgrade_replace_and_remove_what_it_wrote() {
    let home = tempfile::tempdir().expect("a directory");
    let (host, places) = (Fake::default(), places(home.path()));
    let cron_mark = "# podssh node n --install wrote the lines below; podssh node n --uninstall removes them";
    let unit = podrunner()
        .older_mark("# Written by podssh ")
        .older_mark("<!-- Written by podssh")
        .older_mark("podssh node n --install wrote this task")
        .older_mark(cron_mark);
    let old = "# Written by podssh node n --install; podssh node n --uninstall removes it.\n[Service]\n";
    let path = places.systemd_user.join("podrunner.service");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, old).unwrap();
    install::install(&unit, Form::Systemd, &places, &host).expect("an upgrade");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), pod_unit::systemd::render(&unit).unwrap());
    std::fs::write(&path, old).unwrap();
    assert!(matches!(install::remove(&unit, Form::Systemd, &places, &host), Ok(Removed::Done(_))));
    assert_eq!(files(home.path()), Vec::<String>::new());
    // Not on the first line: the file of another program.
    std::fs::write(&path, format!("[Unit]\n{old}")).unwrap();
    let fault = install::remove(&unit, Form::Systemd, &places, &host).expect_err("not ours");
    assert_eq!(fault.class, Class::Config);
    std::fs::remove_file(&path).unwrap();

    let plist = places.launch_agents.join("podrunner.plist");
    std::fs::create_dir_all(&places.launch_agents).unwrap();
    std::fs::write(&plist, "<?xml version=\"1.0\"?>\n<!-- Written by podssh ssh --install n. -->\n<plist/>\n").unwrap();
    assert!(matches!(install::remove(&unit, Form::Launchd, &places, &host), Ok(Removed::Done(_))));
    assert!(!plist.exists());

    *host.task.borrow_mut() =
        Some("<Task><Description>podssh node n --install wrote this task.</Description></Task>".into());
    assert!(matches!(install::remove(&unit, Form::Task, &places, &host), Ok(Removed::Done(_))));
    assert!(host.task.borrow().is_none());

    let table = format!("0 1 * * * mine\n{cron_mark}\n@reboot /old n\n{} /old n\n", pod_unit::cron::WATCHDOG);
    *host.table.borrow_mut() = Some(table);
    install::install(&unit, Form::Cron, &places, &host).expect("an upgrade");
    let table = host.table.borrow().clone().unwrap();
    assert!(!table.contains("/old") && table.matches("@reboot").count() == 1, "{table}");
    assert!(matches!(install::remove(&unit, Form::Cron, &places, &host), Ok(Removed::Done(_))));
    assert_eq!(host.table.borrow().as_deref(), Some("0 1 * * * mine\n"));
}

/// A mark of an older version is one line of 8 characters or more, so that
/// it does not mark the file of another program by chance.
#[test]
fn an_older_mark_that_is_short_or_holds_a_line_break_is_refused() {
    for bad in ["", "   # x   ", "# Written by\npodssh"] {
        let fault = pod_unit::systemd::render(&podrunner().older_mark(bad)).expect_err(bad);
        assert_eq!(fault.class, Class::Usage, "{bad:?}");
    }
    let unit = podrunner().older_mark("<!-- Written by podssh node n --install");
    assert!(pod_unit::systemd::render(&unit).is_ok());
    assert!(pod_unit::launchd::render(&unit).is_ok(), "no form writes an older mark");
}
