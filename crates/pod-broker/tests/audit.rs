//! The audit: one JSON line for each connection, in the form of
//! `schemas/log.v1.schema.json`, and no secret value in any line. The test
//! plants a token, an `Authorization`, a cookie and a secret in a query and
//! a path, and looks for each in each line.

mod common;

use std::sync::Arc;

use common::*;
use pod_broker::{Broker, JsonLines, Settings};

const PLANTED: &str = "planted-secret-value-0001";

#[tokio::test]
async fn no_audit_line_holds_a_planted_secret() {
    let answer = b"HTTP/1.1 200 OK\r\nSet-Cookie: s=planted-secret-value-0001\r\nContent-Length: 0\r\n\r\n".to_vec();
    let far = Far::new(Kind::Http { answer, body: 0 });
    let out = Arc::new(std::sync::Mutex::new(Vec::new()));
    let writer = Shared(out.clone());
    let lines = Arc::new(JsonLines::new(writer));
    let broker = Arc::new(Broker::new(Settings::default(), Answers::public(), far).with_audit(lines));
    let token = format!("{PLANTED}-token");
    load(&broker, "job1", &token, OPEN);
    let auth = basic("job1", &token);
    let requests = [
        format!(
            "GET http://api.example.com/{PLANTED}/x?key={PLANTED} HTTP/1.1\r\n{auth}Authorization: Bearer {PLANTED}\r\n\
             Cookie: session={PLANTED}\r\n\r\n"
        ),
        format!("CONNECT a.example:443 HTTP/1.1\r\n{auth}\r\n"),
        format!("CONNECT 10.0.0.1:443 HTTP/1.1\r\n{auth}\r\n"),
        format!("CONNECT a.example:443 HTTP/1.1\r\nHost: b.example:443\r\n{auth}\r\n"),
        format!("CONNECT a.example:443 HTTP/1.1\r\n{}\r\n", basic("job1", &format!("{PLANTED}-wrong"))),
        format!("CONNECT a.example:443 HTTP/1.1\r\nProxy-Authorization: Bearer {PLANTED}\r\n\r\n"),
        format!("GET http://a.example/%2e/{PLANTED} HTTP/1.1\r\n{auth}\r\n"),
    ];
    for request in &requests {
        let (_, mut rest) = ask(&broker, request).await;
        // The end of the job's side ends a tunnel too.
        tokio::io::AsyncWriteExt::shutdown(&mut rest).await.unwrap();
        let read =
            tokio::time::timeout(pod_testkit::host::scaled(std::time::Duration::from_secs(10)), read_all(&mut rest))
                .await;
        assert!(read.is_ok(), "{request}");
    }
    broker.drop_job("job1");
    let text = wait_lines(&out, requests.len() + 2).await;
    assert!(!text.contains(PLANTED), "a secret in the audit:\n{text}");
    for line in text.lines() {
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        let keys: Vec<&str> = value.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        for key in &keys {
            assert!(["schema", "time", "level", "event", "fields", "stage", "class"].contains(key), "{key} in {line}");
        }
        for key in ["schema", "time", "level", "event", "fields"] {
            assert!(keys.contains(&key), "{key} is not in {line}");
        }
        assert!(line.starts_with("{\"schema\":1,"), "the schema is not the first key: {line}");
        assert!(value["time"].as_str().unwrap().ends_with('Z'));
        assert!(value["event"].as_str().unwrap().starts_with("broker."));
    }
    let connections = text.lines().filter(|l| l.contains("\"event\":\"broker.connection\"")).count();
    assert_eq!(connections, requests.len(), "one line for each connection:\n{text}");
}

/// The lines of the writer, when at least `count` came, within 10 s.
async fn wait_lines(out: &Arc<std::sync::Mutex<Vec<u8>>>, count: usize) -> String {
    let wait = async {
        loop {
            let text = String::from_utf8(out.lock().unwrap().clone()).unwrap();
            if text.lines().count() >= count {
                return text;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(pod_testkit::host::scaled(std::time::Duration::from_secs(10)), wait)
        .await
        .expect("the audit lines in time")
}

/// A writer that each test task can read.
struct Shared(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Shared {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The audit goes to `tracing` by default, with no output of the crate.
#[tokio::test]
async fn the_default_audit_is_tracing() {
    let broker = Arc::new(Broker::new(Settings::default(), Answers::public(), Far::new(Kind::Echo)));
    load(&broker, "job1", TOKEN, OPEN);
    assert_eq!(status(&broker, &connect("10.0.0.1:443")).await, 403);
}
