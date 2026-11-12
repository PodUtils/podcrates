//! No token in a `Debug`, an event, a line of `tracing` or the text of a
//! failure: a whole life of a client (a connect, a stream, a reconnect, a
//! refusal) runs with each line of each level kept, and no kept text holds
//! the user's token or a connect token.

mod common;

use pod_devtunnel::standin::{Standin, TOKEN_PREFIX};
use pod_devtunnel::{Locator, Management};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use common::{Lines, USER_TOKEN, connected, plan, settings, user, way};

/// True when `text` holds a token of the tests.
fn holds_token(text: &str) -> bool {
    text.contains(USER_TOKEN) || text.contains(TOKEN_PREFIX)
}

#[tokio::test]
async fn no_token_reaches_a_debug_an_event_a_line_or_a_failure() {
    pod_testkit::need_loopback!();
    let lines = Lines::default();
    // This runtime has one thread: each task of the test logs here.
    let _guard = tracing::subscriber::set_default(lines.clone());
    let mut texts = Vec::new();

    let (standin, relay, kept) = connected(plan()).await;
    let mut stream = relay.open(22).await.unwrap();
    stream.write_all(b"ping").await.unwrap();
    stream.read_exact(&mut [0u8; 4]).await.unwrap();
    standin.drop_relays();
    let _ = stream.read(&mut [0u8; 4]).await;
    let mut again = relay.open(22).await.unwrap();
    again.write_all(b"x").await.unwrap();
    again.read_exact(&mut [0u8; 1]).await.unwrap();
    texts.push(format!("{relay:?} {stream:?} {again:?} {:?} {:?}", user(), way()));
    texts.push(format!("{:?}", settings(&standin.base())));

    let api = Management::new(way().http, &settings(&standin.base()));
    let info = api.connect_info(&Locator::parse("standin-box").unwrap(), &user()).await.unwrap();
    texts.push(format!("{info:?} {:?} {:?} {api:?}", info.token(), info.endpoints));

    // The failures: a refused user, a refused upgrade, a reconnect that
    // ends.
    standin.refuse_upgrades(100);
    standin.drop_relays();
    let reconnect = relay.open(22).await.unwrap_err();
    texts.push(format!("{} {reconnect:?}", reconnect.chain()));
    standin.refuse_user();
    let refused = api.connect_info(&Locator::parse("standin-box").unwrap(), &user()).await.unwrap_err();
    texts.push(format!("{} {refused:?}", refused.chain()));
    let other = Standin::start(plan()).await.unwrap();
    other.refuse_user();
    let first = pod_devtunnel::Relay::connect(
        way(),
        settings(&other.base()),
        Locator::parse("standin-box").unwrap(),
        user(),
        common::report().0,
    )
    .await
    .unwrap_err();
    texts.push(format!("{} {first:?}", first.chain()));

    texts.extend(kept.texts());
    texts.extend(kept.decisions.lock().unwrap().iter().map(|(d, o)| format!("{d} {o}")));
    let lines = lines.0.lock().unwrap().clone();
    assert!(!lines.is_empty(), "the subscriber kept no line");
    for text in texts.iter().chain(lines.iter()) {
        assert!(!holds_token(text), "a token in: {text}");
    }
    // The checks see the tokens where they go: the stand-in took them.
    assert!(standin.minted() >= 2);
    assert!(standin.seen().relay_tokens.iter().all(|t| *t == "fresh"));
}
