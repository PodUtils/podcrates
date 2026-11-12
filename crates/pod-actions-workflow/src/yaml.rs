//! The YAML of a workflow file: the events of saphyr-parser into a tree with
//! the line of each node and the order of each mapping. Plain scalars take
//! the core schema of YAML 1.2, as the runner's reader does (`on` is a
//! string; `yes` too). A duplicate key, a key that is not a scalar and a
//! second document are errors. An alias copies its anchored node, within
//! the limits on the count of nodes and on the depth.

use std::collections::{HashMap, HashSet};
use std::fmt;

use saphyr_parser::{Event, Parser, ScalarStyle};

use crate::error::{Error, ErrorKind};

/// A place in the file: the line and the column, each from 1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Mark {
    pub line: u32,
    pub column: u32,
}

impl fmt::Display for Mark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}, column {}", self.line, self.column)
    }
}

/// A node of the file, with its place.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub kind: Kind,
    pub mark: Mark,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Null,
    Bool(bool),
    /// Each number is a double, as in the runner.
    Number(f64),
    String(String),
    Seq(Vec<Node>),
    /// The entries in the order of the file.
    Map(Vec<(Key, Node)>),
}

/// A key of a mapping: the text of its scalar, and its place.
#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    pub name: String,
    pub mark: Mark,
}

/// The bounds of a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The size of the text: 1 MiB.
    pub max_bytes: usize,
    /// The nodes of the tree, with each copy of an alias: 100,000.
    pub max_nodes: usize,
    /// The depth of the tree: 64.
    pub max_depth: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_bytes: 1 << 20, max_nodes: 100_000, max_depth: 64 }
    }
}

enum Frame {
    Seq { items: Vec<Node>, mark: Mark, anchor: usize, height: usize },
    Map { entries: Vec<(Key, Node)>, seen: HashSet<String>, key: Option<Key>, mark: Mark, anchor: usize, height: usize },
}

struct Loader {
    limits: Limits,
    stack: Vec<Frame>,
    /// Each anchor: its node, its count of nodes and its height.
    anchors: HashMap<usize, (Node, usize, usize)>,
    nodes: usize,
    root: Option<Node>,
}

/// Read the one document of `text` into a tree.
pub fn load(text: &str, limits: Limits) -> Result<Node, Error> {
    if text.len() > limits.max_bytes {
        return Err(Error::new("", None, ErrorKind::Limit { what: "bytes", max: limits.max_bytes }));
    }
    let mut loader = Loader { limits, stack: Vec::new(), anchors: HashMap::new(), nodes: 0, root: None };
    let mut documents = 0;
    let mut parser = Parser::new_from_str(text);
    while let Some(next) = parser.next_event() {
        let (event, span) = next.map_err(|e| {
            let mark = Mark { line: e.marker().line() as u32, column: e.marker().col() as u32 + 1 };
            Error::new("", Some(mark), ErrorKind::Yaml(e.info().to_string()))
        })?;
        let mark = Mark { line: span.start.line() as u32, column: span.start.col() as u32 + 1 };
        match event {
            Event::DocumentStart(_) => {
                documents += 1;
                if documents > 1 {
                    return Err(Error::new("", Some(mark), ErrorKind::Documents));
                }
            }
            Event::Scalar(value, style, anchor, tag) => {
                let forced = tag.as_ref().is_some_and(|t| t.is_yaml_core_schema() && t.suffix == "str");
                let kind = if style == ScalarStyle::Plain && !forced {
                    resolve(&value)
                } else {
                    Kind::String(value.to_string())
                };
                loader.count(1, mark)?;
                loader.put(Node { kind, mark }, Some(value.to_string()), anchor, 1)?;
            }
            Event::SequenceStart(anchor, _) => {
                loader.open(mark)?;
                loader.stack.push(Frame::Seq { items: Vec::new(), mark, anchor, height: 0 });
            }
            Event::MappingStart(anchor, _) => {
                loader.open(mark)?;
                let seen = HashSet::new();
                loader.stack.push(Frame::Map { entries: Vec::new(), seen, key: None, mark, anchor, height: 0 });
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let (node, anchor, height) = match loader.stack.pop() {
                    Some(Frame::Seq { items, mark, anchor, height }) => {
                        (Node { kind: Kind::Seq(items), mark }, anchor, height)
                    }
                    Some(Frame::Map { entries, mark, anchor, height, .. }) => {
                        (Node { kind: Kind::Map(entries), mark }, anchor, height)
                    }
                    None => continue,
                };
                loader.put(node, None, anchor, height + 1)?;
            }
            Event::Alias(id) => {
                let Some((node, size, height)) = loader.anchors.get(&id).cloned() else {
                    return Err(Error::new("", Some(mark), ErrorKind::Yaml("an alias of no anchor".into())));
                };
                if loader.stack.len() + height > loader.limits.max_depth {
                    return Err(Error::new("", Some(mark), ErrorKind::Limit { what: "levels", max: limits.max_depth }));
                }
                loader.count(size, mark)?;
                let text = scalar_text(&node);
                loader.put(node, text, 0, height)?;
            }
            Event::StreamStart | Event::StreamEnd | Event::DocumentEnd | Event::Nothing => {}
        }
    }
    loader.root.ok_or(Error::new("", None, ErrorKind::Empty))
}

impl Loader {
    fn count(&mut self, nodes: usize, mark: Mark) -> Result<(), Error> {
        self.nodes += nodes;
        if self.nodes > self.limits.max_nodes {
            return Err(Error::new("", Some(mark), ErrorKind::Limit { what: "nodes", max: self.limits.max_nodes }));
        }
        Ok(())
    }

    fn open(&mut self, mark: Mark) -> Result<(), Error> {
        self.count(1, mark)?;
        if self.stack.len() + 1 > self.limits.max_depth {
            return Err(Error::new("", Some(mark), ErrorKind::Limit { what: "levels", max: self.limits.max_depth }));
        }
        Ok(())
    }

    /// Put a finished node into its parent. `text` is the text of a scalar,
    /// for a key.
    fn put(&mut self, node: Node, text: Option<String>, anchor: usize, height: usize) -> Result<(), Error> {
        if anchor != 0 {
            let size = size(&node);
            self.anchors.insert(anchor, (node.clone(), size, height));
        }
        match self.stack.last_mut() {
            None => self.root = Some(node),
            Some(Frame::Seq { items, height: h, .. }) => {
                *h = (*h).max(height);
                items.push(node);
            }
            Some(Frame::Map { entries, seen, key, height: h, .. }) => match key.take() {
                None => {
                    let Some(name) = text else {
                        return Err(Error::new("", Some(node.mark), ErrorKind::KeyNotScalar));
                    };
                    if !seen.insert(name.clone()) {
                        return Err(Error::new("", Some(node.mark), ErrorKind::DuplicateKey(name)));
                    }
                    *key = Some(Key { name, mark: node.mark });
                }
                Some(k) => {
                    *h = (*h).max(height);
                    entries.push((k, node));
                }
            },
        }
        Ok(())
    }
}

/// The count of nodes of a tree.
fn size(node: &Node) -> usize {
    1 + match &node.kind {
        Kind::Seq(items) => items.iter().map(size).sum(),
        Kind::Map(entries) => entries.iter().map(|(_, v)| 1 + size(v)).sum(),
        _ => 0,
    }
}

/// The text of a scalar node, as a key.
fn scalar_text(node: &Node) -> Option<String> {
    Some(match &node.kind {
        Kind::Null => String::new(),
        Kind::Bool(b) => b.to_string(),
        Kind::Number(n) => pod_actions_expr::Value::Number(*n).to_text(),
        Kind::String(s) => s.clone(),
        Kind::Seq(_) | Kind::Map(_) => return None,
    })
}

/// A plain scalar by the core schema of YAML 1.2.
fn resolve(s: &str) -> Kind {
    match s {
        "" | "~" | "null" | "Null" | "NULL" => return Kind::Null,
        "true" | "True" | "TRUE" => return Kind::Bool(true),
        "false" | "False" | "FALSE" => return Kind::Bool(false),
        ".nan" | ".NaN" | ".NAN" => return Kind::Number(f64::NAN),
        _ => {}
    }
    let (sign, body) = match s.as_bytes()[0] {
        b'-' => (-1.0, &s[1..]),
        b'+' => (1.0, &s[1..]),
        _ => (1.0, s),
    };
    if matches!(body, ".inf" | ".Inf" | ".INF") {
        return Kind::Number(sign * f64::INFINITY);
    }
    if let Some(digits) = s.strip_prefix("0x")
        && !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Kind::Number(radix(digits, 16));
    }
    if let Some(digits) = s.strip_prefix("0o")
        && !digits.is_empty()
        && digits.bytes().all(|b| (b'0'..=b'7').contains(&b))
    {
        return Kind::Number(radix(digits, 8));
    }
    if is_float(body) {
        return s.parse().map_or_else(|_| Kind::String(s.to_string()), Kind::Number);
    }
    Kind::String(s.to_string())
}

fn radix(digits: &str, radix: u32) -> f64 {
    digits.chars().fold(0.0, |n, c| n * f64::from(radix) + f64::from(c.to_digit(radix).unwrap_or(0)))
}

/// `(\.[0-9]+|[0-9]+(\.[0-9]*)?)([eE][-+]?[0-9]+)?`: the integers and the
/// floats of the core schema, with no sign.
fn is_float(s: &str) -> bool {
    let b = s.as_bytes();
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i - start
    };
    let mut i = 0;
    if b.first() == Some(&b'.') {
        i = 1;
        if digits(&mut i) == 0 {
            return false;
        }
    } else {
        if digits(&mut i) == 0 {
            return false;
        }
        if b.get(i) == Some(&b'.') {
            i += 1;
            digits(&mut i);
        }
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if digits(&mut i) == 0 {
            return false;
        }
    }
    i == b.len()
}
