//! The tests of the edge discovery.

use super::*;

#[test]
fn parses_an_srv_data_field() {
    assert_eq!(parse_srv("2 1 7844 region2.v2.argotunnel.com."), Some(("region2.v2.argotunnel.com".to_string(), 7844)));
    assert_eq!(parse_srv("garbage"), None);
}

#[test]
fn decodes_a_cloudflare_doh_answer() {
    let body = r#"{"Status":0,"Answer":[{"name":"_v2-origintunneld._tcp.argotunnel.com","type":33,"TTL":300,"data":"2 1 7844 region2.v2.argotunnel.com"}]}"#;
    let parsed = pod_dns::json::parse(body.as_bytes()).unwrap();
    assert_eq!(parsed.status, Some(0));
    assert_eq!(parsed.records.len(), 1);
    assert_eq!(parsed.records[0].kind, 33);
}

#[test]
fn an_srv_answer_of_another_shape_gives_the_fixed_names() {
    let t = |list: &[(&str, u16)]| -> Vec<(String, u16)> { list.iter().map(|(n, p)| (n.to_string(), *p)).collect() };
    let fixed = t(&fixed_targets());
    let good = t(&[("region1.v2.argotunnel.com", 7844), ("region2.v2.argotunnel.com", 7844)]);
    assert_eq!(checked_targets(good.clone()), good);
    let three = t(&[("a.argotunnel.com", 7844), ("b.argotunnel.com", 7844), ("c.argotunnel.com", 7844)]);
    assert_eq!(checked_targets(three), fixed);
    assert_eq!(checked_targets(t(&[("evil.example.com", 7844)])), fixed);
    assert_eq!(checked_targets(t(&[("a.argotunnel.com", 7844), ("b.argotunnel.com", 443)])), fixed);
    assert_eq!(checked_targets(Vec::new()), fixed);
}

/// The reason of the fixed names is an event for the user.
#[test]
fn an_srv_answer_of_another_shape_is_an_event() {
    let (report, seen) = crate::testing::recorded();
    let three = ["a", "b", "c"].map(|n| (format!("{n}.argotunnel.com"), 7844)).to_vec();
    checked_targets_with(three, &report);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!((seen[0].level, seen[0].kind), (pod_core::Level::Warn, "edge.discovery"));
    assert!(seen[0].text.contains("3 targets"), "{}", seen[0].text);
}

#[test]
fn the_static_list_respects_the_filter() {
    assert!(static_edges(IpVersionFilter::V4Only).iter().all(|e| e.version == EdgeIpVersion::V4));
    assert!(static_edges(IpVersionFilter::V6Only).is_empty());
}

/// The events of `seen` that say a slow source.
fn slow_events(seen: &std::sync::Mutex<Vec<pod_core::Event>>) -> Vec<pod_core::Event> {
    seen.lock().unwrap().iter().filter(|e| e.kind == "edge.discovery" && e.text.contains(" waited ")).cloned().collect()
}

/// A DoH resolver that accepts each connection and never answers
/// (PodUtils/podnet#755): the DoH source ends in its limit, the static
/// list follows, and one event gives the time, the limit and the cause.
#[tokio::test(start_paused = true)]
async fn a_doh_resolver_that_never_answers_ends_in_the_limit_with_one_event() {
    let (host, seen, accepted) = crate::testing::silent_doh_host(4).await;
    let start = Instant::now();
    let edges = discover(&host, IpVersionFilter::Auto).await.unwrap();
    let took = start.elapsed();
    assert_eq!(edges, static_edges(IpVersionFilter::Auto));
    assert!(took >= DOH_LIMIT && took < DOH_LIMIT + DOH_EACH, "{took:?}");
    assert!(accepted.load(std::sync::atomic::Ordering::SeqCst) >= 2);
    let slow = slow_events(&seen);
    assert_eq!(slow.len(), 1, "{slow:?}");
    let text = &slow[0].text;
    assert_eq!(slow[0].level, pod_core::Level::Warn);
    for words in [
        "DNS over HTTPS",
        "the limit of 8 s",
        "DNS over HTTPS SRV _v2-origintunneld",
        "no answer within 3 s",
        "the static list",
    ] {
        assert!(text.contains(words), "{words:?} is not in {text}");
    }
    // The line of the static list does not say the cause again.
    let events = seen.lock().unwrap();
    let last = &events.last().unwrap().text;
    assert!(last.contains("DoH: the line before gives the cause") && !last.contains("no answer"), "{last}");
}

/// A slow source that gives addresses is an event at the level info, and a
/// fast source gives none.
#[test]
fn a_slow_source_that_answers_is_an_info_event() {
    let edges = Ok(static_edges(IpVersionFilter::Auto));
    let event = slow_event("DNS over HTTPS", "the static list", Duration::from_millis(4200), DOH_LIMIT, &edges);
    assert_eq!((event.level, event.kind), (pod_core::Level::Info, "edge.discovery"));
    assert_eq!(event.text, "the DNS of the edge took 4.2 s on DNS over HTTPS");
}
