//! The meeting point over Nostr (`pod_wormhole::nostr`) on the stand-in
//! relay of `harness/nostr.rs`, which keeps no event and passes each event
//! to the subscriptions of the moment that hold it. Two ends pair through
//! the tag of a code and move a message each way, each with a key of its
//! own for the pairing; an event with the tag of another code is not
//! delivered; the taker sends its first message again until the giver
//! answers, each time as a new event; the last word of an end that closes
//! at once reaches the other end (T-634); a relay that closes the `REQ`,
//! refuses the events, or says a `NOTICE` and nothing more is passed over
//! with its words; an event that does not check is dropped; no event holds
//! the words of the code.

mod harness;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness::nostr::{self as stand, Script};
use harness::stand_in::scripted;
use harness::{LIMIT, config, nobody_there};
use pod_core::Event;
use pod_testkit::need_loopback;
use pod_wormhole::mailbox::Mood;
use pod_wormhole::mqtt::{self, Role};
use pod_wormhole::nostr::event::{self, Key};
use pod_wormhole::nostr::{self, KIND, Relay, Relays, TAG};
use pod_wormhole::{Code, Config, Failed, Wormhole};
use pod_ws::url::WsUrl;
use serde_json::{Value, json};

type Seen = Arc<Mutex<Vec<Event>>>;

const RESEND: Duration = Duration::from_millis(100);

fn versions() -> Value {
    json!({ "pod-wormhole-test": 1 })
}

/// The relays `points`, with the events of a run in `seen`, the taker
/// sending again each 100 ms.
fn ends(points: Vec<WsUrl>, seen: &Seen) -> Relays {
    let kept = seen.clone();
    let events = Arc::new(move |event: Event| kept.lock().unwrap().push(event));
    Relays { resend: RESEND, ..Relays::new(points, Config { events, ..config() }) }
}

/// The taker of `code` at `ends`, its giver waiting at most `wait` for it
/// meanwhile, as the two run in two processes.
async fn meet(ends: &Relays, giver: &mut Wormhole, code: &Code, wait: Duration) -> (Result<Wormhole, Failed>, bool) {
    let (taker, heard) = tokio::join!(nostr::joined(ends, code, versions(), wait), giver.agree(wait));
    (taker, heard.expect("the giver waits"))
}

/// A pairing at `relay`: the versions checked and a message each way.
async fn pair(relay: &stand::Relay, seen: &Seen) -> Code {
    let ends = ends(vec![relay.url.clone()], seen);
    let (mut giver, code) = nostr::opened(&ends, versions()).await.expect("a code");
    let (taker, heard) = meet(&ends, &mut giver, &code, LIMIT).await;
    let mut taker = taker.expect("the taker");
    assert!(heard, "the giver hears the taker's pake");
    assert_eq!(giver.confirm(LIMIT).await.unwrap(), versions());
    assert_eq!(taker.confirm(LIMIT).await.unwrap(), versions());
    giver.send(b"the offer").await.unwrap();
    assert_eq!(taker.receive(LIMIT).await.unwrap(), b"the offer");
    taker.send(b"the answer").await.unwrap();
    taker.send(b"and more").await.unwrap();
    assert_eq!(giver.receive(LIMIT).await.unwrap(), b"the answer");
    assert_eq!(giver.receive(LIMIT).await.unwrap(), b"and more");
    assert_eq!((giver.shown(), taker.shown()), (relay.url.shown().as_str(), relay.url.shown().as_str()));
    giver.close(Mood::Happy).await;
    taker.close(Mood::Happy).await;
    code
}

#[tokio::test]
async fn two_ends_pair_through_the_tag_of_a_code_and_move_a_message_each_way() {
    need_loopback!();
    let relay = stand::start().await;
    let seen = Seen::default();
    let code = pair(&relay, &seen).await;
    let events = relay.events();
    let tag = mqtt::topic(harness::APPID, code.nameplate());
    let words = code.for_terminal().split_once('-').unwrap().1.to_string();
    for e in &events {
        assert_eq!(e.kind, KIND);
        assert!(e.tag(TAG).is_some_and(|t| t.starts_with(&tag)), "{e:?}");
        let parts = format!("{:?} {}", e.tags, e.content);
        assert!(words.split('-').all(|w| !parts.contains(w)), "an event holds a word of the code: {parts}");
    }
    let first: HashSet<String> = events.iter().map(|e| e.pubkey.clone()).collect();
    assert_eq!(first.len(), 2, "a key for each end: {first:?}");
    // A second pairing draws two new keys: no key links two pairings.
    let again = stand::start().await;
    pair(&again, &seen).await;
    let second: HashSet<String> = again.events().iter().map(|e| e.pubkey.clone()).collect();
    assert_eq!(second.len(), 2);
    assert!(first.is_disjoint(&second), "{first:?} {second:?}");
    for event in seen.lock().unwrap().iter() {
        assert!(!event.text.contains(&words), "an event holds the code: {event:?}");
    }
}

#[tokio::test]
async fn an_event_with_the_tag_of_another_code_is_not_delivered() {
    need_loopback!();
    let relay = stand::start().await;
    let ends = ends(vec![relay.url.clone()], &Seen::default());
    let (mut giver, code) = nostr::opened(&ends, versions()).await.expect("a code");
    let plate = if code.nameplate() == "1000" { "1001" } else { "1000" };
    let stranger = Code::make(plate, pod_wormhole::WORDS, &mut pod_core::entropy::OsEntropy).unwrap();
    let (met, heard) = meet(&ends, &mut giver, &stranger, Duration::from_secs(2)).await;
    assert_eq!(met.map(|_| ()), Err(Failed::Nobody), "no giver waits at the tag of {stranger:?}");
    assert!(!heard, "the giver heard another code");
    // The giver still waits at its own tag.
    let (taker, heard) = meet(&ends, &mut giver, &code, LIMIT).await;
    let mut taker = taker.expect("the taker");
    assert!(heard);
    assert_eq!(taker.confirm(LIMIT).await.unwrap(), versions());
    giver.close(Mood::Happy).await;
    taker.close(Mood::Happy).await;
}

/// The giver says its last word and closes at once, as the run of a code
/// does; the stand-in, as a public relay, drops an event of a connection
/// that ended before it took the event.
#[tokio::test]
async fn the_last_word_of_an_end_that_closes_at_once_reaches_the_other_end() {
    need_loopback!();
    let relay = stand::start().await;
    let ends = ends(vec![relay.url.clone()], &Seen::default());
    let (mut giver, code) = nostr::opened(&ends, versions()).await.expect("a code");
    let (taker, heard) = meet(&ends, &mut giver, &code, LIMIT).await;
    let mut taker = taker.expect("the taker");
    assert!(heard);
    assert_eq!(giver.confirm(LIMIT).await.unwrap(), versions());
    assert_eq!(taker.confirm(LIMIT).await.unwrap(), versions());
    giver.send(b"the last word").await.unwrap();
    giver.close(Mood::Happy).await;
    let heard = taker.receive(stand::LAG * 50).await;
    assert_eq!(heard.as_deref(), Ok(&b"the last word"[..]), "the relay took the last word before the close");
    taker.close(Mood::Happy).await;
}

#[tokio::test]
async fn the_taker_sends_its_first_message_again_until_the_giver_answers() {
    need_loopback!();
    let relay = stand::start().await;
    let ends = ends(vec![relay.url.clone()], &Seen::default());
    let (mut giver, code) = nostr::opened(&ends, versions()).await.expect("a code");
    // The taker's first three messages reach no subscriber.
    relay.lose(3);
    let (taker, heard) = meet(&ends, &mut giver, &code, LIMIT).await;
    let mut taker = taker.expect("the taker after its losses");
    assert!(heard);
    assert_eq!(giver.confirm(LIMIT).await.unwrap(), versions());
    assert_eq!(taker.confirm(LIMIT).await.unwrap(), versions());
    let to_giver = format!("{}{}", mqtt::topic(harness::APPID, code.nameplate()), mqtt::TO_GIVER);
    let sent: Vec<_> =
        relay.events().into_iter().filter(|e| e.tag(TAG) == Some(&to_giver) && !e.content.is_empty()).collect();
    // Three lost pakes, the one that came, the version: each a new event.
    assert!(sent.len() >= 5, "the taker sent {} messages to the giver", sent.len());
    let ids: HashSet<&str> = sent.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids.len(), sent.len(), "each message again is a new event");
    giver.close(Mood::Happy).await;
    taker.close(Mood::Happy).await;
}

#[tokio::test]
async fn a_relay_that_refuses_is_passed_over_with_its_words() {
    need_loopback!();
    let closes = stand::start_with(Script::ClosesEachReq("auth-required: members only".into())).await;
    let refuses = stand::start_with(Script::RefusesEachEvent("pow: difficulty 20 is needed".into())).await;
    let notices = stand::start_with(Script::NoticeOnly("restricted: no subscriptions here".into())).await;
    let takes = stand::start().await;
    let urls = [&closes, &refuses, &notices, &takes].map(|r| r.url.clone()).to_vec();
    let seen = Seen::default();
    let mut ends = ends(urls, &seen);
    ends.config.answer_limit = Duration::from_secs(2);
    let (mut giver, code) = nostr::opened(&ends, versions()).await.expect("a code at the fourth relay");
    let (taker, heard) = meet(&ends, &mut giver, &code, LIMIT).await;
    let mut taker = taker.expect("the taker");
    assert!(heard);
    assert_eq!(taker.confirm(LIMIT).await.unwrap(), versions());
    assert_eq!(giver.shown(), takes.url.shown());
    let lines: Vec<String> = seen.lock().unwrap().iter().map(|e| format!("{} {}", e.kind, e.text)).collect();
    for words in [
        "ended the subscription: auth-required: members only",
        "refused an event: pow: difficulty 20 is needed",
        "did not send EOSE within 2s; its last notice: restricted: no subscriptions here",
    ] {
        let told = lines.iter().filter(|l| l.starts_with("wormhole.nostr ") && l.contains(words)).count();
        assert_eq!(told, 2, "each end said {words:?}: {lines:?}");
    }
    giver.close(Mood::Happy).await;
    taker.close(Mood::Happy).await;
}

#[tokio::test]
async fn no_relay_that_answers_is_unreachable() {
    need_loopback!();
    let ends = ends(vec![nobody_there()], &Seen::default());
    assert!(matches!(nostr::opened(&ends, versions()).await.map(|_| ()), Err(Failed::Unreachable(_))));
    let code = Code::make("1234", pod_wormhole::WORDS, &mut pod_core::entropy::OsEntropy).unwrap();
    let met = nostr::joined(&ends, &code, versions(), Duration::from_secs(1)).await.map(|_| ());
    assert!(matches!(met, Err(Failed::Unreachable(_))), "{met:?}");
}

/// An event that does not check: the content of `message`, the id of that
/// content, and the signature of another event.
fn forged(tag: &str, message: &str) -> Value {
    let key = Key::new().unwrap();
    let other = key.sign(1, KIND, &json!([[TAG, tag]]), "another").unwrap();
    let mut event = key.sign(1, KIND, &json!([[TAG, tag]]), message).unwrap();
    event["sig"] = other["sig"].clone();
    event
}

/// The stand-in refuses an event that does not check, and an end drops
/// one that a relay passes on: only the signed message comes.
#[tokio::test]
async fn an_event_that_does_not_check_is_refused_and_dropped() {
    need_loopback!();
    let relay = stand::start().await;
    let session = pod_ws::url::dial(&relay.url, &harness::way(), &pod_ws::Trust::Default, LIMIT, None).await.unwrap();
    session.send_text(&json!(["EVENT", forged("pwtestg", "x")]).to_string()).await.unwrap();
    let answer = session.read_frame().await.unwrap();
    let said = event::said(&answer.payload);
    assert!(
        matches!(&said, Some(event::Said::Ok { accepted: false, message, .. }) if message.starts_with("invalid:")),
        "{said:?}"
    );

    let forged_message = json!({ "side": "abcdefabcd", "phase": "forged", "body": "00" }).to_string();
    let signed_message = json!({ "side": "abcdefabcd", "phase": "0", "body": "01" }).to_string();
    let (url, _task) = scripted(move |session| async move {
        let req: Value = serde_json::from_slice(&session.read_frame().await.unwrap().payload).unwrap();
        let sub = req[1].clone();
        let _ = session.send_text(&json!(["EOSE", sub]).to_string()).await;
        let probe: Value = serde_json::from_slice(&session.read_frame().await.unwrap().payload).unwrap();
        let _ = session.send_text(&json!(["OK", probe[1]["id"], true, ""]).to_string()).await;
        let _ = session.send_text(&json!(["EVENT", sub, probe[1]]).to_string()).await;
        let signed = Key::new().unwrap().sign(1, KIND, &json!([[TAG, "pwtestg"]]), &signed_message).unwrap();
        for event in [forged("pwtestg", &forged_message), signed] {
            let _ = session.send_text(&json!(["EVENT", sub, event]).to_string()).await;
        }
        while session.read_frame().await.is_ok_and(|f| f.opcode != pod_ws::frame::OPCODE_CLOSE) {}
    })
    .await;
    let mut giver = Relay::connect(&url, &config(), "0123456789", "pwtest", Role::Giver, RESEND).await.unwrap();
    let message = giver.message(LIMIT).await.unwrap().expect("the signed message");
    assert_eq!((message.side.as_str(), message.phase.as_str()), ("abcdefabcd", "0"), "the forged one is dropped");
    giver.end().await;
}

/// Each public relay of `pod-services` takes the largest event of a
/// meeting point.
#[test]
fn each_public_relay_takes_the_largest_event() {
    let relays: Vec<_> = pod_services::of_kind("nostr-relay").collect();
    assert!(relays.len() >= 2, "the catalog lists fewer than two relays");
    let key = Key::new().unwrap();
    let (inbox, _) = Role::Giver.topics(&mqtt::topic(harness::APPID, "1234"));
    // A message of the mailbox of CONTENT_MOST bytes, its body in hex.
    let empty = json!({ "side": "0123456789", "phase": "4294967295", "body": "" }).to_string();
    let body = "ab".repeat((nostr::CONTENT_MOST - empty.len()) / 2);
    let content = json!({ "side": "0123456789", "phase": "4294967295", "body": body }).to_string();
    assert!(content.len() <= nostr::CONTENT_MOST && content.len() + 2 > nostr::CONTENT_MOST);
    let largest = key.sign(u64::from(u32::MAX), KIND, &json!([[TAG, inbox]]), &content).unwrap();
    let message = json!(["EVENT", largest]).to_string().len();
    // strfry's maxEventSize: 65,536 bytes of JSON for each relay of strfry.
    assert!(message <= 65_536, "{message}");
    for s in relays {
        let most = s.int("message-bytes-most").unwrap_or_else(|| panic!("{}: no message-bytes-most", s.id()));
        assert!(message as i64 <= most, "{}: {message} > {most}", s.id());
    }
}

/// A relay that accepts the events of one end and passes none to the
/// other (T-963): the quiet end names what it sent and what came, so a
/// drop is told from an other end that sent nothing. The giver sent its
/// part of the key and its versions, and heard those of the taker, the
/// part of the key once or more.
#[tokio::test]
async fn an_end_that_hears_nothing_names_what_the_relay_passed() {
    need_loopback!();
    let relay = stand::start().await;
    let seen = Seen::default();
    let ends = ends(vec![relay.url.clone()], &seen);
    let (mut giver, code) = nostr::opened(&ends, versions()).await.expect("a code");
    let (taker, heard) = meet(&ends, &mut giver, &code, LIMIT).await;
    let mut taker = taker.expect("the taker");
    assert!(heard);
    assert_eq!(giver.confirm(LIMIT).await.unwrap(), versions());
    assert_eq!(taker.confirm(LIMIT).await.unwrap(), versions());
    relay.lose(1);
    taker.send(b"lost").await.unwrap();
    let Err(Failed::Peer(why)) = giver.receive(Duration::from_millis(500)).await else { panic!("the giver heard") };
    let at = relay.url.shown();
    assert!(why.starts_with("the other end said nothing for 0 s (at the Nostr relay "), "{why}");
    assert!(why.contains(&format!("{at} this end sent 2 events and the relay accepted 2;")), "{why}");
    // The taker sends its first message again until the giver answers, so
    // the giver can get the taker's part of the key more than once; each
    // event passed is still a message of the other end.
    let tail = why.split_once("; it passed ").map(|(_, t)| t).unwrap_or_default();
    let (passed, rest) = tail.split_once(" events of the other key, ").expect("the passed events");
    let taken = rest.strip_suffix(" of them messages of the other end)").expect("the messages");
    let (passed, taken): (u32, u32) = (passed.parse().unwrap(), taken.parse().unwrap());
    assert!(passed >= 2 && taken == passed, "{why}");
    taker.close(Mood::Happy).await;
}
