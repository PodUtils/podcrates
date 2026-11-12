//! The model of a workflow file: each key of the ask, the unknown keys as
//! warnings, the errors by name.

use pod_actions_workflow::{ErrorKind, Kind, Mark, parse};

const FULL: &str = r#"
name: CI
run-name: Deploy by @${{ github.actor }}
on:
  push:
    branches: [main]
  workflow_dispatch:
env:
  GLOBAL: one
defaults:
  run:
    shell: bash
    working-directory: src
permissions: read-all
concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true
jobs:
  build:
    name: Build ${{ matrix.os }}
    runs-on: ${{ matrix.os }}
    if: github.event_name == 'push'
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest]
        include:
          - os: ubuntu-latest
            extra: x
        exclude:
          - os: windows-latest
      fail-fast: false
      max-parallel: 2
    env:
      LOCAL: two
    defaults:
      run:
        shell: sh
    timeout-minutes: 30
    continue-on-error: ${{ matrix.experimental }}
    container:
      image: node:20-alpine
    services:
      redis:
        image: redis:7-alpine
    outputs:
      greeting: ${{ steps.hello.outputs.text }}
      count: 3
    steps:
      - id: hello
        name: Say hello
        if: ${{ always() }}
        run: echo "text=hi" >> "$GITHUB_OUTPUT"
        env:
          A: b
        shell: bash
        working-directory: app
        continue-on-error: true
        timeout-minutes: 5
      - uses: actions/checkout@v5
        with:
          fetch-depth: 0
  call:
    needs: [build]
    uses: ./.github/workflows/reusable.yml
    with:
      x: 1
    secrets: inherit
"#;

#[test]
fn each_key_of_the_ask_is_in_the_model() {
    let parsed = parse(FULL).unwrap();
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    let w = parsed.workflow;
    assert_eq!(w.name.as_deref(), Some("CI"));
    assert_eq!(w.run_name.as_deref(), Some("Deploy by @${{ github.actor }}"));
    assert_eq!(w.events(), ["push", "workflow_dispatch"]);
    assert_eq!(w.env.as_ref().and_then(|e| e.get("GLOBAL")).and_then(|v| v.text()).as_deref(), Some("one"));
    let defaults = w.defaults.as_ref().unwrap();
    assert_eq!((defaults.shell.as_deref(), defaults.working_directory.as_deref()), (Some("bash"), Some("src")));
    assert_eq!(w.permissions.as_ref().unwrap().kind, Kind::String("read-all".into()));
    assert!(w.concurrency.as_ref().unwrap().get("cancel-in-progress").is_some());
    assert_eq!(w.jobs.iter().map(|j| j.id.as_str()).collect::<Vec<_>>(), ["build", "call"]);

    let build = w.job("build").unwrap();
    assert_eq!(build.mark, Mark { line: 19, column: 3 });
    assert_eq!(build.name.as_deref(), Some("Build ${{ matrix.os }}"));
    assert_eq!(build.condition.as_deref(), Some("github.event_name == 'push'"));
    let strategy = build.strategy.as_ref().unwrap();
    assert!(strategy.matrix.as_ref().unwrap().get("include").is_some());
    assert_eq!(strategy.fail_fast.as_ref().unwrap().kind, Kind::Bool(false));
    assert_eq!(strategy.max_parallel.as_ref().unwrap().kind, Kind::Number(2.0));
    assert!(build.env.is_some() && build.container.is_some() && build.services.is_some());
    assert_eq!(build.defaults.as_ref().unwrap().shell.as_deref(), Some("sh"));
    assert_eq!(build.timeout_minutes.as_ref().unwrap().kind, Kind::Number(30.0));
    assert!(build.continue_on_error.is_some());
    assert_eq!(
        build.outputs,
        [("greeting".to_string(), "${{ steps.hello.outputs.text }}".to_string()), ("count".into(), "3".into())]
    );
    assert!(build.has_matrix() && !build.is_reusable());
    assert_eq!(build.steps.len(), 2);
    let hello = &build.steps[0];
    assert_eq!(hello.id.as_deref(), Some("hello"));
    assert_eq!(hello.name.as_deref(), Some("Say hello"));
    assert_eq!(hello.condition.as_deref(), Some("${{ always() }}"));
    assert_eq!(hello.run.as_deref(), Some("echo \"text=hi\" >> \"$GITHUB_OUTPUT\""));
    assert_eq!(hello.shell.as_deref(), Some("bash"));
    assert_eq!(hello.working_directory.as_deref(), Some("app"));
    assert_eq!(hello.continue_on_error.as_ref().unwrap().kind, Kind::Bool(true));
    assert_eq!(hello.timeout_minutes.as_ref().unwrap().kind, Kind::Number(5.0));
    assert!(hello.env.is_some());
    let checkout = &build.steps[1];
    assert_eq!(checkout.uses.as_deref(), Some("actions/checkout@v5"));
    assert_eq!(checkout.with.as_ref().unwrap().get("fetch-depth").unwrap().kind, Kind::Number(0.0));

    let call = w.job("call").unwrap();
    assert!(call.is_reusable());
    assert_eq!(call.uses.as_deref(), Some("./.github/workflows/reusable.yml"));
    assert_eq!(call.needs, ["build"]);
    assert!(call.with.is_some());
    assert_eq!(call.secrets.as_ref().unwrap().kind, Kind::String("inherit".into()));
}

#[test]
fn an_unknown_key_is_kept_and_named() {
    let text = "on: push\nextra-top: 1\njobs:\n  a:\n    runs-on: x\n    flavour: y\n    strategy:\n      matrix: {a: [1]}\n      spread: 2\n    steps:\n      - run: echo\n        colour: red\n    defaults:\n      run:\n        user: me\n";
    let parsed = parse(text).unwrap();
    let warnings: Vec<String> = parsed.warnings.iter().map(ToString::to_string).collect();
    assert_eq!(
        warnings,
        [
            "the top level (line 2, column 1): unknown key `extra-top`, kept as data",
            "jobs.a (line 6, column 5): unknown key `flavour`, kept as data",
            "jobs.a.strategy (line 9, column 7): unknown key `spread`, kept as data",
            "jobs.a.defaults.run (line 15, column 9): unknown key `user`, kept as data",
            "jobs.a.steps[0] (line 12, column 9): unknown key `colour`, kept as data",
        ]
    );
    let w = parsed.workflow;
    assert_eq!(w.extra[0].0.name, "extra-top");
    let job = w.job("a").unwrap();
    assert_eq!(job.extra[0].1.text().as_deref(), Some("y"));
    assert_eq!(job.strategy.as_ref().unwrap().extra[0].0.name, "spread");
    assert_eq!(job.steps[0].extra[0].1.text().as_deref(), Some("red"));
    assert_eq!(job.defaults.as_ref().unwrap().extra[0].0.name, "user");
}

fn fault(text: &str) -> (String, ErrorKind) {
    let e = parse(text).unwrap_err();
    (e.path.clone(), e.kind)
}

#[test]
fn the_errors_of_the_file_name_their_cause() {
    assert_eq!(fault("jobs:\n  a:\n    runs-on: x\n"), (String::new(), ErrorKind::Missing("on")));
    assert_eq!(fault("on: push\n"), (String::new(), ErrorKind::Missing("jobs")));
    assert_eq!(fault("on: push\njobs: {}\n").1, ErrorKind::Type("a mapping with at least one job"));
    assert_eq!(fault("- a\n").1, ErrorKind::Type("a mapping"));
    assert_eq!(fault("on: push\njobs:\n  a:\n    steps: []\n"), ("jobs.a".into(), ErrorKind::Missing("runs-on")));
    assert_eq!(fault("on: push\njobs:\n  1a:\n    runs-on: x\n"), ("jobs.1a".into(), ErrorKind::BadId("1a".into())));
    assert_eq!(fault("on: push\njobs:\n  a b:\n    runs-on: x\n").1, ErrorKind::BadId("a b".into()));
    let step = |s: &str| fault(&format!("on: push\njobs:\n  a:\n    runs-on: x\n    steps:\n{s}"));
    assert_eq!(step("      - name: nothing\n"), ("jobs.a.steps[0]".into(), ErrorKind::StepKind));
    assert_eq!(step("      - run: a\n        uses: b\n").1, ErrorKind::StepKind);
    assert_eq!(step("      - id: 9x\n        run: a\n").1, ErrorKind::BadId("9x".into()));
    assert_eq!(
        step("      - id: s\n        run: a\n      - id: s\n        run: b\n"),
        ("jobs.a.steps".into(), ErrorKind::DuplicateStepId("s".into()))
    );
    assert_eq!(step("      run: a\n").1, ErrorKind::Type("a sequence"));
    let reusable = "on: push\njobs:\n  a:\n    uses: ./x.yml\n    runs-on: y\n";
    assert_eq!(fault(reusable).1, ErrorKind::Reusable("runs-on"));
    let reusable = "on: push\njobs:\n  a:\n    uses: ./x.yml\n    steps: []\n";
    assert_eq!(fault(reusable).1, ErrorKind::Reusable("steps"));
    let needs = "on: push\njobs:\n  a:\n    runs-on: x\n    needs: {b: 1}\n";
    assert_eq!(fault(needs), ("jobs.a.needs".into(), ErrorKind::Type("a job id or a sequence of job ids")));
    assert_eq!(fault("on: push\njobs:\n  a:\n    runs-on: x\n    name: [1]\n").1, ErrorKind::Type("a scalar"));
}

#[test]
fn a_bad_template_or_condition_is_an_error_of_the_file() {
    let (path, kind) = fault("on: push\njobs:\n  a:\n    runs-on: x\n    env:\n      A: ${{ github.ref ==  }}\n");
    assert_eq!(path, "jobs.a.env.A");
    assert!(matches!(kind, ErrorKind::Expression(_)), "{kind:?}");
    let e = parse("on: push\njobs:\n  a:\n    runs-on: x\n    if: github.ref == = 1\n").unwrap_err();
    assert_eq!(e.path, "jobs.a.if");
    assert_eq!(e.to_string(), "jobs.a.if (line 5, column 9): unexpected symbol `=` (at character 15)");
    let e = parse("on: push\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: echo ${{ nope() }}\n").unwrap_err();
    assert_eq!(e.path, "jobs.a.steps[0].run");
    // `hashFiles` parses: the runner evaluates it in a step.
    let ok = "on: push\njobs:\n  a:\n    runs-on: x\n    steps:\n      - if: hashFiles('x') != ''\n        run: echo ${{ hashFiles('**/lock') }}\n";
    assert!(parse(ok).is_ok());
    // `on` is data: a `${{` there is not read.
    assert!(parse("on:\n  push:\n    tags: ['${{']\njobs:\n  a:\n    runs-on: x\n").is_ok());
}

#[test]
fn needs_and_events_in_their_forms() {
    let parsed = parse("on: [push, pull_request]\njobs:\n  a:\n    runs-on: x\n  b:\n    runs-on: x\n    needs: a\n  c:\n    runs-on: x\n    needs: [a, b, a]\n").unwrap();
    let w = parsed.workflow;
    assert_eq!(w.events(), ["push", "pull_request"]);
    assert_eq!(w.job("b").unwrap().needs, ["a"]);
    assert_eq!(w.job("c").unwrap().needs, ["a", "b"]);
    assert_eq!(w.job("a").unwrap().needs, Vec::<String>::new());
    // A boolean or a number of `if` is its text.
    let w = parse("on: push\njobs:\n  a:\n    runs-on: x\n    if: false\n").unwrap().workflow;
    assert_eq!(w.jobs[0].condition.as_deref(), Some("false"));
}
