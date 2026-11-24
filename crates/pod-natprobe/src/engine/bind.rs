//! Socket bind lifecycle and allocation scheduling helpers.

use std::net::{IpAddr, SocketAddr};

use super::inflight::TxTag;
use super::io::{
    ProbeEvent, SOCK_ALLOC_BASE, SOCK_DEEP_GAP, SOCK_F, SOCK_LIFE_RUNG_BASE, SOCK_LIFE_Y, SOCK_M, SOCK_M6, SOCK_P,
};
use super::machine::ProbeMachine;
use crate::report::{Family, Profile, QuestionId, SockId};
use crate::wire::{canonical_addr, canonical_ip};

impl ProbeMachine {
    pub(crate) fn on_bound(&mut self, sock: SockId, local: SocketAddr) {
        if sock == SOCK_M {
            self.map.local_addr = Some(local);
            self.launch_map_primaries();
            return;
        }
        if sock == SOCK_F {
            self.filter.local_addr = Some(local);
            if let Some(d) = self.preferred_v4_server() {
                self.filter.tried_ips.push(d.ip());
                self.queue_stun(self.now, TxTag::FilterT1, SOCK_F, d, d, None, None, 0);
            } else {
                self.filter.done = true;
            }
            return;
        }
        if sock == SOCK_M6 {
            self.v6.local_addr = Some(local);
            let addrs: Vec<SocketAddr> =
                self.settings.servers_v6.iter().take(2).map(|s| canonical_addr(s.addr)).collect();
            for (idx, d) in addrs.into_iter().enumerate() {
                self.v6.tried_servers += 1;
                self.queue_stun(
                    self.now.saturating_add((idx as u64) * 25_000),
                    TxTag::V6Binding { server_idx: idx },
                    SOCK_M6,
                    d,
                    d,
                    None,
                    None,
                    0,
                );
            }
            return;
        }
        if sock == SOCK_P {
            self.send_portmap_burst();
            return;
        }
        if Self::is_alloc_sock(sock) {
            self.alloc.pending_binds = self.alloc.pending_binds.saturating_sub(1);
            let idx = usize::from(sock.0 - SOCK_ALLOC_BASE);
            self.alloc.sock_locals.insert(sock, local);
            if let Some(d) = self.preferred_v4_server() {
                self.alloc.dest = Some(d);
                let delay = u64::from(self.settings.alloc_pace_ms) * 1_000 * (idx as u64);
                self.alloc.scheduled += 1;
                self.queue_stun(
                    self.now.saturating_add(delay),
                    TxTag::AllocSample { index: idx },
                    sock,
                    d,
                    d,
                    None,
                    None,
                    0,
                );
            }
            return;
        }
        if sock == SOCK_DEEP_GAP {
            if let Some(d) = self.preferred_v4_server() {
                self.queue_stun(self.now, TxTag::DeepGapSample, SOCK_DEEP_GAP, d, d, None, None, 0);
            }
            return;
        }
        if sock == SOCK_LIFE_Y {
            self.lifetime.y_bound = true;
            if let Some(srv) = self.lifetime.server {
                self.queue_stun(self.now, TxTag::LifeCtrlMap, SOCK_LIFE_Y, srv, srv, None, None, 0);
            }
            return;
        }
        if sock.0 >= SOCK_LIFE_RUNG_BASE {
            let r_idx = usize::from(sock.0 - SOCK_LIFE_RUNG_BASE);
            if let Some(srv) = self.lifetime.server
                && r_idx < self.lifetime.rungs.len()
            {
                self.queue_stun(self.now, TxTag::LifeRungOpen { rung_idx: r_idx }, sock, srv, srv, None, None, 0);
            }
        }
    }

    pub(crate) fn on_bind_failed(&mut self, sock: SockId) {
        if sock == SOCK_M {
            self.map.bind_failed = true;
            self.map.done = true;
            self.maybe_start_hairpin();
            self.maybe_start_lifetime();
        } else if sock == SOCK_F {
            self.filter.bind_failed = true;
            self.filter.done = true;
            self.events.push_back(ProbeEvent::Partial(QuestionId::Filtering));
            self.maybe_finish_alloc();
            self.maybe_start_hairpin();
        } else if sock == SOCK_M6 {
            self.v6.bind_failed = true;
            self.v6.done = true;
        } else if sock == SOCK_P {
            self.portmap.done = true;
        } else if Self::is_alloc_sock(sock) {
            self.alloc.pending_binds = self.alloc.pending_binds.saturating_sub(1);
            self.maybe_finish_alloc();
        } else if sock == SOCK_DEEP_GAP {
            self.alloc.deep_gap_done = true;
            self.maybe_finish_alloc();
        } else if sock == SOCK_LIFE_Y || sock.0 >= SOCK_LIFE_RUNG_BASE {
            self.lifetime.done = true;
            self.events.push_back(ProbeEvent::Partial(QuestionId::Lifetime));
        }
    }

    fn launch_map_primaries(&mut self) {
        let addrs: Vec<SocketAddr> = self.settings.servers_v4.iter().map(|s| canonical_addr(s.addr)).collect();
        let mut seen = Vec::new();
        for (idx, dest) in addrs.into_iter().enumerate() {
            if seen.contains(&dest) {
                continue;
            }
            seen.push(dest);
            self.map.tried_servers += 1;
            let stagger = (idx as u64) * 25_000;
            self.queue_stun(
                self.now.saturating_add(stagger),
                TxTag::MapPrimary { server_idx: idx },
                SOCK_M,
                dest,
                dest,
                None,
                None,
                0,
            );
        }
    }

    pub(crate) fn maybe_finish_alloc(&mut self) {
        if self.alloc.done {
            return;
        }
        let still_running = (1..self.alloc.target_count).any(|i| {
            let tag = TxTag::AllocSample { index: i };
            self.inflight.has_tag(tag) || self.queued.iter().any(|q| q.tag == tag)
        });
        let f_pending = !self.filter.t1_done && !self.filter.bind_failed;
        if still_running || f_pending || self.alloc.pending_binds > 0 {
            return;
        }
        if self.settings.profile == Profile::Deep
            && self.alloc.deep_gap_at.is_none()
            && !self.alloc.deep_gap_done
            && self.alloc.samples_by_idx.len() >= 5
        {
            self.alloc.deep_gap_at = Some(self.now.saturating_add(1_500_000));
            return;
        }
        if self.settings.profile == Profile::Deep && self.alloc.deep_gap_at.is_some() && !self.alloc.deep_gap_done {
            return;
        }
        self.alloc.done = true;
        self.events.push_back(ProbeEvent::Partial(QuestionId::Allocation));
        self.events.push_back(ProbeEvent::Partial(QuestionId::Pooling));
        self.maybe_start_hairpin();
    }

    pub(crate) fn tick_deep_gap(&mut self) {
        if let Some(at) = self.alloc.deep_gap_at
            && self.now >= at
            && !self.alloc.deep_gap_done
            && !self.bound_socks.contains_key(&SOCK_DEEP_GAP)
        {
            self.alloc.deep_gap_at = None;
            self.emit_bind(SOCK_DEEP_GAP, Family::V4, None);
        }
    }

    pub(crate) fn preferred_v4_server(&self) -> Option<SocketAddr> {
        self.settings
            .servers_v4
            .iter()
            .find(|s| s.rfc5780)
            .or_else(|| self.settings.servers_v4.first())
            .map(|s| canonical_addr(s.addr))
    }

    pub(crate) fn next_untried_filter_server(&self) -> Option<SocketAddr> {
        self.settings
            .servers_v4
            .iter()
            .map(|s| canonical_addr(s.addr))
            .find(|d| !self.filter.tried_ips.contains(&d.ip()))
    }

    pub(crate) fn has_distinct_v4_server_ips(&self) -> usize {
        let mut ips: Vec<IpAddr> = Vec::new();
        for s in &self.settings.servers_v4 {
            let ip = canonical_ip(s.addr.ip());
            if !ips.contains(&ip) {
                ips.push(ip);
            }
        }
        ips.len()
    }
}
