//! The strategy file: its evidence and its refusal when the date is absent.

use pod_sidecar::{Date, Params, Primitive, Step, Strategy, StrategyError, Trigger};

const FILE: &str = r#"
date = "2026-11-12"
network = "lab-a"
found_by = "hand"
[[steps]]
trigger = "FirstOutboundData"
primitive = "drop"

[[steps]]
trigger = "SynAck"
primitive = "tamper"
offset = 1
byte = 90
count = 2
"#;

#[test]
fn a_strategy_file_carries_its_evidence_and_its_steps() {
    let strategy = Strategy::from_toml(FILE).unwrap();
    assert_eq!(strategy.date, Date::parse("2026-11-12").unwrap());
    assert_eq!(strategy.network, "lab-a");
    assert_eq!(strategy.found_by, "hand");
    assert_eq!(strategy.steps.len(), 2);
    assert_eq!(strategy.steps[0], Step::new(Trigger::FirstOutboundData, Primitive::Drop));
    assert_eq!(
        strategy.steps[1],
        Step {
            trigger: Trigger::SynAck,
            primitive: Primitive::Tamper,
            params: Params { offset: Some(1), byte: Some(90), count: Some(2) },
        }
    );
}

#[test]
fn a_strategy_with_no_date_is_refused_with_a_named_error() {
    let no_date = FILE.replace("date = \"2026-11-12\"", "");
    assert_eq!(Strategy::from_toml(&no_date), Err(StrategyError::NoDate));
    let empty = FILE.replace("date = \"2026-11-12\"", "date = \"\"");
    assert_eq!(Strategy::from_toml(&empty), Err(StrategyError::NoDate));
    let text = StrategyError::NoDate.to_string();
    assert!(text.contains("no date"), "{text}");
}

#[test]
fn a_date_that_is_not_a_day_of_the_calendar_is_refused() {
    for bad in ["2026-13-01", "2026-02-30", "2026-00-10", "2026-1-1", "not-a-date", "2026-11-12T00:00:00"] {
        let file = FILE.replace("2026-11-12", bad);
        assert!(matches!(Strategy::from_toml(&file), Err(StrategyError::BadDate(_))), "{bad}");
    }
    assert!(Date::parse("2024-02-29").is_ok());
    assert!(Date::parse("2026-02-29").is_err());
}

#[test]
fn a_trigger_and_a_primitive_read_a_second_spelling() {
    assert_eq!(Trigger::parse("first-outbound-data"), Ok(Trigger::FirstOutboundData));
    assert_eq!(Trigger::parse("SYN_ACK"), Ok(Trigger::SynAck));
    assert_eq!(Primitive::parse("Duplicate"), Ok(Primitive::Duplicate));
    assert_eq!(Primitive::parse("co-alesce"), Ok(Primitive::Coalesce));
    assert!(matches!(Primitive::parse("split"), Err(StrategyError::UnknownPrimitive(_))));
    assert!(matches!(Trigger::parse("reset"), Err(StrategyError::UnknownTrigger(_))));
}

#[test]
fn a_strategy_with_no_step_is_refused() {
    let file = "date = \"2026-11-12\"\nnetwork = \"lab\"\nfound_by = \"hand\"\nsteps = []\n";
    assert_eq!(Strategy::from_toml(file), Err(StrategyError::NoSteps));
    let file = "date = \"2026-11-12\"\nnetwork = \"lab\"\nfound_by = \"hand\"\n";
    assert_eq!(Strategy::from_toml(file), Err(StrategyError::NoSteps));
}

#[test]
fn a_missing_field_is_named() {
    let file = FILE.replace("network = \"lab-a\"", "");
    assert_eq!(Strategy::from_toml(&file), Err(StrategyError::Missing("network")));
    let file = FILE.replace("found_by = \"hand\"", "");
    assert_eq!(Strategy::from_toml(&file), Err(StrategyError::Missing("found_by")));
}

#[test]
fn a_parameter_outside_its_range_is_refused() {
    let file = FILE.replace("byte = 90", "byte = 300");
    assert!(matches!(Strategy::from_toml(&file), Err(StrategyError::Toml(_))));
    let file = FILE.replace("count = 2", "count = 0");
    assert!(matches!(Strategy::from_toml(&file), Err(StrategyError::Toml(_))));
    let file = FILE.replace("offset = 1", "offset = -1");
    assert!(matches!(Strategy::from_toml(&file), Err(StrategyError::Toml(_))));
}

#[test]
fn a_date_that_toml_reads_as_a_date_is_accepted() {
    let file = FILE.replace("date = \"2026-11-12\"", "date = 2026-11-12");
    let strategy = Strategy::from_toml(&file).unwrap();
    assert_eq!(strategy.date.to_string(), "2026-11-12");
}
