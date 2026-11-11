//! A node's door with a grant used once, and the tokens of one leg, through
//! the channel: a session that a grant let in opens its target marked as a
//! grant's ([`ends::granted`]), and one of the door unmarked; a grant meets
//! the node's person as a key of the door does; an operator's end gives the
//! node's seed of its tokens of one leg to the taker of its session. podssh
//! proves the same with its own grants and relay (its tests `grant.rs` and
//! `relay_serve_once.rs`, plants t226-route-unmarked, t638-grant-skips-the-
//! person and t615-seed-unread); these tests run the crate alone.

mod e2e_harness;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use e2e_harness::{PIPE, echo, echo_back, identity, node};
use pod_e2e::ends::{self, Redeem};
use pod_e2e::legs::{self, Frames};
use pod_e2e::person::{Answer, Asker, Gate, Question};
use pod_e2e::{Ask, Error};
use pod_identity::PublicKey;
use tokio::io::AsyncWriteExt;

const LIMIT: Duration = Duration::from_secs(30);
/// The grant that the node's check takes, made up for the tests.
const GRANT: &[u8] = b"a grant of the tests";

/// A check of grants that takes [`GRANT`] alone.
fn redeem() -> Arc<Redeem> {
    Arc::new(|_: &PublicKey, _: &Ask, grant: &[u8]| {
        (grant == GRANT).then(|| "the grant of the tests".to_string()).ok_or_else(|| "not the test's grant".into())
    })
}

/// One session of `grant` to a node whose admission refuses each key, with
/// `redeem` and `gate`: whether the target opened marked as a grant's, the
/// operator's end, and the node's lines.
async fn granted_session(
    grant: Option<&[u8]>,
    gate: Option<Arc<Gate>>,
) -> (Vec<bool>, Result<PublicKey, Error>, Vec<String>) {
    let (mut node_end, lines) = node(identity(2), |_| Err("not in the test's list".into()));
    node_end.redeem = Some(redeem());
    node_end.gate = gate;
    let marks: Arc<Mutex<Vec<bool>>> = Arc::default();
    let seen = marks.clone();
    let (mut app, app_end) = tokio::io::duplex(PIPE);
    let (cipher, session) = ends::operator_granted(app_end, identity(1), |_| Ok(()), None, grant.map(<[u8]>::to_vec));
    tokio::spawn(async move {
        ends::serve_node(cipher, &node_end, move || {
            seen.lock().unwrap().push(ends::granted());
            echo(Arc::default())()
        })
        .await
    });
    let session = tokio::spawn(session);
    if let Ok(back) = echo_back(&mut app, b"through the door").await {
        assert_eq!(back, b"through the door");
    }
    let _ = app.shutdown().await;
    let ended = tokio::time::timeout(LIMIT, session).await.unwrap().unwrap();
    let lines = lines.lock().unwrap().clone();
    let marks = marks.lock().unwrap().clone();
    (marks, ended, lines)
}

/// A target that a grant let in opens as a grant's, so that the route's own
/// check of the key stands aside; a grant that the check refuses, and a key
/// of no grant that the admission refuses, open nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_session_that_a_grant_let_in_opens_its_target_marked() {
    let (marks, ended, lines) = granted_session(Some(GRANT), None).await;
    assert!(ended.is_ok(), "{ended:?}");
    assert_eq!(marks, vec![true], "the grant's target opened unmarked");
    assert!(lines.iter().any(|l| l.contains("came with the grant of the tests")), "{lines:?}");
    let (marks, ended, _) = granted_session(Some(b"another grant"), None).await;
    assert!(matches!(ended, Err(Error::Refused(_))), "{ended:?}");
    assert!(marks.is_empty());
    let (marks, ended, _) = granted_session(None, None).await;
    assert!(matches!(ended, Err(Error::Refused(_))), "{ended:?}");
    assert!(marks.is_empty());
}

/// A key that a grant let through the door meets the node's person, as a
/// key of the door does: one question, which names the grant.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_grant_meets_the_node_s_person_as_a_key_of_the_door_does() {
    let asked: Arc<Mutex<Vec<Question>>> = Arc::default();
    let kept = asked.clone();
    let asker: Arc<Asker> = Arc::new(move |question: Question| -> Answer {
        kept.lock().unwrap().push(question);
        Box::pin(async { Ok(()) })
    });
    let (marks, ended, lines) = granted_session(Some(GRANT), Some(Gate::new(asker))).await;
    assert!(ended.is_ok(), "{ended:?}");
    assert_eq!(marks, vec![true]);
    let questions = asked.lock().unwrap().clone();
    assert_eq!(questions.len(), 1, "the grant passed the person with no question");
    assert_eq!((questions[0].key, questions[0].granted), (identity(1).public(), true));
    assert!(lines.iter().any(|l| l.contains("by a grant and a yes of the node's person")), "{lines:?}");
}

/// The operator's end of a session gives the seed of the node's verdict to
/// the taker of its session, so that the legs that resume it take the
/// tokens of the seed; the node registered them at its role first.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_operator_s_end_gives_the_node_s_seed_to_the_taker_of_its_session() {
    let (node_end, _lines) = node(identity(2), |_| Ok(()));
    let registered: Arc<Mutex<Vec<String>>> = Arc::default();
    let into = registered.clone();
    let frames: Frames = Arc::new(move |frame: String| into.lock().unwrap().push(frame));
    node_end.leases.attach(Some(frames));
    let (mut app, app_end) = tokio::io::duplex(PIPE);
    let (cipher, session) = ends::operator(app_end, identity(1), |_| Ok(()));
    tokio::spawn(async move { ends::serve_node(cipher, &node_end, echo(Arc::default())).await });
    let operator = tokio::spawn(legs::during(async move {
        let ended = session.await;
        let taker = legs::current().expect("a taker inside the session");
        (ended, taker.holds(), taker.token_at(1))
    }));
    assert_eq!(echo_back(&mut app, b"a leg").await.unwrap(), b"a leg");
    app.shutdown().await.unwrap();
    let (ended, holds, first) = tokio::time::timeout(LIMIT, operator).await.unwrap().unwrap();
    assert!(ended.is_ok(), "{ended:?}");
    assert!(holds, "the operator's end set the node's seed aside");
    let first = first.expect("the token of the first leg");
    let digest = legs::hex(&legs::digest(&first));
    let registered = registered.lock().unwrap().clone();
    assert!(registered.iter().any(|f| f.contains(&digest)), "the node registered the operator's first token");
}
