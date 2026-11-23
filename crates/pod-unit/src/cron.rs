//! The lines of the crontab of the user, for a host with no user manager of
//! systemd and no launchd: one line at boot, and with a restart after each
//! end, a watchdog each 5 minutes that starts the program again when it
//! ended. A program under the watchdog holds a lock of its own for its life
//! (`pod_core::lock`), so that the start of the watchdog ends at once while
//! the first run serves. The base is podssh's `node_install/text.rs`;
//! podnet's `autostart.rs` writes the line at boot the same way.
//!
//! cron runs each line with `sh`, and reads `%` as a line break: `\%` is a
//! `%`. A variable or the path of a secret goes before the command
//! (`KEY=VALUE program ...`).

use pod_core::Failure;

use crate::model::{Scope, Unit, unsupported, usage};
use crate::quote::sh_word;

/// The time of the watchdog: each 5 minutes.
pub const WATCHDOG: &str = "*/5 * * * *";

/// The comment above the lines of `unit`, which a removal finds.
pub fn mark(unit: &Unit) -> String {
    format!("# Written by {}: the lines below run the service {}", unit.mark, unit.name)
}

/// The lines of the crontab: the line at boot, then the watchdog when the
/// unit restarts after each end.
pub fn lines(unit: &Unit) -> Result<Vec<String>, Failure> {
    unit.check()?;
    if unit.scope != Scope::User {
        return Err(usage("the crontab is a form of the user: give a form of the system"));
    }
    if unit.env_file.is_some() || unit.state_dir.is_some() {
        return Err(unsupported(
            "cron reads no file of variables and makes no state directory: give each to the program",
        ));
    }
    let mut words: Vec<String> = unit.env.iter().map(|e| sh_word(&format!("{}={}", e.key, e.value))).collect();
    words.extend(unit.secrets.iter().map(|s| sh_word(&format!("{}={}", s.variable, s.path.display()))));
    words.push(sh_word(&unit.program.display().to_string()));
    words.extend(unit.args.iter().map(|w| sh_word(w)));
    let mut run = words.join(" ");
    if let Some(dir) = &unit.dir {
        run = format!("cd {} 2>/dev/null; {run}", sh_word(&dir.display().to_string()));
    }
    if let Some(log) = &unit.log {
        run = format!("{run} >>{} 2>&1", sh_word(&log.display().to_string()));
    }
    let run = run.replace('%', "\\%");
    let mut out = vec![format!("@reboot {run}")];
    if unit.restart.always {
        out.push(format!("{WATCHDOG} {run}"));
    }
    Ok(out)
}

/// `table` without the lines of `unit`, then with `lines` and their mark when
/// they are given; and whether lines of `unit` were there. A line that is an
/// older mark of `unit` ([`Unit::older_marks`]) is a mark too. Below a mark,
/// only an `@reboot` line, and after it the watchdog's line of the same
/// command, are removed: any other line is the user's.
pub fn table(table: &str, unit: &Unit, lines: Option<&[String]>) -> (String, bool) {
    let mark = mark(unit);
    let mut kept: Vec<&str> = Vec::new();
    let mut found = false;
    let mut rest = table.lines().peekable();
    while let Some(next) = rest.next() {
        if next != mark && !unit.older_mark_in(|old| old == next) {
            kept.push(next);
            continue;
        }
        found = true;
        let Some(command) = rest.peek().copied().and_then(|line| line.strip_prefix("@reboot ")) else { continue };
        let watchdog = format!("{WATCHDOG} {command}");
        rest.next();
        if rest.peek().copied() == Some(watchdog.as_str()) {
            rest.next();
        }
    }
    let mut text: String = kept.iter().map(|l| format!("{l}\n")).collect();
    if let Some(lines) = lines {
        text.push_str(&format!("{mark}\n"));
        for line in lines {
            text.push_str(&format!("{line}\n"));
        }
    }
    (text, found)
}
