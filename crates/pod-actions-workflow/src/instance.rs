//! The concrete jobs of one job: one for each cell of its matrix, with its
//! name, `runs-on`, `timeout-minutes` and `continue-on-error` evaluated.

use pod_actions_expr::{Contexts, Object, Template, Value};

use crate::call::Secrets;
use crate::error::{Error, ErrorKind};
use crate::matrix;
use crate::model::Job;
use crate::run::{Instance, RunsOn};
use crate::yaml::Node;

pub(crate) fn expr_error(path: String, node: Option<&Node>, job: &Job, e: pod_actions_expr::Error) -> Error {
    Error::new(path, Some(node.map_or(job.mark, |n| n.mark)), ErrorKind::Expression(e))
}

fn value_error(path: String, node: &Node, why: String) -> Error {
    Error::new(path, Some(node.mark), ErrorKind::Value(why))
}

/// The concrete jobs of a job whose condition is true.
pub(crate) fn instances(job: &Job, contexts: &mut Contexts, needs: Value) -> Result<Vec<Instance>, Error> {
    let path = format!("jobs.{}.strategy", job.id);
    let strategy = job.strategy.as_ref();
    let eval = |node, key: &str, contexts: &Contexts| evaluate_at(node, &format!("{path}.{key}"), contexts);
    let cells: Vec<Option<Object>> = match eval(strategy.and_then(|s| s.matrix.as_ref()), "matrix", contexts)? {
        Some((value, node)) => matrix::cells(&value)
            .map_err(|why| Error::new(format!("{path}.matrix"), Some(node.mark), ErrorKind::Matrix(why)))?
            .into_iter()
            .map(Some)
            .collect(),
        None => vec![None],
    };
    let fail_fast = match eval(strategy.and_then(|s| s.fail_fast.as_ref()), "fail-fast", contexts)? {
        Some((Value::Bool(b), _)) => b,
        Some((other, node)) => {
            return Err(value_error(format!("{path}.fail-fast"), node, kind_of("a boolean", &other)));
        }
        None => true,
    };
    let total = cells.len() as f64;
    let max_parallel = match eval(strategy.and_then(|s| s.max_parallel.as_ref()), "max-parallel", contexts)? {
        Some((Value::Number(n), _)) if n >= 1.0 && n.fract() == 0.0 => n,
        Some((other, node)) => {
            let why = kind_of("a whole number of 1 or more", &other);
            return Err(value_error(format!("{path}.max-parallel"), node, why));
        }
        None => total,
    };
    let mut out = Vec::with_capacity(cells.len());
    for (index, cell) in cells.into_iter().enumerate() {
        let strategy = Value::object([
            ("fail-fast", Value::Bool(fail_fast)),
            ("job-index", Value::from(index as f64)),
            ("job-total", Value::from(total)),
            ("max-parallel", Value::from(max_parallel)),
        ]);
        let matrix = cell.clone().map_or(Value::Null, Value::from);
        contexts.insert("strategy", strategy.clone()).insert("matrix", matrix.clone());
        out.push(Instance {
            job: job.id.clone(),
            name: name(job, cell.as_ref(), contexts)?,
            matrix,
            strategy,
            needs: needs.clone(),
            runs_on: match (&job.uses, &job.runs_on) {
                (None, Some(node)) => Some(runs_on(job, node, contexts)?),
                _ => None,
            },
            uses: job.uses.clone(),
            timeout_minutes: number(job, job.timeout_minutes.as_ref(), "timeout-minutes", contexts)?.unwrap_or(360.0),
            continue_on_error: boolean(job, job.continue_on_error.as_ref(), "continue-on-error", contexts)?,
            inputs: contexts.get("inputs").cloned().unwrap_or_default(),
            secrets: Secrets::Run,
        });
    }
    Ok(out)
}

/// The value of a node that may be there, with the node.
fn evaluate_at<'a>(
    node: Option<&'a Node>,
    path: &str,
    contexts: &Contexts,
) -> Result<Option<(Value, &'a Node)>, Error> {
    let Some(node) = node else { return Ok(None) };
    Ok(Some((node.evaluate(contexts, path)?, node)))
}

fn kind_of(want: &str, got: &Value) -> String {
    format!("the value must be {want}, not the {} {}", got.kind(), got.to_json_line())
}

/// The name to show: `name` evaluated, or the id; with a matrix and a name
/// that does not read `matrix`, the values of the cell in parentheses.
fn name(job: &Job, cell: Option<&Object>, contexts: &Contexts) -> Result<String, Error> {
    let path = format!("jobs.{}.name", job.id);
    let (base, reads_matrix) = match &job.name {
        None => (job.id.clone(), false),
        Some(text) => {
            let template = Template::parse(text).map_err(|e| expr_error(path.clone(), None, job, e))?;
            let reads = template.expressions().any(|e| e.contexts().iter().any(|c| c.eq_ignore_ascii_case("matrix")));
            (template.evaluate(contexts).map_err(|e| expr_error(path, None, job, e))?.to_text(), reads)
        }
    };
    match cell {
        Some(cell) if !reads_matrix => {
            let values: Vec<String> =
                cell.values().map(|v| if v.is_primitive() { v.to_text() } else { v.to_json_line() }).collect();
            Ok(format!("{base} ({})", values.join(", ")))
        }
        _ => Ok(base),
    }
}

fn runs_on(job: &Job, node: &Node, contexts: &Contexts) -> Result<RunsOn, Error> {
    let path = format!("jobs.{}.runs-on", job.id);
    let value = node.evaluate(contexts, &path)?;
    let bad = || value_error(path.clone(), node, kind_of("a label, a list of labels or {group, labels}", &value));
    let labels = |v: &Value| -> Option<Vec<String>> {
        match v {
            Value::String(s) => Some(vec![s.clone()]),
            Value::Array(items) => items.iter().map(|i| i.as_str().map(str::to_string)).collect(),
            _ => None,
        }
    };
    let out = match &value {
        Value::Object(o) => {
            let group = match o.get("group") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) => Some(s.clone()),
                Some(_) => return Err(bad()),
            };
            let labels = match o.get("labels") {
                None | Some(Value::Null) => Vec::new(),
                Some(v) => labels(v).ok_or_else(bad)?,
            };
            RunsOn { group, labels }
        }
        other => RunsOn { group: None, labels: labels(other).ok_or_else(bad)? },
    };
    if out.group.is_none() && out.labels.is_empty() {
        return Err(bad());
    }
    Ok(out)
}

fn number(job: &Job, node: Option<&Node>, key: &str, contexts: &Contexts) -> Result<Option<f64>, Error> {
    let Some(node) = node else { return Ok(None) };
    let path = format!("jobs.{}.{key}", job.id);
    match node.evaluate(contexts, &path)? {
        Value::Number(n) if n > 0.0 => Ok(Some(n)),
        other => Err(value_error(path, node, kind_of("a number more than 0", &other))),
    }
}

fn boolean(job: &Job, node: Option<&Node>, key: &str, contexts: &Contexts) -> Result<bool, Error> {
    let Some(node) = node else { return Ok(false) };
    let path = format!("jobs.{}.{key}", job.id);
    match node.evaluate(contexts, &path)? {
        Value::Bool(b) => Ok(b),
        other => Err(value_error(path, node, kind_of("a boolean", &other))),
    }
}
