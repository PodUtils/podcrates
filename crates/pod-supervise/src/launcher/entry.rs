//! Why the entry of the caller did not start the payload, as the ready pipe
//! carries it to [`super::start`]: the text, and the exit code that a run
//! in the foreground gives for the same failure (127 for a program that is
//! not there, 126 for one that cannot run), as a value, not as words of the
//! text.

use std::fmt;

use pod_core::{Class, Failure, Stage};

/// The failure of the entry. A `String` or a `&str` converts to one with no
/// code, so an entry that knows no code returns its text as before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryFailure {
    /// What failed, for a person.
    pub text: String,
    /// The exit code of the failed spawn (docker gives 127 and 126), when
    /// the entry knows one.
    pub code: Option<i32>,
}

/// The prefix of a failure with no code on the ready pipe.
const ERR: &str = "err ";
/// The prefix of a failure with a code: `exit CODE TEXT`.
const EXIT: &str = "exit ";

impl EntryFailure {
    /// A failure with the exit code `code`.
    pub fn coded(code: i32, text: impl Into<String>) -> EntryFailure {
        EntryFailure { text: text.into(), code: Some(code) }
    }

    /// The failure of the entry in the chain of `f`, when the start failed
    /// in the entry.
    pub fn of(f: &Failure) -> Option<&EntryFailure> {
        let mut next: Option<&(dyn std::error::Error + 'static)> = Some(f);
        while let Some(e) = next {
            if let Some(r) = e.downcast_ref::<EntryFailure>() {
                return Some(r);
            }
            next = e.source();
        }
        None
    }

    /// The message of the launcher on the ready pipe.
    pub(super) fn line(&self) -> String {
        match self.code {
            Some(code) => format!("{EXIT}{code} {}", self.text),
            None => format!("{ERR}{}", self.text),
        }
    }

    /// The message of the launcher, back as a failure; `None` for a message
    /// that is not a failure. A code that does not parse stays in the text.
    pub(super) fn parse(line: &str) -> Option<EntryFailure> {
        if let Some(text) = line.strip_prefix(ERR) {
            return Some(text.into());
        }
        let rest = line.strip_prefix(EXIT)?;
        let (code, text) = rest.split_once(' ').unwrap_or((rest, ""));
        match code.parse::<i32>() {
            Ok(code) => Some(EntryFailure::coded(code, text)),
            Err(_) => Some(line.into()),
        }
    }

    /// The failure of the start, class `Unavailable`, with this failure as
    /// its cause so that [`EntryFailure::of`] finds the code.
    pub(super) fn failure(self) -> Failure {
        Failure::caused(Stage::Host, Class::Unavailable, self.text.clone(), self)
    }
}

impl From<String> for EntryFailure {
    fn from(text: String) -> EntryFailure {
        EntryFailure { text, code: None }
    }
}

impl From<&str> for EntryFailure {
    fn from(text: &str) -> EntryFailure {
        text.to_string().into()
    }
}

impl fmt::Display for EntryFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl std::error::Error for EntryFailure {}

#[cfg(test)]
mod tests {
    use super::*;

    /// The line of the ready pipe gives the same failure back, with its code
    /// as a value and no word of the code in the text.
    #[test]
    fn an_entry_failure_crosses_the_ready_pipe_with_its_code() {
        let rows: [(EntryFailure, &str); 4] = [
            (EntryFailure::coded(127, "exec /nonexistent: not found"), "exit 127 exec /nonexistent: not found"),
            (EntryFailure::coded(126, "exec /etc: permission denied"), "exit 126 exec /etc: permission denied"),
            ("the root could not be opened".into(), "err the root could not be opened"),
            ("exit 127 a text that looks like a code".into(), "err exit 127 a text that looks like a code"),
        ];
        for (failure, line) in rows {
            assert_eq!(failure.line(), line);
            let back = EntryFailure::parse(line).expect("a failure");
            assert_eq!(back, failure, "{line}");
            let f = back.failure();
            assert_eq!(f.class, Class::Unavailable);
            assert_eq!(f.text(), failure.text);
            assert_eq!(f.chain(), failure.text, "the chain says the text once");
            assert_eq!(EntryFailure::of(&f).and_then(|e| e.code), failure.code);
        }
        assert_eq!(EntryFailure::parse("ok 1 2"), None);
        assert_eq!(EntryFailure::parse("exit x y"), Some("exit x y".into()));
    }
}
