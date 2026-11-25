//! The edge addresses and the WARP session of the `quic+warp` leg.

use std::sync::Arc;
use std::time::Duration;

use crate::fail::{Context, Result, bail};
use pod_core::{Class, Stage};

use super::{IpVersionFilter, edge, edge_override};
use crate::Host;
use crate::host::{WarpSession, WarpSessions};

pub(super) async fn warp_targets(
    host: &Host,
    discovery: &super::Discovery,
) -> Result<(Vec<edge::EdgeAddr>, Arc<WarpLink>, noq::Endpoint)> {
    let Some(sessions) = host.warp.clone() else {
        bail!(Stage::Warp, Class::Unsupported, "the consumer gives no WARP sessions");
    };
    let discovered = match edge_override(&host.settings) {
        Some(list) => list,
        None => discovery.edges(host, IpVersionFilter::V4Only).await.context("edge discovery")?,
    };
    let Some(first) = discovered.first() else {
        bail!(Stage::Dns, Class::Unavailable, "edge discovery returned no addresses");
    };
    let session = sessions.open().await.context("opening the WARP MASQUE tunnel on TCP 443")?;
    let endpoint = session.endpoint.clone();
    let text = format!("the tunnel edge {} is reached through WARP on TCP 443", first.socket());
    host.event(pod_core::Event::info("warp.session", text));
    let link = Arc::new(WarpLink { sessions, current: tokio::sync::Mutex::new((session, std::time::Instant::now())) });
    Ok((discovered, link, endpoint))
}

/// The WARP session of the `quic+warp` leg, and the QUIC endpoint in it.
/// When the session closes, or a reconnect fails twice, the next reconnect
/// opens a new session with the same device (no new registration).
pub(crate) struct WarpLink {
    pub(super) sessions: Arc<dyn WarpSessions>,
    pub(super) current: tokio::sync::Mutex<(WarpSession, std::time::Instant)>,
}

/// A new session at most this often for failed reconnects (an HA leg must
/// not replace the session that another leg opened a short time before).
const WARP_REBUILD_GAP: Duration = Duration::from_secs(10);

impl WarpLink {
    async fn endpoint(&self, stale: bool) -> Result<noq::Endpoint> {
        let mut current = self.current.lock().await;
        let closed = (current.0.closed)();
        if closed || (stale && current.1.elapsed() >= WARP_REBUILD_GAP) {
            tracing::debug!(session_closed = closed, "opening a new WARP session for the edge leg");
            let session = self.sessions.open().await.context("opening the WARP MASQUE tunnel on TCP 443 again")?;
            *current = (session, std::time::Instant::now());
        }
        Ok(current.0.endpoint.clone())
    }
}

/// Where the QUIC reactor gets its endpoint for each reconnect.
#[derive(Clone)]
pub(super) enum EndpointSource {
    /// A kernel UDP socket: always the same endpoint.
    Fixed(noq::Endpoint),
    /// The WARP session: a new one when it closed (see [`WarpLink`]).
    Warp(Arc<WarpLink>),
}

impl EndpointSource {
    /// The endpoint for reconnect attempt `attempt` (1 for the first).
    pub(super) async fn endpoint(&self, attempt: u32) -> Result<noq::Endpoint> {
        match self {
            EndpointSource::Fixed(endpoint) => Ok(endpoint.clone()),
            EndpointSource::Warp(link) => link.endpoint(attempt >= 3).await,
        }
    }

    /// Ends when the WARP session under the endpoint has closed; a kernel
    /// socket never closes this way. The session is read each 2 s.
    pub(super) async fn closed(&self) {
        let EndpointSource::Warp(link) = self else {
            return std::future::pending().await;
        };
        loop {
            tokio::time::sleep(WARP_WATCH).await;
            if link.current.try_lock().is_ok_and(|c| (c.0.closed)()) {
                return;
            }
        }
    }
}

/// How often a leg reads its WARP session.
const WARP_WATCH: Duration = Duration::from_secs(2);
