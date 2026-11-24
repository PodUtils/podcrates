//! Core sans-IO NAT probe state machine (`poll_timeout`, `handle_timeout`, `poll_output`,
//! `handle_input`, `poll_event`). Pure std, no tokio, no sockets, no ambient clock.

use std::collections::{BTreeMap, VecDeque};
use std::net::{IpAddr, SocketAddr};

use pod_core::entropy::Entropy;
use pod_turn::stun::TransactionId;

use super::inflight::{InflightTable, TxTag};
use super::io::{
    Input, Output, ProbeEvent, SOCK_ALLOC_BASE, SOCK_DEEP_GAP, SOCK_F, SOCK_M, SOCK_M6, SOCK_P, encode_hairpin,
};
use super::ladder::{SrttMap, TokenBucket};
use super::state::{
    AllocState, FilterState, HairpinState, LifetimeState, MapState, Nat64State, PortmapState, QueuedSend,
    RecordedValid, ServerNote, V6State,
};
use crate::error::NatError;
use crate::report::{
    Family, MAX_DROPPED_OBS, ObsId, Observation, ObservationKind, Profile, QuestionId, ServerFlag, SockId, Tick,
    UnknownReason,
};
use crate::seams::{BindSpec, LocalEnv};
use crate::settings::Settings;
use crate::wire::addr::{AddrClass, canonical_addr, canonical_ip, classify_ip};

/// Deterministic sans-IO NAT probe engine.
pub struct ProbeMachine {
    pub(crate) settings: Settings,
    pub(crate) local_env: LocalEnv,
    pub(crate) entropy: Box<dyn Entropy + Send>,
    pub(crate) started_at: Tick,
    pub(crate) now: Tick,
    pub(crate) deadline: Tick,
    pub(crate) finished: bool,
    pub(crate) cancelled: bool,
    pub(crate) next_obs_id: u32,
    pub(crate) next_http_id: u32,
    pub(crate) observations: Vec<Observation>,
    pub(crate) valid_responses: Vec<RecordedValid>,
    pub(crate) server_notes: BTreeMap<IpAddr, ServerNote>,
    pub(crate) outputs: VecDeque<Output>,
    pub(crate) events: VecDeque<ProbeEvent>,
    pub(crate) inflight: InflightTable,
    pub(crate) queued: Vec<QueuedSend>,
    pub(crate) srtt: SrttMap,
    pub(crate) bucket: TokenBucket,
    pub(crate) map: MapState,
    pub(crate) filter: FilterState,
    pub(crate) alloc: AllocState,
    pub(crate) hairpin: HairpinState,
    pub(crate) portmap: PortmapState,
    pub(crate) v6: V6State,
    pub(crate) nat64: Nat64State,
    pub(crate) lifetime: LifetimeState,
    pub(crate) online_checked: bool,
    pub(crate) online_ok: Option<bool>,
    pub(crate) bound_socks: BTreeMap<SockId, SocketAddr>,
    pub(crate) dropped_recorded: u32,
    pub(crate) dropped_unrecorded: BTreeMap<String, u64>,
}

impl ProbeMachine {
    /// Construct a new machine. Call [`Self::start`] at tick `0` to begin emitting outputs.
    pub fn new(settings: Settings, local_env: LocalEnv, entropy: Box<dyn Entropy + Send>) -> Result<Self, NatError> {
        settings.validate().map_err(NatError::InvalidSettings)?;
        let deadline = u64::from(settings.max_duration_ms) * 1_000;
        let bucket = TokenBucket::new(settings.bucket);
        let online_ok = settings.corroborated_online;
        let mut server_notes = BTreeMap::new();
        for s in settings.servers_v4.iter().chain(settings.servers_v6.iter()) {
            let ip = canonical_ip(s.addr.ip());
            server_notes.entry(ip).or_insert_with(|| ServerNote { name: s.name.clone(), ip, flags: Vec::new() });
        }
        let alloc_target = settings.alloc_samples.max(1);
        let life_enabled = settings.lifetime.is_some();
        Ok(Self {
            settings,
            local_env,
            entropy,
            started_at: 0,
            now: 0,
            deadline,
            finished: false,
            cancelled: false,
            next_obs_id: 0,
            next_http_id: 1,
            observations: Vec::new(),
            valid_responses: Vec::new(),
            server_notes,
            outputs: VecDeque::new(),
            events: VecDeque::new(),
            inflight: InflightTable::default(),
            queued: Vec::new(),
            srtt: SrttMap::default(),
            bucket,
            map: MapState::default(),
            filter: FilterState::default(),
            alloc: AllocState { target_count: alloc_target, ..AllocState::default() },
            hairpin: HairpinState::default(),
            portmap: PortmapState::default(),
            v6: V6State::default(),
            nat64: Nat64State::default(),
            lifetime: LifetimeState { enabled: life_enabled, ..LifetimeState::default() },
            online_checked: online_ok.is_some(),
            online_ok,
            bound_socks: BTreeMap::new(),
            dropped_recorded: 0,
            dropped_unrecorded: BTreeMap::new(),
        })
    }

    /// Start the probe run at `now`.
    pub fn start(&mut self, now: Tick) {
        self.started_at = now;
        self.now = now;
        self.deadline = now.saturating_add(u64::from(self.settings.max_duration_ms) * 1_000);
        let _ = self.record_obs(SOCK_M, ObservationKind::LocalFact { note: "local-env-captured".to_string() });
        self.events.push_back(ProbeEvent::Partial(QuestionId::Local));

        if !self.settings.servers_v4.is_empty() {
            self.emit_bind(SOCK_M, Family::V4, None);
            self.emit_bind(SOCK_F, Family::V4, None);
            self.alloc.pending_binds = self.alloc.target_count.saturating_sub(1);
            for i in 1..self.alloc.target_count {
                self.emit_bind(SockId(SOCK_ALLOC_BASE + i as u16), Family::V4, None);
            }
        } else {
            self.map.done = true;
            self.filter.done = true;
            self.alloc.done = true;
            self.hairpin.done = true;
        }

        if self.settings.run_ipv6
            && self.local_env.v6_stack_available
            && !self.local_env.v6_no_route
            && !self.settings.servers_v6.is_empty()
        {
            self.emit_bind(SOCK_M6, Family::V6, None);
        } else {
            self.v6.done = true;
        }

        if self.settings.run_nat64 {
            self.nat64.queried = true;
            self.outputs.push_back(Output::QueryDns64 { domain: crate::wire::nat64::IPV4ONLY_ARPA.to_string() });
        } else {
            self.nat64.done = true;
        }

        self.start_portmap_if_applicable();
        self.check_completion();
    }

    /// Current logical virtual tick observed by the machine.
    pub fn current_tick(&self) -> Tick {
        self.now
    }

    /// Next tick at which [`Self::handle_timeout`] must be called.
    pub fn poll_timeout(&self) -> Option<Tick> {
        if self.finished {
            return None;
        }
        let mut next = self.deadline;
        if let Some(t) = self.inflight.next_deadline() {
            next = next.min(t);
        }
        for q in &self.queued {
            next = next.min(q.at);
        }
        for t in [
            self.portmap.retry_at,
            self.portmap.deadline,
            self.portmap.upnp_end_at,
            self.hairpin.next_round_at,
            self.alloc.deep_gap_at,
        ]
        .into_iter()
        .flatten()
        {
            next = next.min(t);
        }
        for r in &self.lifetime.rungs {
            if !r.probed
                && let Some(t) = r.probe_at
            {
                next = next.min(t);
            }
        }
        Some(next)
    }

    /// Pop the next non-blocking I/O output, if any.
    pub fn poll_output(&mut self) -> Option<Output> {
        self.outputs.pop_front()
    }

    /// Pop the next partial or final event, if any.
    pub fn poll_event(&mut self) -> Option<ProbeEvent> {
        self.events.pop_front()
    }

    /// Advance virtual time to `now` and fire all due timers, retransmits and queued sends.
    pub fn handle_timeout(&mut self, now: Tick) {
        if self.finished {
            return;
        }
        self.now = now.max(self.now);
        self.flush_queued();
        self.process_due_inflight();
        self.tick_portmap();
        self.tick_hairpin();
        self.tick_deep_gap();
        self.tick_lifetime();
        self.flush_queued();
        if self.now >= self.deadline {
            self.finish_now();
        } else {
            self.check_completion();
        }
    }

    /// Feed an external I/O result at `now`.
    pub fn handle_input(&mut self, now: Tick, input: Input) {
        if self.finished {
            return;
        }
        self.now = now.max(self.now);
        match input {
            Input::Cancel => {
                self.cancelled = true;
                self.finish_now();
                return;
            }
            Input::Bound { sock, local } => {
                let local = canonical_addr(local);
                self.bound_socks.insert(sock, local);
                self.on_bound(sock, local);
            }
            Input::BindFailed { sock, reason } => {
                let _ = self.record_obs(sock, ObservationKind::Dropped { reason });
                self.on_bind_failed(sock);
            }
            Input::Datagram { sock, from, bytes } => {
                let from = canonical_addr(from);
                self.on_datagram(sock, from, &bytes);
            }
            Input::Dns64Answer { aaaa, has_a, resolver_is_doh, failed } => {
                self.on_dns64_answer(aaaa, has_a, resolver_is_doh, failed);
            }
            Input::HttpReply { id, body } => self.on_http_reply(id, body),
            Input::OnlineResult { ok } => {
                self.online_ok = Some(ok);
                self.online_checked = true;
            }
        }
        self.flush_queued();
        self.check_completion();
    }

    pub(crate) fn emit_bind(&mut self, sock: SockId, family: Family, local_ip: Option<IpAddr>) {
        self.outputs.push_back(Output::Bind { sock, spec: BindSpec { family, local_ip, local_port: None } });
    }

    pub(crate) fn record_obs(&mut self, sock: SockId, kind: ObservationKind) -> ObsId {
        let id = ObsId(self.next_obs_id);
        self.next_obs_id = self.next_obs_id.saturating_add(1);
        self.observations.push(Observation { id, at: self.now, socket: sock, kind });
        id
    }

    /// Record a dropped datagram: an observation for each of the first [`MAX_DROPPED_OBS`],
    /// then a count by reason. A sender of stray datagrams cannot grow the report.
    pub(crate) fn record_dropped(&mut self, sock: SockId, reason: &str) {
        if self.dropped_recorded < MAX_DROPPED_OBS {
            self.dropped_recorded += 1;
            let _ = self.record_obs(sock, ObservationKind::Dropped { reason: reason.to_string() });
        } else {
            let count = self.dropped_unrecorded.entry(reason.to_string()).or_insert(0);
            *count = count.saturating_add(1);
        }
    }

    pub(crate) fn flag_server(&mut self, ip: IpAddr, flag: ServerFlag) {
        let ip = canonical_ip(ip);
        let note =
            self.server_notes.entry(ip).or_insert_with(|| ServerNote { name: ip.to_string(), ip, flags: Vec::new() });
        if !note.flags.contains(&flag) {
            note.flags.push(flag);
        }
    }

    pub(crate) fn next_tid(&mut self) -> Result<TransactionId, NatError> {
        let mut tid = [0u8; 12];
        self.entropy.fill(&mut tid).map_err(|_| NatError::NoRandom)?;
        Ok(tid)
    }

    pub(crate) fn next_txid16(&mut self) -> Result<[u8; 16], NatError> {
        let mut id = [0u8; 16];
        self.entropy.fill(&mut id).map_err(|_| NatError::NoRandom)?;
        Ok(id)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn queue_stun(
        &mut self,
        at: Tick,
        tag: TxTag,
        sock: SockId,
        dest: SocketAddr,
        expected_src: SocketAddr,
        change_flags: Option<u8>,
        response_port: Option<u16>,
        _linger_us: u64,
    ) {
        self.queued.push(QueuedSend {
            at,
            tag,
            sock,
            dest: canonical_addr(dest),
            expected_src: canonical_addr(expected_src),
            change_flags,
            response_port,
        });
    }

    pub(crate) fn start_portmap_if_applicable(&mut self) {
        if !self.settings.run_portmap {
            self.portmap.done = true;
            return;
        }
        let Some(gw) = self.local_env.gateway_v4 else {
            self.portmap.nat_pmp = crate::report::Obs::unknown(UnknownReason::NoGateway);
            self.portmap.pcp = crate::report::Obs::unknown(UnknownReason::NoGateway);
            self.portmap.done = true;
            return;
        };
        if gw.is_p2p {
            self.portmap.nat_pmp = crate::report::Obs::unknown(UnknownReason::GatewayRange);
            self.portmap.pcp = crate::report::Obs::unknown(UnknownReason::GatewayRange);
            self.portmap.done = true;
            return;
        }
        let local_ip = self.local_env.route_source_v4.map(IpAddr::V4);
        let cls = self.local_env.route_source_v4.map(|ip| classify_ip(IpAddr::V4(ip), self.settings.addr_policy));
        if matches!(cls, Some(AddrClass::Global)) {
            self.portmap.nat_pmp = crate::report::Obs::NotApplicable(crate::report::NaReason::PublicLocalAddress);
        }
        self.emit_bind(SOCK_P, Family::V4, local_ip);
    }

    pub(crate) fn check_completion(&mut self) {
        if self.finished {
            return;
        }
        let all_done = self.map.done
            && self.filter.done
            && self.alloc.done
            && self.hairpin.done
            && self.portmap.done
            && self.v6.done
            && self.nat64.done
            && (!self.lifetime.enabled || self.lifetime.done)
            && (!self.needs_online_check() || self.online_checked);
        if all_done {
            self.finish_now();
        }
    }

    pub(crate) fn needs_online_check(&self) -> bool {
        self.settings.profile != Profile::Fast
            && self.map.done
            && self.map.m1.is_none()
            && self.has_distinct_v4_server_ips() >= 2
            && self.map.primary_timeouts >= 2
    }

    pub(crate) fn finish_now(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        for &sock in self.bound_socks.keys() {
            self.outputs.push_back(Output::Close { sock });
        }
        let report = super::assemble::assemble_report(self);
        self.events.push_back(ProbeEvent::Final(Box::new(report)));
    }

    pub(crate) fn send_hairpin_round(&mut self) {
        let (Some((s1, _, m1)), Some((s2, _, m2))) = (self.hairpin.s1, self.hairpin.s2) else {
            self.hairpin.done = true;
            return;
        };
        if let Ok(tx12) = self.next_txid16() {
            self.hairpin.sent_txids_12.push(tx12);
            self.outputs.push_back(Output::Send { sock: s1, dest: m2, bytes: encode_hairpin(&tx12).to_vec() });
        }
        if self.hairpin.punch_used
            && let Ok(tx21) = self.next_txid16()
        {
            self.hairpin.sent_txids_21.push(tx21);
            self.outputs.push_back(Output::Send { sock: s2, dest: m1, bytes: encode_hairpin(&tx21).to_vec() });
        }
        self.hairpin.rounds_sent += 1;
        self.hairpin.next_round_at = match self.hairpin.rounds_sent {
            1 => Some(self.now.saturating_add(250_000)),
            2 => Some(self.now.saturating_add(500_000)),
            _ => Some(self.now.saturating_add(300_000)),
        };
    }

    pub(crate) fn local_v4_endpoint(&self, port: u16) -> Option<SocketAddr> {
        self.local_env.route_source_v4.map(|ip| SocketAddr::new(IpAddr::V4(ip), port))
    }

    pub(crate) fn is_alloc_sock(sock: SockId) -> bool {
        sock.0 >= SOCK_ALLOC_BASE && sock.0 < SOCK_DEEP_GAP.0
    }
}
