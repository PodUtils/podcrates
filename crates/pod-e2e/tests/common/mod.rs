//! Shared test helpers for `pod-e2e` integration tests.

use pod_e2e::{self as e2e, Channel};
use pod_identity::Identity;
use tokio::io::DuplexStream;

pub const PIPE: usize = 64 * 1024;

/// The two channels of one handshake: the operator's, then the node's.
pub async fn pair() -> (Channel<DuplexStream>, Channel<DuplexStream>) {
    let (a, b) = tokio::io::duplex(PIPE);
    let operator = tokio::spawn(async move {
        let me = Identity::from_seed(&[1; 32]);
        e2e::initiate(a, &me).await?.accept(None).await
    });
    let node = Identity::from_seed(&[2; 32]);
    let node = e2e::respond(b, &node).await.unwrap().admit().await.unwrap();
    (operator.await.unwrap().unwrap(), node)
}
