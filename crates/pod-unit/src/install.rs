//! The install and the removal of a unit of the user, each an explicit call
//! of the program: a user unit of systemd (`systemctl --user`), a
//! LaunchAgent (`launchctl` in `gui/UID`), a task at the logon
//! (`schtasks`), or the lines of the crontab. No privilege is necessary.
//! The base is podssh's `node_install/unix.rs`, `launchd.rs` and
//! `windows.rs`, and podnet's `autostart.rs`.
//!
//! An install that fails leaves no file. A removal removes only what
//! carries the mark of the unit, and leaves nothing of it: a file or a task
//! of another mark is refused, and stays.
//!
//! The crate chooses no directory: the caller gives the [`Places`]. Each
//! program runs through the caller's [`Host`], within a limit.

use std::path::{Path, PathBuf};
use std::time::Duration;

use pod_core::process::Ran;
use pod_core::{Class, Failure, Stage};

use crate::model::{Scope, Unit, usage};

/// A form of a unit of the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    Systemd,
    Launchd,
    /// A task of the Task Scheduler at the logon of the user.
    Task,
    Cron,
}

impl Form {
    pub fn name(self) -> &'static str {
        match self {
            Form::Systemd => "systemd",
            Form::Launchd => "launchd",
            Form::Task => "task",
            Form::Cron => "cron",
        }
    }
}

/// The places of the units of the user, which the caller gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Places {
    /// The user units of systemd: `~/.config/systemd/user`.
    pub systemd_user: PathBuf,
    /// The LaunchAgents: `~/Library/LaunchAgents`.
    pub launch_agents: PathBuf,
    /// A private directory of the program, where the XML of a task stays
    /// while `schtasks` reads it.
    pub scratch: PathBuf,
    /// The uid of the user: launchd's domain is `gui/UID`.
    pub uid: u32,
    /// The user of a task, as its logon trigger names it: `DOMAIN\USER`.
    pub windows_user: String,
}

impl Places {
    /// The places under the home `home`, with the default directory of the
    /// configuration (`~/.config`).
    pub fn of_home(home: &Path, scratch: &Path, uid: u32, windows_user: &str) -> Places {
        Places {
            systemd_user: home.join(".config/systemd/user"),
            launch_agents: home.join("Library/LaunchAgents"),
            scratch: scratch.to_path_buf(),
            uid,
            windows_user: windows_user.to_string(),
        }
    }
}

/// The runs of programs: [`System`] runs them; a test gives its own.
pub trait Host {
    /// Run `program` with `args`, and `input` on its stdin.
    fn run(&self, program: &str, args: &[&str], input: Option<&[u8]>) -> Result<Ran, Failure>;
}

/// The programs of this host, each run within `limit`.
#[derive(Debug, Clone, Copy)]
pub struct System {
    pub limit: Duration,
}

impl Default for System {
    fn default() -> System {
        System { limit: Duration::from_secs(30) }
    }
}

impl Host for System {
    fn run(&self, program: &str, args: &[&str], input: Option<&[u8]>) -> Result<Ran, Failure> {
        pod_core::process::run_bounded(program, args, input, self.limit).map_err(|e| {
            let class = match e {
                pod_core::process::RunFault::Limit(_) => Class::Timeout,
                _ => Class::Unavailable,
            };
            Failure::caused(Stage::Host, class, format!("{program} {}", args.join(" ")), e)
        })
    }
}

/// What a removal did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removed {
    /// No file, task or line of the unit was there.
    Nothing,
    /// The steps that were done, one line each.
    Done(Vec<String>),
}

/// The first form of the user that this host has, by a probe of each in
/// order: `schtasks` on Windows; else `systemctl --user`, `launchctl` with
/// the login session of the user, then `crontab -l`. With none, the reason
/// of each.
pub fn probe(host: &dyn Host, places: &Places) -> Result<Form, Failure> {
    let mut why = Vec::new();
    let mut try_form = |form: Form, program: &str, args: &[&str]| match host.run(program, args, None) {
        Ok(ran) if ran.success() => true,
        Ok(ran) if form == Form::Cron && ran.stderr.contains("no crontab") => true,
        Ok(ran) => {
            why.push(format!("{program}: {}", ran.said()));
            false
        }
        Err(e) => {
            why.push(e.chain());
            false
        }
    };
    if cfg!(windows) {
        if try_form(Form::Task, "schtasks", &["/Query", "/FO", "LIST"]) {
            return Ok(Form::Task);
        }
    } else {
        if try_form(Form::Systemd, "systemctl", &["--user", "show-environment"]) {
            return Ok(Form::Systemd);
        }
        if try_form(Form::Launchd, "launchctl", &["print", &format!("gui/{}", places.uid)]) {
            return Ok(Form::Launchd);
        }
        if try_form(Form::Cron, "crontab", &["-l"]) {
            return Ok(Form::Cron);
        }
    }
    Err(Failure::new(Stage::Host, Class::Unavailable, format!("no form of a service of the user: {}", why.join("; "))))
}

/// Install `unit`, a unit of the user, in `form`, and start it now. The
/// lines say what was done.
pub fn install(unit: &Unit, form: Form, places: &Places, host: &dyn Host) -> Result<Vec<String>, Failure> {
    if unit.scope != Scope::User {
        return Err(usage("an install of the system is not here: give a unit of the user"));
    }
    match form {
        Form::Systemd => crate::install_ways::systemd(unit, places, host),
        Form::Launchd => crate::install_ways::launchd(unit, places, host),
        Form::Task => crate::install_ways::task(unit, places, host),
        Form::Cron => crate::install_ways::cron(unit, host),
    }
}

/// Remove the unit of the user `unit` in `form`, stopped first. Only a file,
/// a task or lines with the mark of `unit` are removed.
pub fn remove(unit: &Unit, form: Form, places: &Places, host: &dyn Host) -> Result<Removed, Failure> {
    match form {
        Form::Systemd => crate::install_ways::remove_systemd(unit, places, host),
        Form::Launchd => crate::install_ways::remove_launchd(unit, places, host),
        Form::Task => crate::install_ways::remove_task(unit, host),
        Form::Cron => crate::install_ways::remove_cron(unit, host),
    }
}

/// `program ARGS` as a step that must pass.
pub(crate) fn step(host: &dyn Host, program: &str, args: &[&str], input: Option<&[u8]>) -> Result<Ran, Failure> {
    let ran = host.run(program, args, input)?;
    if ran.success() {
        return Ok(ran);
    }
    let code = ran.code.map_or_else(|| "a signal".to_string(), |c| c.to_string());
    Err(Failure::new(
        Stage::Host,
        Class::Unavailable,
        format!("{program} {} ended with {code}: {}", args.join(" "), ran.said()),
    ))
}

/// A file of another program, which an install or a removal leaves.
pub(crate) fn not_ours(path: &Path, mark: &str) -> Failure {
    Failure::new(
        Stage::Host,
        Class::Config,
        format!("{} is not written by {mark}: it stays; remove it or give another name", path.display()),
    )
}
