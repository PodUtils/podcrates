//! One relay session: the WebSocket to a client relay URI with the
//! subprotocol and the connect token, its frames carried as bytes by
//! `pod_ws::bridge`, and the SSH session over those bytes.

use std::sync::Arc;

use pod_core::Failure;
use pod_ws::url::{self, Asking};
use russh::Disconnect;
use russh::client::Handle;
use tokio::task::JoinHandle;

use crate::failure::{Step, wrap};
use crate::info::{ConnectInfo, Endpoint};
use crate::service;
use crate::settings::{Settings, Way};
use crate::ssh::{self, Host};

/// The buffer of the pipe between the bridge and SSH.
const PIPE: usize = 256 * 1024;
/// TLS of the relay offers HTTP/1.1 alone: an upgrade does not go over
/// HTTP/2.
const ALPN: &[&[u8]] = &[b"http/1.1"];

/// A relay session: its SSH handle and the task that carries its bytes.
pub(crate) struct Live {
    pub handle: Arc<Handle<Host>>,
    pump: JoinHandle<()>,
    /// The number of the session: 1 for the first.
    pub number: u64,
    /// The relay endpoint, for a line.
    pub relay: String,
    pub host_id: String,
}

impl Live {
    /// True while the SSH session and the WebSocket under it run.
    pub fn alive(&self) -> bool {
        !self.handle.is_closed() && !self.pump.is_finished()
    }

    /// End the session: a disconnect to the host, then the end of the
    /// WebSocket. Each channel of the session ends with it.
    pub async fn end(self) {
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            self.handle.disconnect(Disconnect::ByApplication, "", "en"),
        )
        .await;
        self.pump.abort();
    }
}

/// The session through the first endpoint of `info` that opens one; the
/// failure of the last endpoint.
pub(crate) async fn open(way: &Way, settings: &Settings, info: &ConnectInfo, number: u64) -> Result<Live, Failure> {
    let mut last = None;
    for endpoint in &info.endpoints {
        match open_one(way, settings, info, endpoint, number).await {
            Ok(live) => return Ok(live),
            Err(failure) => {
                tracing::debug!(relay = %endpoint.relay.shown(), why = %failure.chain(), "a relay endpoint failed");
                last = Some(failure);
            }
        }
    }
    Err(last.expect("the connect information has a relay endpoint"))
}

async fn open_one(
    way: &Way,
    settings: &Settings,
    info: &ConnectInfo,
    endpoint: &Endpoint,
    number: u64,
) -> Result<Live, Failure> {
    let shown = endpoint.relay.shown();
    let authorization = info.token().header();
    let asking = Asking {
        protocol: Some(service::subprotocol()),
        authorization: Some(authorization.as_str()),
        alpn: ALPN,
        limits: settings.limits,
    };
    let session = url::dial_asking(&endpoint.relay, &asking, &way.ws, &way.trust, settings.timeout, None)
        .await
        .map_err(|e| wrap(Step::WebSocket, &shown, Failure::from(e)))?;
    drop(authorization);
    let (ssh_io, ws_io) = tokio::io::duplex(PIPE);
    let pump = tokio::spawn(async move {
        if let Err(e) = pod_ws::bridge(session, ws_io, &pod_ws::NoTally).await {
            tracing::debug!(why = %e.chain(), "the relay WebSocket ended");
        }
    });
    match ssh::start(ssh_io, endpoint.host_keys.clone(), settings).await {
        Ok(handle) => {
            Ok(Live { handle: Arc::new(handle), pump, number, relay: shown, host_id: endpoint.host_id.clone() })
        }
        Err(failure) => {
            pump.abort();
            Err(failure)
        }
    }
}
