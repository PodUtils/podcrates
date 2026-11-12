//! Templates (`${{ }}` in a text) and the condition of an `if:`, as the
//! runner's template reader makes them.

use pod_actions_expr::{Condition, Contexts, ErrorKind, Status, Template, Value};

fn contexts() -> Contexts {
    let mut c = Contexts::new();
    c.insert("github", Value::object([("ref", Value::from("refs/heads/main")), ("event_name", Value::from("push"))]))
        .insert("matrix", Value::object([("os", Value::from("ubuntu-latest")), ("node", Value::from(20))]))
        .insert("needs", Value::object([("build", Value::object([("result", Value::from("failure"))]))]));
    c
}

fn template(text: &str) -> Value {
    Template::parse(text).and_then(|t| t.evaluate(&contexts())).unwrap_or_else(|e| panic!("{text}: {e}"))
}

#[test]
fn one_expression_keeps_its_kind() {
    assert_eq!(template("${{ matrix.node }}"), Value::from(20));
    assert_eq!(template("${{ fromJSON('[1, 2]') }}"), Value::array([Value::from(1), Value::from(2)]));
    assert_eq!(template("${{ null }}"), Value::Null);
    assert_eq!(template("${{matrix.os}}"), Value::from("ubuntu-latest"));
}

#[test]
fn text_around_an_expression_gives_a_string() {
    assert_eq!(template("node-${{ matrix.node }}"), Value::from("node-20"));
    assert_eq!(template("${{ matrix.os }}/${{ matrix.node }}"), Value::from("ubuntu-latest/20"));
    assert_eq!(template(" ${{ true }}"), Value::from(" true"));
    assert_eq!(template("${{ null }}x"), Value::from("x"));
    assert_eq!(template("plain text"), Value::from("plain text"));
    assert_eq!(template(""), Value::from(""));
    assert_eq!(template("{ } {{ }} $"), Value::from("{ } {{ }} $"));
}

#[test]
fn an_expression_ends_at_the_first_close_outside_a_string() {
    assert_eq!(template("${{ '}}' }}"), Value::from("}}"));
    assert_eq!(template("${{ 'it''s }}' }}!"), Value::from("it's }}!"));
    assert_eq!(template("${{ '{{ this is a literal }}' }}"), Value::from("{{ this is a literal }}"));
    assert_eq!(template("a ${{ 'b' }} c ${{ 'd' }}"), Value::from("a b c d"));
}

#[test]
fn a_template_reports_its_errors_at_their_place() {
    let e = Template::parse("abc ${{ matrix.os").unwrap_err();
    assert_eq!(e.kind, ErrorKind::UnclosedTemplate);
    assert_eq!(e.at, Some(4));
    let e = Template::parse("abc ${{ 1 == }}").unwrap_err();
    assert_eq!(e.kind, ErrorKind::UnexpectedEnd);
    let e = Template::parse("abc ${{ nope() }}").unwrap_err();
    assert_eq!(e.kind, ErrorKind::UnknownFunction("nope".into()));
    assert_eq!(e.at, Some(8));
    let e = Template::parse("${{ }}").unwrap_err();
    assert_eq!(e.kind, ErrorKind::Empty);
    let e = Template::parse("${{}}").unwrap_err();
    assert_eq!(e.kind, ErrorKind::Empty);
    let e = Template::parse("x ${{ secrets.TOKEN }}").unwrap().evaluate(&contexts()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::UnknownContext("secrets".into()));
}

#[test]
fn a_template_says_what_it_holds() {
    let t = Template::parse("a ${{ matrix.os }} ${{ github.ref }}").unwrap();
    assert!(!t.is_literal());
    assert!(t.as_expression().is_none());
    assert_eq!(t.expressions().count(), 2);
    assert!(Template::parse("plain").unwrap().is_literal());
    assert!(pod_actions_expr::has_expression("x ${{ y }}"));
    assert!(!pod_actions_expr::has_expression("x { y }"));
}

fn condition(text: &str, status: Status) -> bool {
    let mut c = contexts();
    c.status = status;
    Condition::parse(text).and_then(|cond| cond.evaluate(&c)).unwrap_or_else(|e| panic!("{text}: {e}"))
}

const OK: Status = Status { success: true, failure: false, cancelled: false };
const FAILED: Status = Status { success: false, failure: true, cancelled: false };
const CANCELLED: Status = Status { success: false, failure: false, cancelled: true };

#[test]
fn a_condition_with_no_status_function_needs_success() {
    let c = Condition::parse("github.ref == 'refs/heads/main'").unwrap();
    assert!(c.is_implicit());
    assert_eq!(c.text(), "success() && (github.ref == 'refs/heads/main')");
    assert!(condition("github.ref == 'refs/heads/main'", OK));
    assert!(!condition("github.ref == 'refs/heads/main'", FAILED));
    assert!(!condition("github.ref == 'refs/heads/main'", CANCELLED));
    // The implicit `&&` stops before the body: its fromJSON fails.
    assert!(!condition("fromJSON('bad')", FAILED));
}

#[test]
fn a_condition_with_a_status_function_stands_alone() {
    let c = Condition::parse("${{ always() }}").unwrap();
    assert!(!c.is_implicit());
    assert_eq!(c.text(), "always()");
    assert!(condition("always()", FAILED));
    assert!(condition("failure()", FAILED));
    assert!(!condition("failure()", OK));
    assert!(condition("cancelled()", CANCELLED));
    assert!(condition("!cancelled()", FAILED));
    assert!(condition("failure() && needs.build.result == 'failure'", FAILED));
    assert!(condition("always() && contains(needs.build.result, 'fail')", OK));
    assert!(condition("Success() || true", FAILED));
}

#[test]
fn an_empty_condition_is_success() {
    let c = Condition::parse("  ").unwrap();
    assert_eq!(c.text(), "success()");
    assert!(condition("", OK));
    assert!(!condition("", FAILED));
}

#[test]
fn a_condition_in_a_template() {
    assert!(condition("${{ github.event_name == 'push' }}", OK));
    assert!(!condition("${{ github.event_name == 'pull_request' }}", OK));
    // Text around the expression makes a string, which is true when it is
    // not empty: the pitfall of the documents.
    assert!(condition("${{ false }} && false", OK));
    assert_eq!(Condition::parse("${{ false }} && false").unwrap().text(), "success() && (${{ false }} && false)");
    assert!(!condition("${{ false }} && false", FAILED));
    assert!(condition("${{ failure() }} x", FAILED));
    // `${{ 'text' }}` is the literal text, and the condition is that text.
    assert!(condition("${{ 'github.event_name == ''push''' }}", OK));
    let e = Condition::parse("${{ 'bad ==' }}").unwrap_err();
    assert_eq!(e.kind, ErrorKind::UnexpectedEnd);
}

#[test]
fn a_condition_names_its_contexts() {
    let c = Condition::parse("needs.build.result == 'x' || github.ref && NEEDS.a").unwrap();
    assert_eq!(c.contexts(), ["needs", "github"]);
    let e = Condition::parse("matrix.os == 'x' || inputs.y").unwrap().evaluate(&contexts()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::UnknownContext("inputs".into()));
    // A context that the body names is checked also when `success()` is
    // false.
    let mut c = contexts();
    c.status = FAILED;
    let e = Condition::parse("inputs.y").unwrap().evaluate(&c).unwrap_err();
    assert_eq!(e.kind, ErrorKind::UnknownContext("inputs".into()));
}
