//! The literals, the operators and the coercion of GitHub, from the
//! examples and the tables of the GitHub Actions documents ("Evaluate
//! expressions in workflows and actions").

use pod_actions_expr::{Contexts, Value, evaluate};

fn eval(text: &str) -> Value {
    let mut contexts = Contexts::new();
    contexts.insert(
        "github",
        Value::object([("ref", Value::from("refs/heads/main")), ("event", Value::object([("n", Value::from(1))]))]),
    );
    evaluate(text, &contexts).unwrap_or_else(|e| panic!("{text}: {e}"))
}

fn truth(text: &str) -> bool {
    match eval(text) {
        Value::Bool(b) => b,
        other => panic!("{text}: {other:?} is not a boolean"),
    }
}

#[test]
fn the_literals_of_the_documents() {
    assert_eq!(eval("null"), Value::Null);
    assert_eq!(eval("false"), Value::Bool(false));
    assert_eq!(eval("711"), Value::from(711));
    assert_eq!(eval("-9.2"), Value::Number(-9.2));
    assert_eq!(eval("0xff"), Value::from(255));
    assert_eq!(eval("-2.99e-2"), Value::Number(-0.0299));
    assert_eq!(eval("'It''s open source!'"), Value::from("It's open source!"));
    assert_eq!(eval("0o17"), Value::from(15));
    assert_eq!(eval(".5"), Value::Number(0.5));
    assert_eq!(eval("+3"), Value::from(3));
    assert!(eval("NaN").as_number().is_some_and(f64::is_nan));
    assert_eq!(eval("Infinity"), Value::Number(f64::INFINITY));
    assert_eq!(eval("-Infinity"), Value::Number(f64::NEG_INFINITY));
    assert_eq!(eval("''"), Value::from(""));
}

#[test]
fn the_case_of_a_literal_word_counts() {
    // `True` is a named value, not a boolean: no context of that name.
    let e = evaluate("True", &Contexts::new()).unwrap_err();
    assert_eq!(e.to_string(), "unknown context `True` (at character 1)");
}

#[test]
fn the_coercion_table_of_the_documents() {
    // Null 0; true 1, false 0; a string by JSON, empty 0; a collection NaN.
    assert!(truth("null == 0"));
    assert!(truth("true == 1"));
    assert!(truth("false == 0"));
    assert!(truth("'' == 0"));
    assert!(truth("'1' == 1"));
    assert!(truth("' 1.5 ' == 1.5"));
    assert!(truth("'0x10' == 16"));
    assert!(truth("'abc' != 0"));
    assert!(truth("true == '1'"));
    assert!(truth("null == false"));
    assert!(truth("null == ''"));
    assert!(!truth("fromJSON('[]') == 0"));
    assert!(!truth("fromJSON('{}') == 'Object'"));
    assert!(!truth("fromJSON('[]') == false"));
}

#[test]
fn nan_equals_nothing() {
    assert!(!truth("NaN == NaN"));
    assert!(truth("NaN != NaN"));
    assert!(!truth("NaN < 1"));
    assert!(!truth("NaN >= NaN"));
}

#[test]
fn strings_compare_with_no_regard_to_case() {
    assert!(truth("'abc' == 'ABC'"));
    assert!(truth("github.ref == 'REFS/HEADS/MAIN'"));
    assert!(truth("'a' < 'B'"));
    assert!(truth("'B' > 'a'"));
    // .NET's OrdinalIgnoreCase compares the upper case: `A` (65) is before
    // `_` (95).
    assert!(truth("'a' < '_'"));
    assert!(!truth("'_' < 'a'"));
    // A character with an upper case of two characters keeps its own.
    assert!(!truth("'straße' == 'STRASSE'"));
    assert!(truth("'ÄB' == 'äb'"));
}

#[test]
fn collections_are_equal_only_to_themselves() {
    assert!(truth("github.event == github.event"));
    assert!(!truth("fromJSON('[1]') == fromJSON('[1]')"));
    assert!(!truth("github.event != github.event"));
}

#[test]
fn the_order_of_numbers_strings_and_booleans() {
    assert!(truth("1 < 2"));
    assert!(truth("2 <= 2"));
    assert!(truth("3 > 2"));
    assert!(truth("'10' > 9"));
    assert!(truth("true > false"));
    assert!(!truth("false > true"));
    assert!(truth("null <= null"));
    assert!(!truth("null < null"));
    assert!(truth("null < 1"));
    assert!(!truth("github.event < 1"));
    assert!(!truth("github.event >= 1"));
}

#[test]
fn and_and_or_give_an_operand() {
    assert_eq!(eval("null || 'x'"), Value::from("x"));
    assert_eq!(eval("'a' && 0"), Value::from(0));
    assert_eq!(eval("1 && 'b'"), Value::from("b"));
    assert_eq!(eval("0 || ''"), Value::from(""));
    assert_eq!(eval("'' && 1"), Value::from(""));
    assert_eq!(eval("1 && 2 && 3"), Value::from(3));
    assert_eq!(eval("0 || null || 'last'"), Value::from("last"));
    // The ternary of the documents.
    let text = "github.ref == 'refs/heads/main' && 'value_for_main_branch' || 'value_for_other_branches'";
    assert_eq!(eval(text), Value::from("value_for_main_branch"));
}

#[test]
fn and_and_or_stop_at_their_answer() {
    // The second operand is not evaluated: its fromJSON fails.
    assert_eq!(eval("false && fromJSON('bad')"), Value::Bool(false));
    assert_eq!(eval("true || fromJSON('bad')"), Value::Bool(true));
}

#[test]
fn not_gives_the_truth() {
    assert!(truth("!null"));
    assert!(truth("!0"));
    assert!(truth("!''"));
    assert!(truth("!NaN"));
    assert!(!truth("!'false'"));
    assert!(!truth("!github.event"));
    assert!(!truth("!fromJSON('[]')"));
    assert!(truth("!!1"));
}

#[test]
fn the_precedence_of_the_operators() {
    assert!(truth("1 == 1 && 2 == 2"));
    assert!(truth("true || false && false"));
    assert!(!truth("(true || false) && false"));
    assert!(truth("!false == true"));
    assert!(truth("1 < 2 == true"));
    assert!(truth("!github.ref == false"));
}

#[test]
fn the_index_and_the_property() {
    assert_eq!(eval("github.ref"), Value::from("refs/heads/main"));
    assert_eq!(eval("github['ref']"), Value::from("refs/heads/main"));
    assert_eq!(eval("github.REF"), Value::from("refs/heads/main"));
    assert_eq!(eval("GitHub.event.n"), Value::from(1));
    assert_eq!(eval("github.nothing"), Value::Null);
    assert_eq!(eval("github.nothing.deeper"), Value::Null);
    assert_eq!(eval("fromJSON('[1,2,3]')[1]"), Value::from(2));
    assert_eq!(eval("fromJSON('[1,2,3]')['2']"), Value::from(3));
    assert_eq!(eval("fromJSON('[1,2,3]')[1.9]"), Value::from(2));
    assert_eq!(eval("fromJSON('[1,2,3]')[-1]"), Value::Null);
    assert_eq!(eval("fromJSON('[1,2,3]')[3]"), Value::Null);
    assert_eq!(eval("fromJSON('[1,2,3]').x"), Value::Null);
    assert_eq!(eval("fromJSON('{\"1\": \"one\"}')[1]"), Value::from("one"));
    assert_eq!(eval("'abc'[0]"), Value::Null);
    assert_eq!(eval("github . ref"), Value::from("refs/heads/main"));
    assert_eq!(eval("fromJSON('{\"true\": 1}').true"), Value::from(1));
}
