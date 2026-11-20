//! A unit of systemd: a user unit (`~/.config/systemd/user/NAME.service`)
//! or a unit of the system (`/etc/systemd/system/NAME.service`) that runs
//! as a named user or as an account of the service alone
//! (`DynamicUser=yes`), with its hardening ([`crate::Hardening`]). The base
//! is podssh's `node_install/text.rs` and `node_install/system_text.rs`;
//! `Type=notify`, the watchdog and the drain at a stop are podrunner's
//! `print-unit`.
//!
//! Each user may read a unit, so no secret is in it: a secret of a unit of
//! the system is a credential of systemd (`LoadCredential=NAME:PATH`, which
//! the program reads at `$CREDENTIALS_DIRECTORY/NAME`); a secret of a user
//! unit is the path of its private file.

use pod_core::Failure;

use crate::hardening::LINES;
use crate::model::{Scope, Start, Unit, usage};
use crate::quote::{list_path_ok, systemd_env, systemd_env_spec, systemd_word};

/// The name of the file of the unit: `NAME.service`.
pub fn file_name(unit: &Unit) -> String {
    format!("{}.service", unit.name)
}

/// The text of the unit, after the checks of [`Unit::check`].
pub fn render(unit: &Unit) -> Result<String, Failure> {
    unit.check()?;
    let system = matches!(unit.scope, Scope::System { .. });
    let mut out = vec![
        format!("# Written by {}.", unit.mark),
        "# No secret is in this file, which each user may read.".to_string(),
        "[Unit]".to_string(),
        format!("Description={}", unit.description.replace('%', "%%")),
    ];
    if unit.network {
        out.extend(["Wants=network-online.target", "After=network-online.target"].map(String::from));
    }
    if unit.restart.always {
        out.push("# No limit of starts: a service that stays down locks out what only it reaches.".to_string());
        out.push("StartLimitIntervalSec=0".to_string());
    }
    out.extend([String::new(), "[Service]".to_string()]);
    match unit.start {
        Start::Exec => out.push("Type=exec".to_string()),
        Start::Notify { watchdog_s } => {
            out.push("# The program says READY, and STOPPING when a stop starts (sd_notify).".to_string());
            out.extend(["Type=notify", "NotifyAccess=main"].map(String::from));
            if let Some(w) = watchdog_s {
                out.push(format!("WatchdogSec={w}"));
            }
        }
    }
    if let Scope::System { run_as } = &unit.scope {
        out.push(match run_as {
            Some(user) => format!("User={user}"),
            None => "DynamicUser=yes".to_string(),
        });
    }
    if let Some(file) = &unit.env_file {
        out.push("# The variables of the program, in a private file of the host.".to_string());
        out.push(format!("EnvironmentFile={}", systemd_word(&file.display().to_string())));
    }
    for env in &unit.env {
        if env.placeholder {
            out.push(format!("# {}: a placeholder, which the broker swaps for its secret.", env.key));
        }
        out.push(format!("Environment={}", systemd_env(&env.key, &env.value)));
    }
    for secret in &unit.secrets {
        let path = secret.path.display().to_string();
        if system {
            if !list_path_ok(&secret.path) {
                return Err(path_refusal("the file of a secret", &secret.path));
            }
            out.push(format!("# The secret {}: systemd copies its file for the program alone.", secret.name));
            out.push(format!("LoadCredential={}:{path}", secret.name));
            out.push(format!("Environment={}", systemd_env_spec(&secret.variable, &format!("%d/{}", secret.name))));
        } else {
            out.push(format!("# The secret {}: the path of its private file.", secret.name));
            out.push(format!("Environment={}", systemd_env(&secret.variable, &path)));
        }
    }
    if let Some(state) = &unit.state_dir {
        out.push("# The state of the program, private to its account.".to_string());
        out.extend([format!("StateDirectory={state}"), "StateDirectoryMode=0700".to_string()]);
    }
    if let Some(dir) = &unit.dir {
        // `-`: a directory that went away is no reason to stay down.
        out.push(format!("WorkingDirectory=-{}", dir.display().to_string().replace('%', "%%")));
    }
    let words: Vec<String> = std::iter::once(unit.program.display().to_string())
        .chain(unit.args.iter().cloned())
        .map(|w| systemd_word(&w))
        .collect();
    out.push(format!("ExecStart={}", words.join(" ")));
    restart_lines(unit, &mut out);
    if system {
        for (key, path) in unit.hardening.paths() {
            if !list_path_ok(path) {
                return Err(path_refusal(key, path));
            }
        }
    }
    out.extend(unit.hardening.lines(system));
    let target = if system { "multi-user.target" } else { "default.target" };
    out.extend([String::new(), "[Install]".to_string(), format!("WantedBy={target}")]);
    Ok(out.join("\n") + "\n")
}

/// The restart and the stop.
fn restart_lines(unit: &Unit, out: &mut Vec<String>) {
    let r = &unit.restart;
    out.push(format!("Restart={}", if r.always { "always" } else { "on-failure" }));
    out.push(format!("RestartSec={}", r.delay_s));
    if !r.prevent.is_empty() {
        out.push("# 64 and 78: a usage or a configuration, or a revoked credential: no start helps.".to_string());
        let codes: Vec<String> = r.prevent.iter().map(u8::to_string).collect();
        out.push(format!("RestartPreventExitStatus={}", codes.join(" ")));
    }
    if unit.stop.drain {
        out.push("# The stop signal goes to the program alone, which drains its work first.".to_string());
        out.push("KillMode=mixed".to_string());
    }
    out.push(match unit.stop.timeout_s {
        Some(s) => format!("TimeoutStopSec={s}"),
        None => "TimeoutStopSec=infinity".to_string(),
    });
}

fn path_refusal(what: &str, path: &std::path::Path) -> Failure {
    usage(format!(
        "{what} {}: a path of a list of systemd is absolute and holds letters, digits and /._-+@ alone",
        path.display()
    ))
}

/// The lines of the hardening that are neither in `text` nor relaxed in a
/// comment of it: a program checks its own unit, also one that a person
/// changed. `system`: the lines of a unit of the system; else the lines of
/// seccomp alone.
pub fn audit(text: &str, system: bool) -> Vec<String> {
    let mut missing = Vec::new();
    for line in LINES.iter().filter(|l| system || l.need == crate::hardening::Need::Seccomp) {
        let key = format!("{}=", line.key);
        let at = |l: &str| {
            let l = l.trim();
            l.starts_with(&key) || (l.starts_with("# Relaxed, as ") && l.contains(&format!(": {key}")))
        };
        let holds = text.lines().any(|l| {
            at(l) && {
                let value = l.trim().rsplit_once(&key).map_or("", |(_, v)| v);
                value.starts_with(line.value)
            }
        });
        if !holds {
            missing.push(format!("{}{}", key, line.value));
        }
    }
    missing
}
