use super::*;

#[test]
fn an_error_response_is_not_ok_and_a_return_is() {
    assert!(response_ok("{\"return\": {}}"));
    assert!(response_ok("{\"QMP\": {\"version\": {}}}"));
    assert!(!response_ok("{\"error\": {\"class\": \"GenericError\", \"desc\": \"nope\"}}"));
    assert!(!response_ok("{\"error\": null}"));
}

#[test]
fn an_event_is_skipped_and_not_read_as_a_response() {
    assert!(is_event("{\"event\": \"RESET\", \"data\": {}}"));
    assert!(is_event("{\"event\": \"STOP\"}"));
    assert!(!is_event("{\"return\": {}}"));
    assert!(!is_event("{\"error\": \"nope\"}"));
    // A response that names an event in a string is a response.
    assert!(!is_event("{\"return\": \"event\"}"));
}

#[test]
fn a_member_of_a_response_is_read_with_or_with_no_space() {
    let status = "{\"return\": {\"status\": \"inmigrate\", \"running\": false}}";
    assert_eq!(json_member(status, "status").as_deref(), Some("inmigrate"));
    assert_eq!(json_member("{\"return\":{\"status\":\"completed\"}}", "status").as_deref(), Some("completed"));
    // A key in a value is not a member; a value that is not a string is
    // no text.
    assert_eq!(json_member("{\"desc\": \"status\", \"status\": \"failed\"}", "status").as_deref(), Some("failed"));
    assert_eq!(json_member("{\"status\": 3}", "status"), None);
    assert_eq!(json_member("{\"status\": \"a\\\"b\"}", "status"), None, "an escape is not read");
    assert_eq!(json_member("{\"return\": {}}", "status"), None);
}

#[test]
fn a_bool_member_is_read_and_a_key_in_a_value_is_not_a_member() {
    assert_eq!(json_bool("{\"locked\": true, \"x\": 1}", "locked"), Some(true));
    assert_eq!(json_bool("{\"locked\":false}", "locked"), Some(false));
    assert_eq!(json_bool("{\"desc\": \"locked\", \"locked\": true}", "locked"), Some(true));
    assert_eq!(json_bool("{\"locked\": \"true\"}", "locked"), None, "a string is not a bool");
    assert_eq!(json_bool("{\"return\": {}}", "locked"), None);
}

/// Each object of the list is one entry, with its nested objects and
/// with braces in its strings.
#[test]
fn each_object_of_a_list_response_is_one_entry() {
    let line = "{\"return\": [{\"a\": {\"b\": [1, {\"c\": 2}]}, \"s\": \"}{\\\"[\"}, {\"d\": 3}]}";
    assert_eq!(list_entries(line), vec!["{\"a\": {\"b\": [1, {\"c\": 2}]}, \"s\": \"}{\\\"[\"}", "{\"d\": 3}"]);
    assert!(list_entries("{\"return\": []}").is_empty());
    assert!(list_entries("{\"return\": {}}").is_empty());
}

#[test]
fn the_key_request_presses_a_modifier_and_a_key_together() {
    // The shape that the emulator parses, with no JSON dependency.
    assert_eq!(
        send_key(&["shift", "semicolon"]),
        "{\"execute\":\"send-key\",\"arguments\":{\"keys\":[{\"type\":\"qcode\",\"data\":\"shift\"},{\"type\":\"qcode\",\"data\":\"semicolon\"}],\"hold-time\":30}}"
    );
}

/// Each printable ASCII character has a key, and a character outside
/// the table has none.
#[test]
fn each_printable_ascii_character_has_a_key() {
    for c in ' '..='~' {
        assert!(key(c).is_some(), "no key for {c:?}");
    }
    assert_eq!(key('a'), Some(("a", false)));
    assert_eq!(key('A'), Some(("a", true)));
    assert_eq!(key(':'), Some(("semicolon", true)));
    assert_eq!(key('\t'), None);
    assert_eq!(key('é'), None);
}

#[test]
fn an_id_goes_at_the_end_of_a_request_and_is_read_back_from_an_answer() {
    assert_eq!(monitor::with_id("{\"execute\":\"cont\"}", 7), "{\"execute\":\"cont\",\"id\":\"pod-qmp-7\"}");
    let change = "{\"execute\":\"x\",\"arguments\":{\"id\":\"wqmbox\"}}";
    let sent = monitor::with_id(change, 2);
    assert_eq!(json_member(&sent, "id").as_deref(), Some("wqmbox"), "the first `id` stays the one of the arguments");
    assert_eq!(monitor::with_id("{}", 3), "{}");
    assert_eq!(monitor::with_id("not json", 3), "not json");
    assert_eq!(monitor::id_of("{\"return\": {}, \"id\": \"pod-qmp-12\"}"), Some(12));
    assert_eq!(monitor::id_of("{\"id\": \"pod-qmp-3\", \"return\": {\"id\": \"wqmbox\"}}"), Some(3));
    assert_eq!(monitor::id_of("{\"return\": {\"id\": \"wqmbox\"}}"), None);
    assert_eq!(monitor::id_of("{\"return\": {\"id\": 4}}"), None);
}

mod late {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use super::super::*;

    const SLOW: Duration = Duration::from_millis(900);
    const SHORT: Duration = Duration::from_millis(200);

    /// A monitor that answers each request in turn: the first after
    /// [`SLOW`], with an event before it and the answer in two parts; each
    /// other at once. `ids`: it gives back the `id` of each request, as
    /// QEMU does.
    fn stand(tag: &str, ids: bool) -> PathBuf {
        let dir = pod_testkit::host::socket_dir(&format!("pod-guest-late-{tag}"));
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("qmp.sock");
        let _ = std::fs::remove_file(&sock);
        let l = UnixListener::bind(&sock).unwrap();
        std::thread::spawn(move || {
            let Ok((s, _)) = l.accept() else { return };
            let mut w = s.try_clone().unwrap();
            let _ = w.write_all(b"{\"QMP\": {}}\n");
            let mut n = 0;
            for line in BufReader::new(s).lines().map_while(std::result::Result::ok) {
                let verb = json_member(&line, "execute").unwrap_or_default();
                let id = match (ids, json_member(&line, "id")) {
                    (true, Some(id)) => format!(", \"id\": {}", json_text(&id)),
                    _ => String::new(),
                };
                let answer = format!("{{\"return\": {{\"verb\": \"{verb}\"}}{id}}}\n");
                if verb != "qmp_capabilities" {
                    n += 1;
                }
                if n == 1 {
                    std::thread::sleep(SLOW);
                    let _ = w.write_all(b"{\"event\": \"MIGRATION\", \"data\": {\"status\": \"completed\"}}\n");
                    let (a, b) = answer.split_at(answer.len() / 2);
                    let _ = w.write_all(a.as_bytes());
                    std::thread::sleep(SHORT);
                    let _ = w.write_all(b.as_bytes());
                } else if w.write_all(answer.as_bytes()).is_err() {
                    break;
                }
            }
        });
        sock
    }

    /// A late answer is read by `late` with the event before it; the next
    /// request gets its own answer.
    #[test]
    fn a_late_answer_is_read_after_its_bound_with_the_event_before_it() {
        pod_testkit::need_loopback!();
        for ids in [true, false] {
            let sock = stand(&format!("read-{ids}"), ids);
            let mut m = Monitor::connect(&sock, Duration::from_secs(5)).unwrap();
            let at = Instant::now();
            let e = m.ask_within("{\"execute\":\"query-migrate\"}", SHORT).unwrap_err();
            assert_eq!(e.class, Class::Timeout, "{ids}: {e}");
            assert!(at.elapsed() < SLOW, "{ids}: the bound of the caller is the bound");
            assert!(m.owes(), "{ids}");
            let line = m.late(Duration::from_secs(10)).unwrap();
            assert_eq!(json_member(&line, "verb").as_deref(), Some("query-migrate"), "{ids}: {line}");
            assert!(!m.owes(), "{ids}");
            let events = m.take_events();
            assert_eq!(events.len(), 1, "{ids}: {events:?}");
            assert!(events[0].contains("MIGRATION"), "{ids}: {events:?}");
            let line = m.ask("{\"execute\":\"cont\"}").unwrap();
            assert_eq!(json_member(&line, "verb").as_deref(), Some("cont"), "{ids}: {line}");
            assert!(m.late(SHORT).is_err_and(|e| e.class == Class::Usage), "{ids}: no request waits");
        }
    }

    /// With no `late`, the next request skips the late answer of the one
    /// before and reads its own.
    #[test]
    fn the_next_request_does_not_read_the_late_answer_of_the_one_before() {
        pod_testkit::need_loopback!();
        for ids in [true, false] {
            let sock = stand(&format!("skip-{ids}"), ids);
            let mut m = Monitor::connect(&sock, Duration::from_secs(5)).unwrap();
            m.set_answer_wait(SHORT);
            assert!(m.ask("{\"execute\":\"query-migrate\"}").is_err(), "{ids}");
            m.set_answer_wait(Duration::from_secs(10));
            let line = m.ask("{\"execute\":\"cont\"}").unwrap();
            assert_eq!(json_member(&line, "verb").as_deref(), Some("cont"), "{ids}: {line}");
            assert!(!m.owes(), "{ids}");
            assert_eq!(m.take_events().len(), 1, "{ids}");
        }
    }
}

pub(crate) mod queue;
