//! The parser: the tokens of an expression into a tree. The precedence, from
//! the highest: `( )`, `[ ]` and `.`; `!`; `<`, `<=`, `>`, `>=`; `==`,
//! `!=`; `&&`; `||`. Each binary operator is left associative. A chain of
//! `&&` or of `||` is one node, as in the runner.

use crate::error::{Error, ErrorKind};
use crate::lex::{self, Op, Tok, Token};
use crate::value::Value;

/// The longest expression that GitHub takes, in characters.
pub const MAX_LENGTH: usize = 21_000;
/// The deepest tree that GitHub takes.
pub const MAX_DEPTH: usize = 50;

#[derive(Clone, Debug)]
pub(crate) enum Node {
    Literal(Value),
    /// A named value (a context), with its place.
    Context(String, usize),
    Index(Box<Node>, Key),
    Not(Box<Node>),
    Compare(Op, Box<Node>, Box<Node>),
    And(Vec<Node>),
    Or(Vec<Node>),
    Call(Function, Vec<Node>, usize),
}

/// The key of an index: an expression (`[e]`, or `.name` as a string), or
/// the filter `*`.
#[derive(Clone, Debug)]
pub(crate) enum Key {
    Value(Box<Node>),
    Star,
}

/// The functions of a workflow. `hashFiles` parses, and its evaluation
/// fails by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Function {
    Contains,
    StartsWith,
    EndsWith,
    Format,
    Join,
    ToJson,
    FromJson,
    HashFiles,
    Success,
    Failure,
    Always,
    Cancelled,
}

impl Function {
    /// The name, the least and the most arguments.
    const ALL: [(Function, &'static str, usize, usize); 12] = [
        (Function::Contains, "contains", 2, 2),
        (Function::StartsWith, "startsWith", 2, 2),
        (Function::EndsWith, "endsWith", 2, 2),
        (Function::Format, "format", 1, usize::MAX),
        (Function::Join, "join", 1, 2),
        (Function::ToJson, "toJSON", 1, 1),
        (Function::FromJson, "fromJSON", 1, 1),
        (Function::HashFiles, "hashFiles", 1, 255),
        (Function::Success, "success", 0, 0),
        (Function::Failure, "failure", 0, 0),
        (Function::Always, "always", 0, 0),
        (Function::Cancelled, "cancelled", 0, 0),
    ];

    /// A function by its name, with no regard to case.
    fn find(name: &str) -> Option<(Function, &'static str, usize, usize)> {
        Function::ALL.into_iter().find(|(_, n, _, _)| n.eq_ignore_ascii_case(name))
    }

    pub(crate) fn name(self) -> &'static str {
        Function::ALL.iter().find(|(f, ..)| *f == self).map_or("", |(_, n, ..)| n)
    }

    pub(crate) fn is_status(self) -> bool {
        matches!(self, Function::Success | Function::Failure | Function::Always | Function::Cancelled)
    }
}

/// Parse `text` into a tree.
pub(crate) fn parse(text: &str) -> Result<Node, Error> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() > MAX_LENGTH {
        return Err(Error::new(ErrorKind::TooLong { length: chars.len(), max: MAX_LENGTH }));
    }
    let tokens = lex::lex(&chars)?;
    if tokens.is_empty() {
        return Err(Error::new(ErrorKind::Empty));
    }
    let mut parser = Parser { tokens, pos: 0, end: chars.len(), nesting: 0 };
    let (node, _) = parser.or()?;
    match parser.tokens.get(parser.pos) {
        None => Ok(node),
        Some(t) => Err(Error::at(ErrorKind::Unexpected(show(&t.tok)), t.at)),
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// The length of the text: the place of an early end.
    end: usize,
    /// The nesting of groups, indexes, calls and `!`: it bounds the
    /// recursion before the depth of the tree is known.
    nesting: usize,
}

/// A node and the depth of its tree.
type Parsed = (Node, usize);

fn deeper(node: Node, depth: usize) -> Result<Parsed, Error> {
    if depth > MAX_DEPTH {
        return Err(Error::new(ErrorKind::TooDeep { max: MAX_DEPTH }));
    }
    Ok((node, depth))
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos).map(|t| &t.tok)
    }

    fn next(&mut self) -> Result<Token, Error> {
        let token = self.tokens.get(self.pos).cloned().ok_or(Error::at(ErrorKind::UnexpectedEnd, self.end))?;
        self.pos += 1;
        Ok(token)
    }

    fn expect(&mut self, want: &Tok) -> Result<(), Error> {
        let token = self.next()?;
        if &token.tok == want { Ok(()) } else { Err(Error::at(ErrorKind::Unexpected(show(&token.tok)), token.at)) }
    }

    fn enter(&mut self) -> Result<(), Error> {
        self.nesting += 1;
        if self.nesting > MAX_DEPTH {
            return Err(Error::new(ErrorKind::TooDeep { max: MAX_DEPTH }));
        }
        Ok(())
    }

    fn or(&mut self) -> Result<Parsed, Error> {
        self.chain(Tok::Or, Parser::and, Node::Or)
    }

    fn and(&mut self) -> Result<Parsed, Error> {
        self.chain(Tok::And, Parser::equality, Node::And)
    }

    /// `item (sep item)*` as one node of each item.
    fn chain(
        &mut self,
        sep: Tok,
        item: fn(&mut Parser) -> Result<Parsed, Error>,
        make: fn(Vec<Node>) -> Node,
    ) -> Result<Parsed, Error> {
        let (first, mut depth) = item(self)?;
        if self.peek() != Some(&sep) {
            return Ok((first, depth));
        }
        let mut items = vec![first];
        while self.peek() == Some(&sep) {
            self.pos += 1;
            let (next, d) = item(self)?;
            depth = depth.max(d);
            items.push(next);
        }
        deeper(make(items), depth + 1)
    }

    fn equality(&mut self) -> Result<Parsed, Error> {
        self.compare(&[Op::Eq, Op::Ne], Parser::relation)
    }

    fn relation(&mut self) -> Result<Parsed, Error> {
        self.compare(&[Op::Lt, Op::Le, Op::Gt, Op::Ge], Parser::unary)
    }

    fn compare(&mut self, ops: &[Op], item: fn(&mut Parser) -> Result<Parsed, Error>) -> Result<Parsed, Error> {
        let (mut left, mut depth) = item(self)?;
        while let Some(Tok::Op(op)) = self.peek().cloned() {
            if !ops.contains(&op) {
                break;
            }
            self.pos += 1;
            let (right, d) = item(self)?;
            (left, depth) = deeper(Node::Compare(op, Box::new(left), Box::new(right)), depth.max(d) + 1)?;
        }
        Ok((left, depth))
    }

    fn unary(&mut self) -> Result<Parsed, Error> {
        if self.peek() == Some(&Tok::Not) {
            self.pos += 1;
            self.enter()?;
            let (inner, depth) = self.unary()?;
            self.nesting -= 1;
            return deeper(Node::Not(Box::new(inner)), depth + 1);
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Parsed, Error> {
        let (mut node, mut depth) = self.primary()?;
        loop {
            match self.peek() {
                Some(Tok::Dot) => {
                    self.pos += 1;
                    let token = self.next()?;
                    let key = match token.tok {
                        Tok::Property(name) => Key::Value(Box::new(Node::Literal(Value::String(name)))),
                        Tok::Star => Key::Star,
                        other => return Err(Error::at(ErrorKind::Unexpected(show(&other)), token.at)),
                    };
                    (node, depth) = deeper(Node::Index(Box::new(node), key), depth + 1)?;
                }
                Some(Tok::OpenIndex) => {
                    self.pos += 1;
                    let (key, d) = if self.peek() == Some(&Tok::Star) {
                        self.pos += 1;
                        (Key::Star, 0)
                    } else {
                        self.enter()?;
                        let (key, d) = self.or()?;
                        self.nesting -= 1;
                        (Key::Value(Box::new(key)), d)
                    };
                    self.expect(&Tok::CloseIndex)?;
                    (node, depth) = deeper(Node::Index(Box::new(node), key), depth.max(d) + 1)?;
                }
                _ => return Ok((node, depth)),
            }
        }
    }

    fn primary(&mut self) -> Result<Parsed, Error> {
        let token = self.next()?;
        let node = match token.tok {
            Tok::Null => Node::Literal(Value::Null),
            Tok::Bool(b) => Node::Literal(Value::Bool(b)),
            Tok::Number(n) => Node::Literal(Value::Number(n)),
            Tok::String(s) => Node::Literal(Value::String(s)),
            Tok::Name(name) => Node::Context(name, token.at),
            Tok::Function(name) => return self.call(name, token.at),
            Tok::Open => {
                self.enter()?;
                let inner = self.or()?;
                self.nesting -= 1;
                self.expect(&Tok::Close)?;
                return Ok(inner);
            }
            other => return Err(Error::at(ErrorKind::Unexpected(show(&other)), token.at)),
        };
        Ok((node, 1))
    }

    fn call(&mut self, name: String, at: usize) -> Result<Parsed, Error> {
        let Some((function, canonical, min, max)) = Function::find(&name) else {
            return Err(Error::at(ErrorKind::UnknownFunction(name), at));
        };
        self.expect(&Tok::Open)?;
        self.enter()?;
        let mut args = Vec::new();
        let mut depth = 0;
        if self.peek() == Some(&Tok::Close) {
            self.pos += 1;
        } else {
            loop {
                let (arg, d) = self.or()?;
                depth = depth.max(d);
                args.push(arg);
                let token = self.next()?;
                match token.tok {
                    Tok::Comma => continue,
                    Tok::Close => break,
                    other => return Err(Error::at(ErrorKind::Unexpected(show(&other)), token.at)),
                }
            }
        }
        self.nesting -= 1;
        if args.len() < min || args.len() > max {
            let kind = ErrorKind::Arity { function: canonical.into(), min, max, given: args.len() };
            return Err(Error::at(kind, at));
        }
        deeper(Node::Call(function, args, at), depth + 1)
    }
}

/// The text of a token, for an error.
fn show(tok: &Tok) -> String {
    match tok {
        Tok::Null => "null".into(),
        Tok::Bool(b) => b.to_string(),
        Tok::Number(n) => crate::number::format(*n),
        Tok::String(s) => format!("'{}'", s.replace('\'', "''")),
        Tok::Name(s) | Tok::Property(s) | Tok::Function(s) => s.clone(),
        Tok::Dot => ".".into(),
        Tok::Star => "*".into(),
        Tok::Open => "(".into(),
        Tok::Close => ")".into(),
        Tok::OpenIndex => "[".into(),
        Tok::CloseIndex => "]".into(),
        Tok::Comma => ",".into(),
        Tok::Not => "!".into(),
        Tok::And => "&&".into(),
        Tok::Or => "||".into(),
        Tok::Op(op) => match op {
            Op::Eq => "==",
            Op::Ne => "!=",
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Gt => ">",
            Op::Ge => ">=",
        }
        .into(),
    }
}

impl Node {
    /// Each node of the tree, this one first.
    pub(crate) fn walk(&self, visit: &mut dyn FnMut(&Node)) {
        visit(self);
        match self {
            Node::Literal(_) | Node::Context(..) => {}
            Node::Index(target, key) => {
                target.walk(visit);
                if let Key::Value(k) = key {
                    k.walk(visit);
                }
            }
            Node::Not(inner) => inner.walk(visit),
            Node::Compare(_, l, r) => {
                l.walk(visit);
                r.walk(visit);
            }
            Node::And(items) | Node::Or(items) | Node::Call(_, items, _) => {
                for item in items {
                    item.walk(visit);
                }
            }
        }
    }
}
