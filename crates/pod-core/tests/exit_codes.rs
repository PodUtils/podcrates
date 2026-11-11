//! The table of exit codes in the README and the classes of the log schema
//! are the code: a new class or a new code changes them in the same commit.

use pod_core::Class;

fn readme() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md")).unwrap()
}

#[test]
fn the_table_of_the_readme_is_the_code() {
    let text = readme();
    let rows: Vec<(String, u8)> = text
        .lines()
        .filter_map(|l| {
            let cells: Vec<&str> = l.split('|').map(str::trim).collect();
            let name = cells.get(1)?.strip_prefix('`')?.strip_suffix('`')?;
            let code = cells.get(2)?.parse().ok()?;
            Some((name.to_string(), code))
        })
        .collect();
    let want: Vec<(String, u8)> = Class::ALL.iter().map(|c| (c.name().to_string(), c.exit_code())).collect();
    let mut have = rows.clone();
    have.sort();
    let mut sorted = want.clone();
    sorted.sort();
    assert_eq!(have, sorted, "the README table of exit codes and Class::exit_code differ");
}

#[test]
fn the_log_schema_names_each_class() {
    let schema =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/log.v1.schema.json")).unwrap();
    for c in Class::ALL {
        assert!(schema.contains(&format!("\"{}\"", c.name())), "the log schema lacks the class {}", c.name());
    }
    let first = "\"required\": [\"schema\",";
    assert!(schema.contains(first), "the log schema does not require the field schema first");
    let constant = format!("\"schema\": {{ \"const\": {},", pod_core::LOG_SCHEMA);
    assert!(schema.contains(&constant), "the log schema and pod_core::LOG_SCHEMA differ");
}
