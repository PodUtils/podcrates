//! The forms of launchd, of Windows and of the crontab: the golden texts,
//! each read back by a parser, and `plutil -lint` where the host has it.

mod common;

use std::path::Path;
use std::process::Command;

use common::{golden, node, podrunner, tool};
use pod_unit::{Unit, cron, launchd, systemd, windows};

fn agent() -> Unit {
    let mut unit = podrunner();
    unit.label = Some("dev.podutils.podrunner".into());
    unit.log = Some("/Users/ci/Library/Logs/podrunner.log".into());
    unit.args.push("a&b <c>".into());
    unit
}

/// A node of a relay as a Windows service of its virtual account.
fn service() -> Unit {
    let mut unit = Unit::new("podnet-relay-node", Path::new("C:\\Program Files\\PodUtils\\podnet.exe"), "podnet unit")
        .system(None)
        .args(["relay", "node", "--state", "C:\\ProgramData\\podnet\\relay node"])
        .env("RUST_LOG", "info")
        .secret_file("tunnel", Path::new("C:\\ProgramData\\podnet\\tunnel.token"), "TUNNEL_TOKEN_FILE");
    unit.description = "podnet relay node".into();
    unit
}

#[test]
fn a_launch_agent_is_its_golden_text() {
    golden("dev.podutils.podrunner.plist", &launchd::render(&agent()).expect("a plist"));
}

/// The LaunchAgent reads back as a property list to the same program and
/// words, with the keys that keep it running.
#[test]
fn the_launch_agent_reads_back_as_its_program_and_keeps_it_alive() {
    let text = launchd::render(&agent()).expect("a plist");
    let value = plist::Value::from_reader_xml(text.as_bytes()).unwrap_or_else(|e| panic!("{e}: {text}"));
    let dict = value.as_dictionary().expect("a dictionary");
    assert_eq!(dict.get("Label").and_then(|v| v.as_string()), Some("dev.podutils.podrunner"));
    let program: Vec<&str> = dict
        .get("ProgramArguments")
        .and_then(|v| v.as_array())
        .expect("ProgramArguments")
        .iter()
        .filter_map(|v| v.as_string())
        .collect();
    let unit = agent();
    let mut want = vec!["/home/ci/.local/bin/podrunner"];
    want.extend(unit.args.iter().map(String::as_str));
    assert_eq!(program, want);
    assert_eq!(dict.get("KeepAlive").and_then(|v| v.as_boolean()), Some(true));
    assert_eq!(dict.get("RunAtLoad").and_then(|v| v.as_boolean()), Some(true));
    assert_eq!(dict.get("ThrottleInterval").and_then(|v| v.as_signed_integer()), Some(5));
    assert_eq!(dict.get("ExitTimeOut").and_then(|v| v.as_signed_integer()), Some(0));
    assert_eq!(dict.get("Umask").and_then(|v| v.as_signed_integer()), Some(0o077));
    assert!(dict.get("UserName").is_none() && dict.get("WorkingDirectory").is_none(), "{text}");
}

#[test]
fn a_launch_daemon_runs_as_its_user_and_needs_one() {
    let mut unit = agent().system(Some("_podrunner"));
    unit.restart.always = false;
    unit = unit.secret_file("gh", Path::new("/Library/Application Support/podrunner/gh"), "GH_FILE");
    let text = launchd::render(&unit).expect("a plist");
    let value = plist::Value::from_reader_xml(text.as_bytes()).expect("a plist");
    let dict = value.as_dictionary().expect("a dictionary");
    assert_eq!(dict.get("UserName").and_then(|v| v.as_string()), Some("_podrunner"));
    let keep = dict.get("KeepAlive").and_then(|v| v.as_dictionary()).expect("KeepAlive");
    assert_eq!(keep.get("SuccessfulExit").and_then(|v| v.as_boolean()), Some(false));
    let env = dict.get("EnvironmentVariables").and_then(|v| v.as_dictionary()).expect("the variables");
    assert_eq!(env.get("GH_FILE").and_then(|v| v.as_string()), Some("/Library/Application Support/podrunner/gh"));
    assert!(launchd::render(&agent().system(None)).is_err(), "a daemon with no user");
    let mut with_file = agent();
    with_file.env_file = Some("/x/env".into());
    assert!(launchd::render(&with_file).is_err(), "a file of variables");
}

#[test]
fn plutil_lints_each_plist() {
    let Some(plutil) = tool("plutil") else { return };
    let dir = tempfile::tempdir().expect("a directory");
    for unit in [agent(), agent().system(Some("_podrunner"))] {
        let path = dir.path().join(launchd::file_name(&unit));
        std::fs::write(&path, launchd::render(&unit).expect("a plist")).expect("a write");
        let out = Command::new(plutil).arg("-lint").arg(&path).output().expect("a run");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    }
}

#[test]
fn a_task_is_its_golden_text_and_reads_back() {
    let mut unit = podrunner();
    unit.args.push("C:\\Users\\a b\\x".into());
    unit.dir = Some("C:\\Users\\ci".into());
    let text = windows::task_xml(&unit, "HOST\\a&b").expect("a task");
    golden("podrunner.task.xml", &text);
    let mut reader = quick_xml::Reader::from_str(&text);
    let mut depth = 0i32;
    loop {
        match reader.read_event().unwrap_or_else(|e| panic!("{e}: {text}")) {
            quick_xml::events::Event::Start(_) => depth += 1,
            quick_xml::events::Event::End(_) => depth -= 1,
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    assert_eq!(depth, 0, "{text}");
    let bytes = windows::utf16(&text);
    assert_eq!(&bytes[..4], &[0xFF, 0xFE, b'<', 0]);
}

/// A task runs as its user at the logon with no privilege, and each 5
/// minutes, never beside a running instance.
#[test]
fn the_task_runs_as_this_user_at_its_logon_with_no_privilege() {
    let task = windows::task_xml(&podrunner(), "HOST\\a&b").expect("a task");
    assert_eq!(task.matches("<UserId>HOST\\a&amp;b</UserId>").count(), 2, "{task}");
    assert!(task.contains("<LogonType>InteractiveToken</LogonType>"), "{task}");
    assert!(task.contains("<RunLevel>LeastPrivilege</RunLevel>"), "{task}");
    assert!(task.contains("<TimeTrigger><Repetition><Interval>PT5M</Interval>"), "{task}");
    assert!(task.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"), "{task}");
    assert!(windows::task_xml(&podrunner().env("A", "b"), "HOST\\me").is_err(), "a task has no variables");
    let mut logged = podrunner();
    logged.log = Some("/x/podrunner.log".into());
    assert!(windows::task_xml(&logged, "HOST\\me").is_err(), "a task has no log");
    assert!(windows::task_xml(&node(), "HOST\\me").is_err(), "a unit of the system");
}

#[test]
fn a_windows_service_is_its_golden_script() {
    golden("podnet-relay-node.cmd", &windows::service_script(&service()).expect("a service"));
}

#[test]
fn a_windows_service_runs_as_an_account_with_no_password_with_its_least_privileges() {
    let commands = windows::service_commands(&service()).expect("a service");
    let create = &commands[0];
    assert_eq!(create.args[..2], ["create", "podnet-relay-node"]);
    let obj = create.args.iter().position(|a| a == "obj=").expect("obj=");
    assert_eq!(create.args[obj + 1], "NT SERVICE\\podnet-relay-node");
    let lines: Vec<String> = commands.iter().map(|c| c.line()).collect();
    assert!(lines.contains(&"sc.exe sidtype podnet-relay-node restricted".to_string()), "{lines:?}");
    assert!(lines.contains(&"sc.exe privs podnet-relay-node SeChangeNotifyPrivilege".to_string()), "{lines:?}");
    assert!(lines.contains(&"sc.exe failureflag podnet-relay-node 1".to_string()), "{lines:?}");
    for account in ["NT AUTHORITY\\LocalService", "CORP\\podnet$", "NT SERVICE\\other"] {
        assert!(windows::service_commands(&service().system(Some(account))).is_ok(), "{account}");
    }
    let refused = windows::service_commands(&service().system(Some("CORP\\alice"))).expect_err("a password");
    assert!(refused.text().contains("password"), "{}", refused.text());
    assert_eq!(
        windows::service_removal("x").iter().map(|c| c.line()).collect::<Vec<_>>(),
        ["sc.exe stop x", "sc.exe delete x"]
    );
}

#[test]
fn the_crontab_gets_the_lines_of_the_name_and_loses_them_again() {
    let mut unit = podrunner().env("A", "1");
    unit.args.push("100%".into());
    let lines = cron::lines(&unit).expect("the lines");
    golden("podrunner.cron", &(lines.join("\n") + "\n"));
    let before = "MAILTO=x\n0 * * * * backup\n";
    let (with, found) = cron::table(before, &unit, Some(&lines));
    assert!(!found);
    assert_eq!(with, format!("{before}{}\n{}\n{}\n", cron::mark(&unit), lines[0], lines[1]));
    // Again: the lines are replaced, never doubled.
    let new = ["@reboot new".to_string(), format!("{} new", cron::WATCHDOG)];
    let (again, found) = cron::table(&with, &unit, Some(&new));
    assert!(found && again.matches("@reboot").count() == 1, "{again}");
    // The lines of another unit stay.
    let mut other = podrunner();
    other.name = "other".into();
    let (both, _) = cron::table(&again, &other, Some(&["@reboot other".to_string()]));
    let (rest, found) = cron::table(&both, &unit, None);
    assert!(found && rest.contains("@reboot other") && !rest.contains("new"), "{rest}");
    assert_eq!(cron::table(&rest, &other, None).0, before);
    // A line below the mark that is not an @reboot line is the user's.
    let edited = format!("{}\n0 1 * * * mine\n", cron::mark(&unit));
    assert_eq!(cron::table(&edited, &unit, None), ("0 1 * * * mine\n".to_string(), true));
}

/// A name of `Unit::NAME_MAX` bytes renders for each manager; one byte more
/// is a usage fault. A secret name keeps its bound of 64.
#[test]
fn a_long_name_renders_for_each_manager_up_to_its_bound() {
    let long = |n: usize| format!("podssh-node-{}", "a".repeat(n - "podssh-node-".len()));
    for n in [64, Unit::NAME_MAX] {
        let mut user = podrunner();
        user.name = long(n);
        user.check().unwrap_or_else(|e| panic!("{n} bytes: {e}"));
        assert_eq!(systemd::file_name(&user), format!("{}.service", user.name));
        systemd::render(&user).unwrap_or_else(|e| panic!("{n} bytes: {e}"));
        let text = launchd::render(&user).unwrap_or_else(|e| panic!("{n} bytes: {e}"));
        assert!(text.contains(&format!("<string>{}</string>", user.name)), "{text}");
        windows::task_xml(&user, "ci").unwrap_or_else(|e| panic!("{n} bytes: {e}"));
        let mut node = service();
        node.name = long(n);
        let script = windows::service_script(&node).unwrap_or_else(|e| panic!("{n} bytes: {e}"));
        assert!(script.contains(&node.name), "{script}");
    }
    let mut over = podrunner();
    over.name = long(Unit::NAME_MAX + 1);
    let fault = over.check().expect_err("one byte over the bound");
    assert_eq!(fault.class, pod_core::Class::Usage);
    assert!(fault.to_string().contains(&format!("1 to {}", Unit::NAME_MAX)), "{fault}");
    let mut label = podrunner();
    label.label = Some(long(Unit::NAME_MAX + 1));
    assert_eq!(label.check().expect_err("a long label").class, pod_core::Class::Usage);
    let secret = podrunner().secret_file(&"s".repeat(65), Path::new("/etc/x/token"), "TOKEN_FILE");
    assert_eq!(secret.check().expect_err("a secret name of 65 bytes").class, pod_core::Class::Usage);
    let mut state = podrunner();
    state.state_dir = Some("d".repeat(65));
    assert_eq!(state.check().expect_err("a state directory of 65 bytes").class, pod_core::Class::Usage);
}
