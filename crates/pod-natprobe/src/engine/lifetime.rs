//! Deep-mode UDP mapping lifetime ladder sub-engine (RFC 5780 section 4.6).

use std::net::SocketAddr;
use std::time::Duration;

use pod_turn::stun::decode;

use super::inflight::TxTag;
use super::io::{ProbeEvent, SOCK_LIFE_RUNG_BASE, SOCK_LIFE_Y};
use super::machine::ProbeMachine;
use super::state::LifeRung;
use crate::classify::{RungOutcome, fine_rungs};
use crate::report::{Family, ObsId, ObservationKind, QuestionId, SockId};
use crate::wire::{ValidateOutcome, canonical_addr, validate_response};

impl ProbeMachine {
    pub(crate) fn maybe_start_lifetime(&mut self) {
        if !self.lifetime.enabled || self.lifetime.started || self.lifetime.done || !self.map.done {
            return;
        }
        let Some(srv) = self.map.primary_dest else {
            self.lifetime.done = true;
            return;
        };
        if let (Some(m1), Some(local_m)) = (self.map.m1, self.map.local_addr)
            && self.local_v4_endpoint(local_m.port()) == Some(m1)
        {
            self.lifetime.done = true;
            return;
        }
        self.lifetime.started = true;
        self.lifetime.server = Some(srv);
        self.emit_bind(SOCK_LIFE_Y, Family::V4, None);
    }

    pub(crate) fn bind_lifetime_rungs(&mut self) {
        let Some(cfg) = self.settings.lifetime.clone() else {
            self.lifetime.done = true;
            return;
        };
        let mut rungs = vec![LifeRung {
            sock: SockId(SOCK_LIFE_RUNG_BASE),
            idle_dur: Duration::from_millis(200),
            mapped: None,
            mapped_at: None,
            probe_at: None,
            probed: false,
            outcome: None,
            is_control: true,
            is_fine: false,
        }];
        for (i, &secs) in cfg.coarse_rungs_secs.iter().enumerate() {
            rungs.push(LifeRung {
                sock: SockId(SOCK_LIFE_RUNG_BASE + 1 + i as u16),
                idle_dur: Duration::from_secs(secs),
                mapped: None,
                mapped_at: None,
                probe_at: None,
                probed: false,
                outcome: None,
                is_control: false,
                is_fine: false,
            });
        }
        let socks: Vec<SockId> = rungs.iter().map(|r| r.sock).collect();
        self.lifetime.rungs = rungs;
        for s in socks {
            self.emit_bind(s, Family::V4, None);
        }
    }

    pub(crate) fn on_lifetime_rung_mapped(&mut self, rung_idx: usize, mapped: SocketAddr, obs_id: ObsId) {
        self.lifetime.obs_ids.push(obs_id);
        if let Some(r) = self.lifetime.rungs.get_mut(rung_idx) {
            r.mapped = Some(mapped);
            r.mapped_at = Some(self.now);
            let wait_us = (r.idle_dur.as_micros().min(u128::from(u64::MAX))) as u64;
            r.probe_at = Some(self.now.saturating_add(wait_us));
        }
    }

    pub(crate) fn tick_lifetime(&mut self) {
        if !self.lifetime.started || self.lifetime.done {
            return;
        }
        let Some(srv) = self.lifetime.server else { return };
        let Some(y_mapped) = self.lifetime.y_mapped else { return };
        let mut to_probe = Vec::new();
        for (idx, r) in self.lifetime.rungs.iter_mut().enumerate() {
            if !r.probed
                && let Some(at) = r.probe_at
                && self.now >= at
            {
                r.probed = true;
                if let Some(m) = r.mapped {
                    if m.ip() != y_mapped.ip() {
                        r.outcome = Some(RungOutcome::InvalidPooling);
                        if r.is_control {
                            self.lifetime.control_outcome = Some(RungOutcome::InvalidPooling);
                        }
                        self.lifetime.done = true;
                    } else {
                        to_probe.push((idx, m.port()));
                    }
                }
            }
        }
        for (idx, port) in to_probe {
            self.queue_stun(
                self.now,
                TxTag::LifeRungProbe { rung_idx: idx },
                SOCK_LIFE_Y,
                srv,
                srv,
                None,
                Some(port),
                0,
            );
        }
        self.check_lifetime_progress();
    }

    pub(crate) fn on_lifetime_rung_datagram(&mut self, sock: SockId, from: SocketAddr, bytes: &[u8]) -> bool {
        let Some(srv) = self.lifetime.server else { return false };
        if canonical_addr(from) != srv {
            return false;
        }
        let Ok(msg) = decode(bytes) else { return false };
        let rung_idx = usize::from(sock.0 - SOCK_LIFE_RUNG_BASE);
        let matched = self.inflight.find_by_tid(SOCK_LIFE_Y, &msg.id).map(|(t, _)| (t.id, t.dest.ip(), t.tag));
        if let Some((tx_id, dest_ip, tag)) = matched
            && matches!(tag, TxTag::LifeRungProbe { rung_idx: idx } if idx == rung_idx)
        {
            if !matches!(validate_response(bytes, from, srv, &msg.id), ValidateOutcome::Ok(_)) {
                return false;
            }
            self.inflight.remove(tx_id);
            self.bucket.release(dest_ip);
            let obs = self.record_obs(sock, ObservationKind::LocalFact { note: format!("rung-{rung_idx}-alive") });
            self.lifetime.obs_ids.push(obs);
            if let Some(r) = self.lifetime.rungs.get_mut(rung_idx) {
                r.outcome = Some(RungOutcome::Alive);
                if r.is_control {
                    self.lifetime.control_outcome = Some(RungOutcome::Alive);
                }
            }
            self.check_lifetime_progress();
            return true;
        }
        false
    }

    pub(crate) fn on_lifetime_y_recreated(&mut self, rung_idx: usize, obs_id: ObsId) {
        self.lifetime.obs_ids.push(obs_id);
        if let Some(r) = self.lifetime.rungs.get_mut(rung_idx) {
            r.outcome = Some(RungOutcome::ExpiredRecreated);
            if r.is_control {
                self.lifetime.control_outcome = Some(RungOutcome::ExpiredRecreated);
                self.lifetime.done = true;
            }
        }
        self.check_lifetime_progress();
    }

    pub(crate) fn on_lifetime_rung_timeout(&mut self, rung_idx: usize) {
        // Initial open failed before the rung ever mapped: do not forge ExpiredSilent.
        if let Some(r) = self.lifetime.rungs.get_mut(rung_idx) {
            r.probed = true;
            if r.is_control {
                self.lifetime.done = true;
            }
        }
        self.check_lifetime_progress();
    }

    pub(crate) fn on_lifetime_probe_timeout(&mut self, rung_idx: usize) {
        if let Some(r) = self.lifetime.rungs.get_mut(rung_idx) {
            r.outcome = Some(RungOutcome::ExpiredSilent);
            if r.is_control {
                self.lifetime.control_outcome = Some(RungOutcome::ExpiredSilent);
                self.lifetime.done = true;
            }
        }
        self.check_lifetime_progress();
    }

    fn check_lifetime_progress(&mut self) {
        if self.lifetime.done {
            self.events.push_back(ProbeEvent::Partial(QuestionId::Lifetime));
            return;
        }
        if matches!(self.lifetime.control_outcome, Some(o) if o != RungOutcome::Alive) {
            self.lifetime.done = true;
            self.events.push_back(ProbeEvent::Partial(QuestionId::Lifetime));
            return;
        }
        let first_expired = self
            .lifetime
            .rungs
            .iter()
            .filter(|r| !r.is_control && !r.is_fine && r.outcome.is_some_and(RungOutcome::is_expired))
            .map(|r| r.idle_dur)
            .min();
        if let Some(exp_dur) = first_expired {
            for r in &mut self.lifetime.rungs {
                if !r.is_control && !r.is_fine && r.idle_dur > exp_dur && !r.probed {
                    r.probed = true;
                }
            }
        }
        let coarse_done = self
            .lifetime
            .rungs
            .iter()
            .filter(|r| !r.is_fine)
            .all(|r| r.outcome.is_some() || (r.probed && first_expired.is_some_and(|e| r.idle_dur > e)));
        if !coarse_done {
            return;
        }
        let want_fine = self.settings.lifetime.as_ref().is_some_and(|c| c.run_fine_rungs);
        if want_fine && !self.lifetime.fine_launched {
            let low = self
                .lifetime
                .rungs
                .iter()
                .filter(|r| !r.is_control && r.outcome == Some(RungOutcome::Alive))
                .map(|r| r.idle_dur)
                .max();
            if let (Some(lo), Some(hi)) = (low, first_expired)
                && lo < hi
            {
                self.lifetime.fine_launched = true;
                let base_idx = self.lifetime.rungs.len();
                let mut new_socks = Vec::new();
                for (j, d) in fine_rungs(lo, hi).into_iter().enumerate() {
                    let sock = SockId(SOCK_LIFE_RUNG_BASE + (base_idx + j) as u16);
                    new_socks.push(sock);
                    self.lifetime.rungs.push(LifeRung {
                        sock,
                        idle_dur: d,
                        mapped: None,
                        mapped_at: None,
                        probe_at: None,
                        probed: false,
                        outcome: None,
                        is_control: false,
                        is_fine: true,
                    });
                }
                for s in new_socks {
                    self.emit_bind(s, Family::V4, None);
                }
                return;
            }
        }
        if self.lifetime.rungs.iter().filter(|r| r.is_fine).all(|r| r.outcome.is_some()) {
            self.lifetime.done = true;
            self.events.push_back(ProbeEvent::Partial(QuestionId::Lifetime));
        }
    }
}
