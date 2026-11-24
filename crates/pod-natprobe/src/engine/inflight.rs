//! Per-socket in-flight STUN transaction table with retransmission tracking and lingering
//! late-response matching.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;

use pod_turn::stun::TransactionId;

use super::ladder::LadderSchedule;
use crate::report::{SockId, Tick};

/// Logical owner of an in-flight STUN transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TxTag {
    MapPrimary { server_idx: usize },
    MapT2Ap,
    MapT3Aa,
    MapControlRepeat,
    MapPaCrossCheck,
    V6Binding { server_idx: usize },
    FilterT1,
    FilterT2ChangeBoth,
    FilterT3ChangePort,
    FilterCtlOpen { is_06: bool },
    FilterCtlSend { is_06: bool },
    AllocSample { index: usize },
    DeepGapSample,
    LifeCtrlMap,
    LifeRungOpen { rung_idx: usize },
    LifeRungProbe { rung_idx: usize },
}

/// Active or lingering STUN request entry.
#[derive(Clone, Debug)]
pub struct InflightTx {
    pub id: u32,
    pub tag: TxTag,
    pub sock: SockId,
    pub dest: SocketAddr,
    pub expected_src: SocketAddr,
    pub change_flags: Option<u8>,
    pub response_port: Option<u16>,
    pub software: Option<String>,
    pub started_at: Tick,
    pub schedule: LadderSchedule,
    pub sends: u8,
    pub tids: Vec<(TransactionId, Tick)>,
    pub server_5xx_retries: u8,
    pub lingering: bool,
    pub bucket_released: bool,
    pub linger_until: Tick,
    pub next_action: Tick,
}

/// Table tracking all active and lingering STUN transactions keyed by `(SockId, TransactionId)`.
#[derive(Clone, Debug, Default)]
pub struct InflightTable {
    next_id: u32,
    by_id: BTreeMap<u32, InflightTx>,
    by_sock_tid: HashMap<(SockId, TransactionId), u32>,
}

impl InflightTable {
    /// Allocate a new unique probe transaction ID.
    pub fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        id
    }

    /// Insert a newly created transaction (already sent once).
    pub fn insert(&mut self, tx: InflightTx) {
        let id = tx.id;
        let sock = tx.sock;
        for &(tid, _) in &tx.tids {
            self.by_sock_tid.insert((sock, tid), id);
        }
        self.by_id.insert(id, tx);
    }

    /// Register an additional TID sent as a retransmission for transaction `id`.
    pub fn add_send(&mut self, id: u32, tid: TransactionId, sent_at: Tick, next_action: Tick) {
        if let Some(tx) = self.by_id.get_mut(&id) {
            tx.sends = tx.sends.saturating_add(1);
            tx.tids.push((tid, sent_at));
            tx.next_action = next_action;
            self.by_sock_tid.insert((tx.sock, tid), id);
        }
    }

    /// Look up the transaction owning `(sock, tid)` and the send timestamp for `tid`.
    pub fn find_by_tid(&self, sock: SockId, tid: &TransactionId) -> Option<(&InflightTx, Tick)> {
        let id = self.by_sock_tid.get(&(sock, *tid))?;
        let tx = self.by_id.get(id)?;
        let sent_at = tx.tids.iter().rev().find(|(t, _)| t == tid).map_or(tx.started_at, |&(_, ts)| ts);
        Some((tx, sent_at))
    }

    /// Look up a transaction by its logical ID.
    pub fn get_mut(&mut self, id: u32) -> Option<&mut InflightTx> {
        self.by_id.get_mut(&id)
    }

    /// Remove and return a completed or expired transaction, cleaning up all its TID mappings.
    pub fn remove(&mut self, id: u32) -> Option<InflightTx> {
        let tx = self.by_id.remove(&id)?;
        for &(tid, _) in &tx.tids {
            self.by_sock_tid.remove(&(tx.sock, tid));
        }
        Some(tx)
    }

    /// Earliest next action timestamp across all active/lingering transactions.
    pub fn next_deadline(&self) -> Option<Tick> {
        self.by_id.values().map(|tx| tx.next_action).min()
    }

    /// Earliest next action timestamp across non-lingering transactions to `server_ip`.
    pub fn earliest_deadline_for_ip(&self, server_ip: std::net::IpAddr) -> Option<Tick> {
        self.by_id.values().filter(|tx| !tx.lingering && tx.dest.ip() == server_ip).map(|tx| tx.next_action).min()
    }

    /// Return IDs of transactions whose `next_action <= now`.
    pub fn due_ids(&self, now: Tick) -> Vec<u32> {
        self.by_id.values().filter(|tx| tx.next_action <= now).map(|tx| tx.id).collect()
    }

    /// Number of non-lingering active transactions.
    pub fn active_count(&self) -> usize {
        self.by_id.values().filter(|tx| !tx.lingering).count()
    }

    /// True if any active or lingering transaction exists for `tag`.
    pub fn has_tag(&self, tag: TxTag) -> bool {
        self.by_id.values().any(|tx| tx.tag == tag)
    }
}
