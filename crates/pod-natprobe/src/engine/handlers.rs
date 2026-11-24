//! STUN transaction completion handlers for Mapping, Filtering, Allocation and IPv6.

use std::net::SocketAddr;

use pod_turn::stun::{CHANGE_IP, CHANGE_PORT};

use super::inflight::{InflightTx, TxTag};
use super::io::{Output, ProbeEvent, SOCK_F, SOCK_M};
use super::machine::ProbeMachine;
use crate::classify::{AllocSample, Control, Probe};
use crate::report::{ObsId, Profile, QuestionId, ServerFlag, UnknownReason};
use crate::wire::{Topology, Validated};

impl ProbeMachine {
    pub(crate) fn on_tx_ok(&mut self, tx: &InflightTx, v: Validated, obs_id: ObsId, was_lingering: bool) {
        match tx.tag {
            TxTag::MapPrimary { server_idx } => self.handle_map_primary_ok(server_idx, tx.dest, v, obs_id),
            TxTag::MapT2Ap => {
                self.map.m2 = Some(v.mapped());
                self.map.t2_done = true;
                self.map.per_dest.push((tx.dest, v.mapped(), obs_id));
                if let Some(topo) = self.map.topo {
                    if self.map.m1 == self.map.m2 {
                        self.map.t3_done = true;
                        self.start_map_repeat();
                    } else {
                        self.queue_stun(self.now, TxTag::MapT3Aa, SOCK_M, topo.aa, topo.aa, None, None, 0);
                    }
                }
            }
            TxTag::MapT3Aa => {
                self.map.m3 = Some(v.mapped());
                self.map.t3_done = true;
                self.map.per_dest.push((tx.dest, v.mapped(), obs_id));
                self.start_map_repeat();
            }
            TxTag::MapControlRepeat => {
                self.map.m1b = Some(v.mapped());
                self.map.repeat_done = true;
                let dependent = self.map.m1.is_some() && self.map.m2.is_some() && self.map.m1 != self.map.m2;
                if self.settings.profile != Profile::Fast
                    && dependent
                    && let Some(topo) = self.map.topo
                {
                    self.queue_stun(self.now, TxTag::MapPaCrossCheck, SOCK_M, topo.pa, topo.pa, None, None, 0);
                } else {
                    self.map.pa_done = true;
                    self.finish_map();
                }
            }
            TxTag::MapPaCrossCheck => {
                self.map.m_pa = Some(v.mapped());
                self.map.pa_done = true;
                self.finish_map();
            }
            TxTag::V6Binding { .. } => {
                if self.v6.mapped.is_none() {
                    self.v6.mapped = Some((v.mapped(), obs_id));
                    self.v6.done = true;
                    self.events.push_back(ProbeEvent::Partial(QuestionId::UdpV6));
                }
            }
            TxTag::FilterT1 => self.handle_filter_t1_ok(tx.dest, v, obs_id),
            TxTag::FilterT2ChangeBoth => {
                if was_lingering {
                    self.filter.late_upgraded = true;
                }
                self.filter.t2 = Some(Probe::Ok(()));
                self.filter.t2_obs = Some(obs_id);
                self.filter.done = true;
                self.events.push_back(ProbeEvent::Partial(QuestionId::Filtering));
                self.maybe_start_hairpin();
            }
            TxTag::FilterT3ChangePort => {
                if was_lingering {
                    self.filter.late_upgraded = true;
                }
                self.filter.t3 = Some(Probe::Ok(()));
                self.filter.t3_obs = Some(obs_id);
                self.maybe_advance_filter_controls();
            }
            TxTag::FilterCtlOpen { is_06 } => self.handle_filter_ctl_open_ok(is_06, v),
            TxTag::FilterCtlSend { is_06 } => {
                if is_06 {
                    self.filter.c06 = Control::Ok;
                } else {
                    self.filter.c02 = Control::Ok;
                }
                self.advance_filter_controls_after_step(is_06);
            }
            TxTag::AllocSample { index } => {
                let local_port = self.alloc.sock_locals.get(&tx.sock).map_or(0, SocketAddr::port);
                self.alloc.sock_mapped.insert(tx.sock, v.mapped());
                self.alloc
                    .samples_by_idx
                    .insert(index, (AllocSample { local_port, mapped: v.mapped(), at: self.now }, obs_id));
                self.maybe_finish_alloc();
            }
            TxTag::DeepGapSample => {
                self.alloc.deep_gap_mapped = Some(v.mapped());
                self.alloc.deep_gap_done = true;
                self.maybe_finish_alloc();
            }
            TxTag::LifeCtrlMap => {
                self.lifetime.y_mapped = Some(v.mapped());
                self.lifetime.obs_ids.push(obs_id);
                self.bind_lifetime_rungs();
            }
            TxTag::LifeRungOpen { rung_idx } => self.on_lifetime_rung_mapped(rung_idx, v.mapped(), obs_id),
            TxTag::LifeRungProbe { rung_idx } => self.on_lifetime_y_recreated(rung_idx, obs_id),
        }
    }

    fn handle_map_primary_ok(&mut self, server_idx: usize, dest: SocketAddr, v: Validated, obs_id: ObsId) {
        self.map.per_dest.push((dest, v.mapped(), obs_id));
        if self.map.done {
            return;
        }
        if self.map.topo.is_none() && (self.map.m1.is_none() || server_idx == 0) {
            self.map.primary_dest = Some(dest);
            self.map.m1 = Some(v.mapped());
            self.map.m1_obs = Some(obs_id);
        }
        if self.map.topo.is_none() {
            if let Some(other) = v.other_address() {
                if let Some(topo) = Topology::pin(dest, other, self.settings.addr_policy) {
                    self.map.topo = Some(topo);
                    self.map.primary_dest = Some(dest);
                    self.map.m1 = Some(v.mapped());
                    self.map.m1_obs = Some(obs_id);
                    let local_port = self.map.local_addr.map_or(0, |a| a.port());
                    if self.local_v4_endpoint(local_port) == Some(v.mapped()) {
                        self.map.t2_done = true;
                        self.map.t3_done = true;
                        self.map.repeat_done = true;
                        self.map.pa_done = true;
                        self.finish_map();
                        return;
                    }
                    self.queue_stun(self.now, TxTag::MapT2Ap, SOCK_M, topo.ap, topo.ap, None, None, 0);
                    return;
                } else {
                    self.flag_server(dest.ip(), ServerFlag::AlternateNotDistinct);
                }
            } else {
                self.flag_server(dest.ip(), ServerFlag::NoOtherAddress);
            }
        }
        if self.map.topo.is_none()
            && self.map.per_dest.len() + usize::from(self.map.primary_timeouts) >= self.map.tried_servers
        {
            let local_port = self.map.local_addr.map_or(0, |a| a.port());
            if self.local_v4_endpoint(local_port) == self.map.m1 {
                self.map.repeat_done = true;
                self.map.pa_done = true;
                self.finish_map();
            } else {
                self.start_map_repeat();
            }
        }
    }

    fn start_map_repeat(&mut self) {
        if self.map.repeat_done || self.inflight.has_tag(TxTag::MapControlRepeat) {
            return;
        }
        if let Some(dest) = self.map.primary_dest {
            self.queue_stun(self.now, TxTag::MapControlRepeat, SOCK_M, dest, dest, None, None, 0);
        } else {
            self.map.repeat_done = true;
            self.map.pa_done = true;
            self.finish_map();
        }
    }

    fn finish_map(&mut self) {
        if self.map.done {
            return;
        }
        self.map.done = true;
        self.events.push_back(ProbeEvent::Partial(QuestionId::UdpV4));
        self.events.push_back(ProbeEvent::Partial(QuestionId::Mapping));
        self.maybe_start_hairpin();
        self.maybe_start_lifetime();
        if self.needs_online_check() && !self.online_checked {
            self.outputs.push_back(Output::CheckOnline);
        }
    }

    fn handle_filter_t1_ok(&mut self, dest: SocketAddr, v: Validated, obs_id: ObsId) {
        let local_port = self.filter.local_addr.map_or(0, |a| a.port());
        // Record F as allocation sample 0 only when it targets the pinned allocation destination.
        if self.filter.m1_f.is_none() && self.preferred_v4_server() == Some(dest) {
            self.alloc.samples_by_idx.insert(0, (AllocSample { local_port, mapped: v.mapped(), at: self.now }, obs_id));
        }
        self.filter.m1_f = Some(v.mapped());
        self.filter.m1_obs = Some(obs_id);

        let other_opt = v.other_address();
        let topo_opt = other_opt.and_then(|o| Topology::pin(dest, o, self.settings.addr_policy));
        let Some(topo) = topo_opt else {
            let reason = if other_opt.is_none() {
                self.flag_server(dest.ip(), ServerFlag::NoOtherAddress);
                UnknownReason::NoOtherAddress
            } else {
                self.flag_server(dest.ip(), ServerFlag::AlternateNotDistinct);
                UnknownReason::AlternateNotDistinct
            };
            // Try the next distinct server IP in servers_v4 before giving up on RFC 5780 filtering.
            if let Some(next_dest) = self.next_untried_filter_server() {
                self.filter.tried_ips.push(next_dest.ip());
                self.queue_stun(self.now, TxTag::FilterT1, SOCK_F, next_dest, next_dest, None, None, 0);
                return;
            }
            self.filter.t1_done = true;
            self.filter.no_other_reason = Some(reason);
            self.filter.done = true;
            self.maybe_finish_alloc();
            self.events.push_back(ProbeEvent::Partial(QuestionId::Filtering));
            self.maybe_start_hairpin();
            return;
        };
        self.filter.t1_done = true;
        self.maybe_finish_alloc();
        self.filter.topo = Some(topo);
        let exp2 = topo.expected_src(topo.pp, CHANGE_IP | CHANGE_PORT);
        let exp3 = topo.expected_src(topo.pp, CHANGE_PORT);
        self.queue_stun(
            self.now,
            TxTag::FilterT2ChangeBoth,
            SOCK_F,
            topo.pp,
            exp2,
            Some(CHANGE_IP | CHANGE_PORT),
            None,
            0,
        );
        self.queue_stun(self.now, TxTag::FilterT3ChangePort, SOCK_F, topo.pp, exp3, Some(CHANGE_PORT), None, 0);
    }

    pub(crate) fn maybe_advance_filter_controls(&mut self) {
        if self.filter.done || self.filter.ctl_running {
            return;
        }
        let (Some(t2), Some(t3)) = (self.filter.t2, self.filter.t3) else { return };
        if matches!(t2, Probe::Ok(()))
            || matches!(t2, Probe::RefusedChangeRequest)
            || matches!(t3, Probe::RefusedChangeRequest)
            || matches!(t2, Probe::WrongSource)
            || matches!(t3, Probe::WrongSource)
            || self.settings.profile == Profile::Fast
        {
            self.filter.done = true;
            self.events.push_back(ProbeEvent::Partial(QuestionId::Filtering));
            self.maybe_start_hairpin();
            return;
        }
        if let Some(topo) = self.filter.topo {
            self.filter.ctl_running = true;
            self.queue_stun(self.now, TxTag::FilterCtlOpen { is_06: true }, SOCK_F, topo.aa, topo.aa, None, None, 0);
        } else {
            self.filter.done = true;
            self.maybe_start_hairpin();
        }
    }

    fn handle_filter_ctl_open_ok(&mut self, is_06: bool, v: Validated) {
        let Some(topo) = self.filter.topo else {
            self.filter.done = true;
            return;
        };
        if self.filter.m1_f != Some(v.mapped()) {
            if is_06 {
                self.filter.c06 = Control::Invalid;
            } else {
                self.filter.c02 = Control::Invalid;
            }
            self.advance_filter_controls_after_step(is_06);
            return;
        }
        let flags = if is_06 { CHANGE_IP | CHANGE_PORT } else { CHANGE_PORT };
        let exp = topo.expected_src(topo.pp, flags);
        self.queue_stun(self.now, TxTag::FilterCtlSend { is_06 }, SOCK_F, topo.pp, exp, Some(flags), None, 0);
    }

    pub(crate) fn advance_filter_controls_after_step(&mut self, was_06: bool) {
        if self.filter.done {
            return;
        }
        if was_06
            && matches!(self.filter.t3, Some(Probe::Timeout { .. }))
            && let Some(topo) = self.filter.topo
        {
            self.queue_stun(self.now, TxTag::FilterCtlOpen { is_06: false }, SOCK_F, topo.pa, topo.pa, None, None, 0);
            return;
        }
        self.filter.done = true;
        self.events.push_back(ProbeEvent::Partial(QuestionId::Filtering));
        self.maybe_start_hairpin();
    }

    pub(crate) fn on_tx_timeout(&mut self, tx: &InflightTx, obs_id: ObsId, elapsed_ms: u32) {
        match tx.tag {
            TxTag::MapPrimary { .. } => {
                self.map.primary_timeouts += 1;
                if self.map.topo.is_none()
                    && self.map.per_dest.len() + usize::from(self.map.primary_timeouts) >= self.map.tried_servers
                {
                    if self.map.m1.is_some() {
                        self.start_map_repeat();
                    } else {
                        self.finish_map();
                    }
                }
            }
            TxTag::MapT2Ap => {
                self.map.t2_done = true;
                self.map.t3_done = true;
                self.start_map_repeat();
            }
            TxTag::MapT3Aa => {
                self.map.t3_done = true;
                self.start_map_repeat();
            }
            TxTag::MapControlRepeat => {
                self.map.repeat_done = true;
                self.map.pa_done = true;
                self.finish_map();
            }
            TxTag::MapPaCrossCheck => {
                self.map.pa_done = true;
                self.finish_map();
            }
            TxTag::V6Binding { .. } => {
                self.v6.timeouts += 1;
                if usize::from(self.v6.timeouts) >= self.v6.tried_servers {
                    self.v6.done = true;
                    self.events.push_back(ProbeEvent::Partial(QuestionId::UdpV6));
                }
            }
            TxTag::FilterT1 => {
                if let Some(next_dest) = self.next_untried_filter_server() {
                    self.filter.tried_ips.push(next_dest.ip());
                    self.queue_stun(self.now, TxTag::FilterT1, SOCK_F, next_dest, next_dest, None, None, 0);
                    return;
                }
                self.filter.t1_done = true;
                if self.filter.m1_f.is_none() {
                    self.filter.t1_timeout = true;
                } else if self.filter.no_other_reason.is_none() {
                    self.filter.no_other_reason = Some(UnknownReason::NoOtherAddress);
                }
                self.filter.done = true;
                self.maybe_finish_alloc();
                self.events.push_back(ProbeEvent::Partial(QuestionId::Filtering));
                self.maybe_start_hairpin();
            }
            TxTag::FilterT2ChangeBoth => {
                self.filter.t2 = Some(Probe::Timeout { sends: tx.sends, elapsed_ms });
                self.filter.t2_obs = Some(obs_id);
                self.maybe_advance_filter_controls();
            }
            TxTag::FilterT3ChangePort => {
                self.filter.t3 = Some(Probe::Timeout { sends: tx.sends, elapsed_ms });
                self.filter.t3_obs = Some(obs_id);
                self.maybe_advance_filter_controls();
            }
            TxTag::FilterCtlOpen { is_06 } | TxTag::FilterCtlSend { is_06 } => {
                if is_06 {
                    self.filter.c06 = Control::Failed;
                } else {
                    self.filter.c02 = Control::Failed;
                }
                self.advance_filter_controls_after_step(is_06);
            }
            TxTag::AllocSample { .. } => self.maybe_finish_alloc(),
            TxTag::DeepGapSample => {
                self.alloc.deep_gap_done = true;
                self.maybe_finish_alloc();
            }
            TxTag::LifeCtrlMap => self.lifetime.done = true,
            TxTag::LifeRungOpen { rung_idx } => self.on_lifetime_rung_timeout(rung_idx),
            TxTag::LifeRungProbe { rung_idx } => self.on_lifetime_probe_timeout(rung_idx),
        }
    }

    pub(crate) fn on_tx_error(&mut self, tx: &InflightTx, code: u16, obs_id: ObsId) {
        if tx.change_flags.is_some() && (code == 420 || code == 400) {
            self.flag_server(tx.dest.ip(), ServerFlag::LacksChangeRequest);
            if self.filter.done {
                return;
            }
            match tx.tag {
                TxTag::FilterT2ChangeBoth => {
                    self.filter.t2 = Some(Probe::RefusedChangeRequest);
                    self.filter.t2_obs = Some(obs_id);
                    self.maybe_advance_filter_controls();
                    return;
                }
                TxTag::FilterT3ChangePort => {
                    self.filter.t3 = Some(Probe::RefusedChangeRequest);
                    self.filter.t3_obs = Some(obs_id);
                    self.maybe_advance_filter_controls();
                    return;
                }
                _ => {}
            }
        }
        self.on_tx_timeout(tx, obs_id, 0);
    }

    pub(crate) fn on_tx_wrong_source(&mut self, tx: &InflightTx, _from: SocketAddr, obs_id: ObsId) {
        if self.filter.done && matches!(tx.tag, TxTag::FilterT2ChangeBoth | TxTag::FilterT3ChangePort) {
            return;
        }
        match tx.tag {
            TxTag::FilterT2ChangeBoth => {
                self.filter.t2 = Some(Probe::WrongSource);
                self.filter.t2_obs = Some(obs_id);
                self.maybe_advance_filter_controls();
            }
            TxTag::FilterT3ChangePort => {
                self.filter.t3 = Some(Probe::WrongSource);
                self.filter.t3_obs = Some(obs_id);
                self.maybe_advance_filter_controls();
            }
            _ => self.on_tx_timeout(tx, obs_id, 0),
        }
    }

    pub(crate) fn on_tx_fault(&mut self, tx: &InflightTx) {
        let obs_id = ObsId(self.next_obs_id.saturating_sub(1));
        self.on_tx_timeout(tx, obs_id, 0);
    }
}
