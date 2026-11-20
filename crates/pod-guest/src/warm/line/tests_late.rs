//! A monitor that answers late: QEMU answers no request while it holds its
//! lock for the last part of a migration (PodUtils/podvm#618). A late
//! answer within the bound of the caller is no failure.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::{Arc, Mutex};

use super::*;
use crate::warm::tests::dir;

/// Longer than the old fixed wait of one answer (5 s).
const LATE: Duration = Duration::from_secs(6);

/// `answer` with the `id` of `request`, as QEMU gives it back.
fn with_id(answer: &str, request: &str) -> String {
    match qmp::json_member(request, "id") {
        Some(id) => {
            format!("{},\"id\":{}}}", answer.trim_end().strip_suffix('}').unwrap_or(answer), qmp::json_text(&id))
        }
        None => answer.to_string(),
    }
}

/// A monitor on `sock` for one connection. `answer` gives the line of each
/// request by its name and its count, and whether that answer is late: a
/// late answer comes after [`LATE`], with an event before it.
fn stand(sock: &Path, answer: impl Fn(&str, usize) -> (String, bool) + Send + 'static) -> Arc<Mutex<Vec<String>>> {
    let _ = std::fs::remove_file(sock);
    let l = UnixListener::bind(sock).expect("the QMP socket");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        let Ok((conn, _)) = l.accept() else { return };
        let mut w = conn.try_clone().expect("a second handle");
        let _ = w.write_all(b"{\"QMP\": {\"version\": {}}}\n");
        for line in BufReader::new(conn).lines().map_while(std::result::Result::ok) {
            let verb = qmp::json_member(&line, "execute").unwrap_or_default();
            let n = {
                let mut seen = log.lock().unwrap_or_else(|e| e.into_inner());
                seen.push(verb.clone());
                seen.iter().filter(|v| **v == verb).count()
            };
            let (text, late) = answer(&verb, n);
            if late {
                std::thread::sleep(LATE);
                let _ = w.write_all(b"{\"event\": \"MIGRATION\", \"data\": {\"status\": \"completed\"}}\n");
            }
            if w.write_all(format!("{}\n", with_id(&text, &line)).as_bytes()).is_err() {
                break;
            }
        }
    });
    seen
}

fn names(seen: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    seen.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The first `query-migrate` after the `migrate` answers after [`LATE`],
/// with an event before it: the save ends well within the bound of the
/// caller, and `cont` gets its own answer.
#[test]
fn a_late_answer_of_query_migrate_within_the_bound_is_no_failure() {
    pod_testkit::need_loopback!();
    let d = dir("line-late-migrate");
    let (sock, stream) = (d.join("qmp"), d.join("state.vm"));
    let path = stream.clone();
    let seen = stand(&sock, move |verb, n| match (verb, n) {
        ("query-migrate", 1) => ("{\"return\": {}}".into(), false),
        ("migrate", _) => {
            std::fs::write(&path, b"STATE").unwrap();
            ("{\"return\": {}}".into(), false)
        }
        ("query-migrate", 2) => ("{\"return\": {\"status\": \"completed\"}}".into(), true),
        ("query-migrate", _) => ("{\"return\": {\"status\": \"completed\"}}".into(), false),
        _ => ("{\"return\": {}}".into(), false),
    });
    let f = Freeze { monitor: &sock, stream: &stream, transport: Transport::File, fallback: false, wait: 4 * LATE };
    let frozen = freeze(&f, &mut ());
    let seen = names(&seen);
    assert_eq!(frozen.stream.as_ref().ok(), Some(&Transport::File), "{:?} {seen:?}", frozen.stream);
    assert!(frozen.goes_on.is_ok(), "{:?}", frozen.goes_on);
    assert_eq!(seen.iter().filter(|v| *v == "query-migrate").count(), 2, "{seen:?}");
    assert_eq!(seen.last().map(String::as_str), Some("cont"), "{seen:?}");
    let _ = std::fs::remove_dir_all(&d);
}

/// The first `query-status` of a load answers after [`LATE`], with an
/// event before it: the wait keeps its connection and gives `cont`.
#[test]
fn a_late_answer_of_query_status_within_the_bound_is_no_failure() {
    pod_testkit::need_loopback!();
    let d = dir("line-late-status");
    let sock = d.join("qmp");
    let seen = stand(&sock, |verb, n| match (verb, n) {
        ("query-status", 1) => ("{\"return\": {\"status\": \"paused\"}}".into(), true),
        _ => ("{\"return\": {}}".into(), false),
    });
    let ran = running(&sock, None, 4 * LATE);
    let seen = names(&seen);
    assert!(ran.is_ok(), "{ran:?} {seen:?}");
    assert_eq!(seen, ["qmp_capabilities", "query-status", "cont"]);
    let _ = std::fs::remove_dir_all(&d);
}
