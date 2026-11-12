//! The evaluator: a tree and the caller's contexts into a value.

use std::sync::Arc;

use crate::error::{Error, ErrorKind};
use crate::funcs;
use crate::lex::Op;
use crate::parse::{Key, Node};
use crate::value::{Value, eq_ignore_case};

/// The answers of the status functions, from the caller. At the level of a
/// job, `success` is true when each job before it (each need, and their
/// needs) succeeded and the run is not cancelled; `failure` is true when one
/// of them failed. `always()` is true in each case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub success: bool,
    pub failure: bool,
    pub cancelled: bool,
}

impl Default for Status {
    /// Nothing before it failed, and the run goes on.
    fn default() -> Status {
        Status { success: true, failure: false, cancelled: false }
    }
}

/// The bounds of one evaluation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The bytes that the text of `format`, `join` and `toJSON`, the input
    /// of `fromJSON`, and the filtered arrays may take in sum.
    pub max_bytes: usize,
}

impl Default for Limits {
    /// 1 MiB, the memory limit of an expression in the runner.
    fn default() -> Limits {
        Limits { max_bytes: 1 << 20 }
    }
}

/// The named values (the contexts) that an expression can read, the
/// answers of the status functions and the limits. A name matches with no
/// regard to case. A name that the caller does not give is an error.
#[derive(Clone, Debug, Default)]
pub struct Contexts {
    entries: Vec<(String, Value)>,
    pub status: Status,
    pub limits: Limits,
}

impl Contexts {
    pub fn new() -> Contexts {
        Contexts::default()
    }

    /// Give the context `name`. A context of the same name goes.
    pub fn insert(&mut self, name: impl Into<String>, value: Value) -> &mut Contexts {
        let name = name.into();
        self.entries.retain(|(n, _)| !eq_ignore_case(n, &name));
        self.entries.push((name, value));
        self
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.entries.iter().find(|(n, _)| eq_ignore_case(n, name)).map(|(_, v)| v)
    }

    pub fn remove(&mut self, name: &str) -> Option<Value> {
        let i = self.entries.iter().position(|(n, _)| eq_ignore_case(n, name))?;
        Some(self.entries.remove(i).1)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(n, _)| n.as_str())
    }
}

/// A value during an evaluation. `filtered` marks the array of a filter
/// (`.*`): an index of it applies to each item.
#[derive(Clone, Debug)]
pub(crate) struct Out {
    pub value: Value,
    pub filtered: bool,
}

impl Out {
    pub(crate) fn plain(value: Value) -> Out {
        Out { value, filtered: false }
    }
}

pub(crate) struct Eval<'a> {
    pub contexts: &'a Contexts,
    /// The bytes left of the limit.
    left: usize,
}

/// Evaluate `node`. Each named value of the tree must be a context of the
/// caller, also in a branch that the evaluation does not reach, as the
/// runner checks them at the parse.
pub(crate) fn evaluate(node: &Node, contexts: &Contexts) -> Result<Value, Error> {
    check_contexts(node, contexts)?;
    let mut eval = Eval { contexts, left: contexts.limits.max_bytes };
    Ok(eval.eval(node)?.value)
}

pub(crate) fn check_contexts(node: &Node, contexts: &Contexts) -> Result<(), Error> {
    let mut missing = None;
    node.walk(&mut |n| {
        if let Node::Context(name, at) = n
            && missing.is_none()
            && contexts.get(name).is_none()
        {
            missing = Some(Error::at(ErrorKind::UnknownContext(name.clone()), *at));
        }
    });
    missing.map_or(Ok(()), Err)
}

impl Eval<'_> {
    /// Take `bytes` of the limit.
    pub(crate) fn spend(&mut self, bytes: usize) -> Result<(), Error> {
        match self.left.checked_sub(bytes) {
            Some(left) => {
                self.left = left;
                Ok(())
            }
            None => Err(Error::new(ErrorKind::TooLarge { max: self.contexts.limits.max_bytes })),
        }
    }

    pub(crate) fn eval(&mut self, node: &Node) -> Result<Out, Error> {
        Ok(match node {
            Node::Literal(v) => Out::plain(v.clone()),
            Node::Context(name, at) => match self.contexts.get(name) {
                Some(v) => Out::plain(v.clone()),
                None => return Err(Error::at(ErrorKind::UnknownContext(name.clone()), *at)),
            },
            Node::Not(inner) => Out::plain(Value::Bool(!self.eval(inner)?.value.is_truthy())),
            Node::Compare(op, l, r) => {
                let (l, r) = (self.eval(l)?.value, self.eval(r)?.value);
                let b = match op {
                    Op::Eq => l.loose_eq(&r),
                    Op::Ne => !l.loose_eq(&r),
                    Op::Lt => l.loose_lt(&r),
                    Op::Le => l.loose_eq(&r) || l.loose_lt(&r),
                    Op::Gt => l.loose_gt(&r),
                    Op::Ge => l.loose_eq(&r) || l.loose_gt(&r),
                };
                Out::plain(Value::Bool(b))
            }
            // `&&` gives the first falsy operand, else the last one; `||`
            // the first truthy operand, else the last one.
            Node::And(items) => self.short(items, false)?,
            Node::Or(items) => self.short(items, true)?,
            Node::Index(target, key) => {
                let target = self.eval(target)?;
                let key = match key {
                    Key::Star => None,
                    Key::Value(k) => Some(self.eval(k)?.value),
                };
                self.index(target, key)?
            }
            Node::Call(function, args, at) => funcs::call(self, *function, args, *at)?,
        })
    }

    fn short(&mut self, items: &[Node], stop_on: bool) -> Result<Out, Error> {
        let mut last = Out::plain(Value::Null);
        for item in items {
            last = self.eval(item)?;
            if last.value.is_truthy() == stop_on {
                break;
            }
        }
        Ok(last)
    }

    /// The runner's `Index`: `key` `None` is the filter `*`.
    fn index(&mut self, target: Out, key: Option<Value>) -> Result<Out, Error> {
        if target.filtered {
            let items = target.value.as_array().unwrap_or_default();
            let mut out = Vec::new();
            for item in items {
                match (item, &key) {
                    (Value::Object(o), None) => out.extend(o.values().cloned()),
                    (Value::Array(a), None) => out.extend(a.iter().cloned()),
                    (Value::Object(o), Some(k)) if k.is_primitive() => out.extend(o.get(&k.to_text()).cloned()),
                    (Value::Array(a), Some(k)) => out.extend(int_index(k).and_then(|i| a.get(i)).cloned()),
                    _ => {}
                }
            }
            return self.filtered(out);
        }
        match (&target.value, key) {
            (Value::Object(o), None) => self.filtered(o.values().cloned().collect()),
            (Value::Array(a), None) => self.filtered(a.to_vec()),
            (_, None) => self.filtered(Vec::new()),
            (Value::Object(o), Some(k)) if k.is_primitive() => {
                Ok(Out::plain(o.get(&k.to_text()).cloned().unwrap_or_default()))
            }
            (Value::Array(a), Some(k)) => {
                Ok(Out::plain(int_index(&k).and_then(|i| a.get(i)).cloned().unwrap_or_default()))
            }
            _ => Ok(Out::plain(Value::Null)),
        }
    }

    fn filtered(&mut self, items: Vec<Value>) -> Result<Out, Error> {
        self.spend(items.len() * std::mem::size_of::<Value>())?;
        Ok(Out { value: Value::Array(Arc::new(items)), filtered: true })
    }
}

/// An index of an array: the number of the key, rounded down; none for NaN,
/// a negative number, or a number past the 32-bit range.
fn int_index(key: &Value) -> Option<usize> {
    let n = key.to_number().floor();
    if n.is_nan() || n < 0.0 || n > f64::from(i32::MAX) {
        return None;
    }
    Some(n as usize)
}
