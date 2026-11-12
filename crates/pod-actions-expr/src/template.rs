//! Text with `${{ }}` parts, and the condition of an `if:`, as the
//! runner's template reader makes them: an expression ends at the first
//! `}}` outside a string literal; a text that is one expression and nothing
//! else gives the value of the expression; each other text is a `format`
//! of its parts and gives a string.

use crate::error::{Error, ErrorKind};
use crate::eval::Contexts;
use crate::expr::Expr;
use crate::value::Value;

/// The start of an expression in a template.
pub const OPEN: &str = "${{";

/// Whether `text` has an expression part.
pub fn has_expression(text: &str) -> bool {
    text.contains(OPEN)
}

#[derive(Clone, Debug)]
enum Part {
    Text(String),
    /// An expression, and the index of its first character in the text.
    Expr(Expr, usize),
}

/// One part of a [`Template`], in the order of the text: what a caller
/// needs to write the template in another form (the runner's `format`
/// token; PodUtils/podrunner@T-089).
#[derive(Clone, Copy, Debug)]
pub enum TemplatePart<'a> {
    /// A literal text.
    Text(&'a str),
    /// An expression ([`Expr::text`] gives its text), and the index of its
    /// first character in the text of the template.
    Expr(&'a Expr, usize),
}

/// A text with `${{ }}` parts.
#[derive(Clone, Debug)]
pub struct Template {
    parts: Vec<Part>,
}

impl Template {
    /// Read `text`. A `${{` with no `}}`, and an expression that does not
    /// parse, is an error at its place in `text`.
    pub fn parse(text: &str) -> Result<Template, Error> {
        let chars: Vec<char> = text.chars().collect();
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut i = 0;
        while i < chars.len() {
            if !(chars[i] == '$' && chars.get(i + 1) == Some(&'{') && chars.get(i + 2) == Some(&'{')) {
                literal.push(chars[i]);
                i += 1;
                continue;
            }
            let start = i + 3;
            let mut quoted = false;
            let mut end = None;
            let mut j = start;
            while j < chars.len() {
                if chars[j] == '\'' {
                    // `''` turns the state two times: an escaped quote.
                    quoted = !quoted;
                } else if !quoted && chars[j] == '}' && chars[j - 1] == '}' && j > start {
                    end = Some(j - 1);
                    break;
                }
                j += 1;
            }
            let Some(end) = end else {
                return Err(Error::at(ErrorKind::UnclosedTemplate, i));
            };
            if !literal.is_empty() {
                parts.push(Part::Text(std::mem::take(&mut literal)));
            }
            let inner: String = chars[start..end].iter().collect();
            let expr = Expr::parse(&inner).map_err(|e| e.shift(start))?;
            parts.push(Part::Expr(expr, start));
            i = end + 2;
        }
        if !literal.is_empty() {
            parts.push(Part::Text(literal));
        }
        Ok(Template { parts })
    }

    /// No expression part: the text is a literal.
    pub fn is_literal(&self) -> bool {
        self.parts.iter().all(|p| matches!(p, Part::Text(_)))
    }

    /// The one expression of a text that is one expression and nothing
    /// else.
    pub fn as_expression(&self) -> Option<&Expr> {
        match self.parts.as_slice() {
            [Part::Expr(e, _)] => Some(e),
            _ => None,
        }
    }

    /// Each part, the literal texts and the expressions, in the order of
    /// the text. Two literal texts are never next to each other.
    pub fn parts(&self) -> impl Iterator<Item = TemplatePart<'_>> {
        self.parts.iter().map(|p| match p {
            Part::Text(t) => TemplatePart::Text(t),
            Part::Expr(e, at) => TemplatePart::Expr(e, *at),
        })
    }

    /// Each expression part.
    pub fn expressions(&self) -> impl Iterator<Item = &Expr> {
        self.parts.iter().filter_map(|p| match p {
            Part::Expr(e, _) => Some(e),
            Part::Text(_) => None,
        })
    }

    /// An error for the first named value that `contexts` does not give,
    /// at its place in the text.
    pub fn check_contexts(&self, contexts: &Contexts) -> Result<(), Error> {
        for part in &self.parts {
            if let Part::Expr(e, at) = part {
                e.check_contexts(contexts).map_err(|e| e.shift(*at))?;
            }
        }
        Ok(())
    }

    /// The value: the value of the one expression where the text is one
    /// expression and nothing else; else the text, with the text of each
    /// expression in its place. Each expression must name only contexts
    /// that the caller gives.
    pub fn evaluate(&self, contexts: &Contexts) -> Result<Value, Error> {
        if let [Part::Expr(expr, at)] = self.parts.as_slice() {
            return expr.evaluate(contexts).map_err(|e| e.shift(*at));
        }
        self.check_contexts(contexts)?;
        let mut out = String::new();
        for part in &self.parts {
            match part {
                Part::Text(t) => out.push_str(t),
                Part::Expr(e, at) => out.push_str(&e.evaluate(contexts).map_err(|e| e.shift(*at))?.to_text()),
            }
        }
        Ok(Value::String(out))
    }
}

/// The condition of an `if:`. A text with no `${{` is an expression; a text
/// with `${{` is a template (a text around an expression gives a string,
/// which is true when it is not empty). An empty condition is
/// `success()`. A condition with no status function (`success`,
/// `failure`, `always`, `cancelled`) is `success() && (condition)`.
#[derive(Clone, Debug)]
pub struct Condition {
    body: Option<Body>,
    implicit: bool,
    text: String,
}

#[derive(Clone, Debug)]
enum Body {
    Expr(Expr),
    Template(Template),
}

impl Condition {
    pub fn parse(text: &str) -> Result<Condition, Error> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(Condition { body: None, implicit: true, text: "success()".into() });
        }
        let (body, source) = if has_expression(trimmed) {
            let template = Template::parse(trimmed)?;
            match template.as_expression().and_then(Expr::as_string) {
                // `${{ 'text' }}` is the literal `text`, and the condition
                // is that text, as in the runner.
                Some(literal) => (Body::Expr(Expr::parse(literal)?), literal.to_string()),
                None => match template.as_expression() {
                    Some(expr) => (Body::Expr(expr.clone()), expr.text().trim().to_string()),
                    None => (Body::Template(template), trimmed.to_string()),
                },
            }
        } else {
            (Body::Expr(Expr::parse(trimmed)?), trimmed.to_string())
        };
        let status = match &body {
            Body::Expr(e) => e.has_status_function(),
            Body::Template(t) => t.expressions().any(Expr::has_status_function),
        };
        let text = if status { source } else { format!("success() && ({source})") };
        Ok(Condition { body: Some(body), implicit: !status, text })
    }

    /// The condition that the runner evaluates, with its implicit
    /// `success() &&`.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The condition has no status function, so `success()` holds it.
    pub fn is_implicit(&self) -> bool {
        self.implicit
    }

    /// Each context that the condition names, in its order, once.
    pub fn contexts(&self) -> Vec<String> {
        match &self.body {
            None => Vec::new(),
            Some(Body::Expr(e)) => e.contexts(),
            Some(Body::Template(t)) => {
                let mut out: Vec<String> = Vec::new();
                for name in t.expressions().flat_map(Expr::contexts) {
                    if !out.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                        out.push(name);
                    }
                }
                out
            }
        }
    }

    /// The truth of the condition. Each context that it names must be in
    /// `contexts`; the body is not evaluated when the implicit `success()`
    /// is false, as `&&` stops there.
    pub fn evaluate(&self, contexts: &Contexts) -> Result<bool, Error> {
        let Some(body) = &self.body else {
            return Ok(contexts.status.success);
        };
        match body {
            Body::Expr(e) => e.check_contexts(contexts)?,
            Body::Template(t) => t.check_contexts(contexts)?,
        }
        if self.implicit && !contexts.status.success {
            return Ok(false);
        }
        let value = match body {
            Body::Expr(e) => e.evaluate(contexts)?,
            Body::Template(t) => t.evaluate(contexts)?,
        };
        Ok(value.is_truthy())
    }
}
