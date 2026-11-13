//! The board of retained messages (`pod_wormhole::mqtt::Board`) on each
//! broker that this host can run on the loopback (`harness/broker.rs`): the
//! stand-in in Rust, and Eclipse Mosquitto with a `protocol websockets`
//! listener. A note put at a topic is taken by a later connection, a second
//! put takes its place, a clear leaves no note, a note put while a take
//! waits is taken; a topic or a payload out of bounds is refused before a
//! connection; a broker that refuses the login is refused with its reason.

mod harness;

use std::time::Duration;

use harness::broker::{self, brokers};
use harness::{config, nobody_there};
use pod_testkit::need_loopback;
use pod_wormhole::Failed;
use pod_wormhole::mqtt::board::TOPIC_MOST;
use pod_wormhole::mqtt::broker::client_id;
use pod_wormhole::mqtt::{Board, Login, PAYLOAD_MOST, Point};
use pod_ws::url::WsUrl;

/// How long a take waits for a note that is not there.
const NONE_WAIT: Duration = Duration::from_millis(500);
/// How long a take waits for a note that is there.
const NOTE_WAIT: Duration = Duration::from_secs(10);

fn board(url: &WsUrl) -> Board {
    Board::new(Point { url: url.clone(), login: None }, config())
}

/// A topic of its own for each run: a broker of CI serves each test.
fn fresh_topic() -> String {
    client_id().expect("random bytes")
}

#[tokio::test]
async fn a_board_puts_takes_and_clears_a_note() {
    need_loopback!();
    for (name, url, keep) in brokers("board").await {
        let (board, topic) = (board(&url), fresh_topic());
        assert_eq!(board.take(&topic, NONE_WAIT).await.unwrap(), None, "{name}: no note before a put");
        board.put(&topic, b"the first note").await.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(board.take(&topic, NOTE_WAIT).await.unwrap().as_deref(), Some(&b"the first note"[..]), "{name}");
        // A take does not use the note up.
        assert_eq!(board.take(&topic, NOTE_WAIT).await.unwrap().as_deref(), Some(&b"the first note"[..]), "{name}");
        board.put(&topic, b"the second note").await.unwrap();
        assert_eq!(board.take(&topic, NOTE_WAIT).await.unwrap().as_deref(), Some(&b"the second note"[..]), "{name}");
        board.clear(&topic).await.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(board.take(&topic, NONE_WAIT).await.unwrap(), None, "{name}: a note after a clear");
        if let broker::Keep::Stand(stand) = &keep {
            let mine: Vec<(usize, bool)> =
                stand.publishes().into_iter().filter(|(t, _, _)| *t == topic).map(|(_, n, r)| (n, r)).collect();
            let clear = (0, true);
            assert_eq!(mine, [(14, true), (15, true), clear], "each put and the clear carry RETAIN");
            assert!(stand.retained().is_empty(), "the stand-in keeps no note after the clear");
            assert!(stand.protocols().iter().all(|p| p == "mqtt"), "the subprotocol: {:?}", stand.protocols());
        }
        eprintln!("{name}: a board put, took and cleared a note");
    }
}

#[tokio::test]
async fn a_note_put_while_a_take_waits_is_taken() {
    need_loopback!();
    for (name, url, _keep) in brokers("board-wait").await {
        let (board, topic) = (board(&url), fresh_topic());
        let (taken, put) = tokio::join!(board.take(&topic, NOTE_WAIT), async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            board.put(&topic, b"late").await
        });
        put.unwrap();
        assert_eq!(taken.unwrap().as_deref(), Some(&b"late"[..]), "{name}");
        board.clear(&topic).await.unwrap();
    }
}

#[tokio::test]
async fn a_bad_topic_or_payload_is_refused_before_a_connection() {
    need_loopback!();
    // Nothing listens: each refusal comes before a dial.
    let board = board(&nobody_there());
    let long = "a".repeat(TOPIC_MOST + 1);
    for topic in ["", "a/b", "a+", "#", "$SYS", "a b", long.as_str()] {
        assert!(matches!(board.put(topic, b"x").await, Err(Failed::Mine(_))), "put {topic:?}");
        assert!(matches!(board.take(topic, NONE_WAIT).await, Err(Failed::Mine(_))), "take {topic:?}");
        assert!(matches!(board.clear(topic).await, Err(Failed::Mine(_))), "clear {topic:?}");
    }
    let topic = "a".repeat(TOPIC_MOST);
    for payload in [Vec::new(), vec![0; PAYLOAD_MOST + 1]] {
        let refused = board.put(&topic, &payload).await;
        assert!(matches!(refused, Err(Failed::Mine(_))), "a payload of {} bytes: {refused:?}", payload.len());
    }
    // In bounds, the board dials, and nothing answers.
    assert!(matches!(board.put(&topic, &vec![0; PAYLOAD_MOST]).await, Err(Failed::Unreachable(_))));
    assert!(matches!(board.take(&topic, NONE_WAIT).await, Err(Failed::Unreachable(_))));
    assert!(matches!(board.clear(&topic).await, Err(Failed::Unreachable(_))));
}

#[tokio::test]
async fn a_broker_that_refuses_the_login_refuses_the_board() {
    need_loopback!();
    let stand = broker::start_with(Some(("public", "public"))).await;
    let wrong = Login { user: "public".into(), password: "other".into() };
    let board = Board::new(Point { url: stand.url.clone(), login: Some(wrong) }, config());
    let topic = fresh_topic();
    match board.put(&topic, b"x").await {
        Err(Failed::Refused(why)) => assert!(why.contains("a bad user name or password"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(stand.publishes().is_empty(), "no publish after a refused login");
    let right = Login { user: "public".into(), password: "public".into() };
    let board = Board::new(Point { url: stand.url.clone(), login: Some(right) }, config());
    board.put(&topic, b"x").await.unwrap();
    assert_eq!(board.take(&topic, NOTE_WAIT).await.unwrap().as_deref(), Some(&b"x"[..]));
}
