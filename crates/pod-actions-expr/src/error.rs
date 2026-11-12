//! The failures of a parse and of an evaluation. Each one names its cause:
//! the token, the function, the context, the limit.

use std::fmt;

/// A failure, with the place in the expression where the parser knows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub kind: ErrorKind,
    /// The index of the character (from 0) in the expression or the
    /// template.
    pub at: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ErrorKind {
    /// An expression longer than the limit of GitHub.
    TooLong {
        length: usize,
        max: usize,
    },
    /// A tree deeper than the limit of GitHub.
    TooDeep {
        max: usize,
    },
    /// No expression where one is necessary.
    Empty,
    /// A token where it cannot be.
    Unexpected(String),
    /// The text ended in a string literal.
    UnclosedString,
    /// The text ended before the expression did.
    UnexpectedEnd,
    /// A `${{` with no `}}`.
    UnclosedTemplate,
    UnknownFunction(String),
    /// A function with a count of arguments that it does not take.
    Arity {
        function: String,
        min: usize,
        max: usize,
        given: usize,
    },
    /// A named value that the caller did not give.
    UnknownContext(String),
    /// A function that needs files or other I/O, which this crate does not
    /// do (`hashFiles`).
    NeedsIo(String),
    /// `fromJSON` of a text that is not JSON.
    Json(String),
    /// `format` of a format string that it cannot read.
    Format(String),
    /// The values of one evaluation passed the byte limit.
    TooLarge {
        max: usize,
    },
}

impl Error {
    pub(crate) fn at(kind: ErrorKind, at: usize) -> Error {
        Error { kind, at: Some(at) }
    }

    pub(crate) fn new(kind: ErrorKind) -> Error {
        Error { kind, at: None }
    }

    /// The same failure, at a place `offset` characters later: the place of
    /// an expression inside a template.
    pub(crate) fn shift(mut self, offset: usize) -> Error {
        self.at = self.at.map(|at| at + offset);
        self
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorKind::TooLong { length, max } => {
                write!(f, "the expression has {length} characters; the limit is {max}")
            }
            ErrorKind::TooDeep { max } => write!(f, "the expression is deeper than {max} levels"),
            ErrorKind::Empty => f.write_str("an expression is necessary"),
            ErrorKind::Unexpected(token) => write!(f, "unexpected symbol `{token}`"),
            ErrorKind::UnclosedString => f.write_str("a string literal has no closing quote"),
            ErrorKind::UnexpectedEnd => f.write_str("the expression ends too early"),
            ErrorKind::UnclosedTemplate => f.write_str("a `${{` has no closing `}}`"),
            ErrorKind::UnknownFunction(name) => write!(f, "unknown function `{name}`"),
            ErrorKind::Arity { function, min, max, given } => {
                if min == max {
                    write!(f, "the function `{function}` takes {min} arguments, not {given}")
                } else if *max == usize::MAX {
                    write!(f, "the function `{function}` takes {min} arguments or more, not {given}")
                } else {
                    write!(f, "the function `{function}` takes {min} to {max} arguments, not {given}")
                }
            }
            ErrorKind::UnknownContext(name) => write!(f, "unknown context `{name}`"),
            ErrorKind::NeedsIo(name) => {
                write!(f, "the function `{name}` reads files; this evaluator does no I/O")
            }
            ErrorKind::Json(why) => write!(f, "fromJSON: {why}"),
            ErrorKind::Format(why) => write!(f, "format: {why}"),
            ErrorKind::TooLarge { max } => write!(f, "the values of the expression pass the limit of {max} bytes"),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.at {
            Some(at) => write!(f, "{} (at character {})", self.kind, at + 1),
            None => write!(f, "{}", self.kind),
        }
    }
}

impl std::error::Error for Error {}
