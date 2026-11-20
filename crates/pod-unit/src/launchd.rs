//! A plist of launchd: a LaunchAgent of the user
//! (`~/Library/LaunchAgents/LABEL.plist`, loaded into `gui/UID`) or a
//! LaunchDaemon of the system (`/Library/LaunchDaemons/LABEL.plist`) that
//! runs as a named user. The base is podssh's `node_install/text.rs`.
//!
//! launchd has no file of variables, no state directory, no credential and
//! no rule of exit codes: a unit with a file of variables or a state
//! directory is refused, a secret is the path of its private file in a
//! variable, and `ThrottleInterval` bounds a loop of starts after a 78.

use pod_core::Failure;

use crate::model::{Scope, Unit, unsupported};
use crate::quote::xml;

/// The start of the comment that names what wrote the plist. A comment of
/// XML may hold no `--`, which [`Unit::check`] refuses in a mark.
pub const MARK: &str = "<!-- Written by ";

/// The name of the file: `LABEL.plist`.
pub fn file_name(unit: &Unit) -> String {
    format!("{}.plist", unit.launchd_label())
}

/// The text of the plist, after the checks of [`Unit::check`].
pub fn render(unit: &Unit) -> Result<String, Failure> {
    unit.check()?;
    if unit.env_file.is_some() {
        return Err(unsupported("launchd reads no file of variables: give the file to the program in its arguments"));
    }
    if unit.state_dir.is_some() {
        return Err(unsupported("launchd makes no state directory: give the directory to the program"));
    }
    let user = match &unit.scope {
        Scope::User => None,
        Scope::System { run_as: Some(user) } => Some(user.as_str()),
        Scope::System { run_as: None } => {
            return Err(unsupported("launchd makes no account for a service: name the user of the daemon"));
        }
    };
    let string = |text: &str| format!("<string>{}</string>", xml(text));
    let key = |name: &str| format!("  <key>{name}</key>");
    let mut lines = vec![
        r#"<?xml version="1.0" encoding="UTF-8"?>"#.to_string(),
        r#"<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">"#
            .to_string(),
        format!("{MARK}{}. No secret is in this file, which each user may read. -->", xml(&unit.mark)),
        r#"<plist version="1.0">"#.to_string(),
        "<dict>".to_string(),
        key("Label"),
        format!("  {}", string(unit.launchd_label())),
        key("ProgramArguments"),
        "  <array>".to_string(),
    ];
    let words = std::iter::once(unit.program.display().to_string()).chain(unit.args.iter().cloned());
    lines.extend(words.map(|w| format!("    {}", string(&w))));
    lines.push("  </array>".to_string());
    if let Some(user) = user {
        lines.extend([key("UserName"), format!("  {}", string(user))]);
    }
    if let Some(dir) = &unit.dir {
        lines.extend([key("WorkingDirectory"), format!("  {}", string(&dir.display().to_string()))]);
    }
    let mut vars: Vec<(String, String)> = unit.env.iter().map(|e| (e.key.clone(), e.value.clone())).collect();
    vars.extend(unit.secrets.iter().map(|s| (s.variable.clone(), s.path.display().to_string())));
    if !vars.is_empty() {
        lines.extend([key("EnvironmentVariables"), "  <dict>".to_string()]);
        for (k, v) in vars {
            lines.extend([format!("    <key>{}</key>", xml(&k)), format!("    {}", string(&v))]);
        }
        lines.push("  </dict>".to_string());
    }
    lines.extend([key("RunAtLoad"), "  <true/>".to_string(), key("KeepAlive")]);
    if unit.restart.always {
        lines.push("  <true/>".to_string());
    } else {
        lines.extend(["  <dict>", "    <key>SuccessfulExit</key>", "    <false/>", "  </dict>"].map(String::from));
    }
    lines.extend([key("ThrottleInterval"), format!("  <integer>{}</integer>", unit.restart.delay_s)]);
    // 0: no limit, as launchd reads it.
    lines.extend([key("ExitTimeOut"), format!("  <integer>{}</integer>", unit.stop.timeout_s.unwrap_or(0))]);
    if unit.hardening.relaxed("UMask").is_none() {
        // 0077 as launchd reads it: a decimal integer.
        lines.extend([key("Umask"), "  <integer>63</integer>".to_string()]);
    }
    if let Some(log) = &unit.log {
        lines.extend([key("StandardErrorPath"), format!("  {}", string(&log.display().to_string()))]);
    }
    lines.extend(["</dict>".to_string(), "</plist>".to_string()]);
    Ok(lines.join("\n") + "\n")
}
