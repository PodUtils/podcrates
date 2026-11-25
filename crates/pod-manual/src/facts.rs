//! The typed tables of a manual that the command tree does not hold: the
//! variables in groups, the exit codes, the files, the keys of a
//! configuration file, and the tables that a program adds. A program keeps
//! one table of each, and the checks of [`crate::check`] compare them with
//! its code.

use serde::{Deserialize, Serialize};

/// Each table of facts of one program.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Facts {
    pub variables: Vec<VarGroup>,
    pub exits: Vec<Exit>,
    pub files: Vec<FileFact>,
    pub config: Vec<ConfigFile>,
    pub tables: Vec<Table>,
}

/// A group of variables: podvm has about 60 `PODVM_*`, in groups.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarGroup {
    /// The name of the group. Empty: the variables show with no sub-heading.
    pub name: String,
    pub vars: Vec<Var>,
}

/// One variable of the environment.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Var {
    /// The first name is the main name; the others are aliases.
    pub names: Vec<String>,
    pub usage: Use,
    /// The commands that use it. Empty: each command.
    pub used_by: String,
    pub meaning: String,
    pub default: Option<String>,
}

impl Var {
    /// A variable that the program reads.
    pub fn reads(names: &[&str], meaning: &str) -> Var {
        Var { names: names.iter().map(|n| n.to_string()).collect(), meaning: meaning.to_string(), ..Var::default() }
    }

    /// A variable that the program sets for a child.
    pub fn sets(names: &[&str], meaning: &str) -> Var {
        Var { usage: Use::Sets, ..Var::reads(names, meaning) }
    }
}

/// What the program does with a variable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Use {
    #[default]
    Reads,
    /// It sets the variable for a child, and does not read it.
    Sets,
    ReadsAndSets,
}

impl Use {
    pub fn word(self) -> &'static str {
        match self {
            Use::Reads => "read",
            Use::Sets => "set",
            Use::ReadsAndSets => "read and set",
        }
    }
}

/// One exit code, or a set of codes with one meaning.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exit {
    pub code: Code,
    /// A short name: `usage`, `config`.
    pub name: String,
    pub meaning: String,
}

impl Exit {
    pub fn new(code: i32, name: &str, meaning: &str) -> Exit {
        Exit { code: Code::Value(code), name: name.to_string(), meaning: meaning.to_string() }
    }
}

/// The codes of one row of EXIT STATUS.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "form", content = "value", rename_all = "kebab-case")]
pub enum Code {
    Value(i32),
    /// Each code from `from` to `to`, both included.
    Range {
        from: i32,
        to: i32,
    },
    /// 128 and the number of the signal that ended the payload.
    Signal,
    /// The exit code of the payload (a command that the program runs), as
    /// it is: each code that no other row names.
    Payload,
}

impl Code {
    /// The code as the manual shows it: `64`, `64-80`, `128+N`, `N`.
    pub fn words(self) -> String {
        match self {
            Code::Value(value) => value.to_string(),
            Code::Range { from, to } => format!("{from}-{to}"),
            Code::Signal => "128+N".to_string(),
            Code::Payload => "N".to_string(),
        }
    }

    /// Whether a code that the program gives is in this row. A payload row
    /// holds no code alone: [`covers`] gives it each code that no other row
    /// names.
    pub fn holds(self, code: i32) -> bool {
        match self {
            Code::Value(value) => value == code,
            Code::Range { from, to } => (from..=to).contains(&code),
            Code::Signal => (129..=192).contains(&code),
            Code::Payload => false,
        }
    }
}

/// Whether a row of `exits` names `code`; a row of [`Code::Payload`] names
/// each code from 0 to 255.
pub fn covers(exits: &[Exit], code: i32) -> bool {
    exits.iter().any(|e| e.code.holds(code))
        || (exits.iter().any(|e| e.code == Code::Payload) && (0..=255).contains(&code))
}

/// One file or directory that the program reads or writes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFact {
    pub path: String,
    pub meaning: String,
}

/// A configuration file whose keys are options (podrunner's `run.toml`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigFile {
    pub path: String,
    pub about: String,
    pub keys: Vec<ConfigKey>,
}

/// One key of a configuration file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigKey {
    pub key: String,
    /// The value name: `SECONDS`.
    pub value: String,
    pub default: Option<String>,
    /// The flag that gives the same option: `--timeout`.
    pub flag: Option<String>,
    pub meaning: String,
}

/// A table that a program adds: podssh's `-o` keywords, retired names.
/// Each writer shows it as a topic with the key [`Table::key`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Table {
    pub key: String,
    pub heading: String,
    pub about: String,
    pub columns: Vec<String>,
    /// Each row has one cell for each column.
    pub rows: Vec<Vec<String>>,
}

/// The exit codes of the failure classes of `pod_core::failure`: one row
/// for each code, the names of its classes joined.
#[cfg(feature = "core")]
pub fn failure_classes() -> Vec<Exit> {
    use pod_core::failure::Class;
    let meaning = |c: Class| match c {
        Class::Usage => "A value of the command line cannot be used.",
        Class::Config => "A configuration cannot be used.",
        Class::Unavailable => "A service or a path is not available.",
        Class::NotFound => "A path or a name does not exist.",
        Class::TempFail => "A service refused for now. A later try can work.",
        Class::Timeout => "A wait ended at its time limit.",
        Class::Protocol => "The far end broke the protocol.",
        Class::Refused => "The far end refused the credentials or the identity.",
        Class::Revoked => "A credential that worked is gone. A restart does not help.",
        Class::Policy => "A local policy refused the action.",
        Class::Unsupported => "The host, the build or the far end does not support the action.",
        Class::Permission => "A file or a directory has the wrong permissions or the wrong owner.",
        Class::Suite => "The far end and this end have no protocol or suite in common.",
        Class::CantCreate => "An output file or directory cannot be made or written.",
        Class::Internal => "A fault occurred inside the tool, or a session ended before its end.",
        Class::Data => "An input was there, and its content is wrong.",
    };
    let mut out: Vec<Exit> = Vec::new();
    for c in Class::ALL {
        let code = i32::from(c.exit_code());
        match out.iter_mut().find(|e| e.code == Code::Value(code)) {
            Some(e) => {
                e.name = format!("{}, {}", e.name, c.name());
                e.meaning = format!("{} {}", e.meaning, meaning(c));
            }
            None => out.push(Exit::new(code, c.name(), meaning(c))),
        }
    }
    out.sort_by_key(|e| match e.code {
        Code::Value(value) => value,
        _ => i32::MAX,
    });
    out
}
