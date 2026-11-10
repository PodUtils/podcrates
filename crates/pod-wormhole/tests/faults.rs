//! What a mailbox that goes wrong does to a client (`harness::stand_in::
//! scripted`: one connection, a script): the bind that the client sends; a
//! welcome with an error or a permission that the client does not give; no
//! welcome in time; a frame that is not JSON; a Close; an answer that is
//! none; a flood of messages while a command waits; a message with a part
//! missing; a refusal of a third side. Each is a typed failure that holds
//! the mailbox's words on one line, cut short.

mod harness;

use std::time::Duration;

use harness::stand_in::{scripted, welcome};
use pod_testkit::need_loopback;
use pod_wormhole::mailbox::{Mailbox, crowded};
use pod_wormhole::{Code, Config, Failed, Wormhole};
use pod_ws::RelaySession;
use serde_json::{Value, json};
use tokio::net::TcpStream;

type Session = RelaySession<TcpStream>;

/// A script that parks the connection once its lines are said.
async fn park() {
    tokio::time::sleep(Duration::from_secs(30)).await;
}

fn quick() -> Config {
    Config { answer_limit: Duration::from_millis(300), ..harness::config() }
}

async fn said(session: &Session, value: Value) {
    session.send_text(&value.to_string()).await.expect("a line to the client");
}

/// The command that the client sent, as JSON.
async fn command(session: &Session) -> Value {
    let f = session.read_frame().await.expect("a command");
    serde_json::from_slice(&f.payload).expect("JSON")
}

/// A client connected to a script that sends its welcome, then runs `rest`.
async fn client<F, Fut>(rest: F) -> Result<Mailbox, Failed>
where
    F: FnOnce(Session) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let (url, _task) = scripted(move |session| async move {
        session.send_text(&welcome()).await.unwrap();
        rest(session).await;
    })
    .await;
    Mailbox::connect(&url, &quick(), "abcdef0123").await
}

#[tokio::test]
async fn the_bind_names_the_appid_the_side_and_the_client() {
    need_loopback!();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mailbox = client(move |session| async move {
        let _ = tx.send(command(&session).await);
        park().await;
    })
    .await
    .expect("connected");
    let bind = rx.await.expect("the bind");
    assert_eq!(bind["type"], "bind");
    assert_eq!(bind["appid"], harness::APPID);
    assert_eq!(bind["side"], "abcdef0123");
    assert_eq!(bind["client_version"], json!(["pod-wormhole-test", "0.0.0"]));
    assert!(bind["id"].is_string());
    assert_eq!(mailbox.appid(), harness::APPID);
    assert_eq!(mailbox.motd.as_deref(), Some("a stand-in"));
}

#[tokio::test]
async fn a_welcome_with_an_error_is_a_refusal_in_the_mailboxs_words_on_one_line() {
    need_loopback!();
    let (url, _task) = scripted(|session| async move {
        let error = format!("down for\nmaintenance {}", "x".repeat(500));
        said(&session, json!({ "type": "welcome", "welcome": { "error": error } })).await;
        park().await;
    })
    .await;
    let Err(Failed::Refused(why)) = Mailbox::connect(&url, &quick(), "ab").await.map(|_| ()) else { panic!() };
    assert!(why.contains("refused this client: down for maintenance xxx"), "{why}");
    assert!(!why.contains('\n') && why.len() < url.shown().len() + 300, "{} bytes", why.len());
}

#[tokio::test]
async fn a_permission_that_the_client_does_not_give_is_refused_unless_none_is_enough() {
    need_loopback!();
    let (url, _task) = scripted(|session| async move {
        let asked = json!({ "hashcash": { "bits": 20, "resource": "r" } });
        said(&session, json!({ "type": "welcome", "welcome": { "permission-required": asked } })).await;
        park().await;
    })
    .await;
    let Err(Failed::Refused(why)) = Mailbox::connect(&url, &quick(), "ab").await.map(|_| ()) else { panic!() };
    assert!(why.contains("asks for a permission that this client does not give"), "{why}");
    let ok = client(|_| park()).await;
    assert!(ok.is_ok());
    let (url, _task) = scripted(|session| async move {
        let asked = json!({ "none": {}, "hashcash": { "bits": 20, "resource": "r" } });
        said(&session, json!({ "type": "welcome", "welcome": { "permission-required": asked } })).await;
        park().await;
    })
    .await;
    assert!(Mailbox::connect(&url, &quick(), "ab").await.is_ok(), "a server that takes none is used as it is");
}

#[tokio::test]
async fn a_mailbox_that_sends_no_welcome_in_time_is_broken() {
    need_loopback!();
    let (url, _task) = scripted(|session| async move {
        let _held = session;
        park().await;
    })
    .await;
    let Err(Failed::Broken(why)) = Mailbox::connect(&url, &quick(), "ab").await.map(|_| ()) else { panic!() };
    assert!(why.contains("did not answer welcome within 300ms"), "{why}");
}

#[tokio::test]
async fn a_frame_that_is_no_json_object_is_broken() {
    need_loopback!();
    for text in ["not json", "[1]", "7"] {
        let (url, _task) = scripted(move |session| async move {
            session.send_text(text).await.unwrap();
            park().await;
        })
        .await;
        let Err(Failed::Broken(why)) = Mailbox::connect(&url, &quick(), "ab").await.map(|_| ()) else { panic!() };
        assert!(why.contains("sent a message that is not JSON"), "{text}: {why}");
    }
}

#[tokio::test]
async fn a_close_ends_the_wait_with_its_code_and_reason() {
    need_loopback!();
    let (url, _task) = scripted(|session| async move {
        session.send_close(1001, "going\naway").await.unwrap();
        park().await;
    })
    .await;
    let Err(Failed::Broken(why)) = Mailbox::connect(&url, &quick(), "ab").await.map(|_| ()) else { panic!() };
    assert!(why.contains("closed the connection (1001 going away)"), "{why}");
}

#[tokio::test]
async fn a_nameplate_that_is_no_digits_and_a_mailbox_with_no_id_are_broken() {
    need_loopback!();
    let mut mailbox = client(|session| async move {
        command(&session).await;
        said(&session, json!({ "type": "allocated", "nameplate": "7; drop" })).await;
        command(&session).await;
        said(&session, json!({ "type": "claimed", "mailbox": "x".repeat(129) })).await;
        park().await;
    })
    .await
    .unwrap();
    let Err(Failed::Broken(why)) = mailbox.allocate().await else { panic!() };
    assert!(why.contains("allocated no nameplate of digits"), "{why}");
    let Err(Failed::Broken(why)) = mailbox.claim("7").await else { panic!() };
    assert!(why.contains("named no mailbox for the nameplate"), "{why}");
}

#[tokio::test]
async fn a_third_side_is_refused_with_the_mailboxs_error() {
    need_loopback!();
    let mut mailbox = client(|session| async move {
        command(&session).await;
        said(&session, json!({ "type": "error", "error": "crowded", "orig": {} })).await;
        command(&session).await;
        said(&session, json!({ "type": "error", "error": "reclaimed", "orig": {} })).await;
        park().await;
    })
    .await
    .unwrap();
    let crowd = mailbox.claim("7").await.unwrap_err();
    assert!(crowded(&crowd), "{crowd:?}");
    let other = mailbox.claim("7").await.unwrap_err();
    assert!(matches!(other, Failed::Refused(_)) && !crowded(&other), "{other:?}");
}

#[tokio::test]
async fn a_flood_of_messages_while_a_command_waits_is_broken_not_kept() {
    need_loopback!();
    let mut mailbox = client(|session| async move {
        command(&session).await;
        for n in 0..300 {
            let message = json!({ "type": "message", "side": "peer", "phase": n.to_string(), "body": "00" });
            said(&session, message).await;
        }
        park().await;
    })
    .await
    .unwrap();
    let Err(Failed::Broken(why)) = mailbox.allocate().await else { panic!() };
    assert!(why.contains("sent more than 256 messages at once"), "{why}");
}

#[tokio::test]
async fn a_message_with_a_part_missing_is_broken() {
    need_loopback!();
    let mut mailbox = client(|session| async move {
        command(&session).await;
        said(&session, json!({ "type": "message", "side": "peer", "phase": "0" })).await;
        park().await;
    })
    .await
    .unwrap();
    let Err(Failed::Broken(why)) = mailbox.message(Duration::from_secs(5)).await else { panic!() };
    assert!(why.contains("without its side, phase or body"), "{why}");
}

#[tokio::test]
async fn a_mailbox_that_sends_more_than_it_was_asked_for_ends_the_exchange() {
    need_loopback!();
    let mut mailbox = client(|session| async move {
        for n in 0..300 {
            let message = json!({ "type": "message", "side": "peer", "phase": n.to_string(), "body": "00" });
            said(&session, message).await;
        }
        park().await;
    })
    .await
    .unwrap();
    // The flood is read as messages of the open mailbox, not as an answer.
    let code = Code::parse("7-crossover-clockwork-adroitness").unwrap();
    mailbox.shown = "ws://stand-in/v1".into();
    let mut hole = Wormhole::open(mailbox, "ab".into(), "7".into(), "mb7".into(), &code, json!({})).await.unwrap();
    let Err(Failed::Broken(why)) = hole.agree(Duration::from_secs(5)).await else { panic!() };
    assert!(why.contains("sent more than 256 messages that were not asked for"), "{why}");
}
