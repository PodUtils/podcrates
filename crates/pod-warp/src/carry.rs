//! A second WireGuard device carried inside a session (PodUtils/podnet
//! issues 154 and 155): its tunnel goes over a UDP socket of the outer
//! [`WarpNet`], and its stack is the network of the caller.

use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::Arc;

use pod_core::{Class, Failure, Stage};

use crate::net::WarpNet;
use crate::session::MasqueSession;
use crate::warp::{CARRY_MTU, Warp};
use crate::wg::Peer;

impl Warp {
    /// The addresses of the device of the outer session: the WireGuard
    /// peer's, else the device record's.
    pub(crate) fn session_addresses(&self) -> (Option<Ipv4Addr>, Option<Ipv6Addr>) {
        match &self.settings.wireguard {
            Some(peer) => (peer.v4, peer.v6),
            None => (self.device.ipv4(), self.device.ipv6()),
        }
    }

    /// The stack of the device of `peer` inside `outer`. Its MTU
    /// ([`CARRY_MTU`]) leaves room for WireGuard and the headers of the
    /// outer packet; it has IPv4 only, as IPv6 needs an MTU of 1280. The
    /// carried session holds `outer`, and its budget is the budget of
    /// `outer`: the relay of `outer` ends both.
    pub(crate) async fn connect_carried(&self, outer: WarpNet, peer: &Peer) -> Result<WarpNet, Failure> {
        let settings = &self.settings;
        let v4 = peer
            .v4
            .ok_or_else(|| Failure::new(Stage::Warp, Class::Config, "the carried device has no IPv4 address"))?;
        let session = MasqueSession::connect_wg_inside(peer, &outer)
            .await
            .map_err(|e| Failure::caused(e.stage, e.class, "the carried WireGuard tunnel", e))?;
        let mut net = settings.net.clone();
        net.mtu = CARRY_MTU;
        let budget = outer.clone();
        net.renew = Some(Arc::new(move || budget.renew_due()));
        let inner = WarpNet::start(session, Some(v4), None, &net, settings.report.clone())?;
        if let Some(check) = &settings.net.check {
            let took = inner
                .check_data_path(check)
                .await
                .map_err(|e| Failure::caused(e.stage, e.class, "the carried tunnel carries no packet", e))?;
            tracing::info!(ms = took.as_millis() as u64, "a packet crossed the carried WireGuard tunnel");
        }
        settings.report.decision("warp.carry", "carried");
        Ok(inner)
    }
}
