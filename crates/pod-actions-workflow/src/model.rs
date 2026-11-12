//! The model of a workflow file. A value that GitHub evaluates later (an
//! expression, a mapping of the runner) stays a [`Node`]; a text that can
//! hold `${{ }}` stays the text of its template. Each unknown key stays in
//! `extra` and has a warning.

use crate::yaml::{Key, Kind, Mark, Node};

/// A workflow file.
#[derive(Clone, Debug, PartialEq)]
pub struct Workflow {
    pub name: Option<String>,
    /// `run-name`: a template (contexts `github`, `inputs`, `vars`).
    pub run_name: Option<String>,
    /// `on`, as data.
    pub on: Node,
    pub env: Option<Node>,
    pub defaults: Option<Defaults>,
    pub permissions: Option<Node>,
    pub concurrency: Option<Node>,
    /// The jobs in the order of the file.
    pub jobs: Vec<Job>,
    pub extra: Vec<(Key, Node)>,
}

/// `defaults.run`, at the top level or of a job.
#[derive(Clone, Debug, PartialEq)]
pub struct Defaults {
    pub shell: Option<String>,
    pub working_directory: Option<String>,
    pub mark: Mark,
    /// The unknown keys of `defaults` and of `defaults.run`.
    pub extra: Vec<(Key, Node)>,
}

/// A job.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    /// The key of the job: a letter or `_`, then letters, digits, `-` and
    /// `_`.
    pub id: String,
    pub mark: Mark,
    /// `name`: a template (with `matrix`).
    pub name: Option<String>,
    /// `runs-on`: a label, a list of labels, or `{group, labels}`; each can
    /// be an expression.
    pub runs_on: Option<Node>,
    /// The ids of `needs`, in their order.
    pub needs: Vec<String>,
    /// The `if:` as text: a condition of `pod_actions_expr::Condition`.
    pub condition: Option<String>,
    pub strategy: Option<Strategy>,
    pub env: Option<Node>,
    pub defaults: Option<Defaults>,
    pub timeout_minutes: Option<Node>,
    pub cancel_timeout_minutes: Option<Node>,
    pub continue_on_error: Option<Node>,
    pub container: Option<Node>,
    pub services: Option<Node>,
    /// `outputs`: each name with its template, in their order.
    pub outputs: Vec<(String, String)>,
    pub environment: Option<Node>,
    pub permissions: Option<Node>,
    pub concurrency: Option<Node>,
    pub snapshot: Option<Node>,
    /// A reusable workflow (`uses:` of a job): named, not expanded.
    pub uses: Option<String>,
    /// `with` and `secrets` of a reusable workflow.
    pub with: Option<Node>,
    pub secrets: Option<Node>,
    pub steps: Vec<Step>,
    pub extra: Vec<(Key, Node)>,
}

/// `strategy` of a job.
#[derive(Clone, Debug, PartialEq)]
pub struct Strategy {
    /// A mapping of vectors with `include` and `exclude`, or an expression.
    pub matrix: Option<Node>,
    pub fail_fast: Option<Node>,
    pub max_parallel: Option<Node>,
    pub mark: Mark,
    pub extra: Vec<(Key, Node)>,
}

/// A step. The runner evaluates its expressions: this crate keeps them.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub id: Option<String>,
    pub name: Option<String>,
    /// The `if:` as text.
    pub condition: Option<String>,
    /// One of `run` and `uses`.
    pub run: Option<String>,
    pub uses: Option<String>,
    pub with: Option<Node>,
    pub env: Option<Node>,
    pub shell: Option<String>,
    pub working_directory: Option<String>,
    pub continue_on_error: Option<Node>,
    pub timeout_minutes: Option<Node>,
    pub mark: Mark,
    pub extra: Vec<(Key, Node)>,
}

impl Workflow {
    /// The job of `id`.
    pub fn job(&self, id: &str) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id == id)
    }

    /// The names of the events of `on`: a name, a list of names, or the keys
    /// of a mapping.
    pub fn events(&self) -> Vec<String> {
        match &self.on.kind {
            Kind::Seq(items) => items.iter().filter_map(Node::text).collect(),
            Kind::Map(entries) => entries.iter().map(|(k, _)| k.name.clone()).collect(),
            _ => self.on.text().into_iter().collect(),
        }
    }
}

impl Job {
    /// The job calls a reusable workflow.
    pub fn is_reusable(&self) -> bool {
        self.uses.is_some()
    }

    /// The job has a matrix.
    pub fn has_matrix(&self) -> bool {
        self.strategy.as_ref().is_some_and(|s| s.matrix.is_some())
    }
}
