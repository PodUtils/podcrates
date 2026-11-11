use super::*;

#[test]
fn a_residual_is_a_number_or_the_word_unknown() {
    assert_eq!(serde_json::to_string(&Residual::Count(0)).unwrap(), "0");
    assert_eq!(serde_json::to_string(&Residual::Count(3)).unwrap(), "3");
    assert_eq!(serde_json::to_string(&Residual::Unknown).unwrap(), "\"unknown\"");
    assert_eq!(serde_json::from_str::<Residual>("2").unwrap(), Residual::Count(2));
    assert_eq!(serde_json::from_str::<Residual>("\"unknown\"").unwrap(), Residual::Unknown);
    let e = serde_json::from_str::<Residual>("\"none\"").unwrap_err().to_string();
    assert!(e.contains("\"none\""), "{e}");
    assert!(serde_json::from_str::<Residual>("-1").is_err());
}

#[test]
fn an_end_keeps_each_field_through_json() {
    let end = End {
        reason: Reason::Wall,
        run_ms: 1500,
        user_ms: 3,
        system_ms: 4,
        max_rss_kib: 2048,
        residual_processes: Residual::Unknown,
    };
    let text = serde_json::to_string(&end).unwrap();
    assert!(text.contains("\"reason\":\"wall\""), "{text}");
    assert!(text.contains("\"residual_processes\":\"unknown\""), "{text}");
    assert_eq!(serde_json::from_str::<End>(&text).unwrap(), end);
}
