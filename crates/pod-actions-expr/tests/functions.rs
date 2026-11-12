//! The functions, from the examples of the GitHub Actions documents
//! ("Evaluate expressions in workflows and actions", "Functions" and
//! "Object filters").

use pod_actions_expr::{Contexts, ErrorKind, Status, Value, evaluate};

fn contexts() -> Contexts {
    let labels = Value::from_json(r#"[{"name": "bug"}, {"name": "help wanted"}]"#).unwrap();
    let issue = Value::object([("labels", labels), ("title", Value::from("Octocat"))]);
    let github = Value::object([
        ("event_name", Value::from("push")),
        ("event", Value::object([("issue", issue)])),
        ("ref", Value::from("refs/heads/main")),
    ]);
    let fruits = Value::from_json(
        r#"[{"name": "apple", "quantity": 1}, {"name": "orange", "quantity": 2}, {"name": "pear", "quantity": 1}]"#,
    )
    .unwrap();
    let vegetables = Value::from_json(
        r#"{
          "scallions": {"colors": ["green", "white", "red"], "ediblePortions": ["roots", "stalks"]},
          "beets": {"colors": ["purple", "red", "gold", "white", "pink"], "ediblePortions": ["roots", "stems", "leaves"]},
          "artichokes": {"colors": ["green", "purple", "red", "black"], "ediblePortions": ["hearts", "stems", "leaves"]}
        }"#,
    )
    .unwrap();
    let mut c = Contexts::new();
    c.insert("github", github)
        .insert("fruits", fruits)
        .insert("vegetables", vegetables)
        .insert("job", Value::object([("status", Value::from("success"))]))
        .insert(
            "env",
            Value::object([
                ("continue", Value::from("true")),
                ("time", Value::from("3")),
                ("list", Value::from("[\"a\", \"b\"]")),
            ]),
        )
        .insert(
            "needs",
            Value::object([(
                "job1",
                Value::object([(
                    "outputs",
                    Value::object([("matrix", Value::from(r#"{"include":[{"project":"foo"}]}"#))]),
                )]),
            )]),
        );
    c
}

fn eval(text: &str) -> Value {
    evaluate(text, &contexts()).unwrap_or_else(|e| panic!("{text}: {e}"))
}

fn json(text: &str) -> Value {
    Value::from_json(text).unwrap()
}

#[test]
fn contains_of_the_documents() {
    assert_eq!(eval("contains('Hello world', 'llo')"), Value::Bool(true));
    assert_eq!(eval("contains('Hello world', 'LLO')"), Value::Bool(true));
    assert_eq!(eval("contains(github.event.issue.labels.*.name, 'bug')"), Value::Bool(true));
    assert_eq!(eval("contains(fromJSON('[\"push\", \"pull_request\"]'), github.event_name)"), Value::Bool(true));
    assert_eq!(eval("contains(fromJSON('[\"PUSH\"]'), github.event_name)"), Value::Bool(true));
    assert_eq!(eval("contains(fromJSON('[1, 2]'), '2')"), Value::Bool(true));
    assert_eq!(eval("contains(fromJSON('[]'), null)"), Value::Bool(false));
    assert_eq!(eval("contains('abc', '')"), Value::Bool(true));
    assert_eq!(eval("contains(null, '')"), Value::Bool(true));
    assert_eq!(eval("contains(123, 2)"), Value::Bool(true));
    assert_eq!(eval("contains('true', true)"), Value::Bool(true));
    assert_eq!(eval("contains(github, 'ref')"), Value::Bool(false));
    assert_eq!(eval("contains('Object', github)"), Value::Bool(false));
}

#[test]
fn starts_with_and_ends_with_of_the_documents() {
    assert_eq!(eval("startsWith('Hello world', 'He')"), Value::Bool(true));
    assert_eq!(eval("startsWith('Hello world', 'he')"), Value::Bool(true));
    assert_eq!(eval("endsWith('Hello world', 'ld')"), Value::Bool(true));
    assert_eq!(eval("endsWith('Hello world', 'LD')"), Value::Bool(true));
    assert_eq!(eval("startsWith(github.ref, 'refs/tags/')"), Value::Bool(false));
    assert_eq!(eval("startsWith(10, 1)"), Value::Bool(true));
    assert_eq!(eval("endsWith(fromJSON('[\"a\"]'), 'a')"), Value::Bool(false));
    assert_eq!(eval("startsWith('a', github)"), Value::Bool(false));
}

#[test]
fn format_of_the_documents() {
    assert_eq!(eval("format('Hello {0} {1} {2}', 'Mona', 'the', 'Octocat')"), Value::from("Hello Mona the Octocat"));
    assert_eq!(
        eval("format('{{Hello {0} {1} {2}!}}', 'Mona', 'the', 'Octocat')"),
        Value::from("{Hello Mona the Octocat!}")
    );
    assert_eq!(eval("format('{0}{0}{1}', 'a', 2.5)"), Value::from("aa2.5"));
    assert_eq!(eval("format('{0}|{1}|{2}|{3}', null, true, github, fruits)"), Value::from("|true|Object|Array"));
    assert_eq!(eval("format('no args')"), Value::from("no args"));
    assert_eq!(eval("format('}}{{')"), Value::from("}{"));
    // An argument that the text does not name is not evaluated.
    assert_eq!(eval("format('{0}', 'a', fromJSON('bad'))"), Value::from("a"));
}

#[test]
fn format_refuses_a_bad_text() {
    for text in [
        "format('{0', 'a')",
        "format('{0}}', 'a')",
        "format('}', 'a')",
        "format('{', 'a')",
        "format('{x}', 'a')",
        "format('{1}', 'a')",
        "format('{0:x}', 'a')",
        "format('{256}', 'a')",
        "format('{-1}', 'a')",
    ] {
        let e = evaluate(text, &contexts()).unwrap_err();
        assert!(matches!(e.kind, ErrorKind::Format(_)), "{text}: {e}");
    }
}

#[test]
fn join_of_the_documents() {
    assert_eq!(eval("join(github.event.issue.labels.*.name, ', ')"), Value::from("bug, help wanted"));
    assert_eq!(eval("join(github.event.issue.labels.*.name)"), Value::from("bug,help wanted"));
    assert_eq!(eval("join(fromJSON('[1, true, null, \"x\"]'), '-')"), Value::from("1-true--x"));
    assert_eq!(eval("join('abc', '-')"), Value::from("abc"));
    assert_eq!(eval("join(fromJSON('[]'), '-')"), Value::from(""));
    assert_eq!(eval("join(github, '-')"), Value::from(""));
    assert_eq!(eval("join(fromJSON('[[1], {}]'), 2)"), Value::from("Array2Object"));
    assert_eq!(eval("join(fromJSON('[1, 2]'), github)"), Value::from("1,2"));
}

#[test]
fn to_json_of_the_documents() {
    assert_eq!(eval("toJSON(job)"), Value::from("{\n  \"status\": \"success\"\n}"));
    assert_eq!(
        eval("toJSON(fromJSON('{\"a\": [1, {\"b\": null}], \"c\": {}, \"d\": []}'))"),
        Value::from("{\n  \"a\": [\n    1,\n    {\n      \"b\": null\n    }\n  ],\n  \"c\": {},\n  \"d\": []\n}")
    );
    assert_eq!(eval("toJSON('a\"b\\\n')"), Value::from("\"a\\\"b\\\\\\n\""));
    assert_eq!(eval("toJSON(1.5)"), Value::from("1.5"));
    assert_eq!(eval("toJSON(null)"), Value::from("null"));
    assert_eq!(eval("toJSON(fruits.*.name)"), Value::from("[\n  \"apple\",\n  \"orange\",\n  \"pear\"\n]"));
}

#[test]
fn from_json_of_the_documents() {
    assert_eq!(eval("fromJSON(needs.job1.outputs.matrix)"), json(r#"{"include":[{"project":"foo"}]}"#));
    assert_eq!(eval("fromJSON(env.continue)"), Value::Bool(true));
    assert_eq!(eval("fromJSON(env.time)"), Value::from(3));
    assert_eq!(eval("fromJSON(env.list)[1]"), Value::from("b"));
    assert_eq!(eval("fromJSON('[1,2,3]')[1]"), Value::from(2));
    assert_eq!(eval("fromJSON(' \"\\u00e9\\ud83d\\ude00\" ')"), Value::from("é😀"));
    assert_eq!(eval("fromJSON('{\"a\": 1, \"A\": 2}').a"), Value::from(2));
    assert_eq!(eval("fromJSON('-1.5e2')"), Value::Number(-150.0));
}

#[test]
fn from_json_refuses_what_is_not_json() {
    for text in [
        "''",
        "'  '",
        "'{'",
        "'[1,]'",
        "'01'",
        "'{a: 1}'",
        "'\"\\x\"'",
        "'nul'",
        "'1 2'",
        "'NaN'",
        "'\"a\nb\"'",
        "'1.'",
    ] {
        let e = evaluate(&format!("fromJSON({text})"), &contexts()).unwrap_err();
        assert!(matches!(e.kind, ErrorKind::Json(_)), "{text}: {e}");
    }
    let deep = format!("fromJSON('{}{}')", "[".repeat(65), "]".repeat(65));
    let e = evaluate(&deep, &contexts()).unwrap_err();
    assert!(e.to_string().contains("deeper than 64"), "{e}");
    let ok = format!("fromJSON('{}{}')", "[".repeat(64), "]".repeat(64));
    assert!(evaluate(&ok, &contexts()).is_ok());
}

#[test]
fn the_object_filter_of_the_documents() {
    assert_eq!(eval("fruits.*.name"), json(r#"["apple", "orange", "pear"]"#));
    assert_eq!(eval("fruits[*].quantity"), json("[1, 2, 1]"));
    assert_eq!(
        eval("vegetables.*.ediblePortions"),
        json(r#"[["roots", "stalks"], ["roots", "stems", "leaves"], ["hearts", "stems", "leaves"]]"#)
    );
    assert_eq!(
        eval("vegetables.*.ediblePortions.*"),
        json(r#"["roots", "stalks", "roots", "stems", "leaves", "hearts", "stems", "leaves"]"#)
    );
    assert_eq!(eval("vegetables.*.colors[0]"), json(r#"["green", "purple", "green"]"#));
    assert_eq!(eval("contains(vegetables.*.ediblePortions.*, 'hearts')"), Value::Bool(true));
    assert_eq!(eval("fruits.*.missing"), json("[]"));
    assert_eq!(eval("'text'.*"), json("[]"));
    assert_eq!(eval("null.*.x"), json("[]"));
    // A filtered array is an array: an index of an array that is not filtered
    // by a name gives null.
    assert_eq!(eval("fromJSON('[{\"a\": 1}]').a"), Value::Null);
    // `||` gives the filtered array itself, and the index goes on over it.
    assert_eq!(eval("(null || fruits.*).name"), json(r#"["apple", "orange", "pear"]"#));
}

#[test]
fn the_status_functions_read_the_status_of_the_caller() {
    let mut c = contexts();
    let ask = |c: &Contexts, f: &str| evaluate(f, c).unwrap();
    assert_eq!(ask(&c, "success()"), Value::Bool(true));
    assert_eq!(ask(&c, "failure()"), Value::Bool(false));
    assert_eq!(ask(&c, "cancelled()"), Value::Bool(false));
    assert_eq!(ask(&c, "always()"), Value::Bool(true));
    c.status = Status { success: false, failure: true, cancelled: false };
    assert_eq!(ask(&c, "success()"), Value::Bool(false));
    assert_eq!(ask(&c, "FAILURE()"), Value::Bool(true));
    c.status = Status { success: false, failure: false, cancelled: true };
    assert_eq!(ask(&c, "cancelled()"), Value::Bool(true));
    assert_eq!(ask(&c, "always()"), Value::Bool(true));
}

#[test]
fn function_names_ignore_case() {
    assert_eq!(eval("TOJSON(1)"), Value::from("1"));
    assert_eq!(eval("fromjson('2')"), Value::from(2));
    assert_eq!(eval("StartsWith('ab', 'A')"), Value::Bool(true));
    assert_eq!(eval("toJSON (1)"), Value::from("1"));
}

#[test]
fn hash_files_parses_and_fails_by_name() {
    assert!(pod_actions_expr::Expr::parse("hashFiles('**/Cargo.lock')").is_ok());
    let e = evaluate("hashFiles('**/Cargo.lock')", &contexts()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::NeedsIo("hashFiles".into()));
    assert_eq!(e.to_string(), "the function `hashFiles` reads files; this evaluator does no I/O (at character 1)");
}
