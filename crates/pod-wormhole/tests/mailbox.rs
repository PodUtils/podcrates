//! The wormhole over each mailbox that this host can run on the loopback
//! (`harness`): the stand-in in Rust, and magic-wormhole's own mailbox
//! server where Python has it. Two ends of one code agree a key and move a
//! message each way; a wrong code ends both ends and spends the code; a
//! second use, a giver nobody takes, a code of another application and a
//! mailbox that lists other codes find nobody; a third end is refused while
//! two hold the code; a message that the mailbox changed ends the exchange;
//! no event and no `Debug` holds the code. The two ends derive one key for
//! a purpose, and another for another purpose. A code of the count of
//! words that the giver asks for meets; a count out of bounds reaches no
//! mailbox. A peer of magic-wormhole's
//! own Python library and this crate finish the `pake` and `version`
//! phases, derive one transit key, and move a message each way, either one
//! giving the code, and a wrong code fails both.

mod harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness::{APPID, Keep, LIMIT, Peer, config, config_for, mailboxes, nobody_there};
use pod_core::{Event, Level};
use pod_testkit::need_loopback;
use pod_wormhole::mailbox::Mood;
use pod_wormhole::{Code, Config, Ends, Failed, Wormhole, joined, opened, words};
use pod_ws::url::WsUrl;
use serde_json::{Value, json};

type Seen = Arc<Mutex<Vec<Event>>>;

fn versions() -> Value {
    json!({ "pod-wormhole-test": 1 })
}

/// The ends at `mailboxes`, with the events of a run in `seen`.
fn ends(mailboxes: Vec<WsUrl>, seen: &Seen) -> Ends {
    let kept = seen.clone();
    let events = Arc::new(move |event: Event| kept.lock().unwrap().push(event));
    Ends { mailboxes, config: Config { events, ..config() } }
}

/// The code with its last word another of its list.
fn wrong(shown: &str) -> String {
    let (head, last) = shown.rsplit_once('-').expect("a word");
    let other = if last == words::ODD[0] { words::ODD[1] } else { words::ODD[0] };
    format!("{head}-{other}")
}

fn hex(bytes: &[u8]) -> String {
    pod_wormhole::hex(bytes)
}

/// The key of the transit of magic-wormhole's file transfer, in hex.
fn transit_key(hole: &Wormhole) -> String {
    hex(&hole.derive_key(&format!("{APPID}/transit-key"), 32).expect("a key"))
}

/// A giver's wormhole and a taker's of one code at `url`, each with its key.
async fn two(url: &WsUrl, seen: &Seen) -> (Wormhole, Wormhole, Code) {
    let ends = ends(vec![url.clone()], seen);
    let (mut giver, code) = opened(&ends, versions()).await.expect("a code");
    let no_key = giver.derive_key("a purpose", 32).err();
    assert_eq!(no_key, Some(Failed::Broken("no key was agreed".into())), "no derived key before the key");
    let taker = joined(&ends, &code, versions(), Duration::from_secs(5)).await.expect("the taker");
    assert!(giver.agree(LIMIT).await.unwrap(), "the giver hears the taker's pake");
    (giver, taker, code)
}

#[tokio::test]
async fn two_ends_of_one_code_agree_a_key_and_move_a_message_each_way() {
    need_loopback!();
    for (name, url, _keep) in mailboxes("agree").await {
        let (mut giver, mut taker, _) = two(&url, &Seen::default()).await;
        assert_eq!(giver.confirm(LIMIT).await.unwrap(), versions(), "{name}");
        assert_eq!(taker.confirm(LIMIT).await.unwrap(), versions(), "{name}");
        assert_eq!(transit_key(&giver), transit_key(&taker), "{name}: one derived key at both ends");
        let other = giver.derive_key("another purpose", 32).unwrap();
        assert_ne!(hex(&other), transit_key(&giver), "{name}: another purpose, another key");
        assert_eq!(taker.derive_key("another purpose", 10).unwrap()[..], other[..10], "{name}");
        giver.send(b"the offer").await.unwrap();
        assert_eq!(taker.receive(LIMIT).await.unwrap(), b"the offer", "{name}");
        taker.send(b"the answer").await.unwrap();
        taker.send(b"and more").await.unwrap();
        assert_eq!(giver.receive(LIMIT).await.unwrap(), b"the answer", "{name}");
        assert_eq!(giver.receive(LIMIT).await.unwrap(), b"and more", "{name}");
        assert_eq!(giver.shown(), url.shown(), "{name}");
        giver.close(Mood::Happy).await;
        taker.close(Mood::Happy).await;
    }
}

#[tokio::test]
async fn a_wrong_code_ends_both_ends_and_spends_the_code() {
    need_loopback!();
    for (name, url, keep) in mailboxes("wrong").await {
        let seen = Seen::default();
        let ends = ends(vec![url.clone()], &seen);
        let (mut giver, code) = opened(&ends, versions()).await.expect("a code");
        let typed = Code::parse(&wrong(code.for_terminal())).expect("a code");
        let mut taker = joined(&ends, &typed, versions(), Duration::from_secs(5)).await.expect("the taker");
        assert!(giver.agree(LIMIT).await.unwrap(), "{name}");
        assert_eq!(giver.confirm(LIMIT).await.err(), Some(Failed::WrongCode), "{name}: the giver");
        assert_eq!(taker.confirm(LIMIT).await.err(), Some(Failed::WrongCode), "{name}: the taker");
        if let Keep::Stand(stand) = &keep {
            let scary = stand.moods().iter().filter(|(_, mood)| mood == "scary").count();
            assert_eq!(scary, 2, "{name}: each end told the mailbox it was scared");
        }
        let again = joined(&ends, &code, versions(), Duration::from_secs(2)).await;
        assert_eq!(again.err(), Some(Failed::Nobody), "{name}: the right code after a wrong one");
    }
}

#[tokio::test]
async fn a_second_use_of_a_code_finds_nobody() {
    need_loopback!();
    for (name, url, _keep) in mailboxes("second").await {
        let seen = Seen::default();
        let (mut giver, mut taker, code) = two(&url, &seen).await;
        giver.confirm(LIMIT).await.unwrap();
        taker.confirm(LIMIT).await.unwrap();
        giver.close(Mood::Happy).await;
        taker.close(Mood::Happy).await;
        let again = joined(&ends(vec![url.clone()], &seen), &code, versions(), Duration::from_secs(2)).await;
        assert_eq!(again.err(), Some(Failed::Nobody), "{name}: the second use");
    }
}

#[tokio::test]
async fn a_giver_that_nobody_takes_gives_up_lonely_and_its_code_finds_nobody() {
    need_loopback!();
    for (name, url, keep) in mailboxes("lonely").await {
        let seen = Seen::default();
        let ends = ends(vec![url.clone()], &seen);
        let (mut giver, code) = opened(&ends, versions()).await.expect("a code");
        assert!(!giver.agree(Duration::from_millis(300)).await.unwrap(), "{name}: nobody came");
        giver.close(Mood::Lonely).await;
        giver.close(Mood::Lonely).await;
        if let Keep::Stand(stand) = &keep {
            assert_eq!(stand.moods(), [(stand.moods()[0].0.clone(), "lonely".to_string())], "{name}: one close");
            assert!(stand.nameplates(APPID).is_empty(), "{name}: the nameplate is free");
        }
        let late = joined(&ends, &code, versions(), Duration::from_secs(2)).await;
        assert_eq!(late.err(), Some(Failed::Nobody), "{name}: a code past its life");
    }
}

#[tokio::test]
async fn an_end_of_another_application_never_meets_the_code() {
    need_loopback!();
    for (name, url, _keep) in mailboxes("appid").await {
        let seen = Seen::default();
        let (mut giver, code) = opened(&ends(vec![url.clone()], &seen), versions()).await.expect("a code");
        let other = Ends { mailboxes: vec![url.clone()], config: config_for("example.org/another/app") };
        let met = joined(&other, &code, versions(), Duration::from_secs(2)).await;
        assert_eq!(met.err(), Some(Failed::Nobody), "{name}");
        giver.close(Mood::Lonely).await;
    }
}

#[tokio::test]
async fn a_code_of_the_words_asked_for_meets_and_a_count_out_of_bounds_reaches_no_mailbox() {
    need_loopback!();
    for (name, url, _keep) in mailboxes("words").await {
        let mut ends = ends(vec![url.clone()], &Seen::default());
        ends.config.words = 5;
        let (mut giver, code) = opened(&ends, versions()).await.expect("a code");
        assert_eq!(code.words(), 5, "{name}");
        let typed = Code::parse(code.for_terminal()).expect("the taker types it");
        let mut taker = joined(&ends, &typed, versions(), Duration::from_secs(5)).await.expect("the taker");
        assert!(giver.agree(LIMIT).await.unwrap(), "{name}");
        assert_eq!(giver.confirm(LIMIT).await.unwrap(), versions(), "{name}");
        assert_eq!(taker.confirm(LIMIT).await.unwrap(), versions(), "{name}");
        giver.close(Mood::Happy).await;
        taker.close(Mood::Happy).await;
    }
    for words in [1, 9] {
        let mut ends = ends(vec![nobody_there()], &Seen::default());
        ends.config.words = words;
        let refused = opened(&ends, versions()).await.err();
        let why = format!("a new code has 2 to 8 words, not {words}");
        assert_eq!(refused, Some(Failed::Mine(why)), "before a dial");
    }
}

#[tokio::test]
async fn a_mailbox_that_lists_other_codes_has_no_giver_and_is_not_claimed() {
    need_loopback!();
    let stand = harness::stand_in::start().await;
    let seen = Seen::default();
    let ends = ends(vec![stand.url.clone()], &seen);
    let (mut giver, code) = opened(&ends, versions()).await.expect("a code");
    let other = Code::parse("99-crossover-clockwork-adroitness").unwrap();
    assert_ne!(other.nameplate(), code.nameplate());
    let before = stand.nameplates(APPID);
    let met = joined(&ends, &other, versions(), Duration::from_secs(2)).await;
    assert_eq!(met.err(), Some(Failed::Nobody));
    assert_eq!(stand.nameplates(APPID), before, "a stranger's code was not spent");
    giver.close(Mood::Lonely).await;
}

#[tokio::test]
async fn a_third_end_is_refused_while_two_hold_the_code() {
    need_loopback!();
    for (name, url, _keep) in mailboxes("third").await {
        let seen = Seen::default();
        let ends = ends(vec![url.clone()], &seen);
        let (mut giver, code) = opened(&ends, versions()).await.expect("a code");
        let first = joined(&ends, &code, versions(), Duration::from_secs(5)).await;
        let mut first = first.unwrap_or_else(|e| panic!("{name}: the first taker: {e}"));
        match joined(&ends, &code, versions(), Duration::from_secs(5)).await {
            Err(Failed::Refused(why)) => assert!(why.contains("two ends hold this code"), "{name}: {why}"),
            other => panic!("{name}: a third end of the code was not refused: {:?}", other.err()),
        }
        // The two that hold it go on.
        assert!(giver.agree(LIMIT).await.unwrap(), "{name}: the giver hears the first taker");
        assert_eq!(giver.confirm(LIMIT).await.unwrap(), versions(), "{name}");
        assert_eq!(first.confirm(LIMIT).await.unwrap(), versions(), "{name}");
        giver.close(Mood::Happy).await;
        first.close(Mood::Happy).await;
    }
}

#[tokio::test]
async fn a_message_that_the_mailbox_changed_ends_the_exchange_scared() {
    need_loopback!();
    let stand = harness::stand_in::start().await;
    let (mut giver, mut taker, _) = two(&stand.url, &Seen::default()).await;
    giver.confirm(LIMIT).await.unwrap();
    taker.confirm(LIMIT).await.unwrap();
    stand.tamper("0");
    giver.send(b"the offer").await.unwrap();
    match taker.receive(LIMIT).await {
        Err(Failed::Peer(why)) => assert!(why.contains("did not open"), "{why}"),
        other => panic!("a changed message was taken: {other:?}"),
    }
    assert!(stand.moods().iter().any(|(_, mood)| mood == "scary"));
}

#[tokio::test]
async fn a_peer_that_says_nothing_after_its_key_ends_the_exchange_errory() {
    need_loopback!();
    let stand = harness::stand_in::start().await;
    let (mut giver, _taker, _) = two(&stand.url, &Seen::default()).await;
    giver.confirm(Duration::from_secs(5)).await.unwrap();
    match giver.receive(Duration::from_millis(300)).await {
        Err(Failed::Peer(why)) => assert!(why.contains("said nothing"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(stand.moods().iter().any(|(_, mood)| mood == "errory"));
}

#[tokio::test]
async fn no_mailbox_to_reach_is_unreachable_with_a_reason_for_each() {
    need_loopback!();
    let seen = Seen::default();
    let ends = ends(vec![nobody_there(), nobody_there()], &seen);
    let Err(Failed::Unreachable(why)) = opened(&ends, versions()).await.map(|_| ()) else { panic!("a giver") };
    assert_eq!(why.matches("; ").count(), 1, "{why}");
    let code = Code::parse("1-crossover-clockwork-adroitness").unwrap();
    let met = joined(&ends, &code, versions(), Duration::from_secs(1)).await;
    assert!(matches!(met, Err(Failed::Unreachable(_))));
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 4);
    assert!(seen.iter().all(|e| e.kind == "wormhole.mailbox" && e.level == Level::Warn));
}

#[tokio::test]
async fn the_code_is_in_no_event_and_no_debug() {
    need_loopback!();
    for (name, url, _keep) in mailboxes("quiet").await {
        let seen = Seen::default();
        // A mailbox where nothing listens first, so that each end has events.
        let ends = ends(vec![nobody_there(), url.clone()], &seen);
        let (mut giver, code) = opened(&ends, versions()).await.expect("a code");
        let mut taker = joined(&ends, &code, versions(), Duration::from_secs(5)).await.expect("the taker");
        assert!(giver.agree(LIMIT).await.unwrap(), "{name}");
        let wrong_failure = Failed::WrongCode.to_string();
        let lines: Vec<String> = seen.lock().unwrap().iter().map(|e| format!("{e:?} {e}")).collect();
        assert!(lines.len() >= 2, "{name}: each end said the mailbox that failed: {lines:?}");
        let debug = format!("{code:?} {ends:?} {wrong_failure}", ends = ends.config);
        let secret: Vec<&str> = code.for_terminal().split('-').skip(1).collect();
        assert_eq!(secret.len(), pod_wormhole::WORDS);
        for word in secret {
            for line in lines.iter().chain([&debug]) {
                assert!(!line.contains(word), "{name}: a word of the code in {line:?}");
            }
        }
        giver.close(Mood::Lonely).await;
        taker.close(Mood::Lonely).await;
    }
}

#[tokio::test]
async fn a_peer_of_magic_wormholes_python_library_takes_this_crate_s_code() {
    need_loopback!();
    for (name, url, _keep) in mailboxes("py-take").await {
        let (mut hole, code) = opened(&ends(vec![url.clone()], &Seen::default()), versions()).await.expect("a code");
        let Some(mut peer) = Peer::start(&url, APPID, "take") else { return };
        peer.type_code(code.for_terminal());
        assert!(hole.agree(LIMIT).await.unwrap(), "{name}: the peer's pake: {}", peer.said());
        let theirs = hole.confirm(LIMIT).await.unwrap_or_else(|e| panic!("{name}: {e}: {}", peer.said()));
        assert_eq!(theirs, versions(), "{name}");
        hole.send(b"from rust").await.unwrap();
        assert_eq!(hole.receive(LIMIT).await.unwrap(), b"from python", "{name}");
        hole.close(Mood::Happy).await;
        assert_eq!(peer.after("versions").as_deref(), Some(r#"{"pod-wormhole-test": 1}"#), "{name}: {}", peer.said());
        assert_eq!(peer.after("derived"), Some(transit_key(&hole)), "{name}: {}", peer.said());
        assert_eq!(peer.after("got"), Some(hex(b"from rust")), "{name}: {}", peer.said());
    }
}

// The test waits on the peer's line with no await; the stand-in, in this
// process, needs a worker of its own meanwhile.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn this_crate_takes_a_code_that_magic_wormholes_python_library_gives() {
    need_loopback!();
    for (name, url, _keep) in mailboxes("py-give").await {
        let ends = ends(vec![url.clone()], &Seen::default());
        let Some(mut peer) = Peer::start(&url, APPID, "give") else { return };
        let typed = peer.after("code").unwrap_or_else(|| panic!("{name}: no code: {}", peer.said()));
        let code = Code::parse(&typed).expect("magic-wormhole's code is this crate's form");
        assert_eq!(code.words(), 3, "{name}");
        let joined = joined(&ends, &code, versions(), Duration::from_secs(5)).await;
        let mut hole = joined.unwrap_or_else(|e| panic!("{name}: {e}: {}", peer.said()));
        let theirs: Value = hole.confirm(LIMIT).await.unwrap();
        assert_eq!(theirs, versions(), "{name}");
        hole.send(b"from rust").await.unwrap();
        assert_eq!(hole.receive(LIMIT).await.unwrap(), b"from python", "{name}");
        hole.close(Mood::Happy).await;
        assert_eq!(peer.after("derived"), Some(transit_key(&hole)), "{name}: {}", peer.said());
        assert_eq!(peer.after("got"), Some(hex(b"from rust")), "{name}: {}", peer.said());
    }
}

#[tokio::test]
async fn a_peer_of_magic_wormholes_python_library_with_a_wrong_code_fails_both_ends() {
    need_loopback!();
    for (name, url, _keep) in mailboxes("py-wrong").await {
        let (mut hole, code) = opened(&ends(vec![url.clone()], &Seen::default()), versions()).await.expect("a code");
        let Some(mut peer) = Peer::start(&url, APPID, "take") else { return };
        peer.type_code(&wrong(code.for_terminal()));
        assert!(hole.agree(LIMIT).await.unwrap(), "{name}: the peer's pake: {}", peer.said());
        assert_eq!(hole.confirm(LIMIT).await.err(), Some(Failed::WrongCode), "{name}");
        assert_eq!(peer.after("wrong").as_deref(), Some(""), "{name}: {}", peer.said());
    }
}
