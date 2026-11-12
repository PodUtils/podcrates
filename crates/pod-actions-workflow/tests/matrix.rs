//! The matrix, from the examples of the GitHub Actions documents ("Running
//! variations of jobs in a workflow").

use pod_actions_expr::Value;
use pod_actions_workflow::matrix::{MAX_CELLS, cells};

fn json(text: &str) -> Value {
    Value::from_json(text).unwrap()
}

/// Each cell as one line of JSON.
fn lines(matrix: &str) -> Vec<String> {
    cells(&json(matrix)).unwrap().into_iter().map(|c| Value::from(c).to_json_line()).collect()
}

#[test]
fn a_single_dimension_matrix() {
    assert_eq!(lines(r#"{"version": [10, 12, 14]}"#), [r#"{"version":10}"#, r#"{"version":12}"#, r#"{"version":14}"#]);
}

#[test]
fn a_multi_dimension_matrix_changes_the_first_key_last() {
    assert_eq!(
        lines(r#"{"os": ["ubuntu-22.04", "ubuntu-20.04"], "version": [10, 12, 14]}"#),
        [
            r#"{"os":"ubuntu-22.04","version":10}"#,
            r#"{"os":"ubuntu-22.04","version":12}"#,
            r#"{"os":"ubuntu-22.04","version":14}"#,
            r#"{"os":"ubuntu-20.04","version":10}"#,
            r#"{"os":"ubuntu-20.04","version":12}"#,
            r#"{"os":"ubuntu-20.04","version":14}"#,
        ]
    );
}

#[test]
fn a_matrix_of_objects() {
    let out = lines(
        r#"{"os": ["ubuntu-latest", "macos-latest"], "node": [{"version": 14}, {"version": 20, "env": "NODE_OPTIONS=--openssl-legacy-provider"}]}"#,
    );
    assert_eq!(out.len(), 4);
    assert_eq!(
        out[1],
        r#"{"os":"ubuntu-latest","node":{"version":20,"env":"NODE_OPTIONS=--openssl-legacy-provider"}}"#
    );
}

#[test]
fn the_include_example_of_the_documents() {
    let matrix = r#"{
        "fruit": ["apple", "pear"],
        "animal": ["cat", "dog"],
        "include": [
            {"color": "green"},
            {"color": "pink", "animal": "cat"},
            {"fruit": "apple", "shape": "circle"},
            {"fruit": "banana"},
            {"fruit": "banana", "animal": "cat"}
        ]
    }"#;
    assert_eq!(
        lines(matrix),
        [
            r#"{"fruit":"apple","animal":"cat","color":"pink","shape":"circle"}"#,
            r#"{"fruit":"apple","animal":"dog","color":"green","shape":"circle"}"#,
            r#"{"fruit":"pear","animal":"cat","color":"pink"}"#,
            r#"{"fruit":"pear","animal":"dog","color":"green"}"#,
            r#"{"fruit":"banana"}"#,
            r#"{"fruit":"banana","animal":"cat"}"#,
        ]
    );
}

#[test]
fn the_expanding_example_of_the_documents() {
    let matrix = r#"{
        "os": ["windows-latest", "ubuntu-latest"],
        "node": [14, 16],
        "include": [{"os": "windows-latest", "node": 16, "npm": 6}]
    }"#;
    assert_eq!(
        lines(matrix),
        [
            r#"{"os":"windows-latest","node":14}"#,
            r#"{"os":"windows-latest","node":16,"npm":6}"#,
            r#"{"os":"ubuntu-latest","node":14}"#,
            r#"{"os":"ubuntu-latest","node":16}"#,
        ]
    );
}

#[test]
fn the_adding_example_of_the_documents() {
    let matrix = r#"{
        "os": ["windows-latest", "ubuntu-latest", "macos-latest"],
        "node": [14, 16, 18],
        "include": [{"os": "windows-latest", "node": 17}]
    }"#;
    let out = lines(matrix);
    assert_eq!(out.len(), 10);
    assert_eq!(out[9], r#"{"os":"windows-latest","node":17}"#);
}

#[test]
fn an_include_only_matrix() {
    let matrix =
        r#"{"include": [{"site": "production", "datacenter": "site-a"}, {"site": "staging", "datacenter": "site-b"}]}"#;
    assert_eq!(
        lines(matrix),
        [r#"{"site":"production","datacenter":"site-a"}"#, r#"{"site":"staging","datacenter":"site-b"}"#]
    );
}

#[test]
fn the_exclude_example_of_the_documents() {
    let matrix = r#"{
        "os": ["macos-latest", "windows-latest"],
        "version": [12, 14, 16],
        "environment": ["staging", "production"],
        "exclude": [
            {"os": "macos-latest", "version": 12, "environment": "production"},
            {"os": "windows-latest", "version": 16}
        ]
    }"#;
    // The documents: "the following workflow will run nine jobs".
    let out = lines(matrix);
    assert_eq!(out.len(), 9);
    assert!(!out.contains(&r#"{"os":"macos-latest","version":12,"environment":"production"}"#.to_string()));
    assert!(!out.iter().any(|c| c.contains(r#""os":"windows-latest","version":16"#)));
}

#[test]
fn exclude_comes_before_include() {
    let matrix = r#"{
        "os": ["a", "b"],
        "exclude": [{"os": "a"}],
        "include": [{"os": "a", "extra": 1}]
    }"#;
    assert_eq!(lines(matrix), [r#"{"os":"b"}"#, r#"{"os":"a","extra":1}"#]);
}

#[test]
fn values_match_deeply_and_with_case() {
    let matrix = r#"{
        "cfg": [{"os": "a"}, {"os": "b"}],
        "exclude": [{"cfg": {"os": "a"}}],
        "include": [{"cfg": {"os": "B"}, "x": 1}]
    }"#;
    assert_eq!(lines(matrix), [r#"{"cfg":{"os":"b"}}"#, r#"{"cfg":{"os":"B"},"x":1}"#]);
    assert_eq!(lines(r#"{"n": [1, 2], "include": [{"n": 1.0, "x": true}]}"#), [r#"{"n":1,"x":true}"#, r#"{"n":2}"#]);
}

#[test]
fn a_bad_matrix_is_an_error_by_name() {
    let fault = |m: &str| cells(&json(m)).unwrap_err();
    assert_eq!(fault("[1]"), "the matrix must be a mapping, not array");
    assert_eq!(fault(r#"{"os": "linux"}"#), "`os` must be a sequence, not string");
    assert_eq!(fault(r#"{"os": []}"#), "the vector `os` has no value");
    assert_eq!(fault(r#"{"include": [1]}"#), "each entry of `include` must be a mapping, not number");
    assert_eq!(fault(r#"{"exclude": {"a": 1}}"#), "`exclude` must be a sequence, not object");
    assert_eq!(fault("{}"), "the matrix has no cell");
    assert_eq!(fault(r#"{"include": []}"#), "the matrix has no cell");
    assert_eq!(fault(r#"{"os": ["a"], "exclude": [{"os": "a"}]}"#), "the matrix has no cell");
    let big: Vec<String> = (0..17).map(|i| i.to_string()).collect();
    let list = big.join(",");
    assert_eq!(
        fault(&format!(r#"{{"a": [{list}], "b": [{list}]}}"#)),
        format!("the matrix has 289 cells; the limit is {MAX_CELLS}")
    );
    let wide: Vec<String> = (0..20).map(|i| format!(r#""k{i}": [1, 2]"#)).collect();
    assert_eq!(fault(&format!("{{{}}}", wide.join(","))), "the product of the vectors passes 65536 cells");
}
