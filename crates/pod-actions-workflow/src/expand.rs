//! The expansion of a workflow: each job, when its needs are done, becomes
//! its concrete jobs (one for each cell of its matrix), a skip with its
//! reason, or the calls of a reusable workflow. The expressions of the job
//! level are evaluated here with the contexts that GitHub gives at each
//! key: `if` and `strategy` with `github`, `needs`, `vars`, `inputs`;
//! `name`, `runs-on`, `timeout-minutes`, `continue-on-error` and `with`
//! also with `strategy` and `matrix`.
//!
//! A call adds the jobs of the called workflow to the run as one scope:
//! their ids are `CALLER/CALLEE`, their `needs` stay in the scope, their
//! `inputs` are those of the call. The calling job ends when each scope of
//! its cells ended, with the outputs of `on.workflow_call.outputs`.

use std::borrow::Borrow;
use std::sync::Arc;

use pod_actions_expr::{Condition, Contexts, Object, Status, Value};

use crate::call::{Call, Called, Interface, Secrets, Source};
use crate::error::{Error, ErrorKind};
use crate::graph::Graph;
use crate::instance::{expr_error, instances};
use crate::model::{Job, Workflow};
use crate::run::{Base, Done, Instance, JobResult, Outcome, Skip};

#[derive(Clone, Debug)]
enum State {
    Waiting,
    Running,
    /// A call: the scope of each cell.
    Calling(Vec<usize>),
    Done(Done),
}

/// The jobs of one workflow of the run: the top workflow, or one call.
#[derive(Clone, Debug)]
struct Scope {
    /// The called workflow; none for the top workflow.
    workflow: Option<Arc<Workflow>>,
    uses: Option<String>,
    interface: Interface,
    graph: Graph,
    /// The node of its first job.
    first: usize,
    /// The levels of workflows down to this one: 1 for the top workflow.
    depth: usize,
    /// The start of the name of each job: `CALLER / `.
    name: String,
    inputs: Value,
    secrets: Secrets,
    caller: Option<usize>,
    /// The result and the outputs of the call, when each job ended.
    ended: Option<Done>,
}

#[derive(Clone, Debug)]
struct Node {
    id: String,
    scope: usize,
    state: State,
}

/// The state of one run of a workflow: which jobs are done, with their
/// results and outputs. `W` is the workflow or a borrow of it: `&Workflow`,
/// or `Workflow` and `Arc<Workflow>` for a run that owns it (a source that
/// must be `'static`; PodUtils/podrunner@T-087).
#[derive(Clone, Debug)]
pub struct Run<W: Borrow<Workflow>> {
    workflow: W,
    base: Base,
    cancelled: bool,
    called: Option<Source>,
    scopes: Vec<Scope>,
    nodes: Vec<Node>,
    /// The different `uses` of the run, for the limit on their count.
    uses: Vec<String>,
}

/// The result of a call: a failure if a job failed; else cancelled if one
/// was cancelled; else success, also when each job was skipped.
fn call_result(results: impl IntoIterator<Item = JobResult>) -> JobResult {
    match JobResult::combine(results) {
        JobResult::Skipped => JobResult::Success,
        other => other,
    }
}

impl<W: Borrow<Workflow>> Run<W> {
    /// A run of `workflow`: the graph of its needs must hold.
    pub fn new(workflow: W, base: Base) -> Result<Run<W>, Error> {
        let graph = Graph::new(workflow.borrow())?;
        let nodes =
            workflow.borrow().jobs.iter().map(|j| Node { id: j.id.clone(), scope: 0, state: State::Waiting }).collect();
        let top = Scope {
            workflow: None,
            uses: None,
            interface: Interface::default(),
            graph,
            first: 0,
            depth: 1,
            name: String::new(),
            inputs: base.inputs.clone(),
            secrets: Secrets::Run,
            caller: None,
            ended: None,
        };
        Ok(Run { workflow, base, cancelled: false, called: None, scopes: vec![top], nodes, uses: Vec::new() })
    }

    /// Expand each job that calls a reusable workflow with the text that
    /// `called` gives. With no source, such a job stays named: an
    /// [`Outcome::Run`] with its `uses`.
    pub fn with_called(mut self, called: Arc<dyn Called + Send + Sync>) -> Run<W> {
        self.called = Some(Source(called));
        self
    }

    /// The workflow of the run.
    pub fn workflow(&self) -> &Workflow {
        self.workflow.borrow()
    }

    /// The graph of the top workflow.
    pub fn graph(&self) -> &Graph {
        &self.scopes[0].graph
    }

    /// The run is cancelled: `cancelled()` is true and `success()` false for
    /// each job that is not expanded.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    fn workflow_of(&self, s: usize) -> &Workflow {
        self.scopes[s].workflow.as_deref().unwrap_or(self.workflow.borrow())
    }

    fn job(&self, n: usize) -> &Job {
        let s = self.nodes[n].scope;
        &self.workflow_of(s).jobs[n - self.scopes[s].first]
    }

    fn index(&self, id: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    /// The node of the job `id` of the workflow of scope `s`.
    fn local(&self, s: usize, id: &str) -> Option<usize> {
        self.scopes[s].graph.index(id).map(|j| self.scopes[s].first + j)
    }

    fn needs_of(&self, n: usize) -> Vec<usize> {
        let s = self.nodes[n].scope;
        self.scopes[s].graph.needs(&self.job(n).id).into_iter().filter_map(|id| self.local(s, id)).collect()
    }

    /// The jobs that wait and whose needs are done: the top workflow in its
    /// topological order, then each call in the order of the calls. The ids
    /// are owned, so that the caller can expand each one.
    pub fn ready(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (s, scope) in self.scopes.iter().enumerate() {
            for id in scope.graph.order() {
                let Some(n) = self.local(s, id) else { continue };
                if matches!(self.nodes[n].state, State::Waiting) && self.needs_done(n) {
                    out.push(self.nodes[n].id.clone());
                }
            }
        }
        out
    }

    /// Each job is done.
    pub fn is_finished(&self) -> bool {
        self.nodes.iter().all(|n| matches!(n.state, State::Done(_)))
    }

    /// The result of a job that is done.
    pub fn result(&self, job: &str) -> Option<JobResult> {
        self.done(self.index(job)?).map(|d| d.result)
    }

    fn done(&self, n: usize) -> Option<&Done> {
        match &self.nodes.get(n)?.state {
            State::Done(done) => Some(done),
            _ => None,
        }
    }

    fn needs_done(&self, n: usize) -> bool {
        self.needs_of(n).iter().all(|&m| self.done(m).is_some())
    }

    /// Record the end of a job that [`Run::expand`] gave as `Run`. A job
    /// that calls a workflow ends when its called jobs end.
    pub fn finish(&mut self, job: &str, done: Done) -> Result<(), Error> {
        let n = self.index(job).ok_or_else(|| usage(format!("no job `{job}`")))?;
        match self.nodes[n].state {
            State::Running => self.end(n, done),
            State::Calling(_) => {
                Err(usage(format!("the job `{job}` calls a workflow: it ends when its called jobs end")))
            }
            _ => Err(usage(format!("the job `{job}` is not running"))),
        }
    }

    /// The end of a job. The end of the last job of a scope ends the scope;
    /// the end of the last scope of a call ends the calling job, with the
    /// outputs of the scope that ended last, as in GitHub.
    fn end(&mut self, mut n: usize, mut done: Done) -> Result<(), Error> {
        loop {
            self.nodes[n].state = State::Done(done);
            let s = self.nodes[n].scope;
            let Some(caller) = self.scopes[s].caller else { return Ok(()) };
            let first = self.scopes[s].first;
            if !(first..first + self.workflow_of(s).jobs.len()).all(|m| self.done(m).is_some()) {
                return Ok(());
            }
            let ended = self.end_scope(s).map_err(|e| self.in_scope(s, e))?;
            let outputs = ended.outputs.clone();
            self.scopes[s].ended = Some(ended);
            let State::Calling(cells) = &self.nodes[caller].state else {
                return Err(usage(format!("the job `{}` does not call", self.nodes[caller].id)));
            };
            let results: Option<Vec<JobResult>> =
                cells.iter().map(|&c| self.scopes[c].ended.as_ref().map(|d| d.result)).collect();
            let Some(results) = results else { return Ok(()) };
            done = Done { result: call_result(results), outputs };
            n = caller;
        }
    }

    /// The result and the outputs of a scope whose jobs all ended.
    fn end_scope(&self, s: usize) -> Result<Done, Error> {
        let scope = &self.scopes[s];
        let mut jobs = Object::new();
        let mut results = Vec::new();
        for (j, job) in self.workflow_of(s).jobs.iter().enumerate() {
            let Some(done) = self.done(scope.first + j) else { continue };
            results.push(done.result);
            jobs.insert(job.id.clone(), done_value(done));
        }
        let mut contexts = Contexts::new();
        contexts.limits = self.base.limits;
        contexts
            .insert("github", self.base.github.clone())
            .insert("vars", self.base.vars.clone())
            .insert("inputs", scope.inputs.clone())
            .insert("jobs", jobs.into());
        Ok(Done { result: call_result(results), outputs: scope.interface.outputs(&contexts)? })
    }

    /// An error of a job of a called workflow names its `uses`.
    fn in_scope(&self, s: usize, e: Error) -> Error {
        match &self.scopes[s].uses {
            Some(uses) => Error::new("", None, ErrorKind::InCalled { uses: uses.clone(), error: Box::new(e) }),
            None => e,
        }
    }

    /// Expand a job whose needs are done: evaluate its condition; a false
    /// condition makes it `skipped` at once. Else its matrix gives its
    /// concrete jobs, and it is running until [`Run::finish`]; or, with a
    /// [`Called`] source, it calls its workflow.
    pub fn expand(&mut self, job: &str) -> Result<Outcome, Error> {
        let n = self.index(job).ok_or_else(|| usage(format!("no job `{job}`")))?;
        if !matches!(self.nodes[n].state, State::Waiting) {
            return Err(usage(format!("the job `{job}` is expanded already")));
        }
        if !self.needs_done(n) {
            return Err(usage(format!("the needs of the job `{job}` are not done")));
        }
        let s = self.nodes[n].scope;
        self.expand_node(n).map_err(|e| self.in_scope(s, e))
    }

    fn expand_node(&mut self, n: usize) -> Result<Outcome, Error> {
        let s = self.nodes[n].scope;
        let job = self.job(n).clone();
        let ancestors: Vec<(String, JobResult)> = self.scopes[s]
            .graph
            .ancestors(&job.id)
            .into_iter()
            .filter_map(|id| self.local(s, id))
            .map(|m| (self.nodes[m].id.clone(), self.done(m).map_or(JobResult::Skipped, |d| d.result)))
            .collect();
        let mut contexts = Contexts::new();
        contexts.limits = self.base.limits;
        contexts.status = Status {
            success: !self.cancelled && ancestors.iter().all(|(_, r)| *r == JobResult::Success),
            failure: ancestors.iter().any(|(_, r)| *r == JobResult::Failure),
            cancelled: self.cancelled,
        };
        let needs = self.needs_context(n);
        contexts
            .insert("github", self.base.github.clone())
            .insert("needs", needs.clone())
            .insert("vars", self.base.vars.clone())
            .insert("inputs", self.scopes[s].inputs.clone());
        let path = format!("jobs.{}.if", job.id);
        let condition = Condition::parse(job.condition.as_deref().unwrap_or_default())
            .map_err(|e| expr_error(path.clone(), None, &job, e))?;
        if !condition.evaluate(&contexts).map_err(|e| expr_error(path, None, &job, e))? {
            let id = self.nodes[n].id.clone();
            let skip = Skip { job: id, condition: condition.text().into(), ancestors, cancelled: self.cancelled };
            self.end(n, Done::of(JobResult::Skipped))?;
            return Ok(Outcome::Skipped(skip));
        }
        let mut cells = instances(&job, &mut contexts, needs)?;
        let scope = &self.scopes[s];
        for cell in &mut cells {
            cell.job = self.nodes[n].id.clone();
            cell.name = format!("{}{}", scope.name, cell.name);
            cell.secrets = scope.secrets.clone();
        }
        if let (Some(uses), Some(source)) = (&job.uses, self.called.clone()) {
            return self.call(n, &job, uses, &source, cells, contexts);
        }
        self.nodes[n].state = State::Running;
        Ok(Outcome::Run(cells))
    }

    /// `needs.<id>.result` and `needs.<id>.outputs` of each direct need, by
    /// the ids of its own workflow.
    fn needs_context(&self, n: usize) -> Value {
        let mut needs = Object::new();
        for m in self.needs_of(n) {
            let Some(done) = self.done(m) else { continue };
            needs.insert(self.job(m).id.clone(), done_value(done));
        }
        needs.into()
    }

    /// A job that calls a workflow: check the limits, read the called
    /// workflow, check `with` and `secrets` for each cell, then add one
    /// scope for each cell. Nothing changes when a check fails.
    fn call(
        &mut self,
        n: usize,
        job: &Job,
        uses: &str,
        source: &Source,
        cells: Vec<Instance>,
        mut contexts: Contexts,
    ) -> Result<Outcome, Error> {
        let s = self.nodes[n].scope;
        let path = format!("jobs.{}.uses", job.id);
        let at = |kind| Error::new(&path, Some(job.mark), kind);
        let limits = self.base.calls;
        let depth = self.scopes[s].depth + 1;
        if depth > limits.levels {
            return Err(at(ErrorKind::CallLimit { what: "levels of workflows", max: limits.levels }));
        }
        let new = !self.uses.iter().any(|u| u == uses);
        if new && self.uses.len() >= limits.workflows {
            return Err(at(ErrorKind::CallLimit { what: "different called workflows", max: limits.workflows }));
        }
        let in_called = |e: Error| at(ErrorKind::InCalled { uses: uses.to_string(), error: Box::new(e) });
        let text = source
            .0
            .text(uses, self.scopes[s].uses.as_deref())
            .map_err(|why| at(ErrorKind::CallText { uses: uses.to_string(), why }))?;
        let parsed = crate::parse(&text).map_err(in_called)?;
        let interface = Interface::read(&parsed.workflow).map_err(in_called)?;
        let graph = Graph::new(&parsed.workflow).map_err(in_called)?;
        let workflow = Arc::new(parsed.workflow);
        if self.nodes.len() + cells.len() * workflow.jobs.len() > limits.jobs {
            return Err(at(ErrorKind::CallLimit { what: "jobs", max: limits.jobs }));
        }
        let mut checked = Vec::with_capacity(cells.len());
        for cell in &cells {
            contexts.insert("strategy", cell.strategy.clone()).insert("matrix", cell.matrix.clone());
            let with = match &job.with {
                Some(node) => Some(node.evaluate(&contexts, &format!("jobs.{}.with", job.id))?),
                None => None,
            };
            checked.push((interface.inputs(job, with.as_ref())?, interface.secrets(job, &self.scopes[s].secrets)?));
        }
        if new {
            self.uses.push(uses.to_string());
        }
        let mut calls = Vec::with_capacity(cells.len());
        let mut scopes = Vec::with_capacity(cells.len());
        for (i, (cell, (inputs, secrets))) in cells.into_iter().zip(checked).enumerate() {
            let prefix = if job.has_matrix() { format!("{}[{i}]/", cell.job) } else { format!("{}/", cell.job) };
            let ids: Vec<String> = workflow.jobs.iter().map(|j| format!("{prefix}{}", j.id)).collect();
            let scope = self.scopes.len();
            let first = self.nodes.len();
            self.nodes.extend(ids.iter().map(|id| Node { id: id.clone(), scope, state: State::Waiting }));
            self.scopes.push(Scope {
                workflow: Some(Arc::clone(&workflow)),
                uses: Some(uses.to_string()),
                interface: interface.clone(),
                graph: graph.clone(),
                first,
                depth,
                name: format!("{} / ", cell.name),
                inputs: inputs.clone(),
                secrets: secrets.clone(),
                caller: Some(n),
                ended: None,
            });
            scopes.push(scope);
            let (job, name, matrix) = (cell.job, cell.name, cell.matrix);
            calls.push(Call { job, name, uses: uses.to_string(), matrix, inputs, secrets, jobs: ids });
        }
        self.nodes[n].state = State::Calling(scopes);
        Ok(Outcome::Called(calls))
    }
}

fn done_value(done: &Done) -> Value {
    let outputs = Value::object(done.outputs.iter().map(|(k, v)| (k.clone(), Value::from(v.as_str()))));
    Value::object([("result", Value::from(done.result.as_str())), ("outputs", outputs)])
}

fn usage(why: String) -> Error {
    Error::new("", None, ErrorKind::Usage(why))
}

/// A dry run: each job in the topological order, and each job that runs
/// ends with `assume` and no output. A matrix or a condition that reads an
/// output sees null. A job that calls a workflow stays named.
pub fn plan(workflow: &Workflow, base: Base, assume: JobResult) -> Result<Vec<(String, Outcome)>, Error> {
    drive(Run::new(workflow, base)?, assume)
}

/// A dry run that expands each call with the text that `called` gives: the
/// called jobs follow their caller.
pub fn plan_with(
    workflow: &Workflow,
    base: Base,
    assume: JobResult,
    called: Arc<dyn Called + Send + Sync>,
) -> Result<Vec<(String, Outcome)>, Error> {
    drive(Run::new(workflow, base)?.with_called(called), assume)
}

fn drive<W: Borrow<Workflow>>(mut run: Run<W>, assume: JobResult) -> Result<Vec<(String, Outcome)>, Error> {
    let mut out = Vec::new();
    while let Some(id) = run.ready().into_iter().next() {
        let outcome = run.expand(&id)?;
        if matches!(outcome, Outcome::Run(_)) {
            run.finish(&id, Done::of(assume))?;
        }
        out.push((id, outcome));
    }
    Ok(out)
}
