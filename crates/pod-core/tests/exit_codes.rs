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
    let text =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/log.v1.schema.json")).unwrap();
    let schema: serde_json::Value = serde_json::from_str(&text).unwrap();
    let classes = schema["properties"]["class"].to_string();
    for c in Class::ALL {
        assert!(classes.contains(&format!("\"{}\"", c.name())), "the log schema lacks the class {}", c.name());
    }
    assert_eq!(schema["required"][0], "schema", "the log schema does not require the field schema first");
    assert_eq!(schema["properties"]["schema"]["const"], pod_core::LOG_SCHEMA, "the log schema and LOG_SCHEMA differ");
    assert_eq!(schema["properties"]["level"]["enum"], serde_json::json!(pod_core::LOG_LEVELS));
}

/// The rows of the section "Codes that a consumer keeps" of the README:
/// the cells of each row of the table, with the header and the rule left out.
fn kept_rows() -> Vec<Vec<String>> {
    let text = readme();
    let section = text
        .split("\n## ")
        .find(|s| s.starts_with("Codes that a consumer keeps"))
        .expect("the README has no section \"Codes that a consumer keeps\"");
    let rows: Vec<Vec<String>> = section
        .lines()
        .filter(|l| l.starts_with('|'))
        .map(|l| l.trim_matches('|').split('|').map(|c| c.trim().to_string()).collect::<Vec<_>>())
        .skip(2)
        .collect();
    assert!(!rows.is_empty(), "the section \"Codes that a consumer keeps\" has no row");
    rows
}

#[test]
fn a_kept_code_differs_from_the_code_of_its_class() {
    let mut seen = Vec::new();
    for row in kept_rows() {
        assert_eq!(
            row.len(),
            6,
            "a row has the consumer, the fault, the code, the class, the code of the class and the effect: {row:?}"
        );
        let [consumer, fault, kept, class, class_code, effect] = [0, 1, 2, 3, 4, 5].map(|i| row[i].as_str());
        let name = class
            .strip_prefix('`')
            .and_then(|c| c.strip_suffix('`'))
            .unwrap_or_else(|| panic!("the class of {fault} is not a code span: {class}"));
        let class = Class::ALL
            .iter()
            .find(|c| c.name() == name)
            .unwrap_or_else(|| panic!("{consumer} {fault}: `{name}` is not a class"));
        assert_eq!(
            class_code.parse::<u8>().ok(),
            Some(class.exit_code()),
            "{consumer} {fault}: the code of `{name}` is {}",
            class.exit_code()
        );
        let kept: u8 =
            kept.parse().unwrap_or_else(|_| panic!("{consumer} {fault}: the kept code is not a number: {kept}"));
        assert_ne!(
            kept,
            class.exit_code(),
            "{consumer} {fault}: the kept code is the code of `{name}`; move the row out of the list"
        );
        assert!(!effect.is_empty(), "{consumer} {fault}: no effect on a supervisor");
        assert!(!seen.contains(&(consumer.to_string(), fault.to_string())), "{consumer} {fault} is twice in the list");
        seen.push((consumer.to_string(), fault.to_string()));
    }
}
