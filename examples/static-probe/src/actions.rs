//! pod-actions-workflow and pod-actions-expr: a workflow with a matrix and
//! a need, its dry run, and one expression, so that the YAML reader, the
//! model, the matrix, the parser and the evaluator link in.

const WORKFLOW: &str = "
on: push
jobs:
  build:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [linux, windows]
    steps:
      - run: make
  notify:
    needs: build
    if: failure()
    runs-on: linux
    steps:
      - run: echo failed
";

pub fn actions() -> String {
    let jobs = pod_actions_workflow::parse(WORKFLOW)
        .and_then(|p| {
            pod_actions_workflow::plan(&p.workflow, Default::default(), pod_actions_workflow::JobResult::Success)
        })
        .map_or_else(
            |e| e.to_string(),
            |all| {
                let outcomes: Vec<String> = all
                    .iter()
                    .map(|(id, o)| match o {
                        pod_actions_workflow::Outcome::Run(cells) => format!("{id}: {} cells", cells.len()),
                        pod_actions_workflow::Outcome::Skipped(skip) => skip.to_string(),
                    })
                    .collect();
                outcomes.join("; ")
            },
        );
    let value = pod_actions_expr::evaluate("format('{0}', toJSON(fromJSON('[1, 2]')[1]))", &Default::default())
        .map_or_else(|e| e.to_string(), |v| v.to_text());
    format!("pod-actions-workflow: {jobs}; pod-actions-expr: {value}")
}
