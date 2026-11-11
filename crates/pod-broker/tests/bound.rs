//! A listener bound to one job (T-160): each connection is the job's with
//! no token; a token of another job gets 407; two listeners keep two
//! policies; a drop ends the listener and its connections, a reload keeps
//! it; the audit line names the job and holds no path, query or header.

mod common;

use std::time::Duration;

use common::*;

const ONLY_A: &str = r#"{"version":1,"mode":"allow","allow":[{"host":"a.example"}]}"#;
const ONLY_B: &str = r#"{"version":1,"mode":"allow","allow":[{"host":"b.example"}]}"#;

/// A CONNECT with no `Proxy-Authorization`, as `podnet pod -x
/// http://unix:/PATH` sends it.
fn bare(target: &str) -> String {
    format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n")
}

fn with_auth(target: &str, auth: &str) -> String {
    format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n{auth}\r\n")
}

#[test]
fn the_readme_names_the_unix_form_that_podnet_parses() {
    let readme = include_str!("../README.md");
    assert!(readme.contains("http://unix:/"), "the README names no http://unix:/PATH");
    assert!(!readme.contains("http+unix:"), "the README names a form that podnet does not parse");
}

#[cfg(unix)]
mod unix {
    use std::path::PathBuf;
    use std::sync::Arc;

    use pod_broker::Broker;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;
    use tokio::task::JoinHandle;

    use super::*;

    fn listen(broker: &Arc<Broker>, job: &str, leaf: &str) -> (PathBuf, JoinHandle<std::io::Result<()>>) {
        let path = pod_testkit::host::socket_path(&format!("pod-broker-{leaf}-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = tokio::net::UnixListener::bind(&path).expect("bind");
        (path, tokio::spawn(broker.clone().serve_unix_job(job.to_string(), listener)))
    }

    async fn ask_at(path: &PathBuf, request: &str) -> (u16, UnixStream) {
        let mut stream = UnixStream::connect(path).await.expect("connect");
        stream.write_all(request.as_bytes()).await.unwrap();
        let head = read_head(&mut stream).await;
        (status_of(&head), stream)
    }

    #[tokio::test]
    async fn a_connection_is_the_job_of_its_listener_with_no_token() {
        let (broker, audit) = broker(Far::new(Kind::Echo), Answers::public());
        load(&broker, "job1", TOKEN, ONLY_A);
        load(&broker, "job2", TOKEN_TWO, ONLY_B);
        let (path, _serving) = listen(&broker, "job1", "own");
        let (status, mut tunnel) = ask_at(&path, &bare("a.example:443")).await;
        assert_eq!(status, 200);
        assert_eq!(round_trip(&mut tunnel, b"ping").await, b"ping");
        // The token of the job itself is no harm.
        assert_eq!(ask_at(&path, &with_auth("a.example:443", &basic("job1", TOKEN))).await.0, 200);
        // The token of another job, or of no job, is refused and audited.
        assert_eq!(ask_at(&path, &with_auth("b.example:443", &basic("job2", TOKEN_TWO))).await.0, 407);
        let line = audit.wait(|l| l.fields["status"] == 407).await;
        assert_eq!((line.fields["job"].as_str(), line.fields["rule"].as_str()), (Some("job1"), Some("auth")));
        assert_eq!(ask_at(&path, &with_auth("a.example:443", "Proxy-Authorization: Bearer nothing\r\n")).await.0, 407);
        let _ = std::fs::remove_file(path);
    }

    /// Two listeners in one broker: each has the policy of its own job. A
    /// listener that took the policy of the other job, or of the broker as
    /// a whole, fails on one of the four lines.
    #[tokio::test]
    async fn two_listeners_keep_two_policies() {
        let (broker, _audit) = broker(Far::new(Kind::Echo), Answers::public());
        load(&broker, "job1", TOKEN, ONLY_A);
        load(&broker, "job2", TOKEN_TWO, ONLY_B);
        let (one, _a) = listen(&broker, "job1", "one");
        let (two, _b) = listen(&broker, "job2", "two");
        assert_eq!(ask_at(&one, &bare("a.example:443")).await.0, 200);
        assert_eq!(ask_at(&one, &bare("b.example:443")).await.0, 403);
        assert_eq!(ask_at(&two, &bare("a.example:443")).await.0, 403);
        assert_eq!(ask_at(&two, &bare("b.example:443")).await.0, 200);
        let _ = (std::fs::remove_file(one), std::fs::remove_file(two));
    }

    #[tokio::test]
    async fn a_drop_ends_the_listener_and_its_connections() {
        let (broker, audit) = broker(Far::new(Kind::Echo), Answers::public());
        load(&broker, "job1", TOKEN, ONLY_A);
        let (path, serving) = listen(&broker, "job1", "drop");
        let (status, mut tunnel) = ask_at(&path, &bare("a.example:443")).await;
        assert_eq!(status, 200);
        assert!(broker.drop_job("job1"));
        let bound = pod_testkit::host::scaled(Duration::from_secs(10));
        let mut rest = Vec::new();
        let read = tokio::time::timeout(bound, tunnel.read_to_end(&mut rest)).await;
        assert!(matches!(read, Ok(Ok(0))), "the tunnel ends: {read:?}");
        let ended = tokio::time::timeout(bound, serving).await.expect("the listener ends").unwrap();
        assert!(ended.is_ok(), "{ended:?}");
        let line = audit.wait(|l| l.event == "broker.listener.end").await;
        assert_eq!(line.fields["job"], "job1");
        assert!(UnixStream::connect(&path).await.is_err(), "a new connect after the drop");
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn a_reload_keeps_the_listener_with_the_new_policy() {
        let (broker, _audit) = broker(Far::new(Kind::Echo), Answers::public());
        load(&broker, "job1", TOKEN, ONLY_A);
        let (path, serving) = listen(&broker, "job1", "reload");
        assert_eq!(ask_at(&path, &bare("c.example:443")).await.0, 403);
        load(&broker, "job1", TOKEN, OPEN);
        assert_eq!(ask_at(&path, &bare("c.example:443")).await.0, 200);
        assert!(!serving.is_finished());
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn a_listener_of_a_job_that_is_not_loaded_is_refused() {
        let (broker, _audit) = broker(Far::new(Kind::Echo), Answers::public());
        let (path, serving) = listen(&broker, "nojob", "none");
        let e = serving.await.unwrap().expect_err("a listener of no job");
        assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
        let _ = std::fs::remove_file(path);
    }

    /// The audit line of a bound connection names its job and holds no
    /// path, no query and no header (call C31).
    #[tokio::test]
    async fn the_audit_line_names_the_job_and_no_path_query_or_header() {
        let far = Far::new(Kind::Http { answer: b"HTTP/1.1 204 No Content\r\n\r\n".to_vec(), body: 0 });
        let (broker, audit) = broker(far, Answers::public());
        load(&broker, "job1", TOKEN, OPEN);
        let (path, _serving) = listen(&broker, "job1", "audit");
        let get = "GET http://a.example/secret-path?key=planted HTTP/1.1\r\nHost: a.example\r\nX-Planted: header-value\r\n\r\n";
        assert_eq!(ask_at(&path, get).await.0, 204);
        let line = audit.connection("allow").await;
        assert_eq!(line.fields["job"], "job1");
        let text = line.to_json();
        for planted in ["secret-path", "planted", "header-value", "X-Planted"] {
            assert!(!text.contains(planted), "{planted} in {text}");
        }
        let _ = std::fs::remove_file(path);
    }
}

#[tokio::test]
async fn a_tcp_listener_on_loopback_is_bound_the_same_way() {
    use tokio::io::AsyncWriteExt;

    let (broker, _audit) = broker(Far::new(Kind::Echo), Answers::public());
    load(&broker, "job1", TOKEN, ONLY_A);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().unwrap();
    let _serving = tokio::spawn(broker.clone().serve_tcp_job("job1".to_string(), listener));
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(bare("a.example:443").as_bytes()).await.unwrap();
    assert_eq!(status_of(&read_head(&mut stream).await), 200);
}
