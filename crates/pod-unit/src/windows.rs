//! The two forms of Windows. A unit of the user is a task of the Task
//! Scheduler at the logon of the user and each 5 minutes while the user is
//! logged on, made from XML by `schtasks /Create /XML`, with no privilege
//! (the base is podssh's `node_install/text.rs`). A unit of the system is a
//! Windows service that the service control manager runs, made by `sc.exe`
//! (call C859): the account `NT SERVICE\NAME` (a virtual account of the
//! service alone) or a named account that needs no password, a restricted
//! SID of the service, the least privileges, a restart after each failure,
//! and the variables in the key `Environment` of the service. No password is
//! in a command: an account that needs one is refused.
//!
//! The program of a Windows service must answer the service control
//! manager (`StartServiceCtrlDispatcherW`); a program that does not is
//! ended at its start with error 1053. `serve` (on Windows) is that answer:
//! the program calls it when an argument of the unit says that the manager
//! started it.

use pod_core::Failure;

use crate::model::{Scope, Unit, unsupported, usage};
use crate::quote::{windows_word, xml};
pub use crate::service::{PANIC_CODE, SPECIFIC_ERROR, Signal, WAIT_HINT, exit_codes, program_code};
#[cfg(windows)]
pub use crate::service_scm::{Service, serve};

/// The privileges of a service: the least that a program needs to cross the
/// directories of a path.
pub const PRIVILEGES: &str = "SeChangeNotifyPrivilege";

/// The XML of the task of the user `user` (`DOMAIN\USER`): at its logon, and
/// each 5 minutes while it is logged on, which starts the program again
/// when it ended, as a second instance never starts beside a running one.
/// It starts again a minute after a failure, and has no time limit. The
/// Task Scheduler gives a task no variables and no log: a unit with
/// variables, a secret, a file of variables, a state directory or a log is
/// refused.
pub fn task_xml(unit: &Unit, user: &str) -> Result<String, Failure> {
    unit.check()?;
    if unit.scope != Scope::User {
        return Err(usage("a task at the logon is a unit of the user: give the system service form"));
    }
    no_variables(unit, "the Task Scheduler")?;
    let arguments: Vec<String> = unit.args.iter().map(|w| windows_word(w)).collect();
    let description = format!("{}. Written by {}.", unit.description, unit.mark);
    let dir = unit.dir.as_ref().map_or_else(String::new, |d| {
        format!("<WorkingDirectory>{}</WorkingDirectory>", xml(&d.display().to_string()))
    });
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n\
         <Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n  \
           <RegistrationInfo><Description>{}</Description></RegistrationInfo>\n  \
           <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger>\
         <TimeTrigger><Repetition><Interval>PT5M</Interval><StopAtDurationEnd>false</StopAtDurationEnd></Repetition>\
         <StartBoundary>2026-01-01T00:00:00</StartBoundary><Enabled>true</Enabled></TimeTrigger></Triggers>\n  \
           <Principals><Principal id=\"Author\"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType>\
         <RunLevel>LeastPrivilege</RunLevel></Principal></Principals>\n  \
           <Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\
         <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>\
         <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><ExecutionTimeLimit>PT0S</ExecutionTimeLimit>\
         <RestartOnFailure><Interval>PT1M</Interval><Count>999</Count></RestartOnFailure></Settings>\n  \
           <Actions Context=\"Author\"><Exec><Command>{}</Command><Arguments>{}</Arguments>{dir}</Exec></Actions>\n\
         </Task>\n",
        xml(&description),
        xml(&unit.program.display().to_string()),
        xml(&arguments.join(" ")),
        user = xml(user),
    ))
}

/// The task's XML as `schtasks` reads it: UTF-16, little-endian, with its
/// mark of the order of bytes.
pub fn utf16(xml: &str) -> Vec<u8> {
    [0xFFu8, 0xFE].into_iter().chain(xml.encode_utf16().flat_map(u16::to_le_bytes)).collect()
}

/// The account of a service: `NT SERVICE\NAME` with no name; else an
/// account that needs no password (`NT AUTHORITY\LocalService`,
/// `NT AUTHORITY\NetworkService`, `NT SERVICE\...`, a managed service
/// account `DOMAIN\NAME$`).
fn account(unit: &Unit) -> Result<String, Failure> {
    let Scope::System { run_as } = &unit.scope else {
        return Err(usage("a Windows service is a unit of the system: give the task form for a user"));
    };
    let Some(user) = run_as else { return Ok(format!("NT SERVICE\\{}", unit.name)) };
    let upper = user.to_ascii_uppercase();
    let no_password = ["NT AUTHORITY\\LOCALSERVICE", "NT AUTHORITY\\NETWORKSERVICE"].contains(&upper.as_str())
        || upper.starts_with("NT SERVICE\\")
        || user.ends_with('$');
    if !no_password {
        return Err(usage(format!(
            "the account {user}: a service of a user account needs its password in the command; give \
             NT SERVICE\\NAME, a managed service account (DOMAIN\\NAME$) or LocalService"
        )));
    }
    Ok(user.clone())
}

/// One command of the form of a Windows service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub program: &'static str,
    pub args: Vec<String>,
}

impl Command {
    fn new(program: &'static str, args: &[&str]) -> Command {
        Command { program, args: args.iter().map(|a| a.to_string()).collect() }
    }

    /// The line of `cmd.exe`.
    pub fn line(&self) -> String {
        let words: Vec<String> = self.args.iter().map(|a| windows_word(a)).collect();
        format!("{} {}", self.program, words.join(" "))
    }
}

/// The commands that make the Windows service of `unit`, in their order:
/// the service, its description, its restricted SID, its privileges, its
/// restart after each failure, and its variables. An administrator runs
/// them; [`service_script`] is their text.
pub fn service_commands(unit: &Unit) -> Result<Vec<Command>, Failure> {
    unit.check()?;
    let account = account(unit)?;
    if unit.dir.is_some() || unit.state_dir.is_some() || unit.env_file.is_some() || unit.log.is_some() {
        return Err(unsupported(
            "a Windows service has no directory of the run, state directory, file of variables or log: give \
             each to the program in its arguments",
        ));
    }
    let name = unit.name.as_str();
    let command: Vec<String> =
        std::iter::once(unit.program.display().to_string()).chain(unit.args.iter().cloned()).collect();
    let bin = command.iter().map(|w| windows_word(w)).collect::<Vec<_>>().join(" ");
    let delay = (unit.restart.delay_s.max(1) * 1000).to_string();
    let restart = format!("restart/{delay}/restart/{delay}/restart/{delay}");
    let mut out = vec![
        Command::new(
            "sc.exe",
            &["create", name, "binPath=", &bin, "start=", "auto", "obj=", &account, "DisplayName=", &unit.description],
        ),
        Command::new("sc.exe", &["description", name, &format!("{}. Written by {}.", unit.description, unit.mark)]),
        // A write-restricted token: the service writes only where its SID
        // may.
        Command::new("sc.exe", &["sidtype", name, "restricted"]),
        Command::new("sc.exe", &["privs", name, PRIVILEGES]),
        // The counts of failures start again after a day.
        Command::new("sc.exe", &["failure", name, "reset=", "86400", "actions=", &restart]),
        // An end with an exit code that is not 0 is a failure too.
        Command::new("sc.exe", &["failureflag", name, "1"]),
    ];
    let mut vars: Vec<String> = unit.env.iter().map(|e| format!("{}={}", e.key, e.value)).collect();
    vars.extend(unit.secrets.iter().map(|s| format!("{}={}", s.variable, s.path.display())));
    if !vars.is_empty() {
        let key = format!("HKLM\\SYSTEM\\CurrentControlSet\\Services\\{name}");
        let data = vars.join("\\0");
        out.push(Command::new("reg.exe", &["add", &key, "/v", "Environment", "/t", "REG_MULTI_SZ", "/d", &data, "/f"]));
    }
    Ok(out)
}

/// The commands that remove the Windows service `name`: a stop, then the
/// delete, which also removes the key of its variables.
pub fn service_removal(name: &str) -> Vec<Command> {
    vec![Command::new("sc.exe", &["stop", name]), Command::new("sc.exe", &["delete", name])]
}

/// The text of the commands, one line each, for a person or a script of
/// `cmd.exe`.
pub fn service_script(unit: &Unit) -> Result<String, Failure> {
    let commands = service_commands(unit)?;
    let mut out = vec![format!(":: Written by {}. No secret is in these commands.", unit.mark)];
    out.extend(commands.iter().map(Command::line));
    Ok(out.join("\r\n") + "\r\n")
}

fn no_variables(unit: &Unit, form: &str) -> Result<(), Failure> {
    let parts = !unit.env.is_empty() || !unit.secrets.is_empty() || unit.env_file.is_some();
    if parts || unit.state_dir.is_some() || unit.log.is_some() {
        return Err(unsupported(format!(
            "{form} gives a program no variables, no state directory and no log: give the file of the variables, \
             the secret files, the directory and the log to the program in its arguments"
        )));
    }
    Ok(())
}
