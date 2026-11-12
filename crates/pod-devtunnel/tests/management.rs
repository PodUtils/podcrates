//! The management API against the stand-in: the path of a tunnel by its
//! name and by its id, the query of the scope and the version, the user's
//! token in `Authorization`, a tunnel that does not exist, a refusal.

mod common;

use pod_core::Class;
use pod_devtunnel::standin::Standin;
use pod_devtunnel::{Locator, Management, Scheme, Settings, Step, UserToken, step_of};

use common::{USER_TOKEN, plan, settings, user, way};

fn management(base: &str) -> Management {
    Management::new(way().http, &settings(base))
}

#[test]
fn the_url_of_a_tunnel_on_the_service() {
    let api = Management::new(way().http, &Settings::default());
    let by_name = api.url(&Locator::parse("my-box").unwrap());
    assert_eq!(
        by_name,
        "https://global.rel.tunnels.api.visualstudio.com/tunnels/my-box?tokenScopes=connect&api-version=2023-09-27-preview"
    );
    let by_id = api.url(&Locator::parse("bcd123.usw2").unwrap());
    assert_eq!(
        by_id,
        "https://usw2.rel.tunnels.api.visualstudio.com/tunnels/bcd123?tokenScopes=connect&api-version=2023-09-27-preview"
    );
}

#[tokio::test]
async fn the_connect_information_of_a_tunnel_by_name_and_by_id() {
    pod_testkit::need_loopback!();
    let standin = Standin::start(plan()).await.unwrap();
    let api = management(&standin.base());
    for text in ["standin-box", "bcd123.usw2"] {
        let info = api.connect_info(&Locator::parse(text).unwrap(), &user()).await.unwrap();
        assert_eq!(info.tunnel.tunnel_id.as_deref(), Some("bcd123"));
        assert_eq!(info.tunnel.cluster_id.as_deref(), Some("usw2"));
        assert_eq!(info.endpoints.len(), 1);
        assert_eq!(info.endpoints[0].host_keys, [standin.host_key()]);
        assert!(info.endpoints[0].relay.target.starts_with(pod_devtunnel::standin::RELAY_PATH));
    }
    let seen = standin.seen();
    assert_eq!(
        seen.gets,
        [
            "/tunnels/standin-box?tokenScopes=connect&api-version=2023-09-27-preview",
            "/tunnels/bcd123?tokenScopes=connect&api-version=2023-09-27-preview",
        ]
    );
    // The user's token, with its scheme, in Authorization.
    assert_eq!(seen.user_ok, [true, true]);
    // Each get mints a fresh connect token.
    assert_eq!(standin.minted(), 2);
}

#[tokio::test]
async fn a_tunnel_that_does_not_exist_is_not_found() {
    pod_testkit::need_loopback!();
    let standin = Standin::start(plan()).await.unwrap();
    let failure =
        management(&standin.base()).connect_info(&Locator::parse("other").unwrap(), &user()).await.unwrap_err();
    assert_eq!((step_of(&failure), failure.class), (Some(Step::Management), Class::NotFound));
    assert!(failure.chain().contains("GET http://127.0.0.1:"), "{}", failure.chain());
    assert!(failure.chain().contains("/tunnels/other answered HTTP 404: Not Found"), "{}", failure.chain());
}

#[tokio::test]
async fn a_token_of_another_scheme_is_refused() {
    pod_testkit::need_loopback!();
    let standin = Standin::start(plan()).await.unwrap();
    let aad = UserToken::new(Scheme::Aad, USER_TOKEN).unwrap();
    let failure =
        management(&standin.base()).connect_info(&Locator::parse("standin-box").unwrap(), &aad).await.unwrap_err();
    assert_eq!((step_of(&failure), failure.class), (Some(Step::Management), Class::Refused));
    assert_eq!(standin.seen().user_ok, [false]);
    assert_eq!(standin.minted(), 0);
}

#[tokio::test]
async fn a_management_api_that_does_not_answer_is_named() {
    pod_testkit::need_loopback!();
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let failure = management(&base).connect_info(&Locator::parse("standin-box").unwrap(), &user()).await.unwrap_err();
    assert_eq!(step_of(&failure), Some(Step::Management));
    assert!(
        failure.chain().starts_with("the Dev Tunnels management API: GET http://127.0.0.1:"),
        "{}",
        failure.chain()
    );
}
