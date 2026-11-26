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
use pod_e2e::ends::{self, BEARER_REFUSED, Credential, GRANT_REFUSED, Redeem, Redemption};
use pod_e2e::legs::{self, Frames};
use pod_e2e::person::{Answer, Asker, Gate, Question};
use pod_e2e::{Ask, Error, Refusal};
use pod_identity::PublicKey;
use tokio::io::AsyncWriteExt;

const LIMIT: Duration = Duration::from_secs(30);
/// The grant that the node's check takes, made up for the tests.
const GRANT: &[u8] = b"a grant of the tests";

/// A check of credentials that takes [`GRANT`] alone, and says that it
/// checked a `credential`.
fn redeem(credential: Credential) -> Arc<Redeem> {
    Arc::new(move |_: &PublicKey, _: &Ask, grant: &[u8]| Redemption {
        credential,
        result: (grant == GRANT)
            .then(|| "the grant of the tests".to_string())
            .ok_or_else(|| "not the test's grant".into()),
    })
}

/// One session of `grant` to a node whose admission refuses each key, with
/// `redeem` and `gate`: whether the target opened marked as a grant's, the
/// operator's end, and the node's lines.
async fn granted_session(
    grant: Option<&[u8]>,
    gate: Option<Arc<Gate>>,
    credential: Credential,
) -> (Vec<bool>, Result<PublicKey, Error>, Vec<String>) {
    let (mut node_end, lines) = node(identity(2), |_| Err("not in the test's list".into()));
    node_end.redeem = Some(redeem(credential));
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
    let (marks, ended, lines) = granted_session(Some(GRANT), None, Credential::Grant).await;
    assert!(ended.is_ok(), "{ended:?}");
    assert_eq!(marks, vec![true], "the grant's target opened unmarked");
    assert!(lines.iter().any(|l| l.contains("came with the grant of the tests")), "{lines:?}");
    let (marks, ended, _) = granted_session(Some(b"another grant"), None, Credential::Grant).await;
    assert!(matches!(ended, Err(Error::Refused(_))), "{ended:?}");
    assert!(marks.is_empty());
    let (marks, ended, _) = granted_session(None, None, Credential::Grant).await;
    assert!(matches!(ended, Err(Error::Refused(_))), "{ended:?}");
    assert!(marks.is_empty());
}

/// The node names the credential that it let in or refused: the operator
/// hears the words of a bearer for a bearer and of a grant for a grant, and
/// the node's lines say the same (PodUtils/podssh#362, ask 3).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_node_names_the_credential_that_it_let_in_or_refused() {
    assert_ne!(GRANT_REFUSED, BEARER_REFUSED);
    for credential in [Credential::Grant, Credential::Bearer] {
        let words = credential.words();
        let (marks, ended, lines) = granted_session(Some(GRANT), None, credential).await;
        assert!(ended.is_ok(), "{credential:?}: {ended:?}");
        assert_eq!(marks, vec![true], "{credential:?}");
        assert!(lines.iter().any(|l| l.contains(&format!("came in, by {words}"))), "{credential:?}: {lines:?}");
        let (marks, ended, lines) = granted_session(Some(b"another grant"), None, credential).await;
        assert!(marks.is_empty(), "{credential:?}");
        let heard = matches!(&ended, Err(Error::Refused(Refusal::Other(why))) if why == credential.refused());
        assert!(heard, "{credential:?}: {ended:?}");
        let said = format!("refused {words} of the operator key {}: not the test's grant", identity(1).public());
        assert!(lines.iter().any(|l| l.contains(&said)), "{credential:?}: {lines:?}");
    }
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
    let (marks, ended, lines) = granted_session(Some(GRANT), Some(Gate::new(asker)), Credential::Grant).await;
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

/// What one session of [`bound_session`] saw: the binding that the
/// operator's maker of the grant got, the one that the node's check got,
/// and whether the target saw one.
struct Bound {
    minted: Option<pod_e2e::Binding>,
    checked: Option<pod_e2e::Binding>,
    at_target: bool,
    ended: Result<PublicKey, Error>,
}

/// One session whose operator makes its grant from the node's key and the
/// binding, to a node whose check takes a grant that names the binding it
/// gets ([`ends::binding`]) and that refuses each key at its door.
async fn bound_session() -> Bound {
    let (mut node_end, _) = node(identity(2), |_| Err("not in the test's list".into()));
    let checked: Arc<Mutex<Option<pod_e2e::Binding>>> = Arc::default();
    let seen = checked.clone();
    node_end.redeem = Some(Arc::new(move |_: &PublicKey, _: &Ask, grant: &[u8]| {
        Redemption::grant((|| -> Result<String, String> {
            let binding = ends::binding().ok_or("no binding")?;
            *seen.lock().unwrap() = Some(binding.clone());
            (grant == binding.bytes()).then(|| "a bound grant".to_string()).ok_or_else(|| "another binding".into())
        })())
    }));
    let minted: Arc<Mutex<Option<pod_e2e::Binding>>> = Arc::default();
    let kept = minted.clone();
    let mint = move |node_key: PublicKey, binding: pod_e2e::Binding| {
        assert_eq!(node_key, identity(2).public(), "the maker gets the node's key");
        let grant = binding.bytes().to_vec();
        *kept.lock().unwrap() = Some(binding);
        std::future::ready(Some(grant))
    };
    let at_target = Arc::new(Mutex::new(false));
    let target_saw = at_target.clone();
    let (mut app, app_end) = tokio::io::duplex(PIPE);
    let (cipher, session) = ends::operator_bound(app_end, identity(1), |_| Ok(()), None, mint, None, |_| {});
    tokio::spawn(async move {
        ends::serve_node(cipher, &node_end, move || {
            *target_saw.lock().unwrap() = ends::binding().is_some();
            echo(Arc::default())()
        })
        .await
    });
    let session = tokio::spawn(session);
    if let Ok(back) = echo_back(&mut app, b"bound").await {
        assert_eq!(back, b"bound");
    }
    let _ = app.shutdown().await;
    let ended = tokio::time::timeout(LIMIT, session).await.unwrap().unwrap();
    let minted = minted.lock().unwrap().take();
    let checked = checked.lock().unwrap().take();
    let at_target = *at_target.lock().unwrap();
    Bound { minted, checked, at_target, ended }
}

/// The operator's maker of a grant and the node's check of it get one
/// binding; another channel of the same two keys gets another; the binding
/// is there only inside the check of the grant.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn both_ends_of_a_channel_get_one_binding_and_another_channel_another() {
    let first = bound_session().await;
    assert!(first.ended.is_ok(), "{:?}", first.ended);
    let minted = first.minted.expect("the operator's maker ran");
    assert_eq!(Some(&minted), first.checked.as_ref(), "the two ends got two bindings");
    assert!(!first.at_target, "the binding stays inside the check of the grant");
    assert!(ends::binding().is_none(), "no binding outside a node's check");
    let second = bound_session().await;
    assert!(second.ended.is_ok(), "{:?}", second.ended);
    assert_ne!(second.minted.as_ref(), Some(&minted), "two channels share a binding");
}
