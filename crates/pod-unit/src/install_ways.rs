//! The steps of each form of an install of the user and of its removal
//! ([`crate::install`]); the files of systemd and launchd are also the steps
//! of an install of the system ([`crate::install_system`]).

use std::path::Path;

use pod_core::{Failure, Stage};

use crate::install::{Host, Places, Removed, not_ours, step};
use crate::model::Unit;

/// Write `text` at `path` (the mode 0644: a manager refuses a unit that
/// others may write), with its directory.
fn write(path: &Path, text: &[u8], mode: u32) -> Result<(), Failure> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Failure::io(Stage::Host, format!("making {}", dir.display()), e))?;
    }
    pod_core::files::replace_private(path, text, mode, false)
        .map(drop)
        .map_err(|e| Failure::io(Stage::Host, format!("writing {}", path.display()), e))
}

/// True when the file at `path` is there; an error when it is there and
/// does not hold `mark`.
fn ours(path: &Path, mark: &str, holds: impl Fn(&str) -> bool) -> Result<bool, Failure> {
    match std::fs::read_to_string(path) {
        Ok(text) if holds(&text) => Ok(true),
        Ok(_) => Err(not_ours(path, mark)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(Failure::io(Stage::Host, format!("reading {}", path.display()), e)),
    }
}

/// The mark of `unit` on the first line, or a first line that starts with
/// an older mark.
fn systemd_mark(unit: &Unit) -> impl Fn(&str) -> bool + '_ {
    move |text: &str| {
        let first = text.lines().next().unwrap_or_default();
        text.starts_with(&format!("# Written by {}.\n", unit.mark)) || unit.older_mark_in(|old| first.starts_with(old))
    }
}

fn launchd_mark(unit: &Unit) -> impl Fn(&str) -> bool + '_ {
    move |text: &str| {
        text.contains(&format!("{}{}.", crate::launchd::MARK, crate::quote::xml(&unit.mark)))
            || unit.older_mark_in(|old| text.contains(old))
    }
}

/// `systemctl ARGS`, with `--user` for a user unit.
fn systemctl<'a>(user: bool, args: &[&'a str]) -> Vec<&'a str> {
    if user { std::iter::once("--user").chain(args.iter().copied()).collect() } else { args.to_vec() }
}

/// The unit of systemd `unit`, rendered as `text`, in `dir`: a user unit
/// when `user`, else a unit of the system.
pub(crate) fn systemd(
    unit: &Unit,
    text: &str,
    dir: &Path,
    user: bool,
    host: &dyn Host,
) -> Result<Vec<String>, Failure> {
    let file = crate::systemd::file_name(unit);
    let path = dir.join(&file);
    ours(&path, &unit.mark, systemd_mark(unit))?;
    write(&path, text.as_bytes(), 0o644)?;
    let run = |args: &[&str]| step(host, "systemctl", &systemctl(user, args), None);
    let started = run(&["daemon-reload"]).and_then(|_| run(&["enable", &file])).and_then(|_| run(&["restart", &file]));
    if let Err(e) = started {
        // A unit that cannot start is not left behind.
        let _ = host.run("systemctl", &systemctl(user, &["disable", &file]), None);
        let _ = std::fs::remove_file(&path);
        let _ = host.run("systemctl", &systemctl(user, &["daemon-reload"]), None);
        return Err(e);
    }
    let what = if user { "a user unit" } else { "a unit of the system" };
    Ok(vec![format!("{what} of systemd, {}, enabled and started", path.display())])
}

pub(crate) fn remove_systemd(unit: &Unit, dir: &Path, user: bool, host: &dyn Host) -> Result<Removed, Failure> {
    let file = crate::systemd::file_name(unit);
    let path = dir.join(&file);
    if !ours(&path, &unit.mark, systemd_mark(unit))? {
        return Ok(Removed::Nothing);
    }
    let stopped = step(host, "systemctl", &systemctl(user, &["disable", "--now", &file]), None);
    std::fs::remove_file(&path).map_err(|e| Failure::io(Stage::Host, format!("removing {}", path.display()), e))?;
    let _ = host.run("systemctl", &systemctl(user, &["daemon-reload"]), None);
    let _ = host.run("systemctl", &systemctl(user, &["reset-failed", &file]), None);
    let what = if user { "the user unit" } else { "the unit of the system" };
    Ok(Removed::Done(vec![format!("removed {what} {} ({})", path.display(), how(stopped, "stopped"))]))
}

/// The plist of `unit`, rendered as `text`, in `dir`, loaded in the domain of launchd `domain`:
/// `gui/UID` for a LaunchAgent, `system` for a LaunchDaemon.
pub(crate) fn launchd(
    unit: &Unit,
    text: &str,
    dir: &Path,
    domain: &str,
    host: &dyn Host,
) -> Result<Vec<String>, Failure> {
    let path = dir.join(crate::launchd::file_name(unit));
    ours(&path, &unit.mark, launchd_mark(unit))?;
    let target = format!("{domain}/{}", unit.launchd_label());
    write(&path, text.as_bytes(), 0o644)?;
    // A job that was loaded leaves first, so that the new words take; one
    // that a person disabled would not load.
    let _ = host.run("launchctl", &["bootout", &target], None);
    let _ = host.run("launchctl", &["enable", &target], None);
    let shown = path.display().to_string();
    if let Err(e) = step(host, "launchctl", &["bootstrap", domain, &shown], None) {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    let (what, when) = if domain == "system" { ("a LaunchDaemon", "boot") } else { ("a LaunchAgent", "login") };
    Ok(vec![format!("{what}, {shown}, loaded in {domain}: it starts now and at each {when}")])
}

pub(crate) fn remove_launchd(unit: &Unit, dir: &Path, domain: &str, host: &dyn Host) -> Result<Removed, Failure> {
    let path = dir.join(crate::launchd::file_name(unit));
    if !ours(&path, &unit.mark, launchd_mark(unit))? {
        return Ok(Removed::Nothing);
    }
    let target = format!("{domain}/{}", unit.launchd_label());
    let stopped = step(host, "launchctl", &["bootout", &target], None);
    std::fs::remove_file(&path).map_err(|e| Failure::io(Stage::Host, format!("removing {}", path.display()), e))?;
    let what = if domain == "system" { "LaunchDaemon" } else { "LaunchAgent" };
    Ok(Removed::Done(vec![format!("removed the {what} {} ({})", path.display(), how(stopped, "stopped"))]))
}

/// The XML of the task `name` that is there, when one is.
fn task_there(host: &dyn Host, name: &str) -> Result<Option<String>, Failure> {
    let ran = host.run("schtasks", &["/Query", "/TN", name, "/XML"], None)?;
    Ok(ran.success().then_some(ran.stdout))
}

/// The mark of `unit` in the XML of a task, or an older mark as it is or
/// with the escapes of XML.
fn task_ours(unit: &Unit, xml: &str) -> bool {
    xml.contains(&crate::quote::xml(&format!("Written by {}.", unit.mark)))
        || unit.older_mark_in(|old| xml.contains(old) || xml.contains(&crate::quote::xml(old)))
}

pub(crate) fn task(unit: &Unit, places: &Places, host: &dyn Host) -> Result<Vec<String>, Failure> {
    let xml = crate::windows::task_xml(unit, &places.windows_user)?;
    if let Some(there) = task_there(host, &unit.name)?
        && !task_ours(unit, &there)
    {
        return Err(not_ours(Path::new(&format!("the task {}", unit.name)), &unit.mark));
    }
    let file = places.scratch.join(format!("{}.task.xml", unit.name));
    write(&file, &crate::windows::utf16(&xml), 0o600)?;
    let shown = file.display().to_string();
    let made = step(host, "schtasks", &["/Create", "/TN", &unit.name, "/XML", &shown, "/F"], None);
    let _ = std::fs::remove_file(&file);
    made?;
    // A run of an older task ends first, so that the new words take.
    let _ = host.run("schtasks", &["/End", "/TN", &unit.name], None);
    let started = step(host, "schtasks", &["/Run", "/TN", &unit.name], None);
    Ok(vec![format!("a task at the logon of {}, {} ({})", places.windows_user, unit.name, how(started, "started now"))])
}

pub(crate) fn remove_task(unit: &Unit, host: &dyn Host) -> Result<Removed, Failure> {
    let Some(there) = task_there(host, &unit.name)? else { return Ok(Removed::Nothing) };
    if !task_ours(unit, &there) {
        return Err(not_ours(Path::new(&format!("the task {}", unit.name)), &unit.mark));
    }
    let _ = host.run("schtasks", &["/End", "/TN", &unit.name], None);
    step(host, "schtasks", &["/Delete", "/TN", &unit.name, "/F"], None)?;
    Ok(Removed::Done(vec![format!("removed the task {}", unit.name)]))
}

/// The crontab of the user; empty when the user has none.
fn crontab(host: &dyn Host) -> Result<String, Failure> {
    let ran = host.run("crontab", &["-l"], None)?;
    if ran.success() {
        return Ok(ran.stdout);
    }
    if ran.stderr.contains("no crontab") {
        return Ok(String::new());
    }
    step(host, "crontab", &["-l"], None).map(|r| r.stdout)
}

pub(crate) fn cron(unit: &Unit, host: &dyn Host) -> Result<Vec<String>, Failure> {
    let lines = crate::cron::lines(unit)?;
    let (table, found) = crate::cron::table(&crontab(host)?, unit, Some(&lines));
    step(host, "crontab", &["-"], Some(table.as_bytes()))?;
    let done = if found { "replaced" } else { "added" };
    Ok(vec![format!("the lines of {} in the crontab, {done}: they start it at the next boot", unit.name)])
}

pub(crate) fn remove_cron(unit: &Unit, host: &dyn Host) -> Result<Removed, Failure> {
    let (table, found) = crate::cron::table(&crontab(host)?, unit, None);
    if !found {
        return Ok(Removed::Nothing);
    }
    step(host, "crontab", &["-"], Some(table.as_bytes()))?;
    Ok(Removed::Done(vec![format!("removed the lines of {} from the crontab", unit.name)]))
}

/// The words of a stop or a start that can fail with no harm.
pub(crate) fn how(done: Result<pod_core::process::Ran, Failure>, did: &str) -> String {
    match done {
        Ok(_) => did.to_string(),
        Err(e) => format!("not now: {}", e.chain()),
    }
}
