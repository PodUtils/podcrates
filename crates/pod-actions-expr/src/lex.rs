//! The lexer of an expression, as the runner's `LexicalAnalyzer`: a token
//! ends at white space or at one of `( [ ) ] , . ! > < = & |`; a number
//! starts with a digit, a sign, or a point where an operand can start; a
//! keyword after `.` is a property name.

use crate::error::{Error, ErrorKind};
use crate::number;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Tok {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    /// A named value: a context.
    Name(String),
    /// A keyword after `.`.
    Property(String),
    /// A keyword before `(`.
    Function(String),
    Dot,
    Star,
    Open,
    Close,
    OpenIndex,
    CloseIndex,
    Comma,
    Not,
    Op(Op),
    And,
    Or,
}

/// A comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Debug)]
pub(crate) struct Token {
    pub tok: Tok,
    /// The index of the first character in the expression.
    pub at: usize,
}

fn boundary(c: char) -> bool {
    matches!(c, '(' | '[' | ')' | ']' | ',' | '.' | '!' | '>' | '<' | '=' | '&' | '|') || c.is_whitespace()
}

/// A keyword: a letter or `_`, then letters, digits, `_` and `-`.
pub(crate) fn is_keyword(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub(crate) fn lex(text: &[char]) -> Result<Vec<Token>, Error> {
    let mut out: Vec<Token> = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let at = i;
        let next = text.get(i + 1).copied();
        let (tok, len) = match c {
            '(' => (Tok::Open, 1),
            ')' => (Tok::Close, 1),
            '[' => (Tok::OpenIndex, 1),
            ']' => (Tok::CloseIndex, 1),
            ',' => (Tok::Comma, 1),
            '*' => (Tok::Star, 1),
            '\'' => {
                let (s, len) = string(text, i)?;
                (Tok::String(s), len)
            }
            '!' if next == Some('=') => (Tok::Op(Op::Ne), 2),
            '!' => (Tok::Not, 1),
            '>' if next == Some('=') => (Tok::Op(Op::Ge), 2),
            '>' => (Tok::Op(Op::Gt), 1),
            '<' if next == Some('=') => (Tok::Op(Op::Le), 2),
            '<' => (Tok::Op(Op::Lt), 1),
            '=' if next == Some('=') => (Tok::Op(Op::Eq), 2),
            '&' if next == Some('&') => (Tok::And, 2),
            '|' if next == Some('|') => (Tok::Or, 2),
            '=' | '&' | '|' => return Err(unexpected(c.to_string(), at)),
            '.' if !operand_can_start(out.last()) => (Tok::Dot, 1),
            '.' | '-' | '+' | '0'..='9' => {
                let end = scan(text, i, true);
                let raw: String = text[i..end].iter().collect();
                let n = number::parse(&raw);
                if n.is_nan() {
                    return Err(unexpected(raw, at));
                }
                (Tok::Number(n), end - i)
            }
            _ => {
                let end = scan(text, i, false);
                let raw: String = text[i..end].iter().collect();
                (keyword(raw, text, end, out.last(), at)?, end - i)
            }
        };
        out.push(Token { tok, at });
        i += len;
    }
    Ok(out)
}

/// Where a `.` starts a number (`.5`): at the start, and after `,`, `(`,
/// `[` or an operator.
fn operand_can_start(last: Option<&Token>) -> bool {
    match last.map(|t| &t.tok) {
        None => true,
        Some(t) => matches!(t, Tok::Comma | Tok::Open | Tok::OpenIndex | Tok::Not | Tok::Op(_) | Tok::And | Tok::Or),
    }
}

/// The end of a number or a keyword: the next boundary. A number goes on
/// over `.`.
fn scan(text: &[char], start: usize, number: bool) -> usize {
    let mut i = start + 1;
    while i < text.len() && (!boundary(text[i]) || (number && text[i] == '.')) {
        i += 1;
    }
    i
}

fn keyword(raw: String, text: &[char], end: usize, last: Option<&Token>, at: usize) -> Result<Tok, Error> {
    if !is_keyword(&raw) {
        return Err(unexpected(raw, at));
    }
    if matches!(last.map(|t| &t.tok), Some(Tok::Dot)) {
        return Ok(Tok::Property(raw));
    }
    Ok(match raw.as_str() {
        "null" => Tok::Null,
        "true" => Tok::Bool(true),
        "false" => Tok::Bool(false),
        "NaN" => Tok::Number(f64::NAN),
        "Infinity" => Tok::Number(f64::INFINITY),
        _ => {
            let mut j = end;
            while j < text.len() && text[j].is_whitespace() {
                j += 1;
            }
            if text.get(j) == Some(&'(') { Tok::Function(raw) } else { Tok::Name(raw) }
        }
    })
}

/// A string literal from its opening quote: `''` is one quote. It gives
/// the text and the count of characters read.
fn string(text: &[char], start: usize) -> Result<(String, usize), Error> {
    let mut out = String::new();
    let mut i = start + 1;
    while i < text.len() {
        if text[i] == '\'' {
            if text.get(i + 1) == Some(&'\'') {
                out.push('\'');
                i += 2;
                continue;
            }
            return Ok((out, i + 1 - start));
        }
        out.push(text[i]);
        i += 1;
    }
    Err(Error::at(ErrorKind::UnclosedString, start))
}

fn unexpected(text: String, at: usize) -> Error {
    Error::at(ErrorKind::Unexpected(text), at)
}
