//! The reader of `nft -j list counters`.

use pod_netlab::counters::MAX_TEXT;
use pod_netlab::{Count, CounterError, parse_counters};

/// The output of nftables 1.0.9 for the table `lab` with two counters, and
/// a counter of another table.
const NFT_1_0_9: &str = r#"{"nftables": [{"metainfo": {"version": "1.0.9", "release_name": "Old Doc Yak #3", "json_schema_version": 1}}, {"counter": {"family": "inet", "name": "return", "table": "lab", "handle": 1, "packets": 12, "bytes": 3400}}, {"counter": {"family": "inet", "name": "dropped", "table": "lab", "handle": 2, "packets": 0, "bytes": 0}}, {"counter": {"family": "inet", "name": "return", "table": "other", "handle": 1, "packets": 7, "bytes": 70}}, {"counter": {"family": "ip", "name": "tcp", "table": "lab", "handle": 3, "packets": 1, "bytes": 60}}]}"#;

#[test]
fn the_counters_of_the_table_read() {
    let c = parse_counters(NFT_1_0_9, "lab").unwrap();
    assert_eq!(c.get("return"), Some(Count { packets: 12, bytes: 3400 }));
    assert_eq!(c.get("dropped"), Some(Count { packets: 0, bytes: 0 }));
    assert_eq!(c.names().collect::<Vec<_>>(), ["dropped", "return"], "no other table, no other family");
    assert_eq!(parse_counters(NFT_1_0_9, "other").unwrap().get("return"), Some(Count { packets: 7, bytes: 70 }));
    assert!(parse_counters(r#"{"nftables": []}"#, "lab").unwrap().is_empty());
}

#[test]
fn a_text_that_is_not_the_list_is_refused() {
    assert!(matches!(parse_counters("", "lab"), Err(CounterError::NotJson(_))));
    assert!(matches!(parse_counters("{\"nftables\": [", "lab"), Err(CounterError::NotJson(_))));
    assert_eq!(parse_counters("{}", "lab"), Err(CounterError::NoList));
    assert_eq!(parse_counters(r#"{"nftables": {}}"#, "lab"), Err(CounterError::NoList));
    assert_eq!(
        parse_counters(r#"{"nftables": [1]}"#, "lab"),
        Err(CounterError::BadItem { index: 0, why: "not an object of one key" })
    );
    let two_keys = r#"{"nftables": [{"counter": {}, "table": {}}]}"#;
    assert!(matches!(parse_counters(two_keys, "lab"), Err(CounterError::BadItem { index: 0, .. })));
    let no_packets = r#"{"nftables": [{"counter": {"family": "inet", "table": "lab", "name": "tcp", "bytes": 1}}]}"#;
    assert_eq!(
        parse_counters(no_packets, "lab"),
        Err(CounterError::BadItem { index: 0, why: "the counter has no packets" })
    );
    let negative =
        r#"{"nftables": [{"counter": {"family": "inet", "table": "lab", "name": "tcp", "packets": -1, "bytes": 1}}]}"#;
    assert!(matches!(parse_counters(negative, "lab"), Err(CounterError::BadItem { .. })));
    let twice = r#"{"nftables": [{"counter": {"family": "inet", "table": "lab", "name": "tcp", "packets": 1, "bytes": 1}}, {"counter": {"family": "inet", "table": "lab", "name": "tcp", "packets": 2, "bytes": 2}}]}"#;
    assert_eq!(parse_counters(twice, "lab"), Err(CounterError::Duplicate("tcp".into())));
    let long = format!("{{\"nftables\": [{}]}}", " ".repeat(MAX_TEXT));
    assert_eq!(parse_counters(&long, "lab"), Err(CounterError::TooLong(long.len())));
}

#[test]
fn a_bad_text_is_a_protocol_failure_of_the_host() {
    let failure: pod_core::Failure = parse_counters("{}", "lab").unwrap_err().into();
    assert_eq!((failure.stage, failure.class), (pod_core::Stage::Host, pod_core::Class::Protocol));
}
