//! A node of the file as a value of an expression: as written, or with each
//! `${{ }}` evaluated, as the runner's template evaluator does it.

use pod_actions_expr::{Contexts, Object, Template, Value, has_expression};

use crate::error::{Error, ErrorKind};
use crate::yaml::{Kind, Node};

impl Node {
    /// The value of the node as written: a `${{ }}` stays text.
    pub fn to_value(&self) -> Value {
        match &self.kind {
            Kind::Null => Value::Null,
            Kind::Bool(b) => Value::Bool(*b),
            Kind::Number(n) => Value::Number(*n),
            Kind::String(s) => Value::String(s.clone()),
            Kind::Seq(items) => Value::array(items.iter().map(Node::to_value)),
            Kind::Map(entries) => Value::object(entries.iter().map(|(k, v)| (k.name.clone(), v.to_value()))),
        }
    }

    /// The value of the node with each template evaluated: a string that is
    /// one `${{ }}` and nothing else gives the value of its expression (an
    /// array, an object, a number); a string with text around its
    /// expressions gives a string. A key with `${{ }}` gives the text of its
    /// value. `path` names the node in an error.
    pub fn evaluate(&self, contexts: &Contexts, path: &str) -> Result<Value, Error> {
        Ok(match &self.kind {
            Kind::String(s) if has_expression(s) => template(s, contexts).map_err(|e| self.error(path, e))?,
            Kind::Seq(items) => {
                let mut out = Vec::with_capacity(items.len());
                for (i, item) in items.iter().enumerate() {
                    out.push(item.evaluate(contexts, &format!("{path}[{i}]"))?);
                }
                Value::from(out)
            }
            Kind::Map(entries) => {
                let mut out = Object::new();
                for (key, value) in entries {
                    let name = if has_expression(&key.name) {
                        template(&key.name, contexts).map_err(|e| self.error(path, e))?.to_text()
                    } else {
                        key.name.clone()
                    };
                    let item = value.evaluate(contexts, &format!("{path}.{}", key.name))?;
                    out.insert(name, item);
                }
                Value::from(out)
            }
            _ => self.to_value(),
        })
    }

    /// The text of a scalar: a string as it is; null, a boolean or a number
    /// by the text of GitHub. `None` for a collection.
    pub fn text(&self) -> Option<String> {
        match &self.kind {
            Kind::Seq(_) | Kind::Map(_) => None,
            _ => Some(self.to_value().to_text()),
        }
    }

    /// The entries of a mapping, or none.
    pub fn entries(&self) -> &[(crate::yaml::Key, Node)] {
        match &self.kind {
            Kind::Map(entries) => entries,
            _ => &[],
        }
    }

    /// The value of `key` of a mapping.
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.entries().iter().find(|(k, _)| k.name == key).map(|(_, v)| v)
    }

    fn error(&self, path: &str, e: pod_actions_expr::Error) -> Error {
        Error::new(path, Some(self.mark), ErrorKind::Expression(e))
    }

    /// Parse each template under the node: a `${{ }}` that does not parse
    /// is an error of the file, as GitHub refuses the file.
    pub(crate) fn check_templates(&self, path: &str) -> Result<(), Error> {
        match &self.kind {
            Kind::String(s) if has_expression(s) => {
                Template::parse(s).map_err(|e| self.error(path, e))?;
            }
            Kind::Seq(items) => {
                for (i, item) in items.iter().enumerate() {
                    item.check_templates(&format!("{path}[{i}]"))?;
                }
            }
            Kind::Map(entries) => {
                for (key, value) in entries {
                    if has_expression(&key.name) {
                        Template::parse(&key.name)
                            .map_err(|e| Error::new(path, Some(key.mark), ErrorKind::Expression(e)))?;
                    }
                    value.check_templates(&format!("{path}.{}", key.name))?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn template(text: &str, contexts: &Contexts) -> Result<Value, pod_actions_expr::Error> {
    Template::parse(text)?.evaluate(contexts)
}
