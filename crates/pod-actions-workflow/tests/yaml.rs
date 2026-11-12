//! The YAML layer: the core schema of YAML 1.2 (as the runner reads a
//! scalar), the order and the lines, duplicate keys, anchors, the limits.

use pod_actions_workflow::yaml::{Kind, Limits, Mark, Node, load};
use pod_actions_workflow::{ErrorKind, parse, parse_with};

fn scalar(text: &str) -> Kind {
    load(&format!("v: {text}\n"), Limits::default()).unwrap().get("v").unwrap().kind.clone()
}

#[test]
fn plain_scalars_take_the_core_schema() {
    for text in ["", "~", "null", "Null", "NULL"] {
        assert_eq!(scalar(text), Kind::Null, "{text:?}");
    }
    for (text, b) in [("true", true), ("True", true), ("TRUE", true), ("false", false), ("FALSE", false)] {
        assert_eq!(scalar(text), Kind::Bool(b), "{text}");
    }
    let numbers: &[(&str, f64)] = &[
        ("0", 0.0),
        ("-12", -12.0),
        ("+7", 7.0),
        ("0123", 123.0),
        ("0x1F", 31.0),
        ("0o17", 15.0),
        ("1.5", 1.5),
        ("3.10", 3.1),
        (".5", 0.5),
        ("1.", 1.0),
        ("1e3", 1000.0),
        ("-2.5E-2", -0.025),
        (".inf", f64::INFINITY),
        ("-.Inf", f64::NEG_INFINITY),
    ];
    for (text, n) in numbers {
        assert_eq!(scalar(text), Kind::Number(*n), "{text}");
    }
    assert!(matches!(scalar(".nan"), Kind::Number(n) if n.is_nan()));
    // YAML 1.1 words, and texts that only look like numbers, are strings.
    for text in ["yes", "no", "on", "off", "y", "1_000", "0b101", "1e", "1.2.3", "0x", "+0x1", "12:30", "v1"] {
        assert_eq!(scalar(text), Kind::String(text.into()), "{text}");
    }
}

#[test]
fn quoted_and_block_scalars_are_strings() {
    assert_eq!(scalar("'true'"), Kind::String("true".into()));
    assert_eq!(scalar("\"12\""), Kind::String("12".into()));
    assert_eq!(scalar("!!str 12"), Kind::String("12".into()));
    let node = load("v: |\n  a\n  b\nw: >\n  c\n  d\n", Limits::default()).unwrap();
    assert_eq!(node.get("v").unwrap().kind, Kind::String("a\nb\n".into()));
    assert_eq!(node.get("w").unwrap().kind, Kind::String("c d\n".into()));
}

#[test]
fn the_key_on_is_a_string() {
    let parsed = parse("on: push\njobs:\n  a:\n    runs-on: x\n").unwrap();
    assert_eq!(parsed.workflow.events(), ["push"]);
}

#[test]
fn keys_keep_their_order_and_their_lines() {
    let node = load("z: 1\na:\n  - x\n  - y\nm: {k: v}\n", Limits::default()).unwrap();
    let keys: Vec<&str> = node.entries().iter().map(|(k, _)| k.name.as_str()).collect();
    assert_eq!(keys, ["z", "a", "m"]);
    assert_eq!(node.entries()[1].0.mark, Mark { line: 2, column: 1 });
    let Kind::Seq(items) = &node.get("a").unwrap().kind else { panic!() };
    assert_eq!(items[1].mark, Mark { line: 4, column: 5 });
    assert_eq!(node.get("m").unwrap().get("k").unwrap().text().as_deref(), Some("v"));
}

#[test]
fn a_duplicate_key_is_an_error_by_name() {
    let e = load("a: 1\nb: 2\na: 3\n", Limits::default()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::DuplicateKey("a".into()));
    assert_eq!(e.mark, Some(Mark { line: 3, column: 1 }));
    assert_eq!(e.to_string(), "(line 3, column 1): the key `a` is there two times");
    let e = parse("on: push\njobs:\n  a:\n    runs-on: x\n  a:\n    runs-on: y\n").unwrap_err();
    assert_eq!(e.kind, ErrorKind::DuplicateKey("a".into()));
}

#[test]
fn a_key_must_be_a_scalar() {
    let e = load("? [a, b]\n: 1\n", Limits::default()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::KeyNotScalar);
}

#[test]
fn one_document_only() {
    assert_eq!(load("a: 1\n---\nb: 2\n", Limits::default()).unwrap_err().kind, ErrorKind::Documents);
    assert_eq!(load("", Limits::default()).unwrap_err().kind, ErrorKind::Empty);
    assert_eq!(load("# only a comment\n", Limits::default()).unwrap_err().kind, ErrorKind::Empty);
    assert_eq!(load("---\na: 1\n...\n", Limits::default()).unwrap().get("a").unwrap().kind, Kind::Number(1.0));
}

#[test]
fn bad_yaml_names_its_line() {
    let e = load("a: [1, 2\nb: 3\n", Limits::default()).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Yaml(_)), "{e}");
    assert!(e.mark.is_some());
    let e = load("a: *nothing\n", Limits::default()).unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Yaml(_)), "{e}");
}

#[test]
fn an_alias_copies_its_anchor() {
    let text = "base: &env {A: 1, B: two}\nuse: *env\nkey: &k name\n*k : 3\n";
    let node = load(text, Limits::default()).unwrap();
    assert_eq!(node.get("use").unwrap().to_value(), node.get("base").unwrap().to_value());
    assert_eq!(node.get("name").unwrap().kind, Kind::Number(3.0));
    // A merge key is not in the YAML of GitHub: it stays an ordinary key.
    let node = load("a: &a {x: 1}\nb:\n  <<: *a\n", Limits::default()).unwrap();
    assert!(node.get("b").unwrap().get("<<").is_some());
}

#[test]
fn the_aliases_of_a_billion_laughs_stop_at_the_node_limit() {
    let mut text = String::from("a0: &a0 [x, x, x, x, x, x, x, x, x, x]\n");
    for i in 1..10 {
        let refs = vec![format!("*a{}", i - 1); 10].join(", ");
        text.push_str(&format!("a{i}: &a{i} [{refs}]\n"));
    }
    let e = load(&text, Limits::default()).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Limit { what: "nodes", max: 100_000 });
}

#[test]
fn the_depth_and_size_limits() {
    let deep = format!("a: {}1{}\n", "[".repeat(70), "]".repeat(70));
    assert_eq!(load(&deep, Limits::default()).unwrap_err().kind, ErrorKind::Limit { what: "levels", max: 64 });
    // Aliases that nest past the depth.
    let mut text = String::from("a0: &a0 [1]\n");
    for i in 1..70 {
        text.push_str(&format!("a{i}: &a{i} [*a{}]\n", i - 1));
    }
    assert_eq!(load(&text, Limits::default()).unwrap_err().kind, ErrorKind::Limit { what: "levels", max: 64 });
    let small = Limits { max_bytes: 10, ..Limits::default() };
    let e = parse_with("on: push\njobs: {}\n", small).unwrap_err();
    assert_eq!(e.kind, ErrorKind::Limit { what: "bytes", max: 10 });
}

#[test]
fn a_node_evaluates_its_templates() {
    use pod_actions_expr::{Contexts, Value};
    let node: Node = load(
        "a: ${{ fromJSON('[1, 2]') }}\nb: x-${{ matrix.n }}\nc: [\"${{ matrix.n }}\", 3]\n${{ matrix.k }}: v\n",
        Limits::default(),
    )
    .unwrap();
    let mut contexts = Contexts::new();
    contexts.insert("matrix", Value::object([("n", Value::from(7)), ("k", Value::from("key"))]));
    let value = node.evaluate(&contexts, "root").unwrap();
    assert_eq!(value.to_json_line(), r#"{"a":[1,2],"b":"x-7","c":[7,3],"key":"v"}"#);
    assert_eq!(node.to_value().get("b"), Some(&Value::from("x-${{ matrix.n }}")));
    let mut contexts = Contexts::new();
    contexts.insert("other", Value::Null);
    let e = node.evaluate(&contexts, "root").unwrap_err();
    assert_eq!(e.path, "root.b");
    assert_eq!(e.to_string(), "root.b (line 2, column 4): unknown context `matrix` (at character 7)");
}
