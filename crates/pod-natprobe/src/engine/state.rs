//! Per-question internal state structs owned by [`super::ProbeMachine`].

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use crate::classify::{AllocSample, Control, Probe, RungOutcome};
use crate::report::{
    ExternalIpSource, Flag, HairpinBehavior, Nat64Status, Obs, ObsId, PcpState, PmpState, SockId, Tick,
};
use crate::wire::{Topology, Validated};

#[derive(Clone, Debug, Default)]
pub struct MapState {
    pub local_addr: Option<SocketAddr>,
    pub bind_failed: bool,
    pub primary_dest: Option<SocketAddr>,
    pub m1: Option<SocketAddr>,
    pub m1_obs: Option<ObsId>,
    pub topo: Option<Topology>,
    pub m2: Option<SocketAddr>,
    pub m3: Option<SocketAddr>,
    pub m1b: Option<SocketAddr>,
    pub m_pa: Option<SocketAddr>,
    pub per_dest: Vec<(SocketAddr, SocketAddr, ObsId)>,
    pub tried_servers: usize,
    pub primary_timeouts: u8,
    pub t2_done: bool,
    pub t3_done: bool,
    pub repeat_done: bool,
    pub pa_done: bool,
    pub done: bool,
}

#[derive(Clone, Debug, Default)]
pub struct FilterState {
    pub local_addr: Option<SocketAddr>,
    pub bind_failed: bool,
    pub m1_f: Option<SocketAddr>,
    pub m1_obs: Option<ObsId>,
    pub tried_ips: Vec<IpAddr>,
    pub t1_done: bool,
    pub t1_timeout: bool,
    pub topo: Option<Topology>,
    pub no_other_reason: Option<crate::report::UnknownReason>,
    pub t2: Option<Probe<()>>,
    pub t3: Option<Probe<()>>,
    pub t2_obs: Option<ObsId>,
    pub t3_obs: Option<ObsId>,
    pub c06: Control,
    pub c02: Control,
    pub ctl_running: bool,
    pub late_upgraded: bool,
    pub done: bool,
}

#[derive(Clone, Debug, Default)]
pub struct AllocState {
    pub target_count: usize,
    pub dest: Option<SocketAddr>,
    pub pending_binds: usize,
    pub samples_by_idx: BTreeMap<usize, (AllocSample, ObsId)>,
    pub sock_locals: BTreeMap<SockId, SocketAddr>,
    pub sock_mapped: BTreeMap<SockId, SocketAddr>,
    pub scheduled: usize,
    pub deep_gap_at: Option<Tick>,
    pub deep_gap_mapped: Option<SocketAddr>,
    pub deep_gap_done: bool,
    pub done: bool,
}

impl AllocState {
    /// Ordered allocation samples sorted by logical socket index (`0` = `SOCK_F`, `1..` = `A_i`).
    pub fn ordered_samples(&self) -> Vec<AllocSample> {
        self.samples_by_idx.values().map(|(s, _)| *s).collect()
    }

    /// Observation IDs matching [`Self::ordered_samples`].
    pub fn ordered_obs_ids(&self) -> Vec<ObsId> {
        self.samples_by_idx.values().map(|(_, id)| *id).collect()
    }
}

#[derive(Clone, Debug, Default)]
pub struct HairpinState {
    pub started: bool,
    pub done: bool,
    pub s1: Option<(SockId, SocketAddr, SocketAddr)>,
    pub s2: Option<(SockId, SocketAddr, SocketAddr)>,
    pub sent_txids_12: Vec<[u8; 16]>,
    pub sent_txids_21: Vec<[u8; 16]>,
    pub rounds_sent: u8,
    pub next_round_at: Option<Tick>,
    pub punch_used: bool,
    pub pool_split: bool,
    pub observed: Option<(HairpinBehavior, ObsId, Vec<Flag>)>,
}

#[derive(Clone, Debug)]
pub struct PortmapState {
    pub started: bool,
    pub done: bool,
    pub retry_sent: bool,
    pub retry_at: Option<Tick>,
    pub deadline: Option<Tick>,
    pub upnp_end_at: Option<Tick>,
    pub nat_pmp: Obs<PmpState>,
    pub pcp: Obs<PcpState>,
    pub igd_locations: Vec<(String, IpAddr)>,
    pub upnp_control_url_seen: bool,
    pub upnp_http_tried: bool,
    pub external_v4: Vec<(ExternalIpSource, Ipv4Addr)>,
    pub pcp_address_mismatch: bool,
    pub not_gateway_drops: u32,
    pub http_desc_id: Option<u32>,
    pub http_desc_location: Option<(String, IpAddr)>,
    pub http_soap_id: Option<u32>,
    pub http_service_type: Option<String>,
}

impl Default for PortmapState {
    fn default() -> Self {
        Self {
            started: false,
            done: false,
            retry_sent: false,
            retry_at: None,
            deadline: None,
            upnp_end_at: None,
            nat_pmp: Obs::unknown(crate::report::UnknownReason::Pending),
            pcp: Obs::unknown(crate::report::UnknownReason::Pending),
            igd_locations: Vec::new(),
            upnp_control_url_seen: false,
            upnp_http_tried: false,
            external_v4: Vec::new(),
            pcp_address_mismatch: false,
            not_gateway_drops: 0,
            http_desc_id: None,
            http_desc_location: None,
            http_soap_id: None,
            http_service_type: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct V6State {
    pub local_addr: Option<SocketAddr>,
    pub bind_failed: bool,
    pub mapped: Option<(SocketAddr, ObsId)>,
    pub tried_servers: usize,
    pub timeouts: u8,
    pub done: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Nat64State {
    pub queried: bool,
    pub done: bool,
    pub status: Option<Nat64Status>,
    pub resolver_is_doh: bool,
    pub failed: bool,
    pub obs_id: Option<ObsId>,
}

#[derive(Clone, Debug)]
pub struct LifeRung {
    pub sock: SockId,
    pub idle_dur: Duration,
    pub mapped: Option<SocketAddr>,
    pub mapped_at: Option<Tick>,
    pub probe_at: Option<Tick>,
    pub probed: bool,
    pub outcome: Option<RungOutcome>,
    pub is_control: bool,
    pub is_fine: bool,
}

#[derive(Clone, Debug, Default)]
pub struct LifetimeState {
    pub enabled: bool,
    pub started: bool,
    pub done: bool,
    pub server: Option<SocketAddr>,
    pub y_bound: bool,
    pub y_mapped: Option<SocketAddr>,
    pub rungs: Vec<LifeRung>,
    pub control_outcome: Option<RungOutcome>,
    pub fine_launched: bool,
    pub obs_ids: Vec<ObsId>,
}

/// Snapshot of validated STUN replies for ALG aggregation and server flags.
#[derive(Clone, Debug)]
pub struct RecordedValid {
    pub validated: Validated,
    pub obs_id: ObsId,
}

/// Deferred STUN send waiting on a token-bucket slot or a scheduled start time.
#[derive(Clone, Debug)]
pub struct QueuedSend {
    pub at: Tick,
    pub tag: super::inflight::TxTag,
    pub sock: SockId,
    pub dest: SocketAddr,
    pub expected_src: SocketAddr,
    pub change_flags: Option<u8>,
    pub response_port: Option<u16>,
}

/// Dedup server record helper entry.
#[derive(Clone, Debug)]
pub struct ServerNote {
    pub name: String,
    pub ip: IpAddr,
    pub flags: Vec<crate::report::ServerFlag>,
}
