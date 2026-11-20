//! The device-code logins against the stand-in (T-378, part 2, step 2): a
//! login of GitHub and of Entra ID gives a user token that the management
//! API takes; a poll waits while the sign-in is pending, waits longer after
//! `slow_down`, and stops at a refusal or at the end of the code's life; an
//! Entra token is refreshed. No test waits: the clock of a poll is the
//! test's.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use pod_core::Class;
use pod_devtunnel::login::{self, Login};
use pod_devtunnel::standin::{Logins, Plan, Standin};
use pod_devtunnel::{Credential, Locator, Management, Scheme};

use common::{USER_TOKEN, settings, way};

const LIMIT: Duration = Duration::from_secs(10);

/// The waits of a poll, kept; none takes time.
fn clock() -> (Arc<Mutex<Vec<Duration>>>, impl Fn(Duration) -> std::future::Ready<()>) {
    let waits = Arc::new(Mutex::new(Vec::new()));
    let kept = waits.clone();
    (waits, move |d| {
        kept.lock().unwrap().push(d);
        std::future::ready(())
    })
}

async fn start(scheme: &str, logins: Logins) -> Standin {
    let standin = Standin::start(Plan::echo(scheme, USER_TOKEN)).await.unwrap();
    standin.set_logins(logins);
    standin
}

#[tokio::test]
async fn a_github_login_gives_a_token_that_the_management_api_takes() {
    pod_testkit::need_loopback!();
    let standin = start("github", Logins { pending: 2, ..Logins::default() }).await;
    let http = way().http;
    let login = Login::github().at(standin.base());
    let code = login::start(&http, &login, LIMIT).await.unwrap();
    assert_eq!((code.user_code.as_str(), code.interval), ("STAN-D1N0", Duration::from_secs(1)));
    let (waits, wait) = clock();
    let granted = login::poll(&http, &login, &code, LIMIT, wait).await.unwrap();
    assert_eq!(granted.token.scheme(), Scheme::Github);
    assert!(granted.refresh.is_none(), "a token of GitHub has no refresh");
    assert_eq!(waits.lock().unwrap().len(), 3, "two pending polls, then the token");
    assert_eq!(standin.logins().polls, 3);
    let api = Management::new(http, &settings(&standin.base()));
    let me: Credential = granted.token.into();
    api.connect_info(&Locator::parse("standin-box").unwrap(), &me).await.expect("the token is the user's");
}

#[tokio::test]
async fn an_entra_login_gives_a_token_and_a_refresh_and_a_refresh_gives_a_new_one() {
    pod_testkit::need_loopback!();
    let standin = start("aad", Logins { pending: 1, ..Logins::default() }).await;
    let http = way().http;
    let login = Login::aad("made-up-client-id").at(standin.base());
    let code = login::start(&http, &login, LIMIT).await.unwrap();
    let (_, wait) = clock();
    let granted = login::poll(&http, &login, &code, LIMIT, wait).await.unwrap();
    assert_eq!((granted.token.scheme(), granted.expires_in), (Scheme::Aad, Some(Duration::from_secs(3600))));
    let refresh = granted.refresh.expect("a refresh token");
    let again = login::refresh(&http, &login, &refresh, LIMIT).await.unwrap();
    assert_eq!(again.token.scheme(), Scheme::Aad);
    assert_eq!(standin.logins().refreshes, 1);
    let refused = login::refresh(&http, &login, "made-up-wrong-refresh", LIMIT).await.unwrap_err();
    assert_eq!(refused.class, Class::Refused, "{}", refused.chain());
    assert!(login::refresh(&http, &Login::github().at(standin.base()), &refresh, LIMIT).await.is_err());
}

#[tokio::test]
async fn a_slow_down_adds_five_seconds_to_the_wait() {
    pod_testkit::need_loopback!();
    let standin = start("github", Logins { slow_down: true, ..Logins::default() }).await;
    let http = way().http;
    let login = Login::github().at(standin.base());
    let code = login::start(&http, &login, LIMIT).await.unwrap();
    let (waits, wait) = clock();
    login::poll(&http, &login, &code, LIMIT, wait).await.unwrap();
    assert_eq!(*waits.lock().unwrap(), [Duration::from_secs(1), Duration::from_secs(6)]);
}

#[tokio::test]
async fn a_refusal_and_the_end_of_the_code_stop_the_poll_with_no_token_in_the_words() {
    pod_testkit::need_loopback!();
    let standin = start("github", Logins { deny: true, ..Logins::default() }).await;
    let http = way().http;
    let login = Login::github().at(standin.base());
    let code = login::start(&http, &login, LIMIT).await.unwrap();
    let (_, wait) = clock();
    let denied = login::poll(&http, &login, &code, LIMIT, wait).await.unwrap_err();
    assert_eq!(denied.class, Class::Refused);
    assert!(denied.chain().contains("refused the sign-in"), "{}", denied.chain());
    // Pending for longer than the code lives (900 s at 1 s).
    standin.set_logins(Logins { pending: 10_000, ..Logins::default() });
    let (waits, wait) = clock();
    let expired = login::poll(&http, &login, &code, LIMIT, wait).await.unwrap_err();
    assert!(expired.chain().contains("expired"), "{}", expired.chain());
    assert_eq!(waits.lock().unwrap().len(), 900);
    for words in [denied.chain(), expired.chain()] {
        assert!(!words.contains(USER_TOKEN) && !words.contains("standin-device-code"), "{words}");
    }
}

/// A base that does not answer: the failure names the request once
/// (PodUtils/podnet#658).
#[tokio::test]
async fn a_dead_base_names_the_request_once() {
    pod_testkit::need_loopback!();
    let (_held, at) = pod_testkit::refused::refused();
    let base = format!("http://{at}");
    let http = way().http;
    let login = Login::github().at(&base);
    let chain = login::start(&http, &login, LIMIT).await.expect_err("nothing answers").chain();
    assert_eq!(chain.matches(&format!("POST {base}")).count(), 1, "{chain}");
    let api = Management::new(http, &settings(&base));
    let failure = api.connect_info(&Locator::parse("standin-box").unwrap(), &Credential::Anonymous).await;
    let chain = failure.expect_err("nothing answers").chain();
    assert_eq!(chain.matches(&format!("GET {base}")).count(), 1, "{chain}");
}
