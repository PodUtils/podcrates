//! The fuzz targets of `pod-actions-expr` and `pod-actions-workflow`.

use pod_actions_expr::{Condition, Contexts, Expr, Template, Value};
use pod_actions_workflow::{Base, JobResult, Run, Trigger, plan};

use crate::text;

fn contexts() -> Contexts {
    let github = Value::from_json(
        r#"{"event_name": "push", "ref": "refs/heads/main", "event": {"labels": [{"name": "bug"}, {"name": "x"}]}}"#,
    )
    .unwrap_or_default();
    let mut c = Contexts::new();
    c.insert("github", github)
        .insert("matrix", Value::object([("os", Value::from("linux")), ("n", Value::from(3))]))
        .insert("needs", Value::object([("a", Value::object([("result", Value::from("success"))]))]))
        .insert("env", Value::object([("list", Value::from("[1, 2]"))]));
    // A small limit keeps each run short: a format of a format stops at it.
    c.limits.max_bytes = 1 << 16;
    c
}

/// An expression, a template and a condition (`pod_actions_expr`; new in
/// `pod-actions-expr`, T-251): untrusted text of a workflow. No panic, no
/// stack overflow (the depth limit holds before the recursion); the same
/// expression gives the same value twice; a condition that parses
/// evaluates to a boolean or fails by name.
pub fn actions_expr(data: &[u8]) {
    let Some(input) = text(data) else { return };
    let contexts = contexts();
    if let Ok(expr) = Expr::parse(input) {
        let first = expr.evaluate(&contexts).map(|v| v.to_json_line());
        let second = expr.evaluate(&contexts).map(|v| v.to_json_line());
        assert_eq!(first, second, "{input:?}");
        let _ = expr.contexts();
        let _ = expr.has_status_function();
    }
    if let Ok(template) = Template::parse(input) {
        let _ = template.evaluate(&contexts);
    }
    if let Ok(condition) = Condition::parse(input) {
        assert!(condition.text().starts_with("success()") || !condition.is_implicit());
        let _ = condition.evaluate(&contexts);
    }
    // A document that `fromJSON` reads reads back from its `toJSON`, but for
    // a number past the range of a double (GitHub writes `Infinity`).
    if let Ok(value) = Value::from_json(input) {
        let line = value.to_json_line();
        if !line.contains("Infinity") {
            let again = Value::from_json(&value.to_json()).map(|v| v.to_json_line());
            assert_eq!(again.ok(), Some(line), "{input:?}");
        }
    }
}

/// A workflow file (`pod_actions_workflow::parse`, then a dry run; new in
/// `pod-actions-workflow`, T-251): untrusted YAML. No panic; the limits of
/// the YAML hold against aliases; the filters of `on:` (T-572) read and
/// decide; a file that parses has a graph or an error by name, and its dry
/// run gives each job once.
pub fn actions_workflow(data: &[u8]) {
    let Some(input) = text(data) else { return };
    let Ok(parsed) = pod_actions_workflow::parse(input) else { return };
    let workflow = parsed.workflow;
    if let Ok(on) = workflow.triggers() {
        let paths = ["src/a.rs".to_string(), "docs/README.md".to_string()];
        for event in &on.events {
            for git_ref in ["refs/heads/releases/v1", "refs/tags/v1.0"] {
                let t = Trigger {
                    action: Some("opened"),
                    git_ref: Some(git_ref),
                    paths: Some(&paths),
                    workflow: Some("Build"),
                    ..Trigger::new(&event.name)
                };
                assert_eq!(Some(on.takes(&t)), workflow.takes(&t).ok(), "{input:?}");
            }
        }
    }
    let Ok(run) = Run::new(&workflow, Base::default()) else { return };
    let order: Vec<String> = run.graph().order().map(str::to_string).collect();
    assert_eq!(order.len(), workflow.jobs.len());
    if let Ok(all) = plan(&workflow, Base::default(), JobResult::Success) {
        let ids: Vec<&str> = all.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, order.iter().map(String::as_str).collect::<Vec<_>>());
    }
}
