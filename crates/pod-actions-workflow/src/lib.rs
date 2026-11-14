//! A GitHub Actions workflow file into a typed model, with no forge and no
//! network, and its expansion into concrete jobs. The model keeps `on` as
//! data; each job (`runs-on`, `needs`, `if`, `strategy`, `env`, `defaults`,
//! `outputs`, `container`, `services`, `steps` and the rest) and each step
//! (`run` or `uses`, `with`, `env`, `shell`, `if` and the rest). An unknown
//! key stays in the `extra` list of its place and has a [`Warning`].
//! [`Workflow::takes`] reads the filters of `on` (`types`, `branches`,
//! `tags`, `paths` and their `-ignore` forms) and tells whether the
//! workflow takes an event ([`Trigger`]), as GitHub decides it.
//!
//! The expansion ([`Run`]) evaluates the expressions of the job level with
//! `pod-actions-expr`: the condition of a job with the results of its needs,
//! its matrix (also one that `fromJSON` gives) with GitHub's `include` and
//! `exclude`, its `runs-on`, its name. A job that calls a reusable workflow
//! (`uses:`) is named, or, with a [`Called`] source of the called text
//! ([`Run::with_called`]), expanded as GitHub does it: the called jobs
//! become jobs of the run, with the inputs, the secrets and the outputs of
//! `on.workflow_call` checked. The runner evaluates the steps. The crate
//! reads no environment and does no I/O.
//!
//! ```
//! use pod_actions_workflow::{Base, Done, JobResult, Outcome, Run};
//!
//! let text = "
//! on: push
//! jobs:
//!   test:
//!     runs-on: ${{ matrix.os }}
//!     strategy:
//!       matrix:
//!         os: [ubuntu-latest, windows-latest]
//!     steps:
//!       - run: cargo test
//!   notify:
//!     needs: test
//!     if: failure()
//!     runs-on: ubuntu-latest
//!     steps:
//!       - run: echo failed
//! ";
//! let parsed = pod_actions_workflow::parse(text).unwrap();
//! assert!(parsed.warnings.is_empty());
//! let mut run = Run::new(&parsed.workflow, Base::default()).unwrap();
//! assert_eq!(run.ready(), ["test"]);
//! let Outcome::Run(cells) = run.expand("test").unwrap() else { panic!() };
//! assert_eq!(cells[1].name, "test (windows-latest)");
//! run.finish("test", Done::of(JobResult::Success)).unwrap();
//! let Outcome::Skipped(skip) = run.expand("notify").unwrap() else { panic!() };
//! assert_eq!(skip.condition, "failure()");
//! assert!(run.is_finished());
//! ```

mod call;
mod data;
mod error;
mod expand;
mod graph;
mod instance;
pub mod matrix;
mod model;
mod on;
mod pattern;
mod read;
mod run;
pub mod yaml;

pub use call::{Call, CallLimits, Called, Secrets};
pub use error::{Error, ErrorKind, Warning};
pub use expand::{Run, plan, plan_with};
pub use graph::Graph;
pub use model::{Defaults, Job, Step, Strategy, Workflow};
pub use on::{DEFAULT_PR_TYPES, Event, Filter, On, Trigger};
pub use pattern::Pattern;
pub use run::{Base, Done, Instance, JobResult, Outcome, RunsOn, Skip};
pub use yaml::{Key, Kind, Limits, Mark, Node};

/// A workflow and the warnings of its unknown keys.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub workflow: Workflow,
    pub warnings: Vec<Warning>,
}

/// Read a workflow file with the default [`Limits`].
pub fn parse(text: &str) -> Result<Parsed, Error> {
    parse_with(text, Limits::default())
}

/// Read a workflow file. The YAML, the keys of the schema, the ids, each
/// template and each `if:` must hold; the needs are checked by
/// [`Graph::new`] and [`Run::new`].
pub fn parse_with(text: &str, limits: Limits) -> Result<Parsed, Error> {
    let root = yaml::load(text, limits)?;
    let mut reader = read::Reader { warnings: Vec::new() };
    let workflow = reader.workflow(&root)?;
    Ok(Parsed { workflow, warnings: reader.warnings })
}
