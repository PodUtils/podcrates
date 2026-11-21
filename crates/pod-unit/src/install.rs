//! The install and the removal of a unit, each an explicit call of the
//! program.
//!
//! * A unit of the user ([`install`], [`remove`]): a user unit of systemd
//!   (`systemctl --user`), a LaunchAgent (`launchctl` in `gui/UID`), a task
//!   at the logon (`schtasks`), or the lines of the crontab. No privilege is
//!   necessary.
//! * A unit of the system ([`install_system`], [`remove_system`]): a unit of
//!   systemd in `/etc/systemd/system`, a LaunchDaemon in
//!   `/Library/LaunchDaemons` (`launchctl` in `system`), or a Windows service
//!   (`sc.exe`). root, or an administrator on Windows, is necessary: a
//!   caller with no such privilege is refused before any write.
//!
//! The base is podssh's `node_install/unix.rs`, `launchd.rs`, `windows.rs`,
//! `system.rs` and `system_install.rs`, and podnet's `autostart.rs`.
//!
//! An install that fails leaves no file. A removal removes only what
//! carries the mark of the unit, and leaves nothing of it: a file, a task or
//! a service of another mark is refused, and stays.
//!
//! The crate chooses no directory: the caller gives the [`Places`] or the
//! [`SystemPlaces`]. Each program runs through the caller's [`Host`], within
//! a limit; each probe of the host (root, systemd, a file of a secret) is a
//! method of [`Host`], so that a test gives its own.

use std::path::{Path, PathBuf};
use std::time::Duration;

use pod_core::process::Ran;
use pod_core::{Class, Failure, Stage};

use crate::model::{Scope, Unit, unsupported, usage};

/// A form of a unit: systemd and launchd hold a unit of the user and one of
/// the system; a task and cron hold a unit of the user alone, and a Windows
/// service a unit of the system alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    Systemd,
    Launchd,
    /// A task of the Task Scheduler at the logon of the user.
    Task,
    Cron,
    /// A Windows service of the service control manager (`sc.exe`).
    Service,
}

impl Form {
    pub fn name(self) -> &'static str {
        match self {
            Form::Systemd => "systemd",
            Form::Launchd => "launchd",
            Form::Task => "task",
            Form::Cron => "cron",
            Form::Service => "service",
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

/// The places of the units of the system. The default is the place of each
/// manager; a test gives its own directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemPlaces {
    /// The units of systemd of the administrator: `/etc/systemd/system`.
    pub systemd: PathBuf,
    /// The LaunchDaemons: `/Library/LaunchDaemons`.
    pub launch_daemons: PathBuf,
}

impl Default for SystemPlaces {
    fn default() -> SystemPlaces {
        SystemPlaces {
            systemd: PathBuf::from("/etc/systemd/system"),
            launch_daemons: PathBuf::from("/Library/LaunchDaemons"),
        }
    }
}

/// The directory that is there when systemd is the service manager of this
/// host, as `sd_booted(3)` reads it.
pub const SYSTEMD_BOOTED: &str = "/run/systemd/system";

/// The owner and the mode of a regular file, as [`Host::file_meta`] reads
/// them. Each is None on Windows, which has neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileMeta {
    pub uid: Option<u32>,
    /// The bits of the permissions (`0o600`).
    pub mode: Option<u32>,
}

/// The runs of programs and the probes of the host: [`System`] runs them on
/// this host; a test gives its own. The default of each probe reads this
/// host.
pub trait Host {
    /// Run `program` with `args`, and `input` on its stdin.
    fn run(&self, program: &str, args: &[&str], input: Option<&[u8]>) -> Result<Ran, Failure>;

    /// True when this process may install a service of the system: the
    /// effective uid 0 on Unix; on Windows, a token that may create a
    /// service (an elevated administrator).
    fn admin(&self) -> bool {
        crate::install_system::this_admin()
    }

    /// True when systemd is the service manager of this host
    /// ([`SYSTEMD_BOOTED`] is a directory).
    fn systemd_booted(&self) -> bool {
        std::fs::symlink_metadata(SYSTEMD_BOOTED).is_ok_and(|m| m.is_dir())
    }

    /// The owner and the mode of the regular file at `path`, read with no
    /// link followed: a link, a directory or no file is an error.
    fn file_meta(&self, path: &Path) -> std::io::Result<FileMeta> {
        crate::install_system::this_file_meta(path)
    }
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

/// The form of the system of this host: a Windows service on Windows; else
/// systemd when it is the service manager, then launchd when `launchctl`
/// reads the domain `system`. With none, the reason of each.
pub fn probe_system(host: &dyn Host) -> Result<Form, Failure> {
    if cfg!(windows) {
        return Ok(Form::Service);
    }
    if host.systemd_booted() {
        return Ok(Form::Systemd);
    }
    let launchd = match host.run("launchctl", &["print", "system"], None) {
        Ok(ran) if ran.success() => return Ok(Form::Launchd),
        Ok(ran) => format!("launchctl: {}", ran.said()),
        Err(e) => e.chain(),
    };
    Err(Failure::new(
        Stage::Host,
        Class::Unavailable,
        format!("no service manager of the system: {SYSTEMD_BOOTED} is not there; {launchd}"),
    ))
}

/// Install `unit`, a unit of the user, in `form`, and start it now. The
/// lines say what was done.
pub fn install(unit: &Unit, form: Form, places: &Places, host: &dyn Host) -> Result<Vec<String>, Failure> {
    if unit.scope != Scope::User {
        return Err(usage("a unit of the system: give it to install_system"));
    }
    match form {
        Form::Systemd => {
            crate::install_ways::systemd(unit, &crate::systemd::render(unit)?, &places.systemd_user, true, host)
        }
        Form::Launchd => {
            let (text, domain) = (crate::launchd::render(unit)?, format!("gui/{}", places.uid));
            crate::install_ways::launchd(unit, &text, &places.launch_agents, &domain, host)
        }
        Form::Task => crate::install_ways::task(unit, places, host),
        Form::Cron => crate::install_ways::cron(unit, host),
        Form::Service => Err(unsupported("a Windows service is a unit of the system: give the task form for a user")),
    }
}

/// Remove the unit of the user `unit` in `form`, stopped first. Only a file,
/// a task or lines with the mark of `unit` are removed.
pub fn remove(unit: &Unit, form: Form, places: &Places, host: &dyn Host) -> Result<Removed, Failure> {
    match form {
        Form::Systemd => crate::install_ways::remove_systemd(unit, &places.systemd_user, true, host),
        Form::Launchd => {
            crate::install_ways::remove_launchd(unit, &places.launch_agents, &format!("gui/{}", places.uid), host)
        }
        Form::Task => crate::install_ways::remove_task(unit, host),
        Form::Cron => crate::install_ways::remove_cron(unit, host),
        Form::Service => Err(unsupported("a Windows service is a unit of the system: give it to remove_system")),
    }
}

/// Install `unit`, a unit of the system, in `form` (systemd, launchd or a
/// Windows service), and start it now. The checks come before any write:
/// the privilege of the caller ([`Host::admin`], else `Class::Refused`),
/// systemd as the manager, the account of the unit (`id -u`; root is
/// refused), and each file of a secret: of root with no access for others
/// (systemd reads it), or of the account (launchd). A unit that does not
/// start is removed. The lines say what was done.
pub fn install_system(unit: &Unit, form: Form, places: &SystemPlaces, host: &dyn Host) -> Result<Vec<String>, Failure> {
    crate::install_system::install(unit, form, places, host)
}

/// Remove the unit of the system `unit` in `form`, stopped first. Only a
/// file or a service with the mark of `unit` is removed. The files of the
/// secrets and the state directory are the caller's, and stay.
pub fn remove_system(unit: &Unit, form: Form, places: &SystemPlaces, host: &dyn Host) -> Result<Removed, Failure> {
    crate::install_system::remove(unit, form, places, host)
}

/// `program ARGS` as a step that must pass.
pub(crate) fn step(host: &dyn Host, program: &str, args: &[&str], input: Option<&[u8]>) -> Result<Ran, Failure> {
    let ran = host.run(program, args, input)?;
    if ran.success() {
        return Ok(ran);
    }
    Err(ended(program, args, &ran))
}

/// The failure of `program ARGS`, which ended as `ran` says.
pub(crate) fn ended(program: &str, args: &[&str], ran: &Ran) -> Failure {
    let code = ran.code.map_or_else(|| "a signal".to_string(), |c| c.to_string());
    Failure::new(
        Stage::Host,
        Class::Unavailable,
        format!("{program} {} ended with {code}: {}", args.join(" "), ran.said()),
    )
}

/// A file of another program, which an install or a removal leaves.
pub(crate) fn not_ours(path: &Path, mark: &str) -> Failure {
    Failure::new(
        Stage::Host,
        Class::Config,
        format!("{} is not written by {mark}: it stays; remove it or give another name", path.display()),
    )
}
