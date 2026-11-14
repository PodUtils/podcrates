//! The failures and the warnings of a workflow file. Each one names its
//! path in the file (`jobs.build.steps[2]`), its line and its cause.

use std::fmt;

use crate::yaml::Mark;

/// A failure, at a path of the file, with the line where it is known.
#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    /// The path in the file (`jobs.build.strategy`), or empty for the file.
    pub path: String,
    pub mark: Option<Mark>,
    pub kind: ErrorKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ErrorKind {
    /// The YAML parser refused the text.
    Yaml(String),
    /// The file passed a limit: `bytes`, `nodes` or `depth`.
    Limit {
        what: &'static str,
        max: usize,
    },
    /// The file has no document.
    Empty,
    /// The file has more than one document.
    Documents,
    DuplicateKey(String),
    /// A key that is a sequence or a mapping.
    KeyNotScalar,
    /// A value of another kind than the one named.
    Type(&'static str),
    /// A necessary key is not there.
    Missing(&'static str),
    /// A job id or a step id that is not a legal name.
    BadId(String),
    DuplicateStepId(String),
    /// A step with both `run` and `uses`, or with neither.
    StepKind,
    /// A job that calls a reusable workflow (`uses`) and has its own
    /// `steps` or `runs-on`.
    Reusable(&'static str),
    /// A need that is not a job of the file.
    UnknownNeed(String),
    /// The needs form a cycle: the jobs in their order, the first again at
    /// the end.
    Cycle(Vec<String>),
    /// An expression that does not parse, or that fails.
    Expression(pod_actions_expr::Error),
    /// A matrix that cannot give its cells.
    Matrix(String),
    /// An evaluated value of another kind than the one necessary.
    Value(String),
    /// A call of the expansion in a wrong order.
    Usage(String),
    /// An event of `on:` with a filter and its `-ignore` form: `branches`,
    /// `tags` or `paths`.
    FilterWithIgnore(&'static str),
    /// A filter of `on:` with no pattern that has no `!`.
    OnlyNegated(&'static str),
    /// A pattern of a filter of `on:` that does not parse, and why.
    BadPattern {
        pattern: String,
        why: &'static str,
    },
    /// The source of a called workflow gave no text.
    CallText {
        uses: String,
        why: String,
    },
    /// An error of a called workflow: of its file, or of the expansion of
    /// one of its jobs.
    InCalled {
        uses: String,
        error: Box<Error>,
    },
    /// A call passes a limit of GitHub: the levels of workflows, or the
    /// count of different called workflows.
    CallLimit {
        what: &'static str,
        max: usize,
    },
    /// A value of `with:` that the called workflow does not take: not an
    /// input, of another type, or a required input not given.
    Input {
        name: String,
        why: String,
    },
    /// A secret of `secrets:` that the called workflow does not declare, or
    /// a required secret not given.
    Secret {
        name: String,
        why: String,
    },
}

impl Error {
    pub(crate) fn new(path: impl Into<String>, mark: Option<Mark>, kind: ErrorKind) -> Error {
        Error { path: path.into(), mark, kind }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorKind::Yaml(why) => write!(f, "YAML: {why}"),
            ErrorKind::Limit { what, max } => write!(f, "the file passes the limit of {max} {what}"),
            ErrorKind::Empty => f.write_str("the file has no document"),
            ErrorKind::Documents => f.write_str("the file has more than one document"),
            ErrorKind::DuplicateKey(key) => write!(f, "the key `{key}` is there two times"),
            ErrorKind::KeyNotScalar => f.write_str("a key must be a scalar"),
            ErrorKind::Type(what) => write!(f, "the value must be {what}"),
            ErrorKind::Missing(key) => write!(f, "the key `{key}` is necessary"),
            ErrorKind::BadId(id) => {
                write!(f, "`{id}` is not a legal id: a letter or `_`, then letters, digits, `-` and `_`")
            }
            ErrorKind::DuplicateStepId(id) => write!(f, "the step id `{id}` is there two times"),
            ErrorKind::StepKind => f.write_str("a step has one of `run` and `uses`"),
            ErrorKind::Reusable(key) => write!(f, "a job that calls a reusable workflow has no `{key}`"),
            ErrorKind::UnknownNeed(need) => write!(f, "the need `{need}` is not a job of the file"),
            ErrorKind::Cycle(jobs) => write!(f, "the needs form a cycle: {}", jobs.join(" -> ")),
            ErrorKind::Expression(e) => write!(f, "{e}"),
            ErrorKind::Matrix(why) => write!(f, "matrix: {why}"),
            ErrorKind::Value(why) => f.write_str(why),
            ErrorKind::Usage(why) => f.write_str(why),
            ErrorKind::FilterWithIgnore(key) => write!(f, "an event has `{key}` or `{key}-ignore`, not both"),
            ErrorKind::OnlyNegated(key) => {
                write!(f, "`{key}` needs a pattern with no `!`; use `{key}-ignore` to exclude")
            }
            ErrorKind::BadPattern { pattern, why } => write!(f, "the pattern `{pattern}`: {why}"),
            ErrorKind::CallText { uses, why } => write!(f, "the called workflow `{uses}`: {why}"),
            ErrorKind::InCalled { uses, error } => write!(f, "in the called workflow `{uses}`: {error}"),
            ErrorKind::CallLimit { what, max } => write!(f, "a run has at most {max} {what}"),
            ErrorKind::Input { name, why } if name.is_empty() => f.write_str(why),
            ErrorKind::Input { name, why } => write!(f, "the input `{name}` {why}"),
            ErrorKind::Secret { name, why } => write!(f, "the secret `{name}` {why}"),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.path.is_empty() {
            write!(f, "{}", self.path)?;
        }
        if let Some(mark) = self.mark {
            let gap = if self.path.is_empty() { "" } else { " " };
            write!(f, "{gap}({mark})")?;
        }
        let gap = if self.path.is_empty() && self.mark.is_none() { "" } else { ": " };
        write!(f, "{gap}{}", self.kind)
    }
}

impl std::error::Error for Error {}

/// A key that the model does not know. Its value stays in the model as raw
/// data (the `extra` list of its place).
#[derive(Clone, Debug, PartialEq)]
pub struct Warning {
    /// The path of the mapping that holds the key.
    pub path: String,
    pub key: String,
    pub mark: Mark,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let place = if self.path.is_empty() { "the top level" } else { &self.path };
        write!(f, "{place} ({}): unknown key `{}`, kept as data", self.mark, self.key)
    }
}
