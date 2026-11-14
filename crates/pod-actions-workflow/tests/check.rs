//! The check of `on:` (T-578): each example of `on:` in GitHub's "Events
//! that trigger workflows" and "Workflow syntax" passes, and each kind of
//! fault that GitHub refuses is found, all of them, in the order of the
//! file. `parse` takes each file still.

use pod_actions_workflow::{Error, ErrorKind, Workflow, event_keys, parse};

/// The `on:` of each example of the two documents, as they give it.
const EXAMPLES: [&str; 64] = [
    "on:\n  branch_protection_rule:\n    types: [created, deleted]",
    "on:\n  check_run:\n    types: [rerequested, completed]",
    "on:\n  check_suite:\n    types: [completed]",
    "on:\n  create",
    "on:\n  delete",
    "on:\n  deployment",
    "on:\n  deployment_status",
    "on:\n  discussion:\n    types: [created, edited, answered]",
    "on:\n  discussion_comment:\n    types: [created, deleted]",
    "on:\n  fork",
    "on:\n  gollum",
    "on:\n  image_version:\n    names:\n    - \"MyNewImage\"\n    - \"MyOtherImage\"\n    versions:\n    - 1.*\n    - 2.*",
    "on:\n  issue_comment:\n    types: [created, deleted]",
    "on: issue_comment",
    "on:\n  issues:\n    types: [opened, edited, milestoned]",
    "on:\n  issues:\n    types: [field_added, field_removed]",
    "on:\n  label:\n    types: [created, deleted]",
    "on:\n  pull_request:\n    branches: [main]\n  merge_group:\n    types: [checks_requested]\n    branches: [main]",
    "on:\n  milestone:\n    types: [opened, deleted]",
    "on:\n  page_build",
    "on:\n  public",
    "on:\n  pull_request:\n    types: [opened, reopened]",
    "on:\n  pull_request:\n    types: [review_requested]",
    "on:\n  pull_request:\n    types:\n      - opened\n    branches:\n      - 'releases/**'",
    "on:\n  pull_request:\n    types:\n      - opened",
    "on:\n  pull_request:\n    paths:\n      - '**.js'",
    "on:\n  pull_request:\n    types:\n      - closed",
    "on:\n  pull_request_review:\n    types: [edited, dismissed]",
    "on:\n  pull_request_review:\n    types: [submitted]",
    "on:\n  pull_request_review_comment:\n    types: [created, deleted]",
    "on:\n  pull_request_target:\n    types: [assigned, opened, synchronize, reopened]",
    "on:\n  pull_request_target:\n    types:\n      - opened\n    branches:\n      - 'releases/**'",
    "on:\n  pull_request_target:\n    types:\n      - opened",
    "on:\n  pull_request_target:\n    paths:\n      - '**.js'",
    "on:\n  pull_request_target:\n    types:\n      - closed",
    "on:\n  push",
    "on:\n  push:\n    branches:\n      - 'main'\n      - 'releases/**'",
    "on:\n  push:\n    tags:\n      - v1.**",
    "on:\n  push:\n    paths:\n      - '**.js'",
    "on:\n  registry_package:\n    types: [published]",
    "on:\n  release:\n    types: [published]",
    "on:\n  repository_dispatch:\n    types: [test_result]",
    "on:\n     schedule:\n       - cron: \"15 4,5 * * *\"",
    "on:\n  status",
    "on:\n  watch:\n    types: [started]",
    "on: workflow_call",
    "on: workflow_dispatch",
    "on:\n  workflow_dispatch:\n    inputs:\n      logLevel:\n        description: 'Log level'\n        required: true\n        default: 'warning'\n        type: choice\n        options:\n        - info\n        - warning\n        - debug\n      tags:\n        description: 'Test scenario tags'\n        required: false\n        type: boolean\n      environment:\n        description: 'Environment to run tests against'\n        type: environment\n        required: true",
    "on:\n  workflow_run:\n    workflows: [Run Tests]\n    types:\n      - completed",
    "on:\n  workflow_run:\n    workflows: [Staging, Lab]\n    types:\n      - completed",
    "on:\n  workflow_run:\n    workflows: [Build]\n    types: [completed]",
    "on:\n  workflow_run:\n    workflows: [Build]\n    types: [requested]\n    branches: [canary]",
    "on:\n  pull_request:",
    "on:\n  workflow_run:\n    workflows: [Upload data]\n    types:\n      - completed",
    "on:\n  merge_group:\n    types: [checks_requested]\n    branches:\n      - main\n      - 'releases/**'",
    "on:\n  merge_group:\n    types: [checks_requested]\n    branches-ignore:\n      - canary\n      - 'releases/**-alpha'",
    "on:\n  merge_group:\n    types: [checks_requested]\n    branches:\n      - 'releases/**'\n      - '!releases/**-alpha'",
    "on:\n  workflow_call:\n    inputs:\n      username:\n        description: 'A username passed from the caller workflow'\n        default: 'john-doe'\n        required: false\n        type: string",
    "on:\n  workflow_call:\n    outputs:\n      workflow_output1:\n        description: \"The first job output\"\n        value: ${{ jobs.my_job.outputs.job_output1 }}\n      workflow_output2:\n        description: \"The second job output\"\n        value: ${{ jobs.my_job.outputs.job_output2 }}",
    "on:\n  workflow_call:\n    secrets:\n      access-token:\n        description: 'A token passed from the caller workflow'\n        required: false",
    "on: push",
    "on: [push]",
    "on:\n  workflow_dispatch:",
    "on:\n  workflow_call:",
];

const JOBS: &str = "\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: echo\n";

fn workflow(on: &str) -> Workflow {
    parse(&format!("{on}{JOBS}")).unwrap().workflow
}

/// The faults of `on`, below `on:`.
fn faults(on: &str) -> Vec<Error> {
    workflow(&format!("on:\n{on}")).check()
}

fn kinds(on: &str) -> Vec<ErrorKind> {
    faults(on).into_iter().map(|e| e.kind).collect()
}

fn not_taken(event: &str, key: &str) -> ErrorKind {
    ErrorKind::KeyNotTaken { event: event.into(), key: key.into() }
}

#[test]
fn each_example_of_the_documents_passes_the_check() {
    for on in EXAMPLES {
        assert_eq!(workflow(on).check(), [], "{on}");
    }
}

#[test]
fn tags_on_a_pull_request_and_other_keys_that_an_event_does_not_take_are_faults() {
    assert_eq!(kinds("  pull_request:\n    tags: [v1]"), [not_taken("pull_request", "tags")]);
    assert_eq!(
        kinds("  pull_request_target:\n    tags-ignore: [v1]"),
        [not_taken("pull_request_target", "tags-ignore")]
    );
    assert_eq!(kinds("  push:\n    types: [created]"), [not_taken("push", "types")]);
    assert_eq!(kinds("  issues:\n    branches: [main]"), [not_taken("issues", "branches")]);
    assert_eq!(kinds("  merge_group:\n    paths: [a]"), [not_taken("merge_group", "paths")]);
    assert_eq!(kinds("  workflow_run:\n    tags: [v1]"), [not_taken("workflow_run", "tags")]);
    assert_eq!(kinds("  workflow_dispatch:\n    outputs: {}"), [not_taken("workflow_dispatch", "outputs")]);
    assert_eq!(kinds("  fork:\n    types: [created]"), [not_taken("fork", "types")]);
    let e = &faults("  push:\n  pull_request:\n    types: [opened]\n    tags: [v1]")[0];
    assert_eq!(e.to_string(), "on.pull_request (line 5, column 5): the event `pull_request` takes no `tags`");
    // `parse` and `takes` do not change: the file parses, and `takes` reads
    // no filter that the event does not take.
    let w = workflow("on:\n  pull_request:\n    tags: [v1]");
    assert!(w.takes(&pod_actions_workflow::Trigger::new("pull_request")).is_ok());
}

#[test]
fn a_filter_with_its_ignore_form_is_a_fault_on_each_event() {
    for (on, key) in [
        ("  push:\n    branches: [a]\n    branches-ignore: [b]", "branches"),
        ("  push:\n    tags: [a]\n    tags-ignore: [b]", "tags"),
        ("  pull_request:\n    paths: [a]\n    paths-ignore: [b]", "paths"),
        ("  merge_group:\n    branches: [a]\n    branches-ignore: [b]", "branches"),
        ("  workflow_run:\n    branches: [a]\n    branches-ignore: [b]", "branches"),
    ] {
        assert_eq!(kinds(on), [ErrorKind::FilterWithIgnore(key)], "{on}");
    }
}

#[test]
fn an_event_that_github_does_not_know_is_a_fault_in_each_form_of_on() {
    let unknown = || vec![ErrorKind::UnknownEvent("pull_request_comment".into())];
    assert_eq!(kinds("  pull_request_comment:\n    types: [created]"), unknown());
    let w = |on: &str| workflow(on).check().into_iter().map(|e| e.kind).collect::<Vec<_>>();
    assert_eq!(w("on: pull_request_comment"), unknown());
    assert_eq!(w("on: [push, pull_request_comment]"), unknown());
    assert_eq!(event_keys("pull_request_comment"), None);
    assert_eq!(event_keys("push").map(|k| k.contains(&"tags")), Some(true));
    assert_eq!(event_keys("pull_request").map(|k| k.contains(&"tags")), Some(false));
}

#[test]
fn schedule_is_a_list_of_cron_entries() {
    assert_eq!(kinds("  schedule:\n    cron: '0 0 * * *'"), [ErrorKind::Type("a list of `cron` entries")]);
    assert_eq!(
        kinds("  schedule:\n    - crons: '0 0 * * *'"),
        [not_taken("schedule", "crons"), ErrorKind::Missing("cron")]
    );
    assert_eq!(kinds("  schedule:\n    - cron: 5"), [ErrorKind::Type("a cron line")]);
    assert_eq!(kinds("  schedule:\n    - '0 0 * * *'"), [ErrorKind::Type("a mapping with `cron`")]);
}

#[test]
fn each_fault_of_on_is_found_in_the_order_of_the_file() {
    let on = "  push:\n    branches: ['!main']\n    tags: ['v[2']\n  pull_request:\n    tags: [v1]\n    types: 3\n  \
              nothing:\n  label:\n    types: [created]\n    paths: [a]";
    let found = faults(on);
    let paths: Vec<&str> = found.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        paths,
        ["on.push.branches", "on.push.tags", "on.pull_request", "on.pull_request.types", "on.nothing", "on.label"],
        "{found:#?}"
    );
    assert!(matches!(found[0].kind, ErrorKind::OnlyNegated("branches")));
    assert!(matches!(&found[1].kind, ErrorKind::BadPattern { pattern, .. } if pattern == "v[2"));
    assert_eq!(found[2].kind, not_taken("pull_request", "tags"));
    assert!(matches!(found[3].kind, ErrorKind::Type(_)));
    assert_eq!(found[4].kind, ErrorKind::UnknownEvent("nothing".into()));
    assert_eq!(found[5].kind, not_taken("label", "paths"));
    // A value of `on:` of another kind.
    assert!(matches!(workflow("on: 3").check()[..], [Error { kind: ErrorKind::Type(_), .. }]));
    assert!(matches!(kinds("  push: [main]")[..], [ErrorKind::Type(_)]));
}
