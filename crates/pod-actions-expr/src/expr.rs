//! A parsed expression: its text and its tree.

use crate::error::Error;
use crate::eval::{self, Contexts};
use crate::parse::{self, Node};
use crate::value::{Value, eq_ignore_case};

/// One expression (the text inside `${{ }}`, or the text of an `if:`).
#[derive(Clone, Debug)]
pub struct Expr {
    text: String,
    root: Node,
}

impl Expr {
    /// Parse `text`. A syntax error, an unknown function, a call with a
    /// count of arguments that the function does not take, an expression
    /// longer than [`crate::MAX_LENGTH`] characters or deeper than
    /// [`crate::MAX_DEPTH`] is an error. A named value is not checked here:
    /// [`Expr::evaluate`] checks it against the caller's contexts.
    pub fn parse(text: &str) -> Result<Expr, Error> {
        Ok(Expr { text: text.to_string(), root: parse::parse(text)? })
    }

    /// The text of the expression, as given.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The value of the expression. Each named value of the expression must
    /// be a context in `contexts`, also in a branch that the evaluation does
    /// not reach.
    pub fn evaluate(&self, contexts: &Contexts) -> Result<Value, Error> {
        eval::evaluate(&self.root, contexts)
    }

    /// An error for the first named value that `contexts` does not give.
    pub fn check_contexts(&self, contexts: &Contexts) -> Result<(), Error> {
        eval::check_contexts(&self.root, contexts)
    }

    /// Each named value (context) that the expression reads, in its order,
    /// once (with no regard to case).
    pub fn contexts(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        self.root.walk(&mut |n| {
            if let Node::Context(name, _) = n
                && !out.iter().any(|o| eq_ignore_case(o, name))
            {
                out.push(name.clone());
            }
        });
        out
    }

    /// Each function that the expression calls, by its name in the
    /// documents of GitHub (`fromJSON`), in its order, once.
    pub fn functions(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        self.root.walk(&mut |n| {
            if let Node::Call(f, ..) = n
                && !out.contains(&f.name())
            {
                out.push(f.name());
            }
        });
        out
    }

    /// The expression calls `success`, `failure`, `always` or `cancelled`.
    pub fn has_status_function(&self) -> bool {
        let mut found = false;
        self.root.walk(&mut |n| found |= matches!(n, Node::Call(f, ..) if f.is_status()));
        found
    }

    /// The text of an expression that is one string literal.
    pub fn as_string(&self) -> Option<&str> {
        match &self.root {
            Node::Literal(Value::String(s)) => Some(s),
            _ => None,
        }
    }
}
