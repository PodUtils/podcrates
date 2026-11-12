//! The expansion of a workflow: each job, when its needs are done, becomes
//! its concrete jobs (one for each cell of its matrix) or a skip with its
//! reason. The expressions of the job level are evaluated here with the
//! contexts that GitHub gives at each key: `if` and `strategy` with
//! `github`, `needs`, `vars`, `inputs`; `name`, `runs-on`,
//! `timeout-minutes` and `continue-on-error` also with `strategy` and
//! `matrix`.

use std::borrow::Borrow;

use pod_actions_expr::{Condition, Contexts, Object, Status, Template, Value};

use crate::error::{Error, ErrorKind};
use crate::graph::Graph;
use crate::matrix;
use crate::model::{Job, Workflow};
use crate::run::{Base, Done, Instance, JobResult, Outcome, RunsOn, Skip};
use crate::yaml::Node;

#[derive(Clone, Debug)]
enum State {
    Waiting,
    Running,
    Done(Done),
}

/// The state of one run of a workflow: which jobs are done, with their
/// results and outputs. `W` is the workflow or a borrow of it: `&Workflow`,
/// or `Workflow` and `Arc<Workflow>` for a run that owns it (a source that
/// must be `'static`; PodUtils/podrunner@T-087).
#[derive(Clone, Debug)]
pub struct Run<W: Borrow<Workflow>> {
    workflow: W,
    graph: Graph,
    base: Base,
    cancelled: bool,
    state: Vec<State>,
}

fn expr_error(path: String, node: Option<&Node>, job: &Job, e: pod_actions_expr::Error) -> Error {
    Error::new(path, Some(node.map_or(job.mark, |n| n.mark)), ErrorKind::Expression(e))
}

fn value_error(path: String, node: &Node, why: String) -> Error {
    Error::new(path, Some(node.mark), ErrorKind::Value(why))
}

impl<W: Borrow<Workflow>> Run<W> {
    /// A run of `workflow`: the graph of its needs must hold.
    pub fn new(workflow: W, base: Base) -> Result<Run<W>, Error> {
        let graph = Graph::new(workflow.borrow())?;
        let state = vec![State::Waiting; workflow.borrow().jobs.len()];
        Ok(Run { workflow, graph, base, cancelled: false, state })
    }

    /// The workflow of the run.
    pub fn workflow(&self) -> &Workflow {
        self.workflow.borrow()
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// The run is cancelled: `cancelled()` is true and `success()` false for
    /// each job that is not expanded.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// The jobs that wait and whose needs are done, in the topological
    /// order. The ids are owned, so that the caller can expand each one.
    pub fn ready(&self) -> Vec<String> {
        let workflow = self.workflow.borrow();
        self.graph
            .order()
            .filter_map(|id| self.graph.index(id))
            .filter(|&j| matches!(self.state[j], State::Waiting) && self.needs_done(j))
            .map(|j| workflow.jobs[j].id.clone())
            .collect()
    }

    /// Each job is done.
    pub fn is_finished(&self) -> bool {
        self.state.iter().all(|s| matches!(s, State::Done(_)))
    }

    /// The result of a job that is done.
    pub fn result(&self, job: &str) -> Option<JobResult> {
        match self.state.get(self.graph.index(job)?)? {
            State::Done(done) => Some(done.result),
            _ => None,
        }
    }

    fn needs_done(&self, j: usize) -> bool {
        self.graph.needs(&self.workflow.borrow().jobs[j].id).iter().all(|n| self.result(n).is_some())
    }

    fn done(&self, job: &str) -> Option<&Done> {
        match self.state.get(self.graph.index(job)?)? {
            State::Done(done) => Some(done),
            _ => None,
        }
    }

    /// Record the end of a job that [`Run::expand`] gave as `Run`.
    pub fn finish(&mut self, job: &str, done: Done) -> Result<(), Error> {
        let j = self.graph.index(job).ok_or_else(|| usage(format!("no job `{job}`")))?;
        if !matches!(self.state[j], State::Running) {
            return Err(usage(format!("the job `{job}` is not running")));
        }
        self.state[j] = State::Done(done);
        Ok(())
    }

    /// Expand a job whose needs are done: evaluate its condition; a false
    /// condition makes it `skipped` at once. Else its matrix gives its
    /// concrete jobs, and it is running until [`Run::finish`].
    pub fn expand(&mut self, job: &str) -> Result<Outcome, Error> {
        let j = self.graph.index(job).ok_or_else(|| usage(format!("no job `{job}`")))?;
        if !matches!(self.state[j], State::Waiting) {
            return Err(usage(format!("the job `{job}` is expanded already")));
        }
        if !self.needs_done(j) {
            return Err(usage(format!("the needs of the job `{job}` are not done")));
        }
        let job = &self.workflow.borrow().jobs[j];
        let ancestors: Vec<(String, JobResult)> = self
            .graph
            .ancestors(&job.id)
            .into_iter()
            .map(|id| (id.to_string(), self.result(id).unwrap_or(JobResult::Skipped)))
            .collect();
        let mut contexts = Contexts::new();
        contexts.limits = self.base.limits;
        contexts.status = Status {
            success: !self.cancelled && ancestors.iter().all(|(_, r)| *r == JobResult::Success),
            failure: ancestors.iter().any(|(_, r)| *r == JobResult::Failure),
            cancelled: self.cancelled,
        };
        let needs = self.needs_context(job);
        contexts
            .insert("github", self.base.github.clone())
            .insert("needs", needs.clone())
            .insert("vars", self.base.vars.clone())
            .insert("inputs", self.base.inputs.clone());
        let path = format!("jobs.{}.if", job.id);
        let condition = Condition::parse(job.condition.as_deref().unwrap_or_default())
            .map_err(|e| expr_error(path.clone(), None, job, e))?;
        if !condition.evaluate(&contexts).map_err(|e| expr_error(path, None, job, e))? {
            self.state[j] = State::Done(Done::of(JobResult::Skipped));
            let skip =
                Skip { job: job.id.clone(), condition: condition.text().into(), ancestors, cancelled: self.cancelled };
            return Ok(Outcome::Skipped(skip));
        }
        let instances = instances(job, &mut contexts, needs)?;
        self.state[j] = State::Running;
        Ok(Outcome::Run(instances))
    }

    /// `needs.<id>.result` and `needs.<id>.outputs` of each direct need.
    fn needs_context(&self, job: &Job) -> Value {
        let mut needs = Object::new();
        for id in self.graph.needs(&job.id) {
            let Some(done) = self.done(id) else { continue };
            let outputs = Value::object(done.outputs.iter().map(|(k, v)| (k.clone(), Value::from(v.as_str()))));
            needs.insert(id, Value::object([("result", Value::from(done.result.as_str())), ("outputs", outputs)]));
        }
        needs.into()
    }
}

fn usage(why: String) -> Error {
    Error::new("", None, ErrorKind::Usage(why))
}

/// The concrete jobs of a job whose condition is true.
fn instances(job: &Job, contexts: &mut Contexts, needs: Value) -> Result<Vec<Instance>, Error> {
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

/// A dry run: each job in the topological order, and each job that runs
/// ends with `assume` and no output. A matrix or a condition that reads an
/// output sees null.
pub fn plan(workflow: &Workflow, base: Base, assume: JobResult) -> Result<Vec<(String, Outcome)>, Error> {
    let mut run = Run::new(workflow, base)?;
    let order: Vec<String> = run.graph().order().map(str::to_string).collect();
    let mut out = Vec::with_capacity(order.len());
    for id in order {
        let outcome = run.expand(&id)?;
        if matches!(outcome, Outcome::Run(_)) {
            run.finish(&id, Done::of(assume))?;
        }
        out.push((id, outcome));
    }
    Ok(out)
}
