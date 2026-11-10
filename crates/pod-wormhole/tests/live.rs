//! Two ends of one code meet at each public mailbox (the services of the kind
//! `mailbox` of `pod-services`): a short run, from a GitHub runner, through
//! `HTTPS_PROXY` where the way is set for it. Each end agrees the key, checks
//! the other's version and moves a message each way. The code is a test's:
//! the mailbox sees ciphertext. Live: `cargo test -p pod-wormhole --test live
//! -- --ignored`.

mod harness;

use std::time::Duration;

use pod_wormhole::mailbox::Mood;
use pod_wormhole::{Config, Ends, joined, opened};
use pod_ws::url::WsUrl;
use serde_json::json;

#[tokio::test]
#[ignore = "live: the public magic-wormhole mailboxes"]
async fn two_ends_meet_at_each_public_mailbox() {
    let urls: Vec<String> =
        pod_services::of_kind("mailbox").filter_map(|s| s.endpoint("mailbox")).map(|e| e.url()).collect();
    assert!(!urls.is_empty(), "the catalog lists no mailbox");
    for url in urls {
        let url = WsUrl::parse(&url).unwrap();
        let shown = url.shown();
        let config = Config::new(harness::APPID, "pod-wormhole-live", "0.0.0", harness::way());
        let ends = Ends { mailboxes: vec![url], config };
        let versions = json!({ "live": 1 });
        let (mut giver, code) = opened(&ends, versions.clone()).await.unwrap_or_else(|e| panic!("{shown}: {e}"));
        let mut taker = joined(&ends, &code, versions.clone(), Duration::from_secs(20)).await.expect("the taker");
        assert!(giver.agree(Duration::from_secs(60)).await.unwrap(), "{shown}");
        assert_eq!(giver.confirm(Duration::from_secs(60)).await.unwrap(), versions, "{shown}");
        assert_eq!(taker.confirm(Duration::from_secs(60)).await.unwrap(), versions, "{shown}");
        giver.send(b"from the giver").await.unwrap();
        assert_eq!(taker.receive(Duration::from_secs(60)).await.unwrap(), b"from the giver", "{shown}");
        giver.close(Mood::Happy).await;
        taker.close(Mood::Happy).await;
        eprintln!("{shown}: two ends met by a code");
    }
}
