//! The expression language of GitHub Actions (`${{ }}`), with no I/O: a
//! lexer and a parser of the runner's grammar, and an evaluator with the
//! rules of GitHub (the loose equality, the coercion to a number, strings
//! compared with no regard to case, `&&` and `||` that give an operand, the
//! object filter `*`). The functions are `contains`, `startsWith`,
//! `endsWith`, `format`, `join`, `toJSON`, `fromJSON`, `success`,
//! `failure`, `always` and `cancelled`; `hashFiles` parses, and its
//! evaluation fails by name. The caller gives each context (`github`,
//! `env`, `vars`, `inputs`, `matrix`, `needs`, `strategy`, `job`) and the
//! answers of the status functions.
//!
//! [`Template`] reads a text with `${{ }}` parts; [`Condition`] reads an
//! `if:` with its implicit `success() &&`. The crate reads no environment,
//! does no I/O and has no dependency.
//!
//! ```
//! use pod_actions_expr::{Condition, Contexts, Expr, Template, Value};
//!
//! let mut contexts = Contexts::new();
//! contexts.insert("matrix", Value::object([("os", Value::from("ubuntu-latest"))]));
//! let expr = Expr::parse("startsWith(matrix.os, 'UBUNTU')").unwrap();
//! assert_eq!(expr.evaluate(&contexts).unwrap(), Value::Bool(true));
//! let name = Template::parse("build (${{ matrix.os }})").unwrap();
//! assert_eq!(name.evaluate(&contexts).unwrap(), Value::from("build (ubuntu-latest)"));
//! let condition = Condition::parse("matrix.os != 'windows-latest'").unwrap();
//! assert_eq!(condition.text(), "success() && (matrix.os != 'windows-latest')");
//! assert!(condition.evaluate(&contexts).unwrap());
//! ```

mod error;
mod eval;
mod expr;
mod funcs;
mod json;
mod lex;
mod number;
mod parse;
mod template;
mod value;

pub use error::{Error, ErrorKind};
pub use eval::{Contexts, Limits, Status};
pub use expr::Expr;
pub use json::MAX_JSON_DEPTH;
pub use parse::{MAX_DEPTH, MAX_LENGTH};
pub use template::{Condition, OPEN, Template, has_expression};
pub use value::{Object, Value};

/// Parse `text` as one expression and evaluate it.
pub fn evaluate(text: &str, contexts: &Contexts) -> Result<Value, Error> {
    Expr::parse(text)?.evaluate(contexts)
}

/// Whether `name` is a legal name of a context or a property: a letter or
/// `_`, then letters, digits, `_` and `-`. A job id and a step id follow the
/// same rule.
pub fn is_name(name: &str) -> bool {
    lex::is_keyword(name)
}

impl Value {
    /// The text of `toJSON`: two spaces of indent, `"key": value`.
    pub fn to_json(&self) -> String {
        json::write(self, true)
    }

    /// The JSON text on one line, with no space.
    pub fn to_json_line(&self) -> String {
        json::write(self, false)
    }

    /// The value of a JSON text, as `fromJSON` reads it: strict JSON, each
    /// number a double, at most [`MAX_JSON_DEPTH`] levels.
    pub fn from_json(text: &str) -> Result<Value, Error> {
        json::read(text).map_err(|why| Error { kind: ErrorKind::Json(why), at: None })
    }
}
