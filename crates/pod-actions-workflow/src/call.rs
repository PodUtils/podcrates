//! A job that calls a reusable workflow (GitHub's "Reusing workflows"):
//! the source of the called text, the limits, the interface of the called
//! workflow (`on.workflow_call`: `inputs`, `secrets`, `outputs`) and the
//! checks of a call against it. [`Run`](crate::Run) does the expansion.

use std::fmt;
use std::sync::Arc;

use pod_actions_expr::{Contexts, Object, Template, Value};

use crate::error::{Error, ErrorKind};
use crate::model::{Job, Workflow};
use crate::yaml::{Kind, Node};

/// The source of each called workflow. The crate reads no file and no
/// network: the caller gives the text.
pub trait Called {
    /// The text of the workflow that `uses` names (`./.github/workflows/x.yml`
    /// or `OWNER/REPO/.github/workflows/x.yml@REF`). `from` is the `uses` of
    /// the workflow that holds the call, so that the caller can resolve a
    /// `./` path; `None` for the top workflow.
    fn text(&self, uses: &str, from: Option<&str>) -> Result<String, String>;
}

/// A shared source that a [`Run`](crate::Run) keeps.
#[derive(Clone)]
pub(crate) struct Source(pub Arc<dyn Called + Send + Sync>);

impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Called")
    }
}

/// The limits on calls: GitHub's ten levels of workflows (the top workflow
/// and nine levels of called workflows) and fifty different called
/// workflows in one run; and the jobs of a run with each call expanded,
/// 10,000, so that a matrix of calls in a matrix of calls cannot fill the
/// memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallLimits {
    pub levels: usize,
    pub workflows: usize,
    pub jobs: usize,
}

impl Default for CallLimits {
    fn default() -> CallLimits {
        CallLimits { levels: 10, workflows: 50, jobs: 10_000 }
    }
}

/// The secrets that a job may read. The crate keeps names and templates;
/// the caller keeps the values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Secrets {
    /// Each secret of the run: a job of the top workflow, or a call with
    /// `secrets: inherit` up to the top.
    Run,
    /// The secrets that a call passes: each name with its template, which
    /// the caller evaluates with the `secrets` of `from`. A call with no
    /// `secrets:` passes none.
    Passed { from: Box<Secrets>, items: Vec<(String, String)> },
}

impl Secrets {
    /// The names that a job may read; `None` for each secret of the run.
    pub fn names(&self) -> Option<Vec<&str>> {
        match self {
            Secrets::Run => None,
            Secrets::Passed { items, .. } => Some(items.iter().map(|(n, _)| n.as_str()).collect()),
        }
    }
}

/// One call: a cell of a job that calls a reusable workflow.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    /// The id of the calling job.
    pub job: String,
    /// The name of the cell, as an [`Instance`](crate::Instance) has it.
    pub name: String,
    pub uses: String,
    /// The cell: null with no matrix.
    pub matrix: Value,
    /// The `inputs` of the called jobs: each input of the called workflow,
    /// checked, with its default.
    pub inputs: Value,
    pub secrets: Secrets,
    /// The ids of the called jobs, now jobs of the run, in the order of
    /// their file: `CALLER/CALLEE`, or `CALLER[N]/CALLEE` for cell N of a
    /// matrix.
    pub jobs: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Type {
    Boolean,
    Number,
    String,
}

#[derive(Clone, Debug)]
struct Input {
    name: String,
    kind: Type,
    required: bool,
    default: Option<Value>,
}

/// `on.workflow_call` of a called workflow.
#[derive(Clone, Debug, Default)]
pub(crate) struct Interface {
    inputs: Vec<Input>,
    secrets: Vec<(String, bool)>,
    outputs: Vec<(String, String)>,
}

fn kind_error(node: &Node, path: &str, what: &'static str) -> Error {
    Error::new(path, Some(node.mark), ErrorKind::Type(what))
}

fn map_of<'a>(node: &'a Node, path: &str) -> Result<&'a [(crate::yaml::Key, Node)], Error> {
    match &node.kind {
        Kind::Map(entries) => Ok(entries),
        Kind::Null => Ok(&[]),
        _ => Err(kind_error(node, path, "a mapping")),
    }
}

fn flag(node: Option<&Node>, path: &str) -> Result<bool, Error> {
    let Some(node) = node else { return Ok(false) };
    match node.kind {
        Kind::Null => Ok(false),
        Kind::Bool(b) => Ok(b),
        _ => Err(kind_error(node, path, "a boolean")),
    }
}

fn type_of(value: &Value) -> Option<Type> {
    match value {
        Value::Bool(_) => Some(Type::Boolean),
        Value::Number(_) => Some(Type::Number),
        Value::String(_) => Some(Type::String),
        _ => None,
    }
}

impl Type {
    fn name(self) -> &'static str {
        match self {
            Type::Boolean => "boolean",
            Type::Number => "number",
            Type::String => "string",
        }
    }

    /// The value of an input of this type: a string takes the text of a
    /// number or a boolean, as YAML reads `3.10` as a number.
    fn take(self, value: &Value) -> Option<Value> {
        match (self, value) {
            (Type::String, Value::Number(_) | Value::Bool(_)) => Some(Value::from(value.to_text())),
            _ if type_of(value) == Some(self) => Some(value.clone()),
            _ => None,
        }
    }

    fn zero(self) -> Value {
        match self {
            Type::Boolean => Value::Bool(false),
            Type::Number => Value::Number(0.0),
            Type::String => Value::from(""),
        }
    }
}

impl Interface {
    /// Read `on.workflow_call`; a workflow with no `workflow_call` event
    /// cannot be called.
    pub(crate) fn read(w: &Workflow) -> Result<Interface, Error> {
        let missing = || Error::new("on", Some(w.on.mark), ErrorKind::Missing("workflow_call"));
        let node = match &w.on.kind {
            Kind::Map(_) => w.on.get("workflow_call").ok_or_else(missing)?,
            _ if w.events().iter().any(|e| e == "workflow_call") => return Ok(Interface::default()),
            _ => return Err(missing()),
        };
        let path = "on.workflow_call";
        let mut out = Interface::default();
        if let Some(inputs) = node.get("inputs") {
            for (key, spec) in map_of(inputs, &format!("{path}.inputs"))? {
                out.inputs.push(input(&key.name, spec, &format!("{path}.inputs.{}", key.name))?);
            }
        }
        if let Some(secrets) = node.get("secrets") {
            for (key, spec) in map_of(secrets, &format!("{path}.secrets"))? {
                let here = format!("{path}.secrets.{}", key.name);
                map_of(spec, &here)?;
                out.secrets.push((key.name.clone(), flag(spec.get("required"), &format!("{here}.required"))?));
            }
        }
        if let Some(outputs) = node.get("outputs") {
            for (key, spec) in map_of(outputs, &format!("{path}.outputs"))? {
                let here = format!("{path}.outputs.{}", key.name);
                map_of(spec, &here)?;
                let value = spec.get("value").ok_or(Error::new(&here, Some(spec.mark), ErrorKind::Missing("value")))?;
                let Kind::String(text) = &value.kind else {
                    return Err(kind_error(value, &format!("{here}.value"), "a template"));
                };
                out.outputs.push((key.name.clone(), text.clone()));
            }
        }
        Ok(out)
    }

    /// The `inputs` of the called jobs from the `with:` of a call, evaluated:
    /// each key must be an input, each value of its type; a required input
    /// must be there; another takes its default, or the zero of its type.
    pub(crate) fn inputs(&self, job: &Job, with: Option<&Value>) -> Result<Value, Error> {
        let path = format!("jobs.{}.with", job.id);
        let mark = Some(job.with.as_ref().map_or(job.mark, |n| n.mark));
        let fail = |name: &str, why: String| Error::new(&path, mark, ErrorKind::Input { name: name.into(), why });
        let empty = Object::new();
        let given = match with {
            None | Some(Value::Null) => &empty,
            Some(Value::Object(o)) => o,
            Some(other) => return Err(fail("", format!("`with` must be a mapping, not the {}", other.kind()))),
        };
        for (name, _) in given.iter() {
            if !self.inputs.iter().any(|i| i.name.eq_ignore_ascii_case(name)) {
                return Err(fail(name, "is not an input of the called workflow".into()));
            }
        }
        let mut out = Object::new();
        for input in &self.inputs {
            let value = match given.get(&input.name) {
                Some(Value::Null) | None if input.required => return Err(fail(&input.name, "is required".into())),
                Some(Value::Null) | None => input.default.clone().unwrap_or_else(|| input.kind.zero()),
                Some(value) => input.kind.take(value).ok_or_else(|| {
                    let got = format!("{} {}", value.kind(), value.to_json_line());
                    fail(&input.name, format!("must be a {}, not the {got}", input.kind.name()))
                })?,
            };
            out.insert(input.name.clone(), value);
        }
        Ok(out.into())
    }

    /// The secrets of the called jobs from the `secrets:` of a call:
    /// `inherit` gives the caller's own; a mapping must name declared
    /// secrets and each required one; no `secrets:` passes none.
    pub(crate) fn secrets(&self, job: &Job, from: &Secrets) -> Result<Secrets, Error> {
        let path = format!("jobs.{}.secrets", job.id);
        let mark = Some(job.secrets.as_ref().map_or(job.mark, |n| n.mark));
        let fail =
            |name: &str, why: &str| Error::new(&path, mark, ErrorKind::Secret { name: name.into(), why: why.into() });
        let mut items = Vec::new();
        if let Some(node) = &job.secrets {
            match &node.kind {
                Kind::String(s) if s == "inherit" => return Ok(from.clone()),
                Kind::Map(entries) => {
                    for (key, value) in entries {
                        if !self.secrets.iter().any(|(n, _)| n.eq_ignore_ascii_case(&key.name)) {
                            return Err(fail(&key.name, "is not a secret of the called workflow"));
                        }
                        items.push((key.name.clone(), value.text().unwrap_or_default()));
                    }
                }
                _ => return Err(kind_error(node, &path, "`inherit` or a mapping of secrets")),
            }
        }
        for (name, required) in &self.secrets {
            if *required && !items.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)) {
                return Err(fail(name, "is required"));
            }
        }
        Ok(Secrets::Passed { from: Box::new(from.clone()), items })
    }

    /// The outputs of a called workflow when each of its jobs ended:
    /// `contexts` has `jobs`, `github`, `vars` and `inputs`.
    pub(crate) fn outputs(&self, contexts: &Contexts) -> Result<Vec<(String, String)>, Error> {
        let mut out = Vec::with_capacity(self.outputs.len());
        for (name, text) in &self.outputs {
            let path = format!("on.workflow_call.outputs.{name}.value");
            let fail = |e| Error::new(&path, None, ErrorKind::Expression(e));
            let value = Template::parse(text).and_then(|t| t.evaluate(contexts)).map_err(fail)?;
            out.push((name.clone(), value.to_text()));
        }
        Ok(out)
    }
}

/// One input of `on.workflow_call.inputs`.
fn input(name: &str, spec: &Node, path: &str) -> Result<Input, Error> {
    map_of(spec, path)?;
    let type_node = spec.get("type").ok_or(Error::new(path, Some(spec.mark), ErrorKind::Missing("type")))?;
    let kind = match type_node.text().as_deref() {
        Some("boolean") => Type::Boolean,
        Some("number") => Type::Number,
        Some("string") => Type::String,
        _ => return Err(kind_error(type_node, &format!("{path}.type"), "`boolean`, `number` or `string`")),
    };
    let required = flag(spec.get("required"), &format!("{path}.required"))?;
    let default = match spec.get("default") {
        None => None,
        Some(node) if node.kind == Kind::Null => None,
        Some(node) => {
            let value = kind.take(&node.to_value()).ok_or_else(|| {
                let why = format!("the default must be a {}", kind.name());
                Error::new(format!("{path}.default"), Some(node.mark), ErrorKind::Value(why))
            })?;
            Some(value)
        }
    };
    Ok(Input { name: name.to_string(), kind, required, default })
}
