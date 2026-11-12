//! The tokens of a run (podnet: one mint for each process): taken again with
//! no file of the cache, shared by the clones of a context, forgotten when
//! the relay refused them, and left when their time is short.

use std::time::Duration;

use pod_ws::Trust;

use super::*;
use crate::testing::context;

/// A made-up token: no relay issued it.
const TOKEN: &str = "testonly-run-token-0001";

/// A relay host where nothing answers: a mint there fails.
fn dead_relay() -> Relay {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    Relay { host: "127.0.0.1".into(), port }
}

async fn take(ctx: &Context, relay: &Relay) -> Result<Token, TokenError> {
    let mint = MintContext { ctx, relay, trust: &Trust::Default, timeout: Duration::from_secs(2) };
    obtain(&mint, false).await
}

#[tokio::test]
async fn a_token_of_the_run_is_taken_again_with_no_file_of_the_cache() {
    pod_testkit::need_loopback!();
    let (ctx, relay) = (context(), dead_relay());
    let key = token_key(&relay);
    remember(&ctx, &key, TOKEN, now_ms() + 3_600_000, Some(&relay_name(&relay)));
    let token = take(&ctx, &relay).await.expect("the run's token, with no mint");
    assert_eq!((token.secret(), token.origin), (TOKEN, Origin::Cache));
    assert_eq!(token.minted_at.as_deref(), Some(relay_name(&relay).as_str()));
    let clone = ctx.clone();
    assert_eq!(take(&clone, &relay).await.expect("a clone shares the run").secret(), TOKEN);
    let other = context();
    assert!(take(&other, &relay).await.is_err(), "another run mints its own");
}

#[tokio::test]
async fn a_refused_token_of_the_run_is_forgotten() {
    pod_testkit::need_loopback!();
    let (ctx, relay) = (context(), dead_relay());
    let key = token_key(&relay);
    remember(&ctx, &key, TOKEN, now_ms() + 3_600_000, None);
    forget(&ctx, &key);
    assert!(matches!(take(&ctx, &relay).await, Err(TokenError::Connect(_))), "a mint, which fails here");
}

#[tokio::test]
async fn a_token_of_the_run_with_little_time_left_is_not_taken() {
    pod_testkit::need_loopback!();
    let (ctx, relay) = (context(), dead_relay());
    remember(&ctx, &token_key(&relay), TOKEN, now_ms() + MIN_REMAINING_MS - 1000, None);
    assert!(take(&ctx, &relay).await.is_err(), "a mint, which fails here");
    // The token that the user gave comes first.
    let given = ctx.clone().with_settings(crate::Settings { token: Some(TOKEN.into()), ..Default::default() });
    assert_eq!(take(&given, &relay).await.expect("the given token").origin, Origin::Environment);
}
