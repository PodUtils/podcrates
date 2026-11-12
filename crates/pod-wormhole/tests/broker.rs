//! The meeting point over MQTT (`pod_wormhole::mqtt`) on each broker that
//! this host can run on the loopback (`harness/broker.rs`): the stand-in in
//! Rust, and Eclipse Mosquitto with a `protocol websockets` listener. Two
//! ends pair through the topic of a code and move a message each way; a
//! message on the topic of another code is not delivered; a stranger on
//! the topic fails the PAKE and spends the code; the taker sends its first
//! message again until the giver answers; a broker that refuses the login,
//! that is not there, or that breaks the protocol is passed over with its
//! reason; no event holds the code.

mod harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness::broker::{self, brokers};
use harness::stand_in::scripted;
use harness::{LIMIT, config, nobody_there};
use pod_core::Event;
use pod_testkit::need_loopback;
use pod_wormhole::mailbox::Mood;
use pod_wormhole::mqtt::packet::{self, Packet};
use pod_wormhole::mqtt::{self, Brokers, Login, Point};
use pod_wormhole::{Code, Config, Failed, Wormhole};
use pod_ws::url::WsUrl;
use serde_json::{Value, json};

type Seen = Arc<Mutex<Vec<Event>>>;

fn versions() -> Value {
    json!({ "pod-wormhole-test": 1 })
}

fn point(url: &WsUrl) -> Point {
    Point { url: url.clone(), login: None }
}

/// The brokers `points`, with the events of a run in `seen`, the taker
/// sending again each 100 ms.
fn ends(points: Vec<Point>, seen: &Seen) -> Brokers {
    let kept = seen.clone();
    let events = Arc::new(move |event: Event| kept.lock().unwrap().push(event));
    Brokers { resend: Duration::from_millis(100), ..Brokers::new(points, Config { events, ..config() }) }
}

/// A code of another nameplate than `code`'s, with its words.
fn other_nameplate(code: &Code) -> Code {
    let plate = if code.nameplate() == "1000" { "1001" } else { "1000" };
    Code::make(plate, &mut pod_core::entropy::OsEntropy).unwrap()
}

/// The code of `code`'s nameplate with its last word another of its list.
fn wrong_words(code: &Code) -> Code {
    let shown = code.for_terminal();
    let (head, last) = shown.rsplit_once('-').expect("a word");
    // The words alternate, odd first: the third is an odd one.
    let words = pod_wormhole::words::ODD;
    let other = if last == words[0] { words[1] } else { words[0] };
    Code::parse(&format!("{head}-{other}")).unwrap()
}

/// The taker of `code` at `ends`, its giver waiting at most `wait` for
/// it meanwhile, as the two run in two processes: a giver holds its
/// messages until it hears the taker.
async fn meet(ends: &Brokers, giver: &mut Wormhole, code: &Code, wait: Duration) -> (Result<Wormhole, Failed>, bool) {
    let (taker, heard) = tokio::join!(mqtt::joined(ends, code, versions(), wait), giver.agree(wait));
    (taker, heard.expect("the giver waits"))
}

/// A giver's wormhole and a taker's of one code at `url`, each with its key.
async fn two(url: &WsUrl, seen: &Seen) -> (Wormhole, Wormhole, Code) {
    let ends = ends(vec![point(url)], seen);
    let (mut giver, code) = mqtt::opened(&ends, versions()).await.expect("a code");
    let (taker, heard) = meet(&ends, &mut giver, &code, LIMIT).await;
    assert!(heard, "the giver hears the taker's pake");
    (giver, taker.expect("the taker"), code)
}

#[tokio::test]
async fn two_ends_pair_through_a_topic_and_move_a_message_each_way() {
    need_loopback!();
    for (name, url, keep) in brokers("pair").await {
        let seen = Seen::default();
        let (mut giver, mut taker, code) = two(&url, &seen).await;
        assert_eq!(giver.confirm(LIMIT).await.unwrap(), versions(), "{name}");
        assert_eq!(taker.confirm(LIMIT).await.unwrap(), versions(), "{name}");
        giver.send(b"the offer").await.unwrap();
        assert_eq!(taker.receive(LIMIT).await.unwrap(), b"the offer", "{name}");
        taker.send(b"the answer").await.unwrap();
        taker.send(b"and more").await.unwrap();
        assert_eq!(giver.receive(LIMIT).await.unwrap(), b"the answer", "{name}");
        assert_eq!(giver.receive(LIMIT).await.unwrap(), b"and more", "{name}");
        assert_eq!((giver.shown(), taker.shown()), (url.shown().as_str(), url.shown().as_str()), "{name}");
        giver.close(Mood::Happy).await;
        taker.close(Mood::Happy).await;
        assert_eq!(code.nameplate().len(), mqtt::NAMEPLATE_DIGITS, "{name}");
        let words = code.for_terminal().split_once('-').unwrap().1.to_string();
        for event in seen.lock().unwrap().iter() {
            assert!(!event.text.contains(&words), "{name}: an event holds the code: {event:?}");
        }
        eprintln!("{name}: two ends paired through the topic of a code");
        if let broker::Keep::Stand(stand) = &keep {
            assert!(stand.protocols().iter().all(|p| p == "mqtt"), "the subprotocol: {:?}", stand.protocols());
            let topic = mqtt::topic(harness::APPID, code.nameplate());
            assert!(stand.published().iter().all(|t| t.starts_with(&topic)), "{:?}", stand.published());
        }
    }
}

#[tokio::test]
async fn a_message_on_the_topic_of_another_code_is_not_delivered() {
    need_loopback!();
    for (name, url, _keep) in brokers("topic").await {
        let seen = Seen::default();
        let ends = ends(vec![point(&url)], &seen);
        let (mut giver, code) = mqtt::opened(&ends, versions()).await.expect("a code");
        let stranger = other_nameplate(&code);
        let (met, heard) = meet(&ends, &mut giver, &stranger, Duration::from_secs(2)).await;
        assert_eq!(met.map(|_| ()), Err(Failed::Nobody), "{name}: no giver waits at the topic of {stranger:?}");
        assert!(!heard, "{name}: the giver heard another code");
        // The giver still waits at its own topic.
        let (taker, heard) = meet(&ends, &mut giver, &code, LIMIT).await;
        let mut taker = taker.expect("the taker");
        assert!(heard, "{name}");
        assert_eq!(taker.confirm(LIMIT).await.unwrap(), versions(), "{name}");
        giver.close(Mood::Happy).await;
        taker.close(Mood::Happy).await;
    }
}

#[tokio::test]
async fn a_stranger_on_the_topic_fails_the_pake_and_spends_the_code() {
    need_loopback!();
    for (name, url, _keep) in brokers("stranger").await {
        let seen = Seen::default();
        let ends = ends(vec![point(&url)], &seen);
        let (mut giver, code) = mqtt::opened(&ends, versions()).await.expect("a code");
        let (stranger, heard) = meet(&ends, &mut giver, &wrong_words(&code), LIMIT).await;
        let mut stranger = stranger.expect("the stranger at the topic");
        assert!(heard, "{name}");
        assert_eq!(giver.confirm(LIMIT).await, Err(Failed::WrongCode), "{name}: the giver");
        assert_eq!(stranger.confirm(LIMIT).await, Err(Failed::WrongCode), "{name}: the stranger");
        // The giver closed: the right code finds nobody now.
        let late = mqtt::joined(&ends, &code, versions(), Duration::from_secs(1)).await.map(|_| ());
        assert_eq!(late, Err(Failed::Nobody), "{name}: the code is spent");
    }
}

#[tokio::test]
async fn the_taker_sends_its_first_message_again_until_the_giver_answers() {
    need_loopback!();
    let stand = broker::start().await;
    let seen = Seen::default();
    let ends = ends(vec![point(&stand.url)], &seen);
    let (mut giver, code) = mqtt::opened(&ends, versions()).await.expect("a code");
    // The taker's first three messages reach no subscriber.
    stand.lose(3);
    let (taker, heard) = meet(&ends, &mut giver, &code, LIMIT).await;
    let mut taker = taker.expect("the taker after its losses");
    assert!(heard);
    assert_eq!(giver.confirm(LIMIT).await.unwrap(), versions());
    assert_eq!(taker.confirm(LIMIT).await.unwrap(), versions());
    let to_giver = format!("{}{}", mqtt::topic(harness::APPID, code.nameplate()), mqtt::TO_GIVER);
    let sent = stand.published().iter().filter(|t| **t == to_giver).count();
    // Three lost pakes, the one that came, the version.
    assert!(sent >= 5, "the taker sent {sent} messages to the giver");
    giver.close(Mood::Happy).await;
    taker.close(Mood::Happy).await;
}

#[tokio::test]
async fn a_broker_that_refuses_the_login_is_passed_over_with_its_reason() {
    need_loopback!();
    let stand = broker::start_with(Some(("public", "public"))).await;
    let login = Login { user: "public".into(), password: "public".into() };
    let wrong = Login { user: "public".into(), password: "not-it".into() };
    let points = vec![
        Point { url: stand.url.clone(), login: Some(wrong) },
        Point { url: stand.url.clone(), login: Some(login) },
    ];
    let seen = Seen::default();
    let ends = ends(points, &seen);
    let (mut giver, code) = mqtt::opened(&ends, versions()).await.expect("a code at the second point");
    let (taker, heard) = meet(&ends, &mut giver, &code, LIMIT).await;
    let mut taker = taker.expect("the taker");
    assert!(heard);
    assert_eq!(taker.confirm(LIMIT).await.unwrap(), versions());
    let lines: Vec<String> = seen.lock().unwrap().iter().map(|e| format!("{} {}", e.kind, e.text)).collect();
    assert_eq!(lines.len(), 2, "each end said the broker that refused it: {lines:?}");
    for line in &lines {
        assert!(line.starts_with("wormhole.broker ") && line.contains("a bad user name or password"), "{line}");
        assert!(!line.contains("not-it"), "a line holds the password: {line}");
    }
    giver.close(Mood::Happy).await;
    taker.close(Mood::Happy).await;
}

#[tokio::test]
async fn no_broker_that_answers_is_unreachable() {
    need_loopback!();
    let ends = ends(vec![point(&nobody_there())], &Seen::default());
    assert!(matches!(mqtt::opened(&ends, versions()).await.map(|_| ()), Err(Failed::Unreachable(_))));
    let code = Code::make("1234", &mut pod_core::entropy::OsEntropy).unwrap();
    let met = mqtt::joined(&ends, &code, versions(), Duration::from_secs(1)).await.map(|_| ());
    assert!(matches!(met, Err(Failed::Unreachable(_))), "{met:?}");
}

/// Each public broker of `pod-services` takes the topic, the client id
/// and the largest message of a meeting point, where it states a bound.
#[test]
fn each_public_broker_takes_the_topic_and_the_payload_of_a_meeting_point() {
    let (inbox, _) = mqtt::Role::Giver.topics(&mqtt::topic(harness::APPID, "1234"));
    let client_id = mqtt::broker::client_id().unwrap();
    let brokers: Vec<_> = pod_services::of_kind("mqtt-broker").collect();
    assert!(!brokers.is_empty(), "the catalog lists no broker");
    for s in brokers {
        let most = |fact| s.int(fact).map_or(usize::MAX, |n| usize::try_from(n).unwrap());
        assert!(inbox.len() <= most("topic-chars-most"), "{}", s.id());
        assert!(client_id.len() <= most("client-id-chars-most"), "{}", s.id());
        assert!(mqtt::PAYLOAD_MOST <= most("payload-bytes-most"), "{}", s.id());
        assert!(mqtt::KEEP_ALIVE.as_secs() as usize <= most("keep-alive-seconds-most"), "{}", s.id());
    }
}

/// The failure of a giver at a broker that runs `script` after it read the
/// `CONNECT`.
async fn giver_at(answer: Vec<u8>, opcode: u8) -> String {
    let (url, task) = scripted(move |session| async move {
        let _ = session.read_frame().await;
        let _ = match opcode {
            pod_ws::frame::OPCODE_TEXT => session.send_text(&String::from_utf8_lossy(&answer)).await,
            _ => session.send_binary(&answer).await,
        };
        let _ = session.read_frame().await;
    })
    .await;
    let ends = ends(vec![point(&url)], &Seen::default());
    let failed = mqtt::opened(&ends, versions()).await.map(|_| ()).expect_err("a broker that goes wrong");
    task.abort();
    let Failed::Unreachable(why) = failed else { panic!("{failed:?}") };
    why
}

#[tokio::test]
async fn a_broker_that_breaks_the_protocol_is_passed_over_with_its_reason() {
    need_loopback!();
    let binary = pod_ws::frame::OPCODE_BINARY;
    let malformed = giver_at(vec![0x20, 0xff, 0xff, 0xff, 0xff, 0x01], binary).await;
    assert!(malformed.contains("malformed packet") && malformed.contains("more than four bytes"), "{malformed}");
    let text = giver_at(b"hello".to_vec(), pod_ws::frame::OPCODE_TEXT).await;
    assert!(text.contains("MQTT goes in binary messages"), "{text}");
    let refused = giver_at(packet::encode(&Packet::ConnAck { present: false, code: 5 }).unwrap(), binary).await;
    assert!(refused.contains("refused this client: not authorized"), "{refused}");
    let early = giver_at(packet::encode(&Packet::PingReq).unwrap(), binary).await;
    assert!(early.contains("a PINGREQ that this client did not ask for"), "{early}");
}
