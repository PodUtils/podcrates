//! pod-actions-workflow and pod-actions-expr: a workflow with a matrix and
//! a need, its dry run, and one expression, so that the YAML reader, the
//! model, the matrix, the parser and the evaluator link in. pod-actions-store:
//! the `uses:` of the workflow, the digest of a tar, and a lock read back.

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
    format!("pod-actions-workflow: {jobs}; pod-actions-expr: {value}; pod-actions-store: {}", store())
}

/// The `uses:` of a workflow, the digest of an empty tar, and a lock of one
/// pin that renders and parses back.
fn store() -> String {
    let text =
        "on: push\njobs:\n  a:\n    runs-on: x\n    steps:\n      - uses: actions/checkout@v4\n      - uses: o/r@v1\n";
    let uses = pod_actions_workflow::parse(text)
        .map(|p| pod_actions_store::workflow_uses(&p.workflow, &["actions/checkout"]).join(","))
        .unwrap_or_else(|e| e.to_string());
    let digest = pod_actions_store::digest(&[0; 1024]);
    let mut lock = pod_actions_store::Lock::default();
    let pin = pod_actions_store::Pin { commit: "0".repeat(40), digest: digest.clone() };
    lock.pins.insert("o/r@v1".into(), pin);
    let back =
        pod_actions_store::Lock::parse(&lock.render()).map_or_else(|e| e.to_string(), |l| l.pins.len().to_string());
    format!("{uses}; {digest}; {back} pin")
}
