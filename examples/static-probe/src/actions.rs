//! pod-actions-workflow and pod-actions-expr: a workflow with a matrix and
//! a need, its dry run, and one expression, so that the YAML reader, the
//! model, the matrix, the parser and the evaluator link in; the filters of
//! `on:` for one push, so that the patterns link in. pod-actions-store:
//! the `uses:` of the workflow, the digest of a tar, and a lock read back.

const WORKFLOW: &str = "
on:
  push:
    branches: ['releases/**', '!releases/**-alpha']
    paths-ignore: ['docs/**']
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
    let paths = ["src/main.rs".to_string()];
    let push = pod_actions_workflow::Trigger {
        git_ref: Some("refs/heads/releases/v1"),
        paths: Some(&paths),
        ..pod_actions_workflow::Trigger::new("push")
    };
    let takes = pod_actions_workflow::parse(WORKFLOW)
        .and_then(|p| p.workflow.takes(&push))
        .map_or_else(|e| e.to_string(), |t| t.to_string());
    let value = pod_actions_expr::evaluate("format('{0}', toJSON(fromJSON('[1, 2]')[1]))", &Default::default())
        .map_or_else(|e| e.to_string(), |v| v.to_text());
    format!(
        "pod-actions-workflow: {jobs}; takes a push: {takes}; pod-actions-expr: {value}; pod-actions-store: {}",
        store()
    )
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

/// pod-actions-results: a store in a scratch directory and one call of the
/// service, so that the routes, the digest and the entropy link in.
pub fn results() -> String {
    let dir = std::env::temp_dir().join(format!("static-probe-results-{}", std::process::id()));
    let line = match pod_actions_results::ArtifactStore::new(&dir, pod_actions_results::LIMIT) {
        Ok(store) => {
            let path = format!("{}CreateArtifact", pod_actions_results::SERVICE);
            let answer = store.route("POST", &path, br#"{"name":"probe"}"#, "http://127.0.0.1/");
            format!("CreateArtifact {}", answer.map_or(0, |(status, _)| status))
        }
        Err(e) => e.to_string(),
    };
    let _ = std::fs::remove_dir_all(&dir);
    let ids = pod_actions_results::backend_ids("a.e30.b").is_some();
    format!("pod-actions-results: {line}; token ids: {ids}")
}
