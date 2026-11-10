//! The control: `load`, `drop` and `list` as JSON lines, on the Unix socket
//! of mode 0600 and as single lines; the end of a job at `drop`, at a new
//! `load` and at its TTL, with each of its open connections.

mod common;

use std::time::Duration;

use common::*;
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;

fn load_line(job: &str, token: &str, ttl: u64) -> String {
    json!({"op": "load", "job": job, "token": token, "ttl_s": ttl, "policy": {"version": 1, "mode": "open"}})
        .to_string()
}

#[cfg(unix)]
#[tokio::test]
async fn the_control_socket_loads_lists_and_drops_jobs() {
    use std::os::unix::fs::PermissionsExt;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = socket_dir("pod-broker-control");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("control.sock");
    let listener = pod_broker::control::bind_unix(&path, 0o600).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    let (broker, audit) = broker(Far::new(Kind::Echo), Answers::public());
    let serving = tokio::spawn(pod_broker::control::serve_control(broker.clone(), listener));
    let stream = tokio::net::UnixStream::connect(&path).await.unwrap();
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    let mut ask = async |line: String| -> Value {
        write.write_all(format!("{line}\n").as_bytes()).await.unwrap();
        serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap()
    };
    let loaded = ask(load_line("job1", TOKEN, 600)).await;
    assert_eq!(loaded, json!({"v":1,"ok":true,"op":"load","job":"job1","mode":"open","rules":0,"expires_in_s":600}));
    assert!(!loaded.to_string().contains(TOKEN));
    assert_eq!(status(&broker, &connect("a.example:443")).await, 200);
    let listed = ask(r#"{"op":"list"}"#.into()).await;
    assert_eq!(listed["jobs"][0]["job"], "job1");
    assert_eq!(listed["jobs"][0]["mode"], "open");
    assert_eq!(ask(r#"{"op":"drop","job":"job1"}"#.into()).await["dropped"], true);
    assert_eq!(ask(r#"{"op":"drop","job":"job1"}"#.into()).await["dropped"], false);
    assert_eq!(ask(r#"{"op":"list"}"#.into()).await["jobs"], json!([]));
    assert_eq!(status(&broker, &connect("a.example:443")).await, 407);
    // A bad request gets an answer of the class usage, and the peer goes on.
    for bad in [
        "{".to_string(),
        r#"{"op":"halt"}"#.to_string(),
        r#"{"op":"load","job":"job1"}"#.to_string(),
        load_line("job 1", TOKEN, 600),
        load_line("job1", "short", 600),
        load_line("job1", TOKEN, 0),
        load_line("job1", TOKEN, 99_999_999),
        json!({"op":"load","job":"j","token":TOKEN,"ttl_s":"600","policy":{"version":1,"mode":"open"}}).to_string(),
        json!({"op":"load","job":"j","token":TOKEN,"ttl_s":60,"policy":{"version":9,"mode":"open"}}).to_string(),
    ] {
        let answer = ask(bad.clone()).await;
        assert_eq!((answer["ok"].as_bool(), answer["class"].as_str()), (Some(false), Some("usage")), "{bad}");
        assert!(!answer.to_string().contains(TOKEN), "{bad}: {answer}");
        assert!(pod_broker::control::answer_failure(&answer).is_some());
    }
    assert!(!audit.lines().iter().any(|l| l.to_json().contains(TOKEN)));
    serving.abort();
    std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn a_socket_path_that_is_another_file_is_refused() {
    let dir = std::env::temp_dir().join(format!("pod-broker-bind-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("file");
    std::fs::write(&path, "x").unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let _guard = runtime.enter();
    assert!(pod_broker::control::bind_unix(&path, 0o600).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "x");
    // A socket that is there is taken away and bound again.
    let socket = dir.join("s.sock");
    drop(pod_broker::control::bind_unix(&socket, 0o600).unwrap());
    assert!(pod_broker::control::bind_unix(&socket, 0o600).is_ok());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A token is the token of one job.
#[tokio::test]
async fn a_token_of_another_job_is_refused_and_a_reload_replaces_the_job() {
    let (broker, _) = broker(Far::new(Kind::Echo), Answers::public());
    assert_eq!(broker.control_line(&load_line("job1", TOKEN, 600))["ok"], true);
    let taken = broker.control_line(&load_line("job2", TOKEN, 600));
    assert_eq!((taken["ok"].as_bool(), taken["class"].as_str()), (Some(false), Some("usage")));
    // A new load of job1 with a new token: the old token is gone.
    assert_eq!(broker.control_line(&load_line("job1", TOKEN_TWO, 600))["ok"], true);
    assert_eq!(status(&broker, &connect("a.example:443")).await, 407);
    let bearer = format!("CONNECT a.example:443 HTTP/1.1\r\nProxy-Authorization: Bearer {TOKEN_TWO}\r\n\r\n");
    assert_eq!(status(&broker, &bearer).await, 200);
}

/// `drop` ends each open connection of the job, and its state is gone.
#[tokio::test]
async fn a_dropped_job_ends_its_open_connection() {
    let (broker, audit) = broker(Far::new(Kind::Echo), Answers::public());
    load(&broker, "job1", TOKEN, OPEN);
    let (head, mut tunnel) = ask(&broker, &connect("a.example:443")).await;
    assert_eq!(status_of(&head), 200);
    assert_eq!(round_trip(&mut tunnel, b"before").await, b"before");
    assert_eq!(broker.list()[0].open, 1);
    assert_eq!(broker.control_line(r#"{"op":"drop","job":"job1"}"#)["dropped"], true);
    let mut rest = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(10), tunnel.read_to_end(&mut rest)).await;
    assert!(matches!(read, Ok(Ok(0))), "the tunnel ends: {read:?}");
    let line = audit.connection("allow").await;
    assert_eq!((line.fields["end"].as_str(), line.class), (Some("job-ended"), Some("revoked")));
    assert_eq!(line.fields["up"], 6);
    assert!(broker.list().is_empty());
    assert_eq!(status(&broker, &connect("a.example:443")).await, 407);
    let drop = audit.wait(|l| l.event == "broker.job.drop").await;
    assert_eq!((drop.fields["job"].as_str(), drop.fields["dropped"].as_bool()), (Some("job1"), Some(true)));
}

/// At its TTL a job ends with each open connection, and its token is gone.
#[tokio::test(start_paused = true)]
async fn a_job_ends_at_its_ttl_with_its_open_connection() {
    let (broker, audit) = broker(Far::new(Kind::Echo), Answers::public());
    assert_eq!(broker.control_line(&load_line("job1", TOKEN, 5))["expires_in_s"], 5);
    let (head, mut tunnel) = ask(&broker, &connect("a.example:443")).await;
    assert_eq!(status_of(&head), 200);
    let started = tokio::time::Instant::now();
    let mut rest = Vec::new();
    assert_eq!(tunnel.read_to_end(&mut rest).await.unwrap(), 0);
    assert!(started.elapsed() >= Duration::from_secs(4), "{:?}", started.elapsed());
    let line = audit.connection("allow").await;
    assert_eq!(line.fields["end"], "job-ended");
    assert!(broker.list().is_empty());
    assert_eq!(status(&broker, &connect("a.example:443")).await, 407);
}

/// A job that expired is gone also with no timer: a lookup drops it.
#[test]
fn a_job_expires_at_a_lookup_with_no_runtime_timer() {
    let (broker, _) = broker(Far::new(Kind::Echo), Answers::public());
    // No runtime yet: the load starts no timer.
    assert_eq!(broker.control_line(&load_line("job1", TOKEN, 2))["ok"], true);
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().start_paused(true).build().unwrap();
    runtime.block_on(async {
        assert_eq!(broker.list().len(), 1);
        tokio::time::advance(Duration::from_secs(3)).await;
        assert!(broker.list().is_empty());
        assert_eq!(status(&broker, &connect("a.example:443")).await, 407);
    });
}
