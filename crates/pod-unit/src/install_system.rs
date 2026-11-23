//! The install and the removal of a unit of the system
//! ([`crate::install::install_system`]): the checks before any write (the
//! privilege of the caller, systemd as the manager, the account of the unit,
//! each file of a secret), then the steps of systemd and launchd of
//! [`crate::install_ways`] in the places of the system, or the commands of
//! `sc.exe` of a Windows service. The base is podssh's
//! `node_install/system.rs` and `system_install.rs`.

use std::path::Path;

use pod_core::{Class, Failure, Stage};

use crate::install::{FileMeta, Form, Host, Removed, SYSTEMD_BOOTED, SystemPlaces, ended, not_ours, step};
use crate::install_ways::how;
use crate::model::{Scope, Unit, unsupported, usage};
use crate::windows::Command;

/// The exit code of `sc.exe` for a service that is not there
/// (`ERROR_SERVICE_DOES_NOT_EXIST`).
const NO_SERVICE: i32 = 1060;

pub(crate) fn install(unit: &Unit, form: Form, places: &SystemPlaces, host: &dyn Host) -> Result<Vec<String>, Failure> {
    let run_as = system_scope(unit)?;
    // Each form is made before a probe, so that a unit that it cannot hold
    // is refused first; each probe comes before a write.
    match form {
        Form::Systemd => {
            let text = crate::systemd::render(unit)?;
            need_admin(host, "an install")?;
            if !host.systemd_booted() {
                return Err(Failure::new(
                    Stage::Host,
                    Class::Unavailable,
                    format!("systemd is not the service manager of this host: {SYSTEMD_BOOTED} is not there"),
                ));
            }
            if let Some(user) = run_as {
                account(host, user)?;
            }
            // systemd reads each file as root: LoadCredential= and
            // EnvironmentFile=.
            secrets(unit, host, 0, "root")?;
            crate::install_ways::systemd(unit, &text, &places.systemd, false, host)
        }
        Form::Launchd => {
            let text = crate::launchd::render(unit)?;
            // launchd::render refuses a daemon with no account.
            let user = run_as.unwrap_or_default();
            need_admin(host, "an install")?;
            // The daemon reads each file as its account.
            secrets(unit, host, account(host, user)?, user)?;
            crate::install_ways::launchd(unit, &text, &places.launch_daemons, "system", host)
        }
        Form::Service => {
            let commands = crate::windows::service_commands(unit)?;
            need_admin(host, "an install")?;
            service(unit, commands, host)
        }
        Form::Task | Form::Cron => Err(user_alone(form)),
    }
}

pub(crate) fn remove(unit: &Unit, form: Form, places: &SystemPlaces, host: &dyn Host) -> Result<Removed, Failure> {
    system_scope(unit)?;
    need_admin(host, "a removal")?;
    match form {
        Form::Systemd => crate::install_ways::remove_systemd(unit, &places.systemd, false, host),
        Form::Launchd => crate::install_ways::remove_launchd(unit, &places.launch_daemons, "system", host),
        Form::Service => remove_service(unit, host),
        Form::Task | Form::Cron => Err(unsupported(format!(
            "{} holds a unit of the user alone: give systemd, launchd or service",
            form.name()
        ))),
    }
}

fn user_alone(form: Form) -> Failure {
    unsupported(format!("{} holds a unit of the user alone: give systemd, launchd or service", form.name()))
}

/// The account of a unit of the system (None: one that the manager makes).
fn system_scope(unit: &Unit) -> Result<Option<&str>, Failure> {
    match &unit.scope {
        Scope::System { run_as } => Ok(run_as.as_deref()),
        Scope::User => Err(usage("a unit of the user: give it to install, which needs no privilege")),
    }
}

fn need_admin(host: &dyn Host, what: &str) -> Result<(), Failure> {
    if host.admin() {
        return Ok(());
    }
    Err(Failure::new(
        Stage::Host,
        Class::Refused,
        format!(
            "{what} of a unit of the system changes the services of the host: run it as root (such as with sudo), \
             or as an administrator on Windows"
        ),
    ))
}

/// The uid of the account `user`, by `id -u`, which reads each user
/// database of the host (a static program reads `/etc/passwd` alone). root
/// is refused: the hardening holds an account with no privilege.
fn account(host: &dyn Host, user: &str) -> Result<u32, Failure> {
    let ran = host.run("id", &["-u", user], None)?;
    if !ran.success() {
        return Err(Failure::new(
            Stage::Host,
            Class::NotFound,
            format!("the account {user} is not on this host: make it first ({})", ran.said()),
        ));
    }
    let uid: u32 = ran.stdout.trim().parse().map_err(|_| {
        Failure::new(Stage::Host, Class::Unavailable, format!("id -u {user} gave no uid: {:?}", ran.stdout.trim()))
    })?;
    if uid == 0 {
        return Err(usage(format!(
            "the account {user} is root: give an account with no privilege, or none for an account of the service \
             alone"
        )));
    }
    Ok(uid)
}

/// Each file of a secret and the file of variables: a regular file of the
/// uid `owner` with no access for the group and others. On Windows, which
/// has no uid and mode, a regular file.
fn secrets(unit: &Unit, host: &dyn Host, owner: u32, who: &str) -> Result<(), Failure> {
    let files = unit.secrets.iter().map(|s| (format!("the secret {}", s.name), s.path.as_path()));
    for (what, path) in files.chain(unit.env_file.iter().map(|f| ("the file of variables".to_string(), f.as_path()))) {
        let meta = host.file_meta(path).map_err(|e| {
            let class = if e.kind() == std::io::ErrorKind::NotFound { Class::NotFound } else { Class::Config };
            Failure::new(Stage::Host, class, format!("{what}, {}: {e}: write it first", path.display()))
        })?;
        if let FileMeta { uid: Some(uid), mode: Some(mode) } = meta
            && (uid != owner || mode & 0o077 != 0)
        {
            return Err(Failure::new(
                Stage::Host,
                Class::Config,
                format!(
                    "{what}, {}: of uid {uid} with the mode {mode:o}; give a file of {who} that no other may read \
                     (chown {who} FILE; chmod 0600 FILE)",
                    path.display()
                ),
            ));
        }
    }
    Ok(())
}

/// True when the service `unit.name` is there and holds the mark of `unit`,
/// or an older mark, in its description; None when there is no such service.
fn service_there(unit: &Unit, host: &dyn Host) -> Result<Option<bool>, Failure> {
    let ran = host.run("sc.exe", &["qdescription", &unit.name, "8192"], None)?;
    if ran.code == Some(NO_SERVICE) {
        return Ok(None);
    }
    if !ran.success() {
        return Err(ended("sc.exe", &["qdescription", &unit.name, "8192"], &ran));
    }
    let ours = ran.stdout.contains(&format!("Written by {}.", unit.mark));
    Ok(Some(ours || unit.older_mark_in(|old| ran.stdout.contains(old))))
}

fn service(unit: &Unit, mut commands: Vec<Command>, host: &dyn Host) -> Result<Vec<String>, Failure> {
    secrets(unit, host, 0, "the administrator")?;
    let name = unit.name.as_str();
    let not_ours_service = || not_ours(Path::new(&format!("the service {name}")), &unit.mark);
    let again = match service_there(unit, host)? {
        Some(false) => return Err(not_ours_service()),
        Some(true) => {
            // The service of an earlier install takes the new words in place:
            // a deleted service stays until each handle to it closes.
            let _ = host.run("sc.exe", &["stop", name], None);
            let key = format!("HKLM\\SYSTEM\\CurrentControlSet\\Services\\{name}");
            let _ = host.run("reg.exe", &["delete", &key, "/v", "Environment", "/f"], None);
            commands[0].args[0] = "config".to_string();
            true
        }
        None => false,
    };
    let made = commands.iter().try_for_each(|c| {
        let args: Vec<&str> = c.args.iter().map(String::as_str).collect();
        step(host, c.program, &args, None).map(drop)
    });
    if let Err(e) = made.and_then(|()| step(host, "sc.exe", &["start", name], None).map(drop)) {
        // A service that cannot start is not left behind.
        let _ = host.run("sc.exe", &["stop", name], None);
        let _ = host.run("sc.exe", &["delete", name], None);
        return Err(e);
    }
    let done = if again { "changed" } else { "made" };
    Ok(vec![format!("a Windows service, {name}, {done} and started: it starts at each boot")])
}

fn remove_service(unit: &Unit, host: &dyn Host) -> Result<Removed, Failure> {
    let name = unit.name.as_str();
    match service_there(unit, host)? {
        None => return Ok(Removed::Nothing),
        Some(false) => return Err(not_ours(Path::new(&format!("the service {name}")), &unit.mark)),
        Some(true) => {}
    }
    let stopped = step(host, "sc.exe", &["stop", name], None);
    // The delete also removes the key of the variables of the service.
    step(host, "sc.exe", &["delete", name], None)?;
    Ok(Removed::Done(vec![format!("removed the Windows service {name} ({})", how(stopped, "stopped"))]))
}

#[cfg(unix)]
pub(crate) fn this_admin() -> bool {
    pod_core::files::euid() == 0
}

/// A token that may create a service: the right that `sc.exe create` needs.
#[cfg(windows)]
pub(crate) fn this_admin() -> bool {
    use windows_sys::Win32::System::Services::{CloseServiceHandle, OpenSCManagerW, SC_MANAGER_CREATE_SERVICE};
    // SAFETY: null names give the manager of this machine and its active
    // database; the handle is closed at once.
    let handle = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CREATE_SERVICE) };
    if handle.is_null() {
        return false;
    }
    // SAFETY: the handle is open, and this is its one close.
    unsafe { CloseServiceHandle(handle) };
    true
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn this_admin() -> bool {
    false
}

pub(crate) fn this_file_meta(path: &Path) -> std::io::Result<FileMeta> {
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_file() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "no regular file (a link is not followed)"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(FileMeta { uid: Some(meta.uid()), mode: Some(meta.mode() & 0o7777) })
    }
    #[cfg(not(unix))]
    {
        Ok(FileMeta { uid: None, mode: None })
    }
}
