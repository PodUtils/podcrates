//! A run with no relay that the user named asks the relay members of its net
//! for their lists before its race (podssh T-617; podssh's plant
//! `t617-ask-skipped`): the role of a member on the loopback hears the ask.
//! The forward opener of the plain wire runs it, so no road leaves the host.
#![cfg(feature = "plain-ws")]

mod support;

use std::time::Duration;

use pod_identity::{Identity, KeyName};
use pod_relay::failover::Mode;
use pod_relay::pool::members::Member;
use pod_relay::relay::{forward_path, select_relays};
use pod_relay::{Relay, Request, Settings, open_on_loopback};
use pod_ws::Trust;
use tokio::net::TcpListener;

const LIMIT: Duration = Duration::from_secs(20);

#[tokio::test]
async fn a_run_with_the_default_list_asks_the_net_s_relay_member_first() {
    // The member's role: a listener that says when a client connects. The
    // ask then fails its TLS; the test needs the connect alone.
    let role = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = role.local_addr().unwrap().port();
    let (heard, mut asked) = tokio::sync::mpsc::channel(1);
    tokio::spawn(async move {
        if let Ok((socket, _)) = role.accept().await {
            let _ = heard.send(()).await;
            drop(socket);
        }
    });

    let dir = std::env::temp_dir().join(format!("pod-relay-members-ask-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let member = Member {
        name: "lab".into(),
        net: "home".into(),
        key: KeyName::Key(Identity::from_seed(&[7; 32]).public()),
        host: Relay { host: "127.0.0.1".into(), port },
    };
    let ctx =
        support::context_in(vec![dir.clone()]).with_settings(Settings { members: vec![member], ..Settings::default() });

    // The default list: not one that the user named.
    let relays = select_relays(None, None, &[]).unwrap();
    assert!(!relays.explicit);
    let path = forward_path("127.0.0.1", 9).unwrap();
    let trust = Trust::Default;
    let request = Request {
        ctx: &ctx,
        relays: &relays,
        path: &path,
        trust: &trust,
        target: "127.0.0.1:9",
        rounds: 1,
        failover: Mode::default(),
    };
    // The default host is off the loopback: the plain wire refuses it, and
    // the run fails. The ask came before.
    let opened = tokio::time::timeout(LIMIT, open_on_loopback(&request, &mut |_| {})).await.expect("an answer in time");
    assert!(opened.is_err(), "the plain wire refuses a host off the loopback");
    let ask = tokio::time::timeout(Duration::from_secs(1), asked.recv()).await;
    assert!(matches!(ask, Ok(Some(()))), "the run asked the net's relay member for its list before its race");
    std::fs::remove_dir_all(&dir).unwrap();
}
