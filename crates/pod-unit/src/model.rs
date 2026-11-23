//! The unit: one long-running program as a service, in the words that each
//! form of service shares. A renderer turns it into one form (a unit of
//! systemd, a plist of launchd, a task or a service of Windows, the lines of
//! a crontab). No value of a unit is a secret: a secret is a reference that
//! the host resolves when the service starts ([`Secret`]), or a placeholder
//! that a broker swaps ([`Env::placeholder`]).

use std::path::{Path, PathBuf};

use pod_core::{Class, Failure, Stage};

use crate::hardening::Hardening;

/// Who runs the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// The service of the user who installs it: a user unit of systemd, a
    /// LaunchAgent, a task at the logon of the user, a line of the crontab.
    /// No privilege is necessary.
    User,
    /// A service of the system, which the administrator installs. `run_as`
    /// names the account. None: an account that the manager makes for the
    /// service alone (`DynamicUser=yes`; `NT SERVICE\NAME` on Windows).
    System { run_as: Option<String> },
}

/// How the manager knows that the service is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// Up when the program started (`Type=exec`).
    Exec,
    /// Up when the program says READY with `sd_notify` (`Type=notify`). A
    /// watchdog in seconds makes the manager restart a program that does not
    /// say WATCHDOG within it.
    Notify { watchdog_s: Option<u32> },
}

/// When the manager starts the program again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restart {
    /// True: after each end. False: after a failure alone.
    pub always: bool,
    /// The wait before a start again, in seconds.
    pub delay_s: u32,
    /// The exit codes after which no start helps: 64 (usage) and 78 (a
    /// configuration, or a credential that is revoked) of
    /// [`pod_core::Class::exit_code`].
    pub prevent: Vec<u8>,
}

impl Default for Restart {
    fn default() -> Restart {
        Restart { always: true, delay_s: 5, prevent: vec![Class::Usage.exit_code(), Class::Config.exit_code()] }
    }
}

/// How the manager stops the program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stop {
    /// True: the stop signal goes to the main process alone, which drains its
    /// work; the rest of the group is killed at the end (`KillMode=mixed`).
    pub drain: bool,
    /// The time for a stop in seconds; None: no limit.
    pub timeout_s: Option<u32>,
}

impl Default for Stop {
    fn default() -> Stop {
        Stop { drain: false, timeout_s: Some(90) }
    }
}

/// One variable of the environment of the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Env {
    pub(crate) key: String,
    pub(crate) value: String,
    pub(crate) placeholder: bool,
}

impl Env {
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    /// True for a placeholder that a broker swaps for its secret.
    pub fn is_placeholder(&self) -> bool {
        self.placeholder
    }

    /// A placeholder credential (`pod-broker`): the variable that names a
    /// secret holds a word that the broker swaps for the real value on the
    /// way to the hosts of the credential. The word has 16 to 256 of
    /// `A-Z a-z 0-9 _ . -`, as the broker reads a placeholder, and must not
    /// look like a real secret.
    pub fn placeholder(key: &str, value: &str) -> Result<Env, Failure> {
        let fine = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-');
        if !(16..=256).contains(&value.len()) || !value.bytes().all(fine) {
            return Err(usage(format!("the placeholder of {key}: give 16 to 256 of A-Z a-z 0-9 _ . -")));
        }
        Ok(Env { key: key.to_string(), value: value.to_string(), placeholder: true })
    }
}

/// A secret that the service reads from a file at its start, never from the
/// unit. The host keeps the file private (mode 0600, of root or of the
/// account of the service). The program finds the path in `variable`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Secret {
    /// The name of the credential: 1 to 64 of `A-Z a-z 0-9 _ . -`.
    pub name: String,
    /// The private file that holds the value.
    pub path: PathBuf,
    /// The variable that gives the program the path of the value
    /// (`GITHUB_TOKEN_FILE`).
    pub variable: String,
}

/// One long-running program as a service. Make it with [`Unit::new`], then
/// set its fields.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Unit {
    /// The name of the service: 1 to 64 of `A-Z a-z 0-9 _ . - @`. The file of
    /// a unit of systemd is `NAME.service`; a task and a service of Windows
    /// have this name.
    pub name: String,
    /// The label of launchd (`dev.podutils.podrunner`); None: the name.
    pub label: Option<String>,
    /// One line that tells what the service does.
    pub description: String,
    /// The first comment of each form: what wrote it. A removal removes only
    /// a file that holds this mark, or one of [`Unit::older_marks`].
    pub mark: String,
    /// The marks of an older version of the program, which an install and a
    /// removal take as the program's own: an upgrade replaces and removes
    /// what the older version wrote. Each is a text as the older version
    /// wrote it, comment signs too, and no render writes it, so it may hold
    /// `--`. It marks the first line of a unit of systemd that starts with
    /// it; a plist, the XML of a task or the description of a service that
    /// holds it; and a line of the crontab that is it.
    pub older_marks: Vec<String>,
    /// The program, by its absolute path.
    pub program: PathBuf,
    pub args: Vec<String>,
    /// The directory of the run; None: the default of the manager.
    pub dir: Option<PathBuf>,
    pub env: Vec<Env>,
    pub secrets: Vec<Secret>,
    /// A private file of variables that systemd reads before the start
    /// (`EnvironmentFile=`). Only systemd has one: the other forms refuse a
    /// unit with one.
    pub env_file: Option<PathBuf>,
    /// The state directory that systemd makes for the program, private to its
    /// account (`StateDirectory=NAME` under `/var/lib`, or under
    /// `~/.local/state` for a user unit): a unit with `DynamicUser=yes`
    /// writes only there. Only systemd has one: the other forms refuse a unit
    /// with one, and their program takes its directory in its arguments.
    pub state_dir: Option<String>,
    /// The file for the lines of stderr where the manager keeps no journal
    /// (launchd).
    pub log: Option<PathBuf>,
    pub scope: Scope,
    pub start: Start,
    pub restart: Restart,
    pub stop: Stop,
    /// True: the start waits for the network (`network-online.target`).
    pub network: bool,
    pub hardening: Hardening,
}

impl Unit {
    /// A unit of the user that runs `program` with no argument, restarts
    /// after each end but after 64 and 78, and has the hardening of its
    /// scope.
    pub fn new(name: &str, program: &Path, mark: &str) -> Unit {
        Unit {
            name: name.to_string(),
            label: None,
            description: name.to_string(),
            mark: mark.to_string(),
            older_marks: Vec::new(),
            program: program.to_path_buf(),
            args: Vec::new(),
            dir: None,
            env: Vec::new(),
            secrets: Vec::new(),
            env_file: None,
            state_dir: None,
            log: None,
            scope: Scope::User,
            start: Start::Exec,
            restart: Restart::default(),
            stop: Stop::default(),
            network: true,
            hardening: Hardening::default(),
        }
    }

    /// The unit as a service of the system that runs as `run_as` (None: an
    /// account of the service alone).
    pub fn system(mut self, run_as: Option<&str>) -> Unit {
        self.scope = Scope::System { run_as: run_as.map(str::to_string) };
        self
    }

    /// A mark of an older version of the program ([`Unit::older_marks`]).
    pub fn older_mark(mut self, text: &str) -> Unit {
        self.older_marks.push(text.to_string());
        self
    }

    pub fn arg(mut self, word: &str) -> Unit {
        self.args.push(word.to_string());
        self
    }

    pub fn args<I: IntoIterator<Item = S>, S: AsRef<str>>(mut self, words: I) -> Unit {
        self.args.extend(words.into_iter().map(|w| w.as_ref().to_string()));
        self
    }

    /// A variable with a plain value. A value that looks like a secret, or a
    /// key that names one, is refused when the unit is rendered.
    pub fn env(mut self, key: &str, value: &str) -> Unit {
        self.env.push(Env { key: key.to_string(), value: value.to_string(), placeholder: false });
        self
    }

    /// A placeholder credential ([`Env::placeholder`]).
    pub fn placeholder(mut self, key: &str, value: &str) -> Result<Unit, Failure> {
        self.env.push(Env::placeholder(key, value)?);
        Ok(self)
    }

    /// A secret in the private file `path`, whose path the program reads in
    /// `variable`.
    pub fn secret_file(mut self, name: &str, path: &Path, variable: &str) -> Unit {
        self.secrets.push(Secret { name: name.to_string(), path: path.to_path_buf(), variable: variable.to_string() });
        self
    }

    /// The label of launchd.
    pub fn launchd_label(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.name)
    }

    /// The checks that each form shares: the names, the paths, no line
    /// break, and no secret ([`crate::secret::check`]).
    pub fn check(&self) -> Result<(), Failure> {
        name_ok("the name", &self.name, b"_.-@")?;
        if let Some(label) = &self.label {
            name_ok("the label", label, b"_.-")?;
        }
        if !absolute(&self.program) {
            return Err(usage(format!("the program {}: give an absolute path", self.program.display())));
        }
        if self.mark.is_empty() || self.mark.contains("--") {
            return Err(usage("the mark: give words with no \"--\", which no comment of XML may hold"));
        }
        for older in &self.older_marks {
            if older.trim().chars().count() < OLDER_MARK_LEAST || older.chars().any(char::is_control) {
                return Err(usage(format!(
                    "the older mark {older:?}: give one line of {OLDER_MARK_LEAST} characters or more, which only \
                     the files of the program hold"
                )));
            }
        }
        for secret in &self.secrets {
            name_ok("the name of a secret", &secret.name, b"_.-")?;
            key_ok(&secret.variable)?;
            if !absolute(&secret.path) {
                return Err(usage(format!("the secret {}: give the absolute path of its file", secret.name)));
            }
        }
        for env in &self.env {
            key_ok(&env.key)?;
        }
        if let Some(state) = &self.state_dir {
            name_ok("the state directory", state, b"_.-")?;
        }
        for (what, word) in self.words() {
            if word.chars().any(char::is_control) {
                return Err(usage(format!("{what}: a line break or a control character, which no unit can hold")));
            }
        }
        crate::secret::check(self)
    }

    /// True when `holds` finds a mark of an older version of the program
    /// ([`Unit::older_marks`]).
    pub(crate) fn older_mark_in(&self, holds: impl Fn(&str) -> bool) -> bool {
        self.older_marks.iter().any(|older| holds(older))
    }

    /// Each word of the unit, with its place: the scan of secrets and the
    /// check of control characters read the same list.
    pub(crate) fn words(&self) -> Vec<(String, String)> {
        let path = |p: &Path| p.display().to_string();
        let mut words = vec![
            ("the description".to_string(), self.description.clone()),
            ("the mark".to_string(), self.mark.clone()),
            ("the program".to_string(), path(&self.program)),
        ];
        words.extend(self.args.iter().enumerate().map(|(n, a)| (format!("argument {}", n + 1), a.clone())));
        words.extend(self.dir.iter().map(|d| ("the directory".to_string(), path(d))));
        words.extend(self.env.iter().map(|e| (format!("the variable {}", e.key), e.value.clone())));
        words.extend(self.secrets.iter().map(|s| (format!("the file of the secret {}", s.name), path(&s.path))));
        words.extend(self.env_file.iter().map(|f| ("the file of variables".to_string(), path(f))));
        words.extend(self.log.iter().map(|f| ("the log".to_string(), path(f))));
        if let Scope::System { run_as: Some(user) } = &self.scope {
            words.push(("the account".to_string(), user.clone()));
        }
        words.extend(self.hardening.paths().map(|(key, p)| (key.to_string(), path(p))));
        words.extend(self.hardening.added().map(|(key, value)| (format!("the line {key}"), value.to_string())));
        words
    }
}

/// The fewest characters of an older mark: a short text could mark the file
/// of another program.
const OLDER_MARK_LEAST: usize = 8;

/// An absolute path of Unix or of Windows, on each system: a form is
/// rendered on any host (a person prints the unit of another system).
pub(crate) fn absolute(path: &Path) -> bool {
    let text = path.to_string_lossy();
    let b = text.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/');
    text.starts_with('/') || text.starts_with("\\\\") || drive
}

/// A failure of a value that the caller gave.
pub(crate) fn usage(text: impl Into<String>) -> Failure {
    Failure::new(Stage::Host, Class::Usage, text)
}

/// A unit that this form cannot hold.
pub(crate) fn unsupported(text: impl Into<String>) -> Failure {
    Failure::new(Stage::Host, Class::Unsupported, text)
}

fn name_ok(what: &str, name: &str, more: &[u8]) -> Result<(), Failure> {
    let fine = |b: u8| b.is_ascii_alphanumeric() || more.contains(&b);
    if (1..=64).contains(&name.len()) && name.bytes().all(fine) && !name.starts_with(['.', '-']) {
        return Ok(());
    }
    Err(usage(format!("{what} {name:?}: give 1 to 64 of A-Z a-z 0-9 {}", String::from_utf8_lossy(more))))
}

/// A name of a variable: a letter or `_`, then letters, digits and `_`.
fn key_ok(key: &str) -> Result<(), Failure> {
    let mut bytes = key.bytes();
    let first = bytes.next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_');
    if first && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Ok(());
    }
    Err(usage(format!("the variable {key:?}: give a letter or _, then letters, digits and _")))
}
