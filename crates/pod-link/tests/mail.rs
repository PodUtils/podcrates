//! The mail carriers (PodUtils/podnet#484): the bytes cross both ways with
//! STARTTLS and with TLS from the first byte, over a submission server and a
//! retrieval server of the loopback; a refused message and a fetched body
//! over the limit are carrier limits, and the session stays open; port 25 is
//! refused by name before a connection.

#![cfg(all(feature = "smtp", feature = "imap"))]

mod common;

use std::time::Duration;

use common::mail::{
    ImapServer, SmtpServer, imap_carrier, imap_server, imap_server_silent, smtp_carrier, smtp_server,
    smtp_server_hidden, wait_messages,
};
use pod_link::smtp::Mode;
use pod_link::{Carrier, LimitKind, mail_body as body_of, mail_pair};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

/// 300 bytes that tell their own place.
fn three_hundred() -> Vec<u8> {
    (0..300u32).map(|at| (at % 251) as u8).collect()
}

/// The bytes up over submission, then the same message down over retrieval.
async fn both_ways(mode: Mode) {
    pod_testkit::need_loopback!();
    let submission = smtp_server(mode, None).await;
    let retrieval = imap_server(mode, None).await;
    let up = smtp_carrier(mode).dial(&submission.addr.to_string()).await.expect("a submission dial");
    let down = imap_carrier(mode).dial(&retrieval.addr.to_string()).await.expect("a retrieval dial");
    let mut io = mail_pair(up, down);
    let data = three_hundred();
    io.write_all(&data).await.expect("the bytes up");
    io.flush().await.expect("a flush");
    let messages = wait_messages(&submission, 1).await;
    assert_eq!(messages.len(), 1, "one message for the batch");
    assert_eq!(body_of(&messages[0]), data, "300 bytes up, as the server took them");
    // The mail server keeps the message; the other end fetches it.
    retrieval.store.lock().unwrap_or_else(|e| e.into_inner()).push(messages[0].clone());
    let mut back = vec![0u8; 300];
    timeout(Duration::from_secs(5), io.read_exact(&mut back))
        .await
        .expect("300 bytes down in time")
        .expect("the bytes down");
    assert_eq!(back, data, "300 bytes down, in order");
    assert_eq!(body_of(&messages[0]), data, "the body of the message is the payload, and no header");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn three_hundred_bytes_each_way_with_starttls() {
    both_ways(Mode::Starttls).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn three_hundred_bytes_each_way_with_implicit_tls() {
    both_ways(Mode::Implicit).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_message_is_a_limit_and_the_session_stays_open() {
    pod_testkit::need_loopback!();
    let server = smtp_server_hidden(Mode::Starttls, 1024).await;
    let mut smtp = smtp_carrier(Mode::Starttls);
    smtp.batch = 4096;
    let mut io = smtp.dial(&server.addr.to_string()).await.expect("a submission dial");
    let data = vec![7u8; 1000];
    io.write_all(&data).await.expect("the bytes up");
    let messages = wait_messages(&server, 2).await;
    assert!(messages.len() >= 2, "the carrier halved the message after the refusal: {}", messages.len());
    let limit = smtp.limit().expect("a reported limit");
    assert_eq!(limit.kind, LimitKind::Refused, "a refused message is a carrier limit");
    assert_eq!(limit.bytes, Some(1024), "the limit of the server, as it said it");
    let whole: Vec<u8> = messages.iter().flat_map(|message| body_of(message)).collect();
    assert_eq!(whole, data, "no byte of the refused message is lost or twice");
    // The session is open: another write goes through it.
    io.write_all(&[9u8; 50]).await.expect("another write");
    let later = wait_messages(&server, messages.len() + 1).await;
    assert!(later.len() > messages.len(), "the session took another message");
    let last = body_of(later.last().expect("a last message"));
    assert_eq!(last, vec![9u8; 50]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fetched_body_over_the_limit_is_a_limit_and_the_session_stays_open() {
    pod_testkit::need_loopback!();
    let server: ImapServer = imap_server(Mode::Starttls, None).await;
    let mut imap = imap_carrier(Mode::Starttls);
    imap.max_fetch = 300;
    let mut io = imap.dial(&server.addr.to_string()).await.expect("a retrieval dial");
    server.store.lock().unwrap_or_else(|e| e.into_inner()).push(vec![1u8; 1000]);
    let mut limit = None;
    for _ in 0..200 {
        limit = imap.limit();
        if limit.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let limit = limit.expect("a reported limit");
    assert_eq!(limit.kind, LimitKind::Truncated, "a body over the limit is cut");
    assert_eq!(limit.bytes, Some(1000));
    // The session is open: a message within the limit comes down.
    server.store.lock().unwrap_or_else(|e| e.into_inner()).push(vec![2u8; 300]);
    let mut got = vec![0u8; 300];
    timeout(Duration::from_secs(5), io.read_exact(&mut got)).await.expect("a body in time").expect("a body");
    assert_eq!(got, vec![2u8; 300]);
}

/// The limit is compared before the allocation: an over-limit literal is
/// read and dropped, and the message just under the limit behind it still
/// comes down. A `last` that moved past the dropped body would lose it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_body_above_the_fetch_limit_is_dropped_and_one_below_it_is_taken() {
    pod_testkit::need_loopback!();
    let server: ImapServer = imap_server(Mode::Implicit, None).await;
    let mut imap = imap_carrier(Mode::Implicit);
    imap.max_fetch = 1000;
    let mut io = imap.dial(&server.addr.to_string()).await.expect("a retrieval dial");
    {
        let mut store = server.store.lock().unwrap_or_else(|e| e.into_inner());
        store.push(vec![1u8; 1001]);
        store.push(vec![2u8; 999]);
    }
    let mut limit = None;
    for _ in 0..200 {
        limit = imap.limit();
        if limit.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let limit = limit.expect("a reported limit");
    assert_eq!(limit.kind, LimitKind::Truncated, "the over-limit body is a limit");
    assert_eq!(limit.bytes, Some(1001));
    let mut got = vec![0u8; 999];
    timeout(Duration::from_secs(5), io.read_exact(&mut got))
        .await
        .expect("the under-limit body in time")
        .expect("a body");
    assert_eq!(got, vec![2u8; 999], "the message under the limit came down");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn port_25_is_refused_by_name_before_a_dial() {
    pod_testkit::need_loopback!();
    let smtp = smtp_carrier(Mode::Starttls);
    let refused = smtp.dial("127.0.0.1:25").await.err().expect("port 25");
    assert_eq!(refused.kind, pod_link::CarrierErrorKind::Usage, "{refused}");
    assert_eq!(refused.class().exit_code(), 64, "a usage error");
    assert!(refused.to_string().contains("port 25"), "the error names the port: {refused}");
    // A connection to 25 would be a NotFound or an Io error: the rule came
    // before any socket.
    assert_ne!(refused.kind, pod_link::CarrierErrorKind::NotFound);
}

/// A server that accepts a command and never answers is the named command
/// timeout, not a carrier task that waits forever.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_that_never_answers_a_command_is_the_named_timeout() {
    pod_testkit::need_loopback!();
    let addr = imap_server_silent(Mode::Implicit).await;
    let mut imap = imap_carrier(Mode::Implicit);
    imap.poll = Duration::from_millis(100);
    imap.command_timeout = Duration::from_millis(200);
    let mut io = imap.dial(&addr.to_string()).await.expect("a retrieval dial");
    let mut buf = [0u8; 1];
    let failed = timeout(Duration::from_secs(5), async {
        loop {
            match io.read(&mut buf).await {
                Ok(0) => break None,
                Ok(_) => {}
                Err(e) => break Some(e.to_string()),
            }
        }
    })
    .await
    .expect("the stream ends within the test limit");
    let text = failed.expect("a failed stream");
    assert!(text.contains("UID FETCH"), "the error names the command: {text}");
    assert!(text.contains("took more than"), "the error names the timeout: {text}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrong_credential_is_refused_by_the_server() {
    pod_testkit::need_loopback!();
    let server: SmtpServer = smtp_server(Mode::Implicit, None).await;
    let credential = pod_link::smtp::Credential::new("session-user", "another-secret").expect("a credential");
    let mut smtp = pod_link::smtp::Smtp::new(credential, "a@example.invalid", "b@example.invalid")
        .with_tls(common::cert::client_config())
        .with_mode(Mode::Implicit);
    smtp.handshake_timeout = Duration::from_secs(5);
    let mut io = smtp.dial(&server.addr.to_string()).await.expect("a dial");
    // The failure of the session comes to the byte stream at the next
    // write.
    let mut refused = false;
    for _ in 0..50 {
        if io.write_all(b"x").await.is_err() {
            refused = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(refused, "a refused credential ends the stream");
}
