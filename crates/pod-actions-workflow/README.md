# pod-actions-workflow

A GitHub Actions workflow file into a typed model, with no forge and no
network, and its expansion into concrete jobs: one for each cell of a
matrix, in the order of `needs`, with each skipped job and its reason.
podrunner asked for it for its local mode (`podrunner run workflow.yml`,
PodUtils/podrunner@T-046): a `local` job source compiles the file into job
offers, and the official runner runs each job and evaluates its steps.
T-251 made it, as new code from the documents of GitHub ("Workflow syntax
for GitHub Actions", "Running variations of jobs in a workflow", "Using
jobs in a workflow") and the workflow schema of the runner. The
expressions are `pod-actions-expr`'s. The crate reads no environment and
does no I/O.

## Use

```rust
use pod_actions_workflow::{Base, Done, JobResult, Outcome, Run};

let parsed = pod_actions_workflow::parse(&text)?;     // YAML, schema, ids, templates
for warning in &parsed.warnings { /* an unknown key, kept as data */ }
let base = Base { github, vars, inputs, ..Base::default() };
let mut run = Run::new(&parsed.workflow, base)?;      // needs: missing, cycle
while !run.is_finished() {
    for id in run.ready() {                           // needs done, topological order
        match run.expand(&id)? {
            Outcome::Skipped(skip) => { /* `skip` says why; the job is done as skipped */ }
            Outcome::Run(cells) => { /* one job offer for each cell */ }
        }
    }
    // when a job ends (all its cells): its result and its outputs
    run.finish("build", Done { result: JobResult::Success, outputs })?;
}
```

`plan(&workflow, base, JobResult::Success)` is a dry run: each job in
order, each job that runs ends with the given result and no output.

## API

| Item | What |
| --- | --- |
| `parse`, `parse_with`, `Parsed`, `Limits` | the file into a `Workflow` and its warnings; the limits of the file (1 MiB, 100,000 nodes with the copies of aliases, 64 levels) |
| `Workflow` | `name`, `run_name`, `on` (data; `events()`), `env`, `defaults`, `permissions`, `concurrency`, `jobs` (in the order of the file; `job(id)`), `extra` |
| `Job` | `id`, `mark`, `name`, `runs_on`, `needs`, `condition` (the `if:`), `strategy`, `env`, `defaults`, `timeout_minutes`, `cancel_timeout_minutes`, `continue_on_error`, `container`, `services`, `outputs`, `environment`, `permissions`, `concurrency`, `snapshot`, `uses` (a reusable workflow: named, not expanded), `with`, `secrets`, `steps`, `extra` |
| `Strategy`, `Defaults` | `matrix`, `fail_fast`, `max_parallel`; `defaults.run` (`shell`, `working_directory`) |
| `Step` | `id`, `name`, `condition`, `run` or `uses`, `with`, `env`, `shell`, `working_directory`, `continue_on_error`, `timeout_minutes`, `mark`, `extra` |
| `Trigger`, `Workflow::takes`, `Workflow::triggers` | the decision whether the workflow takes an event: `event`, `action` (the `types` value), `git_ref` (`refs/heads/X`, `refs/tags/X`, or a branch name; for a pull request the base branch), `paths` (the changed files); `None` is not known and passes its filter (T-572, PodUtils/podrunner#97) |
| `On`, `Event`, `Filter`, `Pattern`, `DEFAULT_PR_TYPES` | the events of `on:` with their filters, read once: `On::read`, `On::takes`, `Event::takes`; `Filter::matches` (the last pattern that matches decides), `Filter::takes`; `Pattern::parse`, `Pattern::matches` |
| `Node`, `Kind`, `Key`, `Mark` | a value of the file with its line and column: `to_value` (as written), `evaluate` (each `${{ }}` evaluated), `text`, `get`, `entries` |
| `Warning` | an unknown key: its path, its name, its place |
| `Graph` | `new` (a missing need and a cycle are errors by name), `order`, `edges`, `needs`, `ancestors` |
| `Run<W>` | `new`, `ready` (owned ids), `expand`, `finish`, `cancel`, `result`, `is_finished`, `graph`, `workflow`; `W` is `&Workflow`, or `Workflow` or `Arc<Workflow>` for a run that owns its workflow (T-309) |
| `Base` | the contexts that do not change: `github` (a local stand-in of the caller), `vars`, `inputs`; the limits of an expression |
| `Outcome`, `Instance`, `RunsOn`, `Skip` | the concrete jobs (`name`, `matrix`, `strategy`, `needs`, `runs_on`, `uses`, `timeout_minutes`, `continue_on_error`), or the skip (`condition` with its implicit `success() &&`, the results of the jobs before it, `cancelled`) |
| `JobResult`, `Done` | `success`, `failure`, `cancelled`, `skipped` (`combine` of the cells of a matrix); a result with its outputs |
| `matrix::cells`, `MAX_CELLS` | the cells of an evaluated matrix; 256 |
| `Error`, `ErrorKind` | each failure with its path (`jobs.build.strategy.matrix`), its line and its cause |

## The rules

* YAML: one document; the core schema of YAML 1.2 for a plain scalar
  (`on` and `yes` are strings; `3.10` is the number 3.1, as in GitHub);
  `!!str` makes a string; a duplicate key, a key that is not a scalar and
  an alias of no anchor are errors. An anchor and its aliases work; a
  merge key (`<<`) is an ordinary key, as GitHub has no merge keys.
* The model: `on` and `jobs` are necessary. A job id and a step id are a
  letter or `_`, then letters, digits, `-` and `_`; a step id is there
  once in a job. A job has `runs-on`, or `uses` with no `runs-on` and no
  `steps`. A step has one of `run` and `uses`. Each `${{ }}` of the file
  (not of `on`) and each `if:` must parse; `hashFiles` parses.
* An unknown key stays in `extra` with a warning.
* The expansion: the contexts of each key are those of GitHub's table
  ("Context availability"). `if` and `strategy`: `github`, `needs`,
  `vars`, `inputs`. `name`, `runs-on`, `timeout-minutes`,
  `continue-on-error`: also `strategy` and `matrix` (null with no
  matrix). The status functions of a job read each job before it (each
  need, and their needs): `success()` is true when each succeeded and the
  run is not cancelled, `failure()` when one failed (the documents: "If
  you have a chain of dependent jobs, failure() returns true if any
  ancestor job fails"). A skipped need thus skips a job with no status
  function.
* The matrix: the product of the vectors (the first key changes last),
  then `exclude` (a partial match), then `include` (added to each cell
  whose original values it keeps, else a cell of its own); at most 256
  cells. Values match deeply, strings with case.
* A value of a job must have its kind after the evaluation:
  `timeout-minutes` a number more than 0 (360 by default),
  `continue-on-error`, `fail-fast` a boolean, `max-parallel` a whole
  number of 1 or more (the count of cells by default), `runs-on` a label,
  a list of labels or `{group, labels}`.
* The filters of `on:` (GitHub's "Workflow syntax", "Filter pattern
  cheat sheet"): the event must be in `on:`. `types` holds the action;
  with no `types`, `pull_request` and `pull_request_target` take
  `opened`, `synchronize` and `reopened`, and another event each action.
  `branches` and `tags` match the name after `refs/heads/` or
  `refs/tags/`; with a filter of one kind of ref only, the other kind does
  not run; with neither, each ref runs. `paths` applies after them and
  not to a tag: one changed file that the filter lets through runs the
  workflow (`paths-ignore`: one file that is not ignored); an empty list
  of files runs nothing. In a list, the last pattern that matches decides.
  A pattern: `*` (no `/`), `**`, `**/` (zero or more directories), `?`
  and `+` (zero or one, one or more of the character or set before),
  `[...]` (ranges in `a-z`, `A-Z`, `0-9`), `\` (the next character is
  literal), `!` at the start. A filter with its `-ignore` form, a filter
  (not `-ignore`) with no pattern that has no `!`, a bad pattern and a
  pattern that is not a string are errors, for each event of `on:`.
* The name of a concrete job: `name` evaluated, else the id; with a matrix
  and a name that does not read `matrix`, the values of the cell follow in
  parentheses (`test (ubuntu-latest, 16)`; an array or an object as one
  line of JSON).

Not measured: the conformance against GitHub (no run here). Not covered:
the `workflows` filter of `workflow_run`, the `inputs` of
`workflow_dispatch`, the times of `schedule` (the caller decides them),
the expansion of a reusable workflow, the `workflow_call` inputs and
outputs, `concurrency`, `environment`, `env` of a job and `container`
(kept as `Node`s for the runner), and the evaluation of the step level
(the runner's).

## Tests

`cargo test -p pod-actions-workflow`: the YAML layer, the model and its
errors, the matrix examples of the documents, the graph and the
expansion; the filters of `on:` with the examples of the documents
(`tests/on.rs`: each filter, the cheat sheet of paths, the order of `!`,
paths after branches, a filter with its `-ignore` form refused) and the
patterns (`src/pattern.rs`). The plants `actions-workflow-bang-order-ignored`,
`actions-workflow-star-crosses-slash`,
`actions-workflow-tags-only-takes-branches` and
`actions-workflow-filter-with-ignore-taken` prove them. The fuzz target
`actions_workflow` (`fuzz/`) runs the parse, the filters of `on:` and the
dry run on untrusted YAML.

## Adoption map

| Consumer | Old | New |
| --- | --- | --- |
| podrunner | none (local mode, PodUtils/podrunner@T-046, waits) | `pod-actions-workflow = { git = "https://github.com/PodUtils/podcrates", tag = "vX.Y.Z" }`; `parse`, then `Run` for the `local` job source; each `Instance` becomes a job offer; `Workflow::takes` chooses the workflows of a directory for an event (PodUtils/podrunner@T-125) |
