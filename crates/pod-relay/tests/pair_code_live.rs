//! Pairing by a short code (podssh T-086) through each public mailbox (the
//! services of the kind `mailbox` of `pod-services`), by podssh's ruling
//! Q65: a short run, from a GitHub runner or a session's own host, through
//! `HTTPS_PROXY` when it is set. Two ends of one code meet at
//! magic-wormhole's mailbox, `wss://relay.magic-wormhole.io/v1`, then at Least
//! Authority's, and move each part. The pair is a test's: no credential goes
//! to a mailbox, and the mailbox sees ciphertext.
#![cfg(all(feature = "pair", feature = "code"))]

mod support;

use std::sync::{Arc, Mutex};

use pod_identity::PublicKey;
use pod_relay::code::Code;
use pod_relay::code::exchange::{Done, Offer};
use pod_relay::code::run::{self, Ends, Waits};
use pod_relay::pair::OperatorPart;
use pod_ws::url::WsUrl;
use zeroize::Zeroizing;

/// The public mailboxes, in their rank.
fn public_mailboxes() -> Vec<String> {
    pod_services::of_kind("mailbox").filter_map(|s| s.endpoint("mailbox")).map(|e| e.url()).collect()
}

const NAME: &str = "0123456789abcdef0123456789abcdef";
const TOKEN: &str = "testonlyconnect0000000000000000000000000000000000000000000000000";
const NODE: PublicKey = PublicKey([0x22; 32]);
const OPERATOR: PublicKey = PublicKey([0x11; 32]);

#[tokio::test]
#[ignore = "live: the public magic-wormhole mailboxes (Q65)"]
async fn two_ends_pair_through_each_public_mailbox() {
    for url in public_mailboxes() {
        let said = Arc::new(Mutex::new(Vec::<String>::new()));
        let kept = said.clone();
        let url = WsUrl::parse(&url).unwrap();
        let way = support::live().way.clone();
        let mut config = pod_wormhole::Config::new("example.org/pod-relay/test", "pod-relay-test", "0.0.0", way);
        config.events = Arc::new(move |event: pod_core::Event| kept.lock().unwrap().push(event.text));
        let ends = Ends { mailboxes: vec![url.clone()], config };
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
        let token = Zeroizing::new(TOKEN.to_string());
        let part = OperatorPart::checked("relay.example.org", NAME.into(), token, now + 3_600_000, now).unwrap();
        let offer = Offer { label: "lab".into(), node: NODE, part: Some(part) };
        let (shown_tx, shown_rx) = tokio::sync::oneshot::channel::<String>();
        let mut shown_tx = Some(shown_tx);
        let mut show = move |made: &Code, _: &str| {
            if let Some(tx) = shown_tx.take() {
                let _ = tx.send(made.for_terminal().to_string());
            }
        };
        let mut admit = |_: &PublicKey| Done { admitted: true, said: "a live test".into() };
        let waits = Waits::default();
        let giving = run::give(&ends, &waits, &offer, &mut show, &mut admit);
        let taking = async {
            let code = Code::parse(&shown_rx.await.expect("a code")).expect("a code");
            let mut check = |_: &Offer| Ok(());
            run::take(&ends, &waits, &code, &OPERATOR, &mut check).await
        };
        let (given, taken) = tokio::join!(giving, taking);
        let shown = url.shown();
        let given = given.unwrap_or_else(|e| panic!("{shown}: the giver: {e}; {:?}", said.lock().unwrap()));
        let taken = taken.unwrap_or_else(|e| panic!("{shown}: the taker: {e}; {:?}", said.lock().unwrap()));
        assert_eq!(given.operator, OPERATOR, "{shown}");
        assert_eq!(taken.offer.node, NODE, "{shown}");
        assert_eq!(taken.offer.part.as_ref().map(|p| p.connect_token()), Some(TOKEN), "{shown}");
        eprintln!("{shown}: two ends paired by a code");
    }
}
