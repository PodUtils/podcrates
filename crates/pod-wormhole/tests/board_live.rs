//! A board of retained messages at each public MQTT broker (the services
//! of the kind `mqtt-broker` of `pod-services`), with its public login: a
//! note put at a topic of the run is taken by a later connection, and after
//! a clear no note is there. A short run, from a GitHub runner, through
//! `HTTPS_PROXY` where the way is set for it. The note is a test's; each
//! client of a public broker reads each topic. Live: `cargo test -p
//! pod-wormhole --test board_live -- --ignored`.

mod harness;

use std::time::Duration;

use pod_wormhole::Config;
use pod_wormhole::mqtt::broker::client_id;
use pod_wormhole::mqtt::{Board, Login, Point};
use pod_ws::url::WsUrl;

#[tokio::test]
#[ignore = "live: the public MQTT brokers"]
async fn a_board_puts_takes_and_clears_a_note_at_each_public_broker() {
    let brokers: Vec<&pod_services::Service> = pod_services::of_kind("mqtt-broker").collect();
    assert!(!brokers.is_empty(), "the catalog lists no broker");
    let mut failed = Vec::new();
    for service in brokers {
        let endpoint = service.endpoint("broker").expect("the endpoint broker");
        let url = WsUrl::parse(&endpoint.url()).unwrap();
        let login = service.text("user").zip(service.text("password"));
        let login = login.map(|(user, password)| Login { user: user.into(), password: password.into() });
        let config = Config::new(harness::APPID, "pod-wormhole-live", "0.0.0", harness::live_way());
        let board = Board::new(Point { url, login }, config);
        let topic = client_id().expect("random bytes");
        let note = format!("a note of the live test of pod-wormhole, {topic}").into_bytes();
        let run = async {
            board.put(&topic, &note).await?;
            let taken = board.take(&topic, Duration::from_secs(20)).await?;
            if taken.as_deref() != Some(&note[..]) {
                return Ok(Some(format!("the take after the put gave {:?} bytes", taken.map(|t| t.len()))));
            }
            board.clear(&topic).await?;
            let after = board.take(&topic, Duration::from_secs(5)).await?;
            Ok::<_, pod_wormhole::Failed>(
                after.map(|n| format!("a note of {} bytes is there after the clear", n.len())),
            )
        };
        let shown = board.shown();
        match run.await {
            Ok(None) => eprintln!("{shown} ({}): a board put, took and cleared a note", service.id()),
            Ok(Some(why)) => {
                eprintln!("{shown} ({}): {why}", service.id());
                failed.push(format!("{}: {why}", service.id()));
            }
            Err(why) => {
                eprintln!("{shown} ({}): {why}", service.id());
                failed.push(format!("{}: {why}", service.id()));
            }
        }
    }
    assert!(failed.is_empty(), "the brokers that failed: {failed:?}");
}
