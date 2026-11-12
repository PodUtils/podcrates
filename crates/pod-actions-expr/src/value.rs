//! The values of an expression and of a context, and the conversions of
//! GitHub between them: the truth of a value, its number, its text, and the
//! loose equality and order of two values.

use std::cmp::Ordering;
use std::sync::Arc;

use crate::number;

/// A value of an expression, or of a context.
///
/// An array and an object are shared: a copy is cheap. `==` in an
/// expression is true for two collections only when they are the same
/// instance, as in GitHub. The Rust `PartialEq` compares the contents.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Arc<Vec<Value>>),
    Object(Arc<Object>),
}

/// An object: its entries in their order. A read of a key ignores case, as
/// GitHub's contexts do.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Object {
    entries: Vec<(String, Value)>,
}

impl Object {
    pub fn new() -> Object {
        Object::default()
    }

    /// The value of `key`, with no regard to case.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| eq_ignore_case(k, key)).map(|(_, v)| v)
    }

    /// Set `key` to `value`. A key that exists with no regard to case keeps
    /// its place and its text, and takes the new value.
    pub fn insert(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        match self.entries.iter_mut().find(|(k, _)| eq_ignore_case(k, &key)) {
            Some((_, slot)) => *slot = value,
            None => self.entries.push((key, value)),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(k, _)| k.as_str())
    }

    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.entries.iter().map(|(_, v)| v)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<K: Into<String>> FromIterator<(K, Value)> for Object {
    fn from_iter<I: IntoIterator<Item = (K, Value)>>(iter: I) -> Object {
        let mut object = Object::new();
        for (k, v) in iter {
            object.insert(k, v);
        }
        object
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Bool(b)
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Value {
        Value::Number(n)
    }
}

impl From<i32> for Value {
    fn from(n: i32) -> Value {
        Value::Number(f64::from(n))
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::String(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Value {
        Value::String(s)
    }
}

impl From<Object> for Value {
    fn from(o: Object) -> Value {
        Value::Object(Arc::new(o))
    }
}

impl From<Vec<Value>> for Value {
    fn from(items: Vec<Value>) -> Value {
        Value::Array(Arc::new(items))
    }
}

impl Value {
    /// An array of `items`.
    pub fn array(items: impl IntoIterator<Item = Value>) -> Value {
        Value::Array(Arc::new(items.into_iter().collect()))
    }

    /// An object of `entries`, in their order.
    pub fn object<K: Into<String>>(entries: impl IntoIterator<Item = (K, Value)>) -> Value {
        Value::Object(Arc::new(entries.into_iter().collect()))
    }

    /// The name of the kind: `null`, `boolean`, `number`, `string`,
    /// `array` or `object`.
    pub fn kind(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        }
    }

    /// Null, a boolean, a number or a string.
    pub fn is_primitive(&self) -> bool {
        !matches!(self, Value::Array(_) | Value::Object(_))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    /// The value of `key` of an object, with no regard to case.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object().and_then(|o| o.get(key))
    }

    /// The truth of a value: false for null, `false`, 0, NaN and the empty
    /// string; true for each other value, each array and each object.
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            Value::Array(_) | Value::Object(_) => true,
        }
    }

    /// The number of a value: null 0, `true` 1, `false` 0, a string by
    /// GitHub's rules (empty 0, else a JSON number, else NaN), a
    /// collection NaN.
    pub fn to_number(&self) -> f64 {
        match self {
            Value::Null => 0.0,
            Value::Bool(b) => f64::from(u8::from(*b)),
            Value::Number(n) => *n,
            Value::String(s) => number::parse(s),
            Value::Array(_) | Value::Object(_) => f64::NAN,
        }
    }

    /// The text of a value: null is empty; `true`, `false`; a number with 15
    /// significant digits; an array `Array` and an object `Object`.
    pub fn to_text(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => number::format(*n),
            Value::String(s) => s.clone(),
            Value::Array(_) => "Array".into(),
            Value::Object(_) => "Object".into(),
        }
    }

    /// `==` of GitHub: the abstract equality of JavaScript, with no regard
    /// to case for two strings, and with no coercion of a collection. NaN
    /// equals nothing.
    pub fn loose_eq(&self, other: &Value) -> bool {
        match coerce(self, other) {
            (Prim::Null, Prim::Null) => true,
            (Prim::Number(a), Prim::Number(b)) => a == b,
            (Prim::String(a), Prim::String(b)) => eq_ignore_case(a, b),
            (Prim::Bool(a), Prim::Bool(b)) => a == b,
            (Prim::Array(a), Prim::Array(b)) => Arc::ptr_eq(a, b),
            (Prim::Object(a), Prim::Object(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// `>` of GitHub, after the same coercion as [`Value::loose_eq`].
    pub fn loose_gt(&self, other: &Value) -> bool {
        order(self, other) == Some(Ordering::Greater)
    }

    /// `<` of GitHub, after the same coercion as [`Value::loose_eq`].
    pub fn loose_lt(&self, other: &Value) -> bool {
        order(self, other) == Some(Ordering::Less)
    }
}

/// A value as a kind with its data, for the coercion.
#[derive(Clone, Copy)]
enum Prim<'a> {
    Null,
    Bool(bool),
    Number(f64),
    String(&'a str),
    Array(&'a Arc<Vec<Value>>),
    Object(&'a Arc<Object>),
}

impl<'a> Prim<'a> {
    fn of(value: &'a Value) -> Prim<'a> {
        match value {
            Value::Null => Prim::Null,
            Value::Bool(b) => Prim::Bool(*b),
            Value::Number(n) => Prim::Number(*n),
            Value::String(s) => Prim::String(s),
            Value::Array(a) => Prim::Array(a),
            Value::Object(o) => Prim::Object(o),
        }
    }

    fn kind(self) -> u8 {
        match self {
            Prim::Null => 0,
            Prim::Bool(_) => 1,
            Prim::Number(_) => 2,
            Prim::String(_) => 3,
            Prim::Array(_) => 4,
            Prim::Object(_) => 5,
        }
    }

    fn number(self) -> Prim<'a> {
        Prim::Number(match self {
            Prim::Null => 0.0,
            Prim::Bool(b) => f64::from(u8::from(b)),
            Prim::Number(n) => n,
            Prim::String(s) => number::parse(s),
            Prim::Array(_) | Prim::Object(_) => f64::NAN,
        })
    }
}

/// The runner's `CoerceTypes`: a number and a string compare as numbers; a
/// boolean or null becomes a number, then the rules apply again. Two
/// other kinds stay apart.
fn coerce<'a>(left: &'a Value, right: &'a Value) -> (Prim<'a>, Prim<'a>) {
    let (mut l, mut r) = (Prim::of(left), Prim::of(right));
    // Each turn makes one side a number, so two turns end it.
    for _ in 0..3 {
        if l.kind() == r.kind() {
            break;
        }
        match (l, r) {
            (Prim::Number(_), Prim::String(_)) => r = r.number(),
            (Prim::String(_), Prim::Number(_)) => l = l.number(),
            (Prim::Bool(_) | Prim::Null, _) => l = l.number(),
            (_, Prim::Bool(_) | Prim::Null) => r = r.number(),
            _ => break,
        }
    }
    (l, r)
}

/// The order of two values after the coercion, or `None` where GitHub
/// gives false for both `<` and `>`.
fn order(left: &Value, right: &Value) -> Option<Ordering> {
    match coerce(left, right) {
        (Prim::Number(a), Prim::Number(b)) => a.partial_cmp(&b),
        (Prim::String(a), Prim::String(b)) => Some(cmp_ignore_case(a, b)),
        (Prim::Bool(a), Prim::Bool(b)) => Some(a.cmp(&b)),
        _ => None,
    }
}

/// A character as .NET's `OrdinalIgnoreCase` compares it: its simple upper
/// case, where that is one character.
pub(crate) fn fold(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_uppercase();
    }
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

/// A text with each character folded: the folded text keeps the number and
/// the order of the characters.
pub(crate) fn folded(s: &str) -> String {
    s.chars().map(fold).collect()
}

pub(crate) fn eq_ignore_case(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.eq_ignore_ascii_case(b) || a.chars().map(fold).eq(b.chars().map(fold))
}

pub(crate) fn cmp_ignore_case(a: &str, b: &str) -> Ordering {
    a.chars().map(fold).cmp(b.chars().map(fold))
}
