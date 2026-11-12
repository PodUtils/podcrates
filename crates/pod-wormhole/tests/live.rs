//! Two ends of one code meet at each public mailbox (the services of the kind
//! `mailbox` of `pod-services`), and pair through the topic of a code at each
//! public MQTT broker (the kind `mqtt-broker`): a short run, from a GitHub
//! runner, through `HTTPS_PROXY` where the way is set for it. Each end agrees
//! the key, checks the other's version and moves a message. The code is a
//! test's: the mailbox and the broker see ciphertext. Live: `cargo test -p
//! pod-wormhole --test live -- --ignored`.

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
        let config = Config::new(harness::APPID, "pod-wormhole-live", "0.0.0", harness::live_way());
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

/// Two ends of one code pair through the topic of the code at each public
/// MQTT broker (the services of the kind `mqtt-broker`), with its public
/// login where it has one: the key agreed, the versions checked and a
/// message each way. Each broker alone, so that each one is proven.
#[tokio::test]
#[ignore = "live: the public MQTT brokers"]
async fn two_ends_meet_at_each_public_broker() {
    use pod_wormhole::mqtt::{self, Brokers, Login, Point};

    let brokers: Vec<&pod_services::Service> = pod_services::of_kind("mqtt-broker").collect();
    assert!(!brokers.is_empty(), "the catalog lists no broker");
    let mut failed = Vec::new();
    for service in brokers {
        let endpoint = service.endpoint("broker").expect("the endpoint broker");
        let url = WsUrl::parse(&endpoint.url()).unwrap();
        let login = service.text("user").zip(service.text("password"));
        let login = login.map(|(user, password)| Login { user: user.into(), password: password.into() });
        let config = Config::new(harness::APPID, "pod-wormhole-live", "0.0.0", harness::live_way());
        let ends = Brokers::new(vec![Point { url: url.clone(), login }], config);
        let versions = json!({ "live": 1 });
        let shown = url.shown();
        let run = async {
            let (mut giver, code) = mqtt::opened(&ends, versions.clone()).await?;
            let wait = Duration::from_secs(60);
            let (taker, heard) = tokio::join!(mqtt::joined(&ends, &code, versions.clone(), wait), giver.agree(wait));
            let (mut taker, heard) = (taker?, heard?);
            assert!(heard, "{shown}: the giver heard the taker");
            assert_eq!(giver.confirm(wait).await?, versions, "{shown}");
            assert_eq!(taker.confirm(wait).await?, versions, "{shown}");
            giver.send(b"from the giver").await?;
            assert_eq!(taker.receive(wait).await?, b"from the giver", "{shown}");
            taker.send(b"from the taker").await?;
            assert_eq!(giver.receive(wait).await?, b"from the taker", "{shown}");
            giver.close(Mood::Happy).await;
            taker.close(Mood::Happy).await;
            Ok::<_, pod_wormhole::Failed>(())
        };
        match run.await {
            Ok(()) => eprintln!("{shown} ({}): two ends paired through a topic", service.id()),
            Err(why) => {
                eprintln!("{shown} ({}): {why}", service.id());
                failed.push(format!("{}: {why}", service.id()));
            }
        }
    }
    assert!(failed.is_empty(), "the brokers that failed: {failed:?}");
}
