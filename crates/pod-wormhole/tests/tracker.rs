//! The meeting point at WebTorrent trackers (`pod_wormhole::tracker`) on
//! the stand-in tracker of `harness/tracker.rs`, which keeps no offer and
//! passes each one to another peer of the swarm, and on
//! webtorrent/bittorrent-tracker where `POD_WORMHOLE_TEST_TRACKER` names
//! it. Two ends pair through the swarm of a code and move a message each
//! way, each with a `peer_id` of its own for the pairing; the swarm of
//! another code is another one; a lost offer is offered again; the giver
//! announces again each interval; a stranger in the swarm after the taker,
//! with the taker's side, is not heard; a tracker that refuses or says nothing is passed over
//! with its words; no announce holds the words of the code.

mod harness;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness::tracker::{self as stand, Bittorrent, Script};
use harness::{LIMIT, config, nobody_there};
use pod_core::Event;
use pod_testkit::need_loopback;
use pod_wormhole::mailbox::Mood;
use pod_wormhole::mqtt;
use pod_wormhole::tracker::wire::Note;
use pod_wormhole::tracker::{self, SDP_MOST, Trackers};
use pod_wormhole::{Code, Config, Failed, Wormhole};
use pod_ws::url::WsUrl;
use serde_json::{Value, json};

type Seen = Arc<Mutex<Vec<Event>>>;

const RESEND: Duration = Duration::from_millis(100);

fn versions() -> Value {
    json!({ "pod-wormhole-test": 1 })
}

/// The trackers `points`, with the events of a run in `seen`, the taker
/// offering again each 100 ms.
fn ends(points: Vec<WsUrl>, seen: &Seen) -> Trackers {
    let kept = seen.clone();
    let events = Arc::new(move |event: Event| kept.lock().unwrap().push(event));
    Trackers { resend: RESEND, ..Trackers::new(points, Config { events, ..config() }) }
}

/// The taker of `code` at `ends`, its giver waiting at most `wait` for it
/// meanwhile, as the two run in two processes.
async fn meet(ends: &Trackers, giver: &mut Wormhole, code: &Code, wait: Duration) -> (Result<Wormhole, Failed>, bool) {
    let (taker, heard) = tokio::join!(tracker::joined(ends, code, versions(), wait), giver.agree(wait));
    (taker, heard.expect("the giver waits"))
}

/// The versions of both ends checked: each reads while the other does, as
/// the giver speaks only in an answer to an offer of the taker.
async fn confirm(giver: &mut Wormhole, taker: &mut Wormhole) {
    let (mine, theirs) = tokio::join!(giver.confirm(LIMIT), taker.confirm(LIMIT));
    assert_eq!((mine.unwrap(), theirs.unwrap()), (versions(), versions()));
}

/// A pairing at `url`: the versions checked and messages each way, the
/// giver's last word delivered at its close.
async fn pair(url: &WsUrl, seen: &Seen) -> Code {
    let ends = ends(vec![url.clone()], seen);
    let (mut giver, code) = tracker::opened(&ends, versions()).await.expect("a code");
    let (taker, heard) = meet(&ends, &mut giver, &code, LIMIT).await;
    let mut taker = taker.expect("the taker");
    assert!(heard, "the giver hears the taker's pake");
    confirm(&mut giver, &mut taker).await;
    assert_eq!((giver.shown(), taker.shown()), (url.shown().as_str(), url.shown().as_str()));
    giver.send(b"the offer").await.unwrap();
    giver.send(b"and its part").await.unwrap();
    let taker_side = async {
        assert_eq!(taker.receive(LIMIT).await.unwrap(), b"the offer");
        assert_eq!(taker.receive(LIMIT).await.unwrap(), b"and its part");
        taker.send(b"the answer").await.unwrap();
        taker.send(b"and more").await.unwrap();
        assert_eq!(taker.receive(LIMIT).await.unwrap(), b"the last word");
        taker.close(Mood::Happy).await;
    };
    let giver_side = async {
        assert_eq!(giver.receive(LIMIT).await.unwrap(), b"the answer");
        assert_eq!(giver.receive(LIMIT).await.unwrap(), b"and more");
        giver.send(b"the last word").await.unwrap();
        giver.close(Mood::Happy).await;
    };
    tokio::join!(taker_side, giver_side);
    code
}

/// The announces of `stand` once an end's `stopped` came, or at the limit.
/// An end writes `stopped` at its close and does not wait for the tracker
/// to read it, so the stand-in can read it after the close returns (T-970,
/// macos-15).
async fn until_stopped(stand: &stand::Tracker) -> Vec<stand::Announce> {
    let deadline = tokio::time::Instant::now() + LIMIT;
    loop {
        let announces = stand.announces();
        let stopped = announces.iter().any(|a| a.event.as_deref() == Some("stopped"));
        if stopped || tokio::time::Instant::now() >= deadline {
            return announces;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn two_ends_pair_through_the_swarm_of_a_code_and_move_messages_each_way() {
    need_loopback!();
    let stand = stand::start().await;
    let seen = Seen::default();
    let code = pair(&stand.url, &seen).await;
    let announces = until_stopped(&stand).await;
    let swarm = tracker::info_hash(&mqtt::topic(harness::APPID, code.nameplate()));
    let words = code.for_terminal().split_once('-').unwrap().1.to_string();
    for a in &announces {
        assert_eq!(a.info_hash, swarm);
        let parts = format!("{} {:?} {:?}", a.peer_id, a.offers, a.answer);
        assert!(words.split('-').all(|w| !parts.contains(w)), "an announce holds a word of the code: {parts}");
    }
    let first: HashSet<String> = announces.iter().map(|a| a.peer_id.clone()).collect();
    assert_eq!(first.len(), 2, "a peer_id for each end: {first:?}");
    assert!(announces.iter().any(|a| a.event.as_deref() == Some("stopped")), "the ends leave the swarm");
    // A second pairing draws two new peer_ids.
    let again = stand::start().await;
    pair(&again.url, &seen).await;
    let second: HashSet<String> = again.announces().iter().map(|a| a.peer_id.clone()).collect();
    assert_eq!(second.len(), 2);
    assert!(first.is_disjoint(&second), "{first:?} {second:?}");
    for event in seen.lock().unwrap().iter() {
        assert!(!event.text.contains(&words), "an event holds the code: {event:?}");
    }
}

#[tokio::test]
async fn two_ends_pair_at_webtorrents_own_tracker() {
    need_loopback!();
    let Some(bittorrent) = Bittorrent::named() else { return };
    pair(&bittorrent.url, &Seen::default()).await;
    println!("webtorrent/bittorrent-tracker: two ends paired");
}

#[tokio::test]
async fn the_swarm_of_another_code_is_another_one() {
    need_loopback!();
    let stand = stand::start().await;
    let ends = ends(vec![stand.url.clone()], &Seen::default());
    let (mut giver, code) = tracker::opened(&ends, versions()).await.expect("a code");
    let plate = if code.nameplate() == "1000" { "1001" } else { "1000" };
    let stranger = Code::make(plate, pod_wormhole::WORDS, &mut pod_core::entropy::OsEntropy).unwrap();
    let (met, heard) = meet(&ends, &mut giver, &stranger, Duration::from_secs(2)).await;
    assert_eq!(met.map(|_| ()), Err(Failed::Nobody), "no giver waits in the swarm of {stranger:?}");
    assert!(!heard, "the giver heard another code");
    // The giver still waits in its own swarm.
    let (taker, heard) = meet(&ends, &mut giver, &code, LIMIT).await;
    let mut taker = taker.expect("the taker");
    assert!(heard);
    confirm(&mut giver, &mut taker).await;
    giver.close(Mood::Lonely).await;
    taker.close(Mood::Lonely).await;
}

#[tokio::test]
async fn a_lost_offer_is_offered_again_until_the_giver_answers() {
    need_loopback!();
    let stand = stand::start().await;
    let ends = ends(vec![stand.url.clone()], &Seen::default());
    let (mut giver, code) = tracker::opened(&ends, versions()).await.expect("a code");
    // The taker's first three offers reach nobody.
    stand.lose(3);
    let (taker, heard) = meet(&ends, &mut giver, &code, LIMIT).await;
    let mut taker = taker.expect("the taker after its losses");
    assert!(heard);
    confirm(&mut giver, &mut taker).await;
    let pakes = stand.announces().iter().flat_map(|a| a.offers.clone()).filter(|sdp| sdp.contains("\"pake\"")).count();
    // Three lost pakes and the one that came.
    assert!(pakes >= 4, "the taker offered its pake {pakes} times");
    tokio::join!(giver.close(Mood::Happy), taker.close(Mood::Happy));
}

#[tokio::test]
async fn the_giver_announces_again_each_interval() {
    need_loopback!();
    let stand = stand::start_with(Script::Takes(1)).await;
    let ends = ends(vec![stand.url.clone()], &Seen::default());
    let (mut giver, _code) = tracker::opened(&ends, versions()).await.expect("a code");
    assert!(!giver.agree(Duration::from_millis(2500)).await.unwrap(), "no taker came");
    let announces = stand.announces();
    let first = announces.first().expect("the first announce").peer_id.clone();
    let giver_announces = announces.iter().filter(|a| a.peer_id == first && a.answer.is_none()).count();
    assert!(giver_announces >= 3, "the giver announced {giver_announces} times in 2.5 s at an interval of 1 s");
    giver.close(Mood::Lonely).await;
}

/// A peer in the swarm of `code` at `url` that says it is the taker of
/// `side` and offers the taker's next phases with bytes of its own.
async fn stranger(url: &WsUrl, code: &Code, side: &str) -> pod_ws::RelaySession<pod_ws::url::Stream> {
    let session = pod_ws::url::dial(url, &harness::way(), &pod_ws::Trust::Default, LIMIT, None).await.unwrap();
    let tag = mqtt::topic(harness::APPID, code.nameplate());
    let junk = vec![0x55; 64];
    let messages = vec![("version".into(), junk.clone()), ("0".into(), junk)];
    let note = Note { tag: tag.clone(), side: side.into(), from: 1, got: 0, messages };
    let offer = json!({
        "action": "announce", "info_hash": tracker::info_hash(&tag), "peer_id": "s".repeat(20), "numwant": 1,
        "offers": [{ "offer": { "type": "offer", "sdp": note.to_sdp() }, "offer_id": "o".repeat(20) }],
    });
    session.send_text(&offer.to_string()).await.unwrap();
    session
}

#[tokio::test]
async fn a_stranger_in_the_swarm_after_the_taker_is_not_heard() {
    need_loopback!();
    let stand = stand::start().await;
    let ends = ends(vec![stand.url.clone()], &Seen::default());
    let (mut giver, code) = tracker::opened(&ends, versions()).await.expect("a code");
    let (taker, heard) = meet(&ends, &mut giver, &code, LIMIT).await;
    let mut taker = taker.expect("the taker");
    assert!(heard);
    // A stranger who read the taker's side in an offer; the stand-in gives
    // its offer to the first peer: the giver.
    let first = stand.announces().into_iter().find_map(|a| a.offers.first().cloned()).expect("an offer");
    let side = pod_wormhole::tracker::wire::note_of(&first).expect("a note").side;
    let _stranger = stranger(&stand.url, &code, &side).await;
    confirm(&mut giver, &mut taker).await;
    taker.send(b"the answer").await.unwrap();
    assert_eq!(giver.receive(LIMIT).await.unwrap(), b"the answer");
    let answered: Vec<Option<String>> =
        stand.announces().into_iter().filter(|a| a.answer.is_some()).map(|a| a.to).collect();
    assert!(!answered.is_empty());
    assert!(!answered.contains(&Some("s".repeat(20))), "the giver answered the stranger: {answered:?}");
    tokio::join!(giver.close(Mood::Happy), taker.close(Mood::Happy));
}

#[tokio::test]
async fn a_tracker_that_refuses_or_says_nothing_is_passed_over_with_its_words() {
    need_loopback!();
    let refuses = stand::start_with(Script::Refuses("torrent not in allowlist".into())).await;
    let silent = stand::start_with(Script::Silent).await;
    let takes = stand::start().await;
    let urls = [&refuses, &silent, &takes].map(|t| t.url.clone()).to_vec();
    let seen = Seen::default();
    let mut ends = ends(urls, &seen);
    ends.config.answer_limit = Duration::from_secs(2);
    let (mut giver, code) = tracker::opened(&ends, versions()).await.expect("a code at the third tracker");
    let (taker, heard) = meet(&ends, &mut giver, &code, LIMIT).await;
    let mut taker = taker.expect("the taker");
    assert!(heard);
    confirm(&mut giver, &mut taker).await;
    assert_eq!(giver.shown(), takes.url.shown());
    let lines: Vec<String> = seen.lock().unwrap().iter().map(|e| format!("{} {}", e.kind, e.text)).collect();
    for words in ["refused: torrent not in allowlist", "did not answer the announce within 2s"] {
        let told = lines.iter().filter(|l| l.starts_with("wormhole.tracker ") && l.contains(words)).count();
        assert_eq!(told, 2, "each end said {words:?}: {lines:?}");
    }
    tokio::join!(giver.close(Mood::Happy), taker.close(Mood::Happy));
}

#[tokio::test]
async fn no_tracker_that_answers_is_unreachable() {
    need_loopback!();
    let ends = ends(vec![nobody_there()], &Seen::default());
    assert!(matches!(tracker::opened(&ends, versions()).await.map(|_| ()), Err(Failed::Unreachable(_))));
    let code = Code::make("1234", pod_wormhole::WORDS, &mut pod_core::entropy::OsEntropy).unwrap();
    let met = tracker::joined(&ends, &code, versions(), Duration::from_secs(1)).await.map(|_| ());
    assert!(matches!(met, Err(Failed::Unreachable(_))), "{met:?}");
}

/// Each public tracker of `pod-services` carries the largest `sdp` of a
/// meeting point.
#[test]
fn each_public_tracker_carries_the_largest_sdp() {
    let trackers: Vec<_> = pod_services::of_kind("webtorrent-tracker").collect();
    assert!(trackers.len() >= 2, "the catalog lists fewer than two trackers");
    for s in trackers {
        let most = s.int("sdp-bytes-most").unwrap_or_else(|| panic!("{}: no sdp-bytes-most", s.id()));
        assert!(SDP_MOST as i64 <= most, "{}: {SDP_MOST} > {most}", s.id());
    }
}
