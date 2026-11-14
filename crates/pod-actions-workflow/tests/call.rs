//! The calls of reusable workflows (PodUtils/podrunner#98): the examples
//! of GitHub's "Reusing workflows", the checks of `with` and `secrets`,
//! the outputs, a matrix of calls, nesting and the limits.

use std::sync::{Arc, Mutex};

use pod_actions_expr::Value;
use pod_actions_workflow::{
    Base, Call, CallLimits, Called, Done, Error, ErrorKind, Instance, JobResult, Outcome, Run, Secrets, Workflow,
    parse, plan_with,
};

/// A source of called files by their `uses`, which keeps each `from`.
#[derive(Default)]
struct Files {
    files: Vec<(&'static str, String)>,
    asked: Mutex<Vec<(String, Option<String>)>>,
}

impl Called for Files {
    fn text(&self, uses: &str, from: Option<&str>) -> Result<String, String> {
        self.asked.lock().unwrap().push((uses.to_string(), from.map(str::to_string)));
        let file = self.files.iter().find(|(u, _)| *u == uses);
        file.map(|(_, text)| text.clone()).ok_or_else(|| format!("no file {uses}"))
    }
}

fn files(list: &[(&'static str, &str)]) -> Arc<Files> {
    Arc::new(Files { files: list.iter().map(|(u, t)| (*u, t.to_string())).collect(), ..Files::default() })
}

fn workflow(text: &str) -> Workflow {
    parse(text).unwrap_or_else(|e| panic!("{e}")).workflow
}

fn calls(outcome: Outcome) -> Vec<Call> {
    match outcome {
        Outcome::Called(calls) => calls,
        other => panic!("{other:?}"),
    }
}

fn cells(outcome: Outcome) -> Vec<Instance> {
    match outcome {
        Outcome::Run(cells) => cells,
        other => panic!("{other:?}"),
    }
}

/// The kind of the innermost error of a chain of called workflows.
fn root(e: &Error) -> &ErrorKind {
    match &e.kind {
        ErrorKind::InCalled { error, .. } => root(error),
        kind => kind,
    }
}

const REUSABLE: &str = "
name: Reusable workflow example
on:
  workflow_call:
    inputs:
      config-path:
        required: true
        type: string
    secrets:
      token:
        required: true
jobs:
  triage:
    runs-on: ubuntu-latest
    steps:
    - uses: actions/labeler@v4
      with:
        repo-token: ${{ secrets.token }}
        configuration-path: ${{ inputs.config-path }}
";

const CALLER: &str = "
on: push
jobs:
  call-workflow-passing-data:
    uses: octo-org/example-repo/.github/workflows/reusable-workflow.yml@main
    with:
      config-path: .github/labeler.yml
    secrets:
      token: ${{ secrets.GITHUB_TOKEN }}
";

const USES: &str = "octo-org/example-repo/.github/workflows/reusable-workflow.yml@main";

#[test]
fn the_example_of_the_documents_passes_inputs_and_secrets() {
    let w = workflow(CALLER);
    let mut run = Run::new(&w, Base::default()).unwrap().with_called(files(&[(USES, REUSABLE)]));
    let call = calls(run.expand("call-workflow-passing-data").unwrap()).remove(0);
    assert_eq!(call.uses, USES);
    assert_eq!(call.inputs.to_json_line(), r#"{"config-path":".github/labeler.yml"}"#);
    let passed = vec![("token".to_string(), "${{ secrets.GITHUB_TOKEN }}".to_string())];
    assert_eq!(call.secrets, Secrets::Passed { from: Box::new(Secrets::Run), items: passed });
    assert_eq!(call.jobs, ["call-workflow-passing-data/triage"]);
    assert_eq!(run.ready(), ["call-workflow-passing-data/triage"]);
    let e = run.finish("call-workflow-passing-data", Done::of(JobResult::Success)).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Usage(_)));
    let triage = cells(run.expand("call-workflow-passing-data/triage").unwrap()).remove(0);
    assert_eq!(triage.name, "call-workflow-passing-data / triage");
    assert_eq!(triage.inputs, call.inputs);
    assert_eq!(triage.secrets.names(), Some(vec!["token"]));
    assert!(!run.is_finished());
    run.finish("call-workflow-passing-data/triage", Done::of(JobResult::Success)).unwrap();
    assert_eq!(run.result("call-workflow-passing-data"), Some(JobResult::Success));
    assert!(run.is_finished());
}

#[test]
fn with_is_checked_against_the_inputs_of_the_called_workflow() {
    let called = "on:\n  workflow_call:\n    inputs:\n      name: {type: string, required: true}\n      flag: {type: boolean, default: true}\n      count: {type: number}\n      word: {type: string}\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: echo\n";
    let call = |with: &str| -> Result<Value, Error> {
        let text = format!("on: push\njobs:\n  c:\n    uses: ./x.yml\n    with:\n{with}");
        let w = workflow(&text);
        let mut run = Run::new(&w, Base::default()).unwrap().with_called(files(&[("./x.yml", called)]));
        Ok(calls(run.expand("c")?).remove(0).inputs)
    };
    let inputs = call("      name: 3.10\n").unwrap();
    assert_eq!(inputs.to_json_line(), r#"{"name":"3.1","flag":true,"count":0,"word":""}"#);
    let inputs = call("      NAME: n\n      flag: false\n      count: ${{ 2 }}\n").unwrap();
    assert_eq!(inputs.to_json_line(), r#"{"name":"n","flag":false,"count":2,"word":""}"#);
    let input = |with: &str| match call(with).unwrap_err().kind {
        ErrorKind::Input { name, why } => format!("{name} {why}"),
        other => panic!("{other:?}"),
    };
    assert_eq!(input("      flag: true\n"), "name is required");
    assert_eq!(input("      name: n\n      other: 1\n"), "other is not an input of the called workflow");
    assert_eq!(input("      name: n\n      flag: 'true'\n"), "flag must be a boolean, not the string \"true\"");
    assert_eq!(input("      name: n\n      count: two\n"), "count must be a number, not the string \"two\"");
}

#[test]
fn secrets_are_passed_by_name_inherited_only_when_asked_and_checked() {
    let called = "on:\n  workflow_call:\n    secrets:\n      token:\n      key: {required: true}\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: echo\n";
    let call = |secrets: &str| -> Result<Secrets, Error> {
        let text = format!("on: push\njobs:\n  c:\n    uses: ./x.yml\n{secrets}");
        let w = workflow(&text);
        let mut run = Run::new(&w, Base::default()).unwrap().with_called(files(&[("./x.yml", called)]));
        Ok(calls(run.expand("c")?).remove(0).secrets)
    };
    assert_eq!(call("    secrets: inherit\n").unwrap(), Secrets::Run);
    assert_eq!(call("    secrets: inherit\n").unwrap().names(), None);
    let named = call("    secrets:\n      key: ${{ secrets.K }}\n").unwrap();
    assert_eq!(named.names(), Some(vec!["key"]));
    let secret = |text: &str| match call(text).unwrap_err().kind {
        ErrorKind::Secret { name, why } => format!("{name} {why}"),
        other => panic!("{other:?}"),
    };
    assert_eq!(secret(""), "key is required");
    assert_eq!(secret("    secrets:\n      key: a\n      other: b\n"), "other is not a secret of the called workflow");
    // A call with no `secrets:` passes none: not each secret of the run.
    let open = "on: workflow_call\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: echo\n";
    let w = workflow("on: push\njobs:\n  c:\n    uses: ./o.yml\n");
    let mut run = Run::new(&w, Base::default()).unwrap().with_called(files(&[("./o.yml", open)]));
    let none = calls(run.expand("c").unwrap()).remove(0).secrets;
    assert_eq!(none.names(), Some(Vec::new()));
}

#[test]
fn the_outputs_of_the_called_workflow_are_needs_of_the_jobs_after_the_call() {
    let called = "
name: Reusable workflow
on:
  workflow_call:
    outputs:
      firstword:
        description: The first output string
        value: ${{ jobs.example_job.outputs.output1 }}
      secondword:
        value: ${{ jobs.example_job.outputs.output2 }}
jobs:
  example_job:
    name: Generate output
    runs-on: ubuntu-latest
    outputs:
      output1: ${{ steps.step1.outputs.firstword }}
      output2: ${{ steps.step2.outputs.secondword }}
    steps:
      - id: step1
        run: echo \"firstword=hello\" >> $GITHUB_OUTPUT
";
    let caller = "on: push\njobs:\n  job1:\n    uses: octo-org/example-repo/.github/workflows/called-workflow.yml@v1\n  job2:\n    runs-on: ubuntu-latest\n    needs: job1\n    steps:\n      - run: echo ${{ needs.job1.outputs.firstword }} ${{ needs.job1.outputs.secondword }}\n";
    let w = workflow(caller);
    let source = files(&[("octo-org/example-repo/.github/workflows/called-workflow.yml@v1", called)]);
    let mut run = Run::new(&w, Base::default()).unwrap().with_called(source);
    calls(run.expand("job1").unwrap());
    let job = cells(run.expand("job1/example_job").unwrap()).remove(0);
    assert_eq!(job.name, "job1 / Generate output");
    let outputs = vec![("output1".to_string(), "hello".to_string()), ("output2".to_string(), "world".to_string())];
    run.finish("job1/example_job", Done { result: JobResult::Success, outputs }).unwrap();
    assert_eq!(run.ready(), ["job2"]);
    let job2 = cells(run.expand("job2").unwrap()).remove(0);
    assert_eq!(
        job2.needs.to_json_line(),
        r#"{"job1":{"result":"success","outputs":{"firstword":"hello","secondword":"world"}}}"#
    );
}

#[test]
fn the_result_of_a_call_is_that_of_its_jobs() {
    let called = "on: workflow_call\njobs:\n  a:\n    runs-on: x\n  b:\n    if: false\n    runs-on: x\n";
    let result = |a: JobResult| {
        let w = workflow("on: push\njobs:\n  c:\n    uses: ./x.yml\n");
        let mut run = Run::new(&w, Base::default()).unwrap().with_called(files(&[("./x.yml", called)]));
        calls(run.expand("c").unwrap());
        assert!(matches!(run.expand("c/b").unwrap(), Outcome::Skipped(_)));
        assert_eq!(run.result("c"), None);
        cells(run.expand("c/a").unwrap());
        run.finish("c/a", Done::of(a)).unwrap();
        run.result("c").unwrap()
    };
    assert_eq!(result(JobResult::Failure), JobResult::Failure);
    assert_eq!(result(JobResult::Cancelled), JobResult::Cancelled);
    assert_eq!(result(JobResult::Success), JobResult::Success);
    // Each called job skipped: the call succeeded.
    assert_eq!(result(JobResult::Skipped), JobResult::Success);
}

#[test]
fn a_matrix_calls_once_for_each_cell() {
    let called = "on:\n  workflow_call:\n    inputs:\n      target: {type: string, required: true}\n    outputs:\n      where: {value: '${{ inputs.target }}'}\njobs:\n  deploy:\n    runs-on: x\n    steps:\n      - run: echo\n";
    let caller = "on: push\njobs:\n  ReuseableMatrixJobForDeployment:\n    strategy:\n      matrix:\n        target: [dev, stage, prod]\n    uses: octocat/octo-repo/.github/workflows/deployment.yml@main\n    with:\n      target: ${{ matrix.target }}\n";
    let w = workflow(caller);
    let source = files(&[("octocat/octo-repo/.github/workflows/deployment.yml@main", called)]);
    let mut run = Run::new(&w, Base::default()).unwrap().with_called(source);
    let all = calls(run.expand("ReuseableMatrixJobForDeployment").unwrap());
    let targets: Vec<String> = all.iter().map(|c| c.inputs.get("target").unwrap().to_text()).collect();
    assert_eq!(targets, ["dev", "stage", "prod"]);
    assert_eq!(all[1].jobs, ["ReuseableMatrixJobForDeployment[1]/deploy"]);
    let names: Vec<String> =
        run.ready().iter().map(|id| cells(run.clone().expand(id).unwrap())[0].name.clone()).collect();
    assert_eq!(names[2], "ReuseableMatrixJobForDeployment (prod) / deploy");
    // The outputs are those of the cell that ended last.
    for i in [2, 0, 1] {
        let id = format!("ReuseableMatrixJobForDeployment[{i}]/deploy");
        cells(run.expand(&id).unwrap());
        run.finish(&id, Done::of(JobResult::Success)).unwrap();
    }
    assert!(run.is_finished());
    let after = "on: push\njobs:\n  m:\n    strategy:\n      matrix:\n        target: [dev, stage]\n    uses: ./d.yml\n    with:\n      target: ${{ matrix.target }}\n  after:\n    needs: m\n    runs-on: x\n";
    let w = workflow(after);
    let all = plan_with(&w, Base::default(), JobResult::Success, files(&[("./d.yml", called)])).unwrap();
    let ids: Vec<&str> = all.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["m", "m[0]/deploy", "m[1]/deploy", "after"]);
    let Outcome::Run(last) = &all[3].1 else { panic!() };
    assert_eq!(last[0].needs.get("m").unwrap().get("outputs").unwrap().to_json_line(), r#"{"where":"stage"}"#);
}

#[test]
fn a_nested_call_inherits_and_names_its_caller() {
    let one = "on:\n  workflow_call:\n    secrets:\n      token:\njobs:\n  inner:\n    uses: ./two.yml\n    secrets: inherit\n";
    let two = "on: workflow_call\njobs:\n  leaf:\n    runs-on: x\n    steps:\n      - run: echo\n";
    let source = files(&[("o/r/.github/workflows/one.yml@v1", one), ("./two.yml", two)]);
    let w = workflow(
        "on: push\njobs:\n  outer:\n    uses: o/r/.github/workflows/one.yml@v1\n    secrets:\n      token: ${{ secrets.T }}\n",
    );
    let all = plan_with(&w, Base::default(), JobResult::Success, source.clone()).unwrap();
    let ids: Vec<&str> = all.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["outer", "outer/inner", "outer/inner/leaf"]);
    let Outcome::Run(leaf) = &all[2].1 else { panic!() };
    assert_eq!(leaf[0].name, "outer / inner / leaf");
    assert_eq!(leaf[0].secrets.names(), Some(vec!["token"]));
    let asked = source.asked.lock().unwrap().clone();
    assert_eq!(asked[1], ("./two.yml".to_string(), Some("o/r/.github/workflows/one.yml@v1".to_string())));
    assert_eq!(asked[0].1, None);
}

#[test]
fn the_limits_of_github_are_refused_by_name() {
    let looped = "on: workflow_call\njobs:\n  again:\n    uses: ./self.yml\n";
    let w = workflow("on: push\njobs:\n  start:\n    uses: ./self.yml\n");
    let mut run = Run::new(&w, Base::default()).unwrap().with_called(files(&[("./self.yml", looped)]));
    let mut called = 0;
    let mut next = || {
        let id = run.ready().remove(0);
        match run.expand(&id) {
            Ok(Outcome::Called(_)) => {
                called += 1;
                None
            }
            Ok(other) => panic!("{other:?}"),
            Err(e) => Some(e),
        }
    };
    let e = (0..20).find_map(|_| next()).expect("the limit of levels stops the loop");
    // Ten levels: the top workflow and nine called ones.
    assert_eq!(called, 9);
    assert_eq!(root(&e), &ErrorKind::CallLimit { what: "levels of workflows", max: 10 });
    assert_eq!(
        e.to_string(),
        "in the called workflow `./self.yml`: jobs.again.uses (line 3, column 3): a run has at most 10 levels of workflows"
    );
    let base = Base { calls: CallLimits { workflows: 2, ..CallLimits::default() }, ..Base::default() };
    let leaf = "on: workflow_call\njobs:\n  a:\n    runs-on: x\n";
    let w = workflow(
        "on: push\njobs:\n  a:\n    uses: ./a.yml\n  b:\n    uses: ./b.yml\n  c:\n    uses: ./a.yml\n  d:\n    uses: ./c.yml\n",
    );
    let source = files(&[("./a.yml", leaf), ("./b.yml", leaf), ("./c.yml", leaf)]);
    let e = plan_with(&w, base, JobResult::Success, source).unwrap_err();
    assert_eq!(e.path, "jobs.d.uses");
    assert_eq!(e.to_string(), "jobs.d.uses (line 9, column 3): a run has at most 2 different called workflows");
}

#[test]
fn the_jobs_of_a_run_with_its_calls_have_a_limit() {
    let leaf = "on: workflow_call\njobs:\n  a:\n    runs-on: x\n  b:\n    runs-on: x\n";
    let w = workflow("on: push\njobs:\n  m:\n    strategy:\n      matrix:\n        n: [1, 2]\n    uses: ./l.yml\n");
    let run = |jobs| {
        let base = Base { calls: CallLimits { jobs, ..CallLimits::default() }, ..Base::default() };
        plan_with(&w, base, JobResult::Success, files(&[("./l.yml", leaf)]))
    };
    // The caller and two cells of two jobs.
    assert_eq!(run(5).unwrap().len(), 5);
    assert_eq!(run(4).unwrap_err().kind, ErrorKind::CallLimit { what: "jobs", max: 4 });
}

#[test]
fn a_called_file_that_fails_is_an_error_by_name() {
    let w = workflow("on: push\njobs:\n  c:\n    uses: ./x.yml\n");
    let fail = |list: &[(&'static str, &str)]| plan_with(&w, Base::default(), JobResult::Success, files(list));
    let e = fail(&[]).unwrap_err();
    assert_eq!(e.kind, ErrorKind::CallText { uses: "./x.yml".into(), why: "no file ./x.yml".into() });
    let e = fail(&[("./x.yml", "on: push\njobs:\n  a:\n    runs-on: x\n")]).unwrap_err();
    assert_eq!(root(&e), &ErrorKind::Missing("workflow_call"));
    let e = fail(&[("./x.yml", "on: workflow_call\njobs:\n  a:\n    runs-on: ${{ matrix.os }}\n")]).unwrap_err();
    assert_eq!(
        e.to_string(),
        "in the called workflow `./x.yml`: jobs.a.runs-on (line 4, column 14): the value must be a label, a list of labels or {group, labels}, not the null null"
    );
    let bad = "on:\n  workflow_call:\n    inputs:\n      a: {type: text}\njobs:\n  a:\n    runs-on: x\n";
    let e = fail(&[("./x.yml", bad)]).unwrap_err();
    assert!(matches!(root(&e), ErrorKind::Type(_)), "{e}");
}
