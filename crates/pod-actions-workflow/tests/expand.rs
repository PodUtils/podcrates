//! The expansion: the graph of needs, the order, the concrete jobs of each
//! matrix (also one from `fromJSON` of an output), the skips with their
//! reasons.

use pod_actions_expr::Value;
use pod_actions_workflow::{
    Base, Done, ErrorKind, Graph, Instance, JobResult, Outcome, Run, RunsOn, Workflow, parse, plan,
};

fn workflow(text: &str) -> Workflow {
    parse(text).unwrap_or_else(|e| panic!("{e}")).workflow
}

fn cells(outcome: Outcome) -> Vec<Instance> {
    match outcome {
        Outcome::Run(cells) => cells,
        Outcome::Skipped(skip) => panic!("{skip}"),
    }
}

const CHAIN: &str = "
on: push
jobs:
  deploy:
    needs: [build, test]
    runs-on: x
  test:
    needs: build
    runs-on: x
  build:
    runs-on: x
  lint:
    runs-on: x
";

#[test]
fn the_graph_orders_the_jobs_by_their_needs() {
    let w = workflow(CHAIN);
    let graph = Graph::new(&w).unwrap();
    assert_eq!(graph.order().collect::<Vec<_>>(), ["build", "test", "deploy", "lint"]);
    assert_eq!(graph.edges(), [("build", "deploy"), ("test", "deploy"), ("build", "test")]);
    assert_eq!(graph.needs("deploy"), ["build", "test"]);
    assert_eq!(graph.ancestors("deploy"), ["build", "test"]);
    assert_eq!(graph.ancestors("lint"), Vec::<&str>::new());
}

#[test]
fn a_missing_need_and_a_cycle_are_errors_by_name() {
    let w = workflow("on: push\njobs:\n  a:\n    runs-on: x\n    needs: ghost\n");
    let e = Graph::new(&w).unwrap_err();
    assert_eq!(e.kind, ErrorKind::UnknownNeed("ghost".into()));
    assert_eq!(e.to_string(), "jobs.a.needs (line 3, column 3): the need `ghost` is not a job of the file");
    let w = workflow(
        "on: push\njobs:\n  z:\n    runs-on: x\n  a:\n    runs-on: x\n    needs: c\n  b:\n    runs-on: x\n    needs: a\n  c:\n    runs-on: x\n    needs: [z, b]\n",
    );
    let e = Run::new(&w, Base::default()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Cycle(vec!["a".into(), "c".into(), "b".into(), "a".into()]));
    assert_eq!(e.path, "jobs.a.needs");
    let w = workflow("on: push\njobs:\n  a:\n    runs-on: x\n    needs: a\n");
    assert_eq!(Graph::new(&w).unwrap_err().kind, ErrorKind::Cycle(vec!["a".into(), "a".into()]));
}

#[test]
fn a_run_gives_the_ready_jobs_in_order() {
    let w = workflow(CHAIN);
    let mut run = Run::new(&w, Base::default()).unwrap();
    assert_eq!(run.ready(), ["build", "lint"]);
    let e = run.expand("deploy").unwrap_err();
    assert_eq!(e.kind, ErrorKind::Usage("the needs of the job `deploy` are not done".into()));
    let build = cells(run.expand("build").unwrap());
    assert_eq!(build.len(), 1);
    assert_eq!(build[0].name, "build");
    assert_eq!(build[0].matrix, Value::Null);
    assert_eq!(build[0].runs_on, Some(RunsOn { group: None, labels: vec!["x".into()] }));
    assert_eq!(build[0].timeout_minutes, 360.0);
    assert!(!build[0].continue_on_error);
    assert_eq!(build[0].strategy.to_json_line(), r#"{"fail-fast":true,"job-index":0,"job-total":1,"max-parallel":1}"#);
    assert_eq!(run.ready(), ["lint"]);
    assert!(run.expand("build").is_err());
    run.finish("build", Done::of(JobResult::Success)).unwrap();
    assert!(run.finish("build", Done::of(JobResult::Success)).is_err());
    assert_eq!(run.ready(), ["test", "lint"]);
    cells(run.expand("test").unwrap());
    run.finish("test", Done::of(JobResult::Success)).unwrap();
    let deploy = cells(run.expand("deploy").unwrap());
    assert_eq!(
        deploy[0].needs.to_json_line(),
        r#"{"build":{"result":"success","outputs":{}},"test":{"result":"success","outputs":{}}}"#
    );
    assert!(!run.is_finished());
    run.finish("deploy", Done::of(JobResult::Success)).unwrap();
    cells(run.expand("lint").unwrap());
    run.finish("lint", Done::of(JobResult::Failure)).unwrap();
    assert!(run.is_finished());
    assert_eq!(run.result("lint"), Some(JobResult::Failure));
}

const STATUS: &str = "
on: push
jobs:
  build:
    runs-on: x
  test:
    needs: build
    runs-on: x
  deploy:
    needs: test
    runs-on: x
  notify:
    needs: deploy
    if: failure()
    runs-on: x
  report:
    needs: deploy
    if: always()
    runs-on: x
  tidy:
    needs: deploy
    if: ${{ !cancelled() }}
    runs-on: x
  on-skip:
    needs: deploy
    if: needs.deploy.result == 'skipped'
    runs-on: x
";

fn outcomes(build: JobResult) -> Vec<(String, String)> {
    let w = workflow(STATUS);
    let mut run = Run::new(&w, Base::default()).unwrap();
    let mut out = Vec::new();
    while let Some(id) = run.ready().first().copied() {
        let text = match run.expand(id).unwrap() {
            Outcome::Run(_) => {
                let result = if id == "build" { build } else { JobResult::Success };
                run.finish(id, Done::of(result)).unwrap();
                "run".to_string()
            }
            Outcome::Skipped(skip) => skip.to_string(),
        };
        out.push((id.to_string(), text));
    }
    out
}

#[test]
fn a_failure_skips_the_jobs_after_it_with_their_reasons() {
    let out = outcomes(JobResult::Failure);
    let get = |id: &str| out.iter().find(|(j, _)| j == id).map(|(_, t)| t.as_str()).unwrap();
    assert_eq!(get("build"), "run");
    assert_eq!(get("test"), "the job `test` is skipped: `success()` is false (build failure)");
    assert_eq!(get("deploy"), "the job `deploy` is skipped: `success()` is false (build failure, test skipped)");
    // failure() reads each job before it, not only its need.
    assert_eq!(get("notify"), "run");
    assert_eq!(get("report"), "run");
    assert_eq!(get("tidy"), "run");
    assert_eq!(
        get("on-skip"),
        "the job `on-skip` is skipped: `success() && (needs.deploy.result == 'skipped')` is false (build failure, test skipped, deploy skipped)"
    );
}

#[test]
fn success_runs_the_default_jobs() {
    let out = outcomes(JobResult::Success);
    let get = |id: &str| out.iter().find(|(j, _)| j == id).map(|(_, t)| t.as_str()).unwrap();
    assert_eq!(get("deploy"), "run");
    assert_eq!(get("notify"), "the job `notify` is skipped: `failure()` is false");
    assert_eq!(get("report"), "run");
    assert!(get("on-skip").starts_with("the job `on-skip` is skipped"));
}

#[test]
fn a_cancelled_run_skips_all_but_always_and_cancelled() {
    let w = workflow(STATUS);
    let mut run = Run::new(&w, Base::default()).unwrap();
    cells(run.expand("build").unwrap());
    run.cancel();
    run.finish("build", Done::of(JobResult::Cancelled)).unwrap();
    let Outcome::Skipped(skip) = run.expand("test").unwrap() else { panic!() };
    assert!(skip.cancelled);
    assert_eq!(
        skip.to_string(),
        "the job `test` is skipped: `success()` is false (build cancelled) (the run is cancelled)"
    );
    let Outcome::Skipped(_) = run.expand("deploy").unwrap() else { panic!() };
    assert!(matches!(run.expand("notify").unwrap(), Outcome::Skipped(_)));
    assert!(matches!(run.expand("report").unwrap(), Outcome::Run(_)));
    assert!(matches!(run.expand("tidy").unwrap(), Outcome::Skipped(_)));
}

const DYNAMIC: &str = r#"
on: push
jobs:
  setup:
    runs-on: ubuntu-latest
    outputs:
      matrix: ${{ steps.m.outputs.matrix }}
    steps:
      - id: m
        run: echo "matrix=..." >> "$GITHUB_OUTPUT"
  test:
    needs: setup
    name: Test ${{ matrix.project }} on ${{ matrix.os }}
    runs-on: [self-hosted, "${{ matrix.os }}"]
    timeout-minutes: ${{ matrix.minutes || 10 }}
    continue-on-error: ${{ matrix.experimental == true }}
    strategy:
      max-parallel: 2
      fail-fast: ${{ vars.FAST == 'yes' }}
      matrix: ${{ fromJSON(needs.setup.outputs.matrix) }}
    steps:
      - run: make test
"#;

#[test]
fn a_matrix_from_the_output_of_a_need() {
    let w = workflow(DYNAMIC);
    let base = Base { vars: Value::object([("FAST", Value::from("yes"))]), ..Base::default() };
    let mut run = Run::new(&w, base).unwrap();
    cells(run.expand("setup").unwrap());
    let matrix = r#"{"project": ["foo", "bar"], "os": ["linux"], "include": [{"project": "bar", "minutes": 5, "experimental": true}]}"#;
    run.finish("setup", Done { result: JobResult::Success, outputs: vec![("matrix".into(), matrix.into())] }).unwrap();
    let test = cells(run.expand("test").unwrap());
    assert_eq!(test.len(), 2);
    assert_eq!(test[0].name, "Test foo on linux");
    assert_eq!(test[0].runs_on, Some(RunsOn { group: None, labels: vec!["self-hosted".into(), "linux".into()] }));
    assert_eq!(test[0].timeout_minutes, 10.0);
    assert!(!test[0].continue_on_error);
    assert_eq!(test[1].matrix.to_json_line(), r#"{"project":"bar","os":"linux","minutes":5,"experimental":true}"#);
    assert_eq!(test[1].timeout_minutes, 5.0);
    assert!(test[1].continue_on_error);
    assert_eq!(test[1].strategy.to_json_line(), r#"{"fail-fast":true,"job-index":1,"job-total":2,"max-parallel":2}"#);
    assert_eq!(test[1].needs.get("setup").and_then(|s| s.get("result")), Some(&Value::from("success")));
}

#[test]
fn a_matrix_that_cannot_expand_is_an_error_by_name() {
    let w = workflow(DYNAMIC);
    let mut run = Run::new(&w, Base::default()).unwrap();
    cells(run.expand("setup").unwrap());
    run.finish("setup", Done { result: JobResult::Success, outputs: vec![("matrix".into(), "[1]".into())] }).unwrap();
    let e = run.expand("test").unwrap_err();
    assert_eq!(e.path, "jobs.test.strategy.matrix");
    assert_eq!(e.kind, ErrorKind::Matrix("the matrix must be a mapping, not array".into()));
    // The dry run has no outputs: `fromJSON` of null fails by name.
    let e = plan(&w, Base::default(), JobResult::Success).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Expression(_)), "{e}");
    assert_eq!(e.path, "jobs.test.strategy.matrix");
}

#[test]
fn the_names_of_matrix_jobs() {
    let text = "
on: push
jobs:
  plain:
    runs-on: x
    strategy:
      matrix:
        os: [a, b]
        cfg: [{deep: 1}]
  named:
    name: Build
    runs-on: x
    strategy:
      matrix:
        os: [a]
  reads:
    name: Build on ${{ MATRIX.os }}
    runs-on: x
    strategy:
      matrix:
        os: [a]
  call:
    uses: ./reusable.yml
    strategy:
      matrix:
        n: [1, 2]
";
    let w = workflow(text);
    let all = plan(&w, Base::default(), JobResult::Success).unwrap();
    let names: Vec<String> = all
        .iter()
        .flat_map(|(_, o)| match o {
            Outcome::Run(cells) => cells.iter().map(|c| c.name.clone()).collect(),
            Outcome::Skipped(_) => Vec::new(),
        })
        .collect();
    assert_eq!(
        names,
        ["plain (a, {\"deep\":1})", "plain (b, {\"deep\":1})", "Build (a)", "Build on a", "call (1)", "call (2)"]
    );
    let Outcome::Run(call) = &all[3].1 else { panic!() };
    assert_eq!(call[0].uses.as_deref(), Some("./reusable.yml"));
    assert_eq!(call[0].runs_on, None);
}

#[test]
fn the_values_of_a_job_must_have_their_kinds() {
    let job = |extra: &str| format!("on: push\njobs:\n  a:\n    runs-on: x\n{extra}");
    let fault = |text: String| plan(&workflow(&text), Base::default(), JobResult::Success).unwrap_err();
    let e = fault(job("    timeout-minutes: '10'\n"));
    assert_eq!(e.path, "jobs.a.timeout-minutes");
    assert_eq!(e.kind, ErrorKind::Value("the value must be a number more than 0, not the string \"10\"".into()));
    assert_eq!(fault(job("    continue-on-error: 'yes'\n")).path, "jobs.a.continue-on-error");
    assert_eq!(fault(job("    strategy:\n      fail-fast: 1\n")).path, "jobs.a.strategy.fail-fast");
    assert_eq!(fault(job("    strategy:\n      max-parallel: 0\n")).path, "jobs.a.strategy.max-parallel");
    let e = fault("on: push\njobs:\n  a:\n    runs-on: [1]\n".into());
    assert_eq!(e.path, "jobs.a.runs-on");
    // `matrix` is null with no matrix: a runs-on that reads it has no label.
    let e = fault("on: push\njobs:\n  a:\n    runs-on: ${{ matrix.os }}\n".into());
    assert_eq!(e.path, "jobs.a.runs-on");
    assert!(matches!(e.kind, ErrorKind::Value(_)), "{e}");
}

#[test]
fn runs_on_in_its_forms() {
    let text = "on: push\njobs:\n  a:\n    runs-on:\n      group: big\n      labels: [linux, x64]\n  b:\n    runs-on:\n      group: small\n";
    let all = plan(&workflow(text), Base::default(), JobResult::Success).unwrap();
    let runs_on = |i: usize| match &all[i].1 {
        Outcome::Run(c) => c[0].runs_on.clone().unwrap(),
        Outcome::Skipped(_) => panic!(),
    };
    assert_eq!(runs_on(0), RunsOn { group: Some("big".into()), labels: vec!["linux".into(), "x64".into()] });
    assert_eq!(runs_on(1), RunsOn { group: Some("small".into()), labels: Vec::new() });
}

#[test]
fn the_contexts_of_the_job_level() {
    // `matrix` is not a context of a job's `if`, as in GitHub.
    let text = "on: push\njobs:\n  a:\n    runs-on: x\n    if: matrix.os == 'a'\n";
    let e = plan(&workflow(text), Base::default(), JobResult::Success).unwrap_err();
    assert_eq!(e.path, "jobs.a.if");
    assert_eq!(e.to_string(), "jobs.a.if (line 3, column 3): unknown context `matrix` (at character 1)");
    let text = "on: push\njobs:\n  a:\n    runs-on: x\n    if: github.event_name == 'push' && inputs.go && vars.v\n";
    let base = Base {
        github: Value::object([("event_name", Value::from("push"))]),
        inputs: Value::object([("go", Value::Bool(true))]),
        vars: Value::object([("v", Value::from("1"))]),
        ..Base::default()
    };
    assert!(matches!(plan(&workflow(text), base, JobResult::Success).unwrap()[0].1, Outcome::Run(_)));
}
