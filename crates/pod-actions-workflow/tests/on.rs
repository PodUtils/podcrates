//! The filters of `on:` (PodUtils/podrunner#97): the examples of GitHub's
//! "Workflow syntax" and "Events that trigger workflows" for `types`,
//! `branches`, `tags`, `paths` and their `-ignore` forms.

use pod_actions_workflow::{ErrorKind, Trigger, Workflow, parse};

fn workflow(on: &str) -> Workflow {
    let text = format!("on:\n{on}\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: echo\n");
    parse(&text).unwrap().workflow
}

fn push(w: &Workflow, git_ref: &str) -> bool {
    w.takes(&Trigger { git_ref: Some(git_ref), ..Trigger::new("push") }).unwrap()
}

fn files(w: &Workflow, git_ref: &str, paths: &[&str]) -> bool {
    let paths: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
    w.takes(&Trigger { git_ref: Some(git_ref), paths: Some(&paths), ..Trigger::new("push") }).unwrap()
}

fn error(on: &str) -> ErrorKind {
    workflow(on).takes(&Trigger::new("push")).unwrap_err().kind
}

#[test]
fn each_form_of_on_names_its_events() {
    let one = parse("on: push\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: echo\n").unwrap().workflow;
    assert!(one.takes(&Trigger::new("push")).unwrap());
    assert!(!one.takes(&Trigger::new("fork")).unwrap());
    let list = workflow("  [push, fork]");
    assert!(list.takes(&Trigger::new("fork")).unwrap() && !list.takes(&Trigger::new("issues")).unwrap());
    let map = workflow("  push:\n  schedule:\n    - cron: '30 5 * * 1'\n  workflow_dispatch:\n    inputs: {}");
    for event in ["push", "schedule", "workflow_dispatch"] {
        assert!(map.takes(&Trigger::new(event)).unwrap(), "{event}");
    }
    assert!(push(&map, "refs/tags/v1") && push(&map, "refs/heads/any/branch"));
    assert!(matches!(error("  - push\n  - 3"), ErrorKind::Type(_)));
}

#[test]
fn types_take_their_actions_and_pull_request_has_three_by_default() {
    let w = workflow("  label:\n    types: [created, edited]\n  issues: {types: opened}\n  pull_request:");
    let t = |event, action| w.takes(&Trigger { action: Some(action), ..Trigger::new(event) }).unwrap();
    assert!(t("label", "created") && t("label", "edited") && !t("label", "deleted"));
    assert!(t("issues", "opened") && !t("issues", "closed"));
    assert!(t("pull_request", "opened") && t("pull_request", "synchronize") && t("pull_request", "reopened"));
    assert!(!t("pull_request", "closed") && !t("pull_request", "labeled"));
    // An action that the caller does not know passes `types`.
    assert!(w.takes(&Trigger::new("label")).unwrap());
    let all = workflow("  pull_request:\n    types: [closed]\n  check_run:");
    assert!(all.takes(&Trigger { action: Some("closed"), ..Trigger::new("pull_request") }).unwrap());
    assert!(all.takes(&Trigger { action: Some("rerequested"), ..Trigger::new("check_run") }).unwrap());
}

#[test]
fn branches_and_tags_take_the_examples_of_the_documents() {
    let w = workflow(
        "  push:\n    branches:\n      - main\n      - 'mona/octocat'\n      - 'releases/**'\n    tags:\n      - v2\n      - v1.*",
    );
    for yes in ["refs/heads/main", "refs/heads/mona/octocat", "refs/heads/releases/10", "refs/heads/releases/beta/mona"]
    {
        assert!(push(&w, yes), "{yes}");
    }
    assert!(push(&w, "refs/tags/v2") && push(&w, "refs/tags/v1.9.1"));
    assert!(!push(&w, "refs/heads/mona") && !push(&w, "refs/tags/v3") && !push(&w, "refs/heads/v2"));
    let ignore = workflow(
        "  push:\n    branches-ignore:\n      - 'mona/octocat'\n      - 'releases/**-alpha'\n    tags-ignore:\n      - v2\n      - v1.*",
    );
    assert!(!push(&ignore, "refs/heads/mona/octocat") && !push(&ignore, "refs/heads/releases/beta/3-alpha"));
    assert!(push(&ignore, "refs/heads/releases/10") && push(&ignore, "refs/heads/main"));
    assert!(!push(&ignore, "refs/tags/v2") && !push(&ignore, "refs/tags/v1.9") && push(&ignore, "refs/tags/v3"));
    let pr = workflow("  pull_request:\n    branches: ['releases/**']");
    let base = |b| pr.takes(&Trigger { git_ref: Some(b), ..Trigger::new("pull_request") }).unwrap();
    assert!(base("releases/10") && base("refs/heads/releases/10") && !base("main"));
}

#[test]
fn a_filter_of_one_kind_of_ref_stops_the_other_kind() {
    let tags = workflow("  push:\n    tags: ['v*']");
    assert!(push(&tags, "refs/tags/v1") && !push(&tags, "refs/heads/main") && !push(&tags, "refs/heads/v1"));
    let branches = workflow("  push:\n    branches-ignore: [dev]");
    assert!(push(&branches, "refs/heads/main") && !push(&branches, "refs/tags/v1"));
    let both = workflow("  push:\n    branches: [main]\n    tags-ignore: [v0*]");
    assert!(push(&both, "refs/heads/main") && push(&both, "refs/tags/v1") && !push(&both, "refs/tags/v0.1"));
    // A ref that the caller does not know passes the ref filters.
    assert!(tags.takes(&Trigger::new("push")).unwrap());
}

#[test]
fn a_later_bang_removes_a_match_and_a_later_pattern_adds_it_again() {
    let w = workflow("  push:\n    branches:\n      - 'releases/**'\n      - '!releases/**-alpha'");
    assert!(push(&w, "refs/heads/releases/10") && !push(&w, "refs/heads/releases/beta/3-alpha"));
    let paths = workflow("  push:\n    paths:\n      - '*.md'\n      - '!README*'\n      - README*");
    assert!(files(&paths, "refs/heads/main", &["README.md"]) && files(&paths, "refs/heads/main", &["hello.md"]));
    let order = workflow("  push:\n    paths:\n      - '*.md'\n      - '!README*'");
    assert!(!files(&order, "refs/heads/main", &["README.md"]) && files(&order, "refs/heads/main", &["hi.md"]));
    let sub = workflow("  push:\n    paths:\n      - 'sub-project/**'\n      - '!sub-project/docs/**'");
    assert!(files(&sub, "refs/heads/main", &["sub-project/app.js"]));
    assert!(!files(&sub, "refs/heads/main", &["sub-project/docs/readme.md"]));
}

#[test]
fn paths_take_the_examples_of_the_cheat_sheet() {
    let cases: &[(&str, &[&str], &[&str])] = &[
        ("'*'", &["README.md", "server.rb"], &["a/b"]),
        ("'*.jsx?'", &["page.js", "page.jsx"], &["page.jsxx"]),
        ("'**'", &["all/the/files.md"], &[]),
        ("'*.js'", &["app.js", "index.js"], &["js/index.js"]),
        ("'**.js'", &["index.js", "js/index.js", "src/js/app.js"], &["index.ts"]),
        ("'docs/*'", &["docs/README.md", "docs/file.txt"], &["docs/mona/octocat.txt"]),
        ("'docs/**'", &["docs/README.md", "docs/mona/octocat.txt"], &["src/docs"]),
        ("'docs/**/*.md'", &["docs/README.md", "docs/mona/hello-world.md", "docs/a/markdown/file.md"], &["docs.md"]),
        ("'**/docs/**'", &["docs/hello.md", "dir/docs/my-file.txt", "space/docs/plan/space.doc"], &["mydocs/a"]),
        ("'**/README.md'", &["README.md", "js/README.md"], &["README.mdx"]),
        ("'**/*src/**'", &["a/src/app.js", "my-src/code/js/app.js"], &["src.js"]),
        ("'**/*-post.md'", &["my-post.md", "path/their-post.md"], &["post.md"]),
        ("'**/migrate-*.sql'", &["migrate-10909.sql", "db/migrate-v1.0.sql", "db/sept/migrate-v1.sql"], &[]),
    ];
    for (pattern, yes, no) in cases {
        let w = workflow(&format!("  push:\n    paths: [{pattern}]"));
        for f in *yes {
            assert!(files(&w, "refs/heads/main", &[f]), "{pattern} must match {f}");
        }
        for f in *no {
            assert!(!files(&w, "refs/heads/main", &[f]), "{pattern} must not match {f}");
        }
    }
}

#[test]
fn paths_apply_after_branches_and_not_to_a_tag() {
    let w = workflow("  push:\n    branches: ['releases/**']\n    paths: ['**.js']");
    assert!(files(&w, "refs/heads/releases/1", &["app.js", "README.md"]));
    assert!(!files(&w, "refs/heads/releases/1", &["README.md"]));
    assert!(!files(&w, "refs/heads/main", &["app.js"]));
    let tag = workflow("  push:\n    tags: ['v*']\n    paths: ['src/**']");
    assert!(files(&tag, "refs/tags/v1", &["docs/a.md"]));
    let ignore = workflow("  push:\n    paths-ignore: ['docs/**']");
    assert!(!files(&ignore, "refs/heads/main", &["docs/a.md", "docs/b/c.md"]));
    assert!(files(&ignore, "refs/heads/main", &["docs/a.md", "src/x.rs"]));
    assert!(!files(&ignore, "refs/heads/main", &[]) && !files(&w, "refs/heads/releases/1", &[]));
    // Paths that the caller does not know pass the path filters.
    assert!(push(&w, "refs/heads/releases/1"));
}

#[test]
fn a_filter_with_its_ignore_form_is_refused() {
    for key in ["branches", "tags", "paths"] {
        let on = format!("  push:\n    {key}: [a]\n    {key}-ignore: [b]");
        assert_eq!(error(&on), ErrorKind::FilterWithIgnore(key), "{key}");
    }
    // Each event is read: a bad filter of another event fails too.
    let e = workflow("  push:\n  pull_request:\n    paths: [a]\n    paths-ignore: [b]").takes(&Trigger::new("push"));
    let e = e.unwrap_err();
    assert_eq!(e.path, "on.pull_request");
    assert_eq!(e.to_string(), "on.pull_request (line 5, column 19): an event has `paths` or `paths-ignore`, not both");
}

#[test]
fn a_bad_filter_is_refused_by_name() {
    assert_eq!(error("  push:\n    branches: ['!main']"), ErrorKind::OnlyNegated("branches"));
    assert_eq!(error("  push:\n    paths: []"), ErrorKind::OnlyNegated("paths"));
    let e = workflow("  push:\n    tags: [v1, 'v[2']").takes(&Trigger::new("push")).unwrap_err();
    assert_eq!((e.path.as_str(), e.mark.unwrap().line), ("on.push.tags", 3));
    assert!(matches!(e.kind, ErrorKind::BadPattern { ref pattern, .. } if pattern == "v[2"));
    assert!(matches!(error("  push:\n    branches: [1.0]"), ErrorKind::Type(_)));
    assert!(matches!(error("  push:\n    types: {a: b}"), ErrorKind::Type(_)));
    // An ignore form may hold `!` only: it then leaves nothing out.
    let w = workflow("  push:\n    branches-ignore: ['!main']");
    assert!(push(&w, "refs/heads/dev"));
}

#[test]
fn workflow_run_takes_the_workflows_that_it_names() {
    let w =
        workflow("  workflow_run:\n    workflows: [Build, 'Run Tests']\n    types: [completed]\n    branches: [main]");
    let run = |name, branch| {
        let t = Trigger {
            action: Some("completed"),
            git_ref: Some(branch),
            workflow: Some(name),
            ..Trigger::new("workflow_run")
        };
        w.takes(&t).unwrap()
    };
    assert!(run("Build", "main") && run("Run Tests", "main"));
    assert!(!run("Deploy", "main") && !run("build", "main") && !run("Build", "dev"));
    let requested = Trigger { action: Some("requested"), workflow: Some("Build"), ..Trigger::new("workflow_run") };
    assert!(!w.takes(&requested).unwrap());
    // A workflow name that the caller does not know passes `workflows`.
    assert!(w.takes(&Trigger::new("workflow_run")).unwrap());
    let one = workflow("  workflow_run:\n    workflows: Build");
    assert!(!one.takes(&Trigger { workflow: Some("Lint"), ..Trigger::new("workflow_run") }).unwrap());
    assert!(matches!(error("  workflow_run:\n    workflows: [1]"), ErrorKind::Type(_)));
}
