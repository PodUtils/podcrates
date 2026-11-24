//! STUN send queue draining, retransmission timer firing and inbound datagram demultiplexing.

use std::net::SocketAddr;

use pod_turn::stun::TransactionId;

use super::inflight::{InflightTx, TxTag};
use super::io::{Output, SOCK_LIFE_RUNG_BASE, SOCK_P, decode_hairpin};
use super::ladder::LadderSchedule;
use super::machine::ProbeMachine;
use super::state::RecordedValid;
use crate::report::{ObservationKind, Profile, ServerFlag, SockId};
use crate::wire::request::{change_request, plain_binding, response_port_request};
use crate::wire::{ValidateOutcome, validate_response};

impl ProbeMachine {
    pub(crate) fn flush_queued(&mut self) {
        let now = self.now;
        let mut remaining = Vec::new();
        let pending = std::mem::take(&mut self.queued);
        for mut item in pending {
            if item.at > now {
                remaining.push(item);
                continue;
            }
            if !self.bucket.try_acquire(now, item.dest.ip()) {
                item.at = if self.bucket.is_inflight_full(item.dest.ip()) {
                    self.inflight
                        .earliest_deadline_for_ip(item.dest.ip())
                        .unwrap_or_else(|| now.saturating_add(50_000))
                        .max(now.saturating_add(10_000))
                } else {
                    self.bucket.next_token_tick(now).max(now.saturating_add(1_000))
                };
                remaining.push(item);
                continue;
            }
            let Ok(tid) = self.next_tid() else {
                self.bucket.release(item.dest.ip());
                self.cancelled = true;
                self.finish_now();
                return;
            };
            let Some(bytes) = self.encode_stun_bytes(tid, item.dest, item.change_flags, item.response_port) else {
                self.bucket.release(item.dest.ip());
                continue;
            };
            let id = self.inflight.alloc_id();
            let srtt = self.srtt.get(item.dest.ip());
            let schedule = LadderSchedule::build(self.settings.ladder, srtt, self.settings.jitter_seed, id);
            let next_action = if schedule.offsets_us.len() > 1 {
                now.saturating_add(schedule.offsets_us[1])
            } else {
                now.saturating_add(schedule.end_us)
            };
            let tx = InflightTx {
                id,
                tag: item.tag,
                sock: item.sock,
                dest: item.dest,
                expected_src: item.expected_src,
                change_flags: item.change_flags,
                response_port: item.response_port,
                software: self.settings.software.clone(),
                started_at: now,
                schedule,
                sends: 1,
                tids: vec![(tid, now)],
                server_5xx_retries: 0,
                lingering: false,
                bucket_released: false,
                linger_until: 0,
                next_action,
            };
            self.inflight.insert(tx);
            self.outputs.push_back(Output::Send { sock: item.sock, dest: item.dest, bytes });
        }
        self.queued = remaining;
    }

    pub(crate) fn encode_stun_bytes(
        &self,
        tid: TransactionId,
        dest: SocketAddr,
        change_flags: Option<u8>,
        response_port: Option<u16>,
    ) -> Option<Vec<u8>> {
        let sw = self.settings.software.as_deref();
        if let Some(flags) = change_flags {
            let topo = self.filter.topo.as_ref()?;
            let (bytes, _) = change_request(tid, topo, dest, flags, sw).ok()?;
            return Some(bytes);
        }
        if let Some(port) = response_port {
            return response_port_request(tid, port, sw).ok();
        }
        plain_binding(tid, sw).ok()
    }

    pub(crate) fn process_due_inflight(&mut self) {
        let due = self.inflight.due_ids(self.now);
        for id in due {
            let Some(tx) = self.inflight.get_mut(id).cloned() else { continue };
            if tx.lingering {
                self.inflight.remove(id);
                if !tx.bucket_released {
                    self.bucket.release(tx.dest.ip());
                }
                continue;
            }
            let next_idx = usize::from(tx.sends);
            let elapsed_us = self.now.saturating_sub(tx.started_at);
            if next_idx < tx.schedule.offsets_us.len() && elapsed_us < tx.schedule.end_us {
                let tid = if tx.schedule.reuse_tid {
                    tx.tids[0].0
                } else if let Ok(t) = self.next_tid() {
                    t
                } else {
                    self.cancelled = true;
                    self.finish_now();
                    return;
                };
                if let Some(bytes) = self.encode_stun_bytes(tid, tx.dest, tx.change_flags, tx.response_port) {
                    let next_action = if next_idx + 1 < tx.schedule.offsets_us.len() {
                        tx.started_at.saturating_add(tx.schedule.offsets_us[next_idx + 1])
                    } else {
                        tx.started_at.saturating_add(tx.schedule.end_us)
                    };
                    self.inflight.add_send(id, tid, self.now, next_action);
                    self.outputs.push_back(Output::Send { sock: tx.sock, dest: tx.dest, bytes });
                    continue;
                }
            }
            let elapsed_ms = (elapsed_us / 1_000).min(u64::from(u32::MAX)) as u32;
            let obs_id =
                self.record_obs(tx.sock, ObservationKind::Timeout { dest: tx.dest, sends: tx.sends, elapsed_ms });
            let keep_lingering = matches!(tx.tag, TxTag::FilterT2ChangeBoth | TxTag::FilterT3ChangePort)
                && self.settings.profile != Profile::Fast;
            if keep_lingering {
                if !tx.bucket_released {
                    self.bucket.release(tx.dest.ip());
                }
                if let Some(entry) = self.inflight.get_mut(id) {
                    entry.lingering = true;
                    entry.bucket_released = true;
                    entry.linger_until = self.deadline;
                    entry.next_action = self.deadline;
                }
            } else {
                self.inflight.remove(id);
                if !tx.bucket_released {
                    self.bucket.release(tx.dest.ip());
                }
            }
            self.on_tx_timeout(&tx, obs_id, elapsed_ms);
        }
    }

    pub(crate) fn on_datagram(&mut self, sock: SockId, from: SocketAddr, bytes: &[u8]) {
        if sock == SOCK_P {
            self.on_portmap_datagram(from, bytes);
            return;
        }
        if let Some(txid) = decode_hairpin(bytes) {
            self.on_hairpin_datagram(sock, from, txid);
            return;
        }
        if sock.0 >= SOCK_LIFE_RUNG_BASE && self.on_lifetime_rung_datagram(sock, from, bytes) {
            return;
        }
        if bytes.len() < 20 {
            self.record_dropped(sock, "short-datagram");
            return;
        }
        let mut tid = [0u8; 12];
        tid.copy_from_slice(&bytes[8..20]);
        let Some((tx, sent_at)) = self.inflight.find_by_tid(sock, &tid).map(|(t, s)| (t.clone(), s)) else {
            self.record_dropped(sock, "unknown-tid");
            return;
        };
        match validate_response(bytes, from, tx.expected_src, &tid) {
            ValidateOutcome::Dropped(why) => {
                self.record_dropped(sock, why);
            }
            ValidateOutcome::ProtocolFault(why) => {
                let _ = self.record_obs(sock, ObservationKind::Dropped { reason: why.to_string() });
                self.inflight.remove(tx.id);
                if !tx.bucket_released {
                    self.bucket.release(tx.dest.ip());
                }
                self.on_tx_fault(&tx);
            }
            ValidateOutcome::Error { code, alternate_server, .. } => {
                let obs_id = self.record_obs(sock, ObservationKind::Error { dest: tx.dest, code });
                if (300..400).contains(&code) || alternate_server.is_some() {
                    self.flag_server(tx.dest.ip(), ServerFlag::Redirects);
                }
                if (500..600).contains(&code) && tx.server_5xx_retries < 2 {
                    if let Some(entry) = self.inflight.get_mut(tx.id) {
                        entry.server_5xx_retries += 1;
                    }
                    return;
                }
                self.inflight.remove(tx.id);
                if !tx.bucket_released {
                    self.bucket.release(tx.dest.ip());
                }
                self.on_tx_error(&tx, code, obs_id);
            }
            ValidateOutcome::WrongSource { from, .. } => {
                let obs_id = self.record_obs(sock, ObservationKind::WrongSource { dest: tx.dest, from });
                if tx.change_flags.is_some() {
                    self.flag_server(tx.dest.ip(), ServerFlag::IgnoresChangeRequest);
                }
                self.inflight.remove(tx.id);
                if !tx.bucket_released {
                    self.bucket.release(tx.dest.ip());
                }
                self.on_tx_wrong_source(&tx, from, obs_id);
            }
            ValidateOutcome::Ok(v) => {
                let rtt_us = self.now.saturating_sub(sent_at).min(u64::from(u32::MAX)) as u32;
                let obs_id =
                    self.record_obs(sock, ObservationKind::Mapped { dest: tx.dest, mapped: v.mapped(), rtt_us });
                if !tx.schedule.reuse_tid || tx.sends == 1 {
                    self.srtt.record(tx.dest.ip(), u64::from(rtt_us));
                }
                if v.xor_mapped().is_some() && v.plain_mapped().is_none() {
                    self.flag_server(tx.dest.ip(), ServerFlag::SendsXorOnly);
                }
                if v.flags().contains(&crate::report::Flag::OriginNeSrc) {
                    self.flag_server(tx.dest.ip(), ServerFlag::OriginNeSrc);
                }
                self.valid_responses.push(RecordedValid { validated: v.clone(), obs_id });
                let was_lingering = tx.lingering;
                self.inflight.remove(tx.id);
                if !tx.bucket_released {
                    self.bucket.release(tx.dest.ip());
                }
                self.on_tx_ok(&tx, v, obs_id, was_lingering);
            }
        }
    }
}
