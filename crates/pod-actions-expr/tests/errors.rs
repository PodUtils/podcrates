//! The failures: each one names its cause, and the limits hold.

use pod_actions_expr::{Contexts, ErrorKind, Expr, Limits, MAX_DEPTH, MAX_LENGTH, Value, evaluate};

fn parse_error(text: &str) -> ErrorKind {
    match Expr::parse(text) {
        Ok(_) => panic!("{text} parses"),
        Err(e) => e.kind,
    }
}

#[test]
fn syntax_errors_name_the_token() {
    assert_eq!(parse_error(""), ErrorKind::Empty);
    assert_eq!(parse_error("   "), ErrorKind::Empty);
    assert_eq!(parse_error("a ="), ErrorKind::Unexpected("=".into()));
    assert_eq!(parse_error("a & b"), ErrorKind::Unexpected("&".into()));
    assert_eq!(parse_error("a | b"), ErrorKind::Unexpected("|".into()));
    assert_eq!(parse_error("1 2"), ErrorKind::Unexpected("2".into()));
    assert_eq!(parse_error("a ."), ErrorKind::UnexpectedEnd);
    assert_eq!(parse_error("a.1"), ErrorKind::Unexpected("1".into()));
    assert_eq!(parse_error("(a"), ErrorKind::UnexpectedEnd);
    assert_eq!(parse_error("a)"), ErrorKind::Unexpected(")".into()));
    assert_eq!(parse_error("a[1"), ErrorKind::UnexpectedEnd);
    assert_eq!(parse_error("'abc"), ErrorKind::UnclosedString);
    assert_eq!(parse_error("1.2.3"), ErrorKind::Unexpected("1.2.3".into()));
    assert_eq!(parse_error("-0x10"), ErrorKind::Unexpected("-0x10".into()));
    assert_eq!(parse_error("9abc"), ErrorKind::Unexpected("9abc".into()));
    assert_eq!(parse_error("a$b"), ErrorKind::Unexpected("a$b".into()));
    assert_eq!(parse_error("== 1"), ErrorKind::Unexpected("==".into()));
    assert_eq!(parse_error("contains('a',)"), ErrorKind::Unexpected(")".into()));
    assert_eq!(parse_error("toJSON(1 2)"), ErrorKind::Unexpected("2".into()));
    assert_eq!(parse_error("a.*.*b"), ErrorKind::Unexpected("b".into()));
}

#[test]
fn an_error_names_its_place() {
    let e = Expr::parse("github.ref == = 'x'").unwrap_err();
    assert_eq!(e.at, Some(14));
    assert_eq!(e.to_string(), "unexpected symbol `=` (at character 15)");
}

#[test]
fn functions_are_checked_at_the_parse() {
    assert_eq!(parse_error("nope()"), ErrorKind::UnknownFunction("nope".into()));
    assert_eq!(parse_error("toJson ( )"), ErrorKind::Arity { function: "toJSON".into(), min: 1, max: 1, given: 0 });
    assert_eq!(
        parse_error("contains('a')"),
        ErrorKind::Arity { function: "contains".into(), min: 2, max: 2, given: 1 }
    );
    assert_eq!(parse_error("join(1, 2, 3)"), ErrorKind::Arity { function: "join".into(), min: 1, max: 2, given: 3 });
    assert_eq!(parse_error("success(1)"), ErrorKind::Arity { function: "success".into(), min: 0, max: 0, given: 1 });
    assert_eq!(
        parse_error("format()"),
        ErrorKind::Arity { function: "format".into(), min: 1, max: usize::MAX, given: 0 }
    );
    let e = Expr::parse("format()").unwrap_err();
    assert_eq!(e.to_string(), "the function `format` takes 1 arguments or more, not 0 (at character 1)");
    let e = Expr::parse("contains('a')").unwrap_err();
    assert_eq!(e.to_string(), "the function `contains` takes 2 arguments, not 1 (at character 1)");
}

#[test]
fn a_missing_context_is_an_error_by_name_in_each_branch() {
    let mut contexts = Contexts::new();
    contexts.insert("github", Value::object([("ref", Value::from("x"))]));
    let e = evaluate("true || secrets.TOKEN", &contexts).unwrap_err();
    assert_eq!(e.kind, ErrorKind::UnknownContext("secrets".into()));
    assert_eq!(e.at, Some(8));
    let expr = Expr::parse("github.ref && matrix.os && GITHUB.sha && Matrix.x").unwrap();
    assert_eq!(expr.contexts(), ["github", "matrix"]);
    assert_eq!(expr.functions(), Vec::<&str>::new());
    let expr = Expr::parse("format('{0}', toJSON(fromJSON('1'))) && toJson(1)").unwrap();
    assert_eq!(expr.functions(), ["format", "toJSON", "fromJSON"]);
}

#[test]
fn the_length_limit_of_github() {
    let ok = format!("'{}'", "a".repeat(MAX_LENGTH - 2));
    assert!(Expr::parse(&ok).is_ok());
    let long = format!("'{}'", "a".repeat(MAX_LENGTH - 1));
    assert_eq!(parse_error(&long), ErrorKind::TooLong { length: MAX_LENGTH + 1, max: MAX_LENGTH });
}

#[test]
fn the_depth_limit_of_github() {
    let ok = format!("{}a{}", "(".repeat(MAX_DEPTH), ")".repeat(MAX_DEPTH));
    assert!(Expr::parse(&ok).is_ok());
    let groups = format!("{}a{}", "(".repeat(MAX_DEPTH + 1), ")".repeat(MAX_DEPTH + 1));
    assert_eq!(parse_error(&groups), ErrorKind::TooDeep { max: MAX_DEPTH });
    assert_eq!(parse_error(&"!".repeat(10_000)), ErrorKind::TooDeep { max: MAX_DEPTH });
    let chain = format!("a{}", ".b".repeat(MAX_DEPTH));
    assert_eq!(parse_error(&chain), ErrorKind::TooDeep { max: MAX_DEPTH });
    assert!(Expr::parse(&format!("a{}", ".b".repeat(MAX_DEPTH - 1))).is_ok());
    let compares = format!("1{}", " == 1".repeat(MAX_DEPTH));
    assert_eq!(parse_error(&compares), ErrorKind::TooDeep { max: MAX_DEPTH });
    let calls = format!("{}1{}", "toJSON(".repeat(MAX_DEPTH + 1), ")".repeat(MAX_DEPTH + 1));
    assert_eq!(parse_error(&calls), ErrorKind::TooDeep { max: MAX_DEPTH });
    let indexes = format!("{}0{}", "a[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
    assert_eq!(parse_error(&indexes), ErrorKind::TooDeep { max: MAX_DEPTH });
    // A chain of `||` is one node: it is long, not deep.
    let ors = format!("a{}", " || a".repeat(3_000));
    assert!(Expr::parse(&ors).is_ok());
}

#[test]
fn a_format_of_a_format_stops_at_the_byte_limit() {
    let mut text = "'abcdefgh'".to_string();
    for _ in 0..40 {
        text = format!("format('{{0}}{{0}}', {text})");
    }
    let mut contexts = Contexts::new();
    contexts.limits = Limits { max_bytes: 1 << 20 };
    let e = evaluate(&text, &contexts).unwrap_err();
    assert_eq!(e.kind, ErrorKind::TooLarge { max: 1 << 20 });
    let mut text = "'ab'".to_string();
    for _ in 0..10 {
        text = format!("format('{{0}}{{0}}', {text})");
    }
    assert_eq!(evaluate(&text, &contexts).unwrap().as_str().map(str::len), Some(2048));
}

#[test]
fn the_byte_limit_holds_for_each_function() {
    let mut contexts = Contexts::new();
    contexts.limits = Limits { max_bytes: 64 };
    contexts.insert("big", Value::array((0..100).map(Value::from)));
    for text in ["toJSON(big)", "join(big, ',')", "big.*", "fromJSON(toJSON(big))"] {
        let e = evaluate(text, &contexts).unwrap_err();
        assert_eq!(e.kind, ErrorKind::TooLarge { max: 64 }, "{text}");
    }
    assert!(evaluate("big[1]", &contexts).is_ok());
}
