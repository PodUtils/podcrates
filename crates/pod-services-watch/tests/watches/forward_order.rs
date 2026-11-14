//! `forward-url` against the stand-in relay of `forward.rs`, where the
//! relay sets the order of the session: a forward only after a session opens
//! (srv.us), and a command in place of a shell (tunnl.gg) (T-552).

use crate::common::{fact, one, text};
use crate::forward::{Script, ended, relay, relay_file};

/// A relay that answers a forward only after a session opens (srv.us,
/// T-552): with the fact `session-first`, the client opens the session
/// before it asks for the forward of the port of the watch. With no fact,
/// the stand-in refuses the forward.
#[tokio::test]
async fn forward_url_opens_the_session_first_where_the_relay_says_so() {
    pod_testkit::need_loopback!();
    let chunks: Vec<&'static [u8]> = vec![b"1: https://abc.tunnel.test/\r\n"];
    let script = Script { chunks, session_first: true, ..Script::default() };
    let (port, seen, end) = relay(script.clone()).await;
    let more = fact("user", "\"nomatch\"") + &fact("session-first", "true");
    let report = one(&relay_file("s1", port, &more, "port = 1\nprove = false\n")).await;
    let all = text(&report);
    assert!(report.holds() && all.contains("as nomatch (a key of the run): the banner names the tunnel"), "{all}");
    ended(end).await;
    assert_eq!(seen.lock().unwrap().forward, Some(("localhost".to_string(), 1)));

    let (port, seen, end) = relay(script).await;
    let report = one(&relay_file("s2", port, &fact("user", "\"nomatch\""), "port = 1\nprove = false\n")).await;
    let all = text(&report);
    assert!(all.contains("FAILED s2 forward-url") && all.contains("the remote forward of port 1"), "{all}");
    ended(end).await;
    assert!(!seen.lock().unwrap().shell);
}

/// A relay whose shell banner has no URL with no terminal, and whose
/// command writes one JSON line (tunnl.gg's `output=json`, T-552): the
/// client asks the command of the fact `command`, and no shell.
#[tokio::test]
async fn forward_url_asks_the_command_of_the_relay() {
    pod_testkit::need_loopback!();
    let chunks: Vec<&'static [u8]> =
        vec![b"{\"event\":\"tunnel\",\"url\":\"https://brave-fox-7c41e09b.tunnel.test\",\"subdomain\":\"brave-fox-7c41e09b\"}\n"];
    let (port, seen, end) = relay(Script { chunks, no_key: true, command_only: true, ..Script::default() }).await;
    let report = one(&relay_file("c1", port, &fact("command", "\"output=json\""), "prove = false\n")).await;
    let all = text(&report);
    assert!(
        report.holds()
            && all.contains("(no key): the banner names the tunnel on the host brave-fox-7c41e09b.tunnel.test"),
        "{all}"
    );
    ended(end).await;
    let seen = seen.lock().unwrap();
    assert_eq!(seen.command.as_deref(), Some("output=json"));
    assert!(!seen.shell);
}
