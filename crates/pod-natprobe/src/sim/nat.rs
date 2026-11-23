//! Stateful virtual-time NAT translator and multi-layer NAT stack (CGNAT, double NAT).

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use pod_turn::stun::{BINDING_SUCCESS, MAPPED_ADDRESS, decode, write_address};

use crate::classify::ring_wrap;
use crate::report::{FilteringBehavior, HairpinBehavior, MappingBehavior, Tick};

/// Port allocation policy for a simulated NAT layer.
#[derive(Clone, Debug)]
pub enum SimAlloc {
    Preserve,
    /// Preserves except on `collide_every`-th allocation (1-based).
    PreserveWithCollision {
        collide_every: usize,
        fallback_start: u16,
    },
    Sequential {
        start: u16,
        step: i16,
    },
    /// Time-driven shared counter advancing at `rate_per_s` ports per second.
    Counter {
        start: u16,
        rate_per_s: i16,
    },
    Random {
        lo: u16,
        hi: u16,
    },
}

/// External IP pooling policy for a simulated NAT layer.
#[derive(Clone, Debug)]
pub enum SimPool {
    Single(IpAddr),
    Arbitrary(Vec<IpAddr>),
    PerDest(Vec<IpAddr>),
}

/// Configuration of one NAT layer.
#[derive(Clone, Debug)]
pub struct NatSpec {
    pub mapping: MappingBehavior,
    pub filtering: FilteringBehavior,
    pub alloc: SimAlloc,
    pub pool: SimPool,
    pub hairpin: HairpinBehavior,
    pub lifetime_us: Option<u64>,
    pub alg_mangle_plain: bool,
    pub udp_blocked: bool,
    /// When set, forces a one-time external port shift after this tick (unstable mapping).
    pub remap_after_tick: Option<Tick>,
}

impl NatSpec {
    /// Typical home router: EIM + APDF, port-preserving, single global IP, no hairpin.
    pub fn home_eim_apdf(wan_ip: IpAddr) -> Self {
        Self {
            mapping: MappingBehavior::EndpointIndependent,
            filtering: FilteringBehavior::AddressAndPortDependent,
            alloc: SimAlloc::Preserve,
            pool: SimPool::Single(wan_ip),
            hairpin: HairpinBehavior::None,
            lifetime_us: Some(120_000_000),
            alg_mangle_plain: false,
            udp_blocked: false,
            remap_after_tick: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum MapKey {
    Ei(SocketAddr),
    Ad(SocketAddr, IpAddr),
    Apd(SocketAddr, SocketAddr),
}

#[derive(Clone, Debug)]
struct MappingEntry {
    internal: SocketAddr,
    external: SocketAddr,
    last_used: Tick,
    permitted_ips: BTreeSet<IpAddr>,
    permitted_eps: BTreeSet<SocketAddr>,
    remapped: bool,
}

/// Runtime state of one NAT layer.
#[derive(Clone, Debug)]
pub struct NatLayer {
    pub spec: NatSpec,
    entries: BTreeMap<MapKey, MappingEntry>,
    by_ext: BTreeMap<SocketAddr, MapKey>,
    alloc_count: usize,
    seq_cursor: i64,
    rng: u64,
}

impl NatLayer {
    pub fn new(spec: NatSpec, seed: u64) -> Self {
        let seq_cursor = match spec.alloc {
            SimAlloc::Sequential { start, .. } => i64::from(start),
            SimAlloc::Counter { start, .. } => i64::from(start),
            SimAlloc::PreserveWithCollision { fallback_start, .. } => i64::from(fallback_start),
            _ => 40_000,
        };
        Self {
            spec,
            entries: BTreeMap::new(),
            by_ext: BTreeMap::new(),
            alloc_count: 0,
            seq_cursor,
            rng: seed ^ 0xa076_1d64_78bd_642f,
        }
    }

    fn key_for(&self, internal: SocketAddr, dst: SocketAddr) -> MapKey {
        match self.spec.mapping {
            MappingBehavior::EndpointIndependent => MapKey::Ei(internal),
            MappingBehavior::AddressDependent => MapKey::Ad(internal, dst.ip()),
            MappingBehavior::AddressAndPortDependent => MapKey::Apd(internal, dst),
        }
    }

    fn expire(&mut self, now: Tick) {
        let Some(life) = self.spec.lifetime_us else { return };
        let expired: Vec<MapKey> =
            self.entries.iter().filter(|(_, e)| now.saturating_sub(e.last_used) >= life).map(|(k, _)| *k).collect();
        for k in expired {
            if let Some(e) = self.entries.remove(&k) {
                self.by_ext.remove(&e.external);
            }
        }
    }

    fn pick_ip(&mut self, dst: SocketAddr) -> IpAddr {
        match &self.spec.pool {
            SimPool::Single(ip) => *ip,
            SimPool::Arbitrary(ips) => {
                let idx = self.alloc_count % ips.len().max(1);
                ips[idx]
            }
            SimPool::PerDest(ips) => {
                let h = match dst.ip() {
                    IpAddr::V4(v4) => u32::from_be_bytes(v4.octets()) as usize,
                    IpAddr::V6(v6) => usize::from(v6.segments()[7]),
                };
                ips[h % ips.len().max(1)]
            }
        }
    }

    fn pick_port(&mut self, internal_port: u16, now: Tick) -> u16 {
        let count = self.alloc_count;
        self.alloc_count += 1;
        match self.spec.alloc {
            SimAlloc::Preserve => internal_port,
            SimAlloc::PreserveWithCollision { collide_every, .. } => {
                if collide_every > 0 && (count + 1).is_multiple_of(collide_every) {
                    let p = ring_wrap(self.seq_cursor);
                    self.seq_cursor += 1;
                    p
                } else {
                    internal_port
                }
            }
            SimAlloc::Sequential { step, .. } => {
                let p = ring_wrap(self.seq_cursor);
                self.seq_cursor += i64::from(step);
                p
            }
            SimAlloc::Counter { start, rate_per_s } => {
                let secs = (now as f64) / 1_000_000.0;
                let time_adv = (f64::from(rate_per_s) * secs).round() as i64;
                ring_wrap(i64::from(start) + time_adv + (count as i64))
            }
            SimAlloc::Random { lo, hi } => {
                self.rng = crate::strategy::splitmix64(self.rng);
                let span = u64::from(hi.saturating_sub(lo)).max(1) + 1;
                (u64::from(lo) + (self.rng % span)) as u16
            }
        }
    }

    /// Translate an outbound datagram `internal -> dst`. Returns `None` if UDP is blocked.
    pub fn translate_outbound(&mut self, now: Tick, internal: SocketAddr, dst: SocketAddr) -> Option<SocketAddr> {
        if self.spec.udp_blocked {
            return None;
        }
        self.expire(now);
        let key = self.key_for(internal, dst);
        if let Some(entry) = self.entries.get_mut(&key) {
            if let Some(remap_at) = self.spec.remap_after_tick
                && now >= remap_at
                && !entry.remapped
            {
                let old_ext = entry.external;
                let new_ext = SocketAddr::new(old_ext.ip(), ring_wrap(i64::from(old_ext.port()) + 77));
                entry.external = new_ext;
                entry.remapped = true;
                self.by_ext.remove(&old_ext);
                self.by_ext.insert(new_ext, key);
            }
            entry.last_used = now;
            entry.permitted_ips.insert(dst.ip());
            entry.permitted_eps.insert(dst);
            return Some(entry.external);
        }
        let ip = self.pick_ip(dst);
        let mut port = self.pick_port(internal.port(), now);
        while self.by_ext.contains_key(&SocketAddr::new(ip, port)) {
            port = ring_wrap(i64::from(port) + 1);
        }
        let external = SocketAddr::new(ip, port);
        let mut permitted_ips = BTreeSet::new();
        permitted_ips.insert(dst.ip());
        let mut permitted_eps = BTreeSet::new();
        permitted_eps.insert(dst);
        let entry = MappingEntry { internal, external, last_used: now, permitted_ips, permitted_eps, remapped: false };
        self.entries.insert(key, entry);
        self.by_ext.insert(external, key);
        Some(external)
    }

    /// Translate an inbound datagram `remote -> external`. Applies filtering & optional ALG rewrite.
    pub fn translate_inbound(
        &mut self,
        now: Tick,
        remote: SocketAddr,
        external: SocketAddr,
        payload: &mut [u8],
    ) -> Option<SocketAddr> {
        if self.spec.udp_blocked {
            return None;
        }
        self.expire(now);
        let key = *self.by_ext.get(&external)?;
        let entry = self.entries.get(&key)?;
        let allowed = match self.spec.filtering {
            FilteringBehavior::EndpointIndependent => true,
            FilteringBehavior::AddressDependent => entry.permitted_ips.contains(&remote.ip()),
            FilteringBehavior::AddressAndPortDependent => entry.permitted_eps.contains(&remote),
            FilteringBehavior::NoReturnTraffic => false,
        };
        if !allowed {
            return None;
        }
        let internal = entry.internal;
        if self.spec.alg_mangle_plain {
            mangle_stun_plain_mapped(payload, internal);
        }
        Some(internal)
    }

    /// Attempt hairpinning when an internal socket `src_internal` sends to `dst_ext` which is
    /// an active external mapping on this layer.
    pub fn try_hairpin(
        &mut self,
        now: Tick,
        src_internal: SocketAddr,
        dst_ext: SocketAddr,
    ) -> Option<(SocketAddr, SocketAddr)> {
        self.expire(now);
        let dst_key = *self.by_ext.get(&dst_ext)?;
        let dst_internal = self.entries.get(&dst_key)?.internal;
        let src_ext = self.translate_outbound(now, src_internal, dst_ext)?;
        // Check filtering on destination mapping (the inbound side of hairpin).
        let dst_entry = self.entries.get(&dst_key)?;
        let allowed = match self.spec.filtering {
            FilteringBehavior::EndpointIndependent => true,
            FilteringBehavior::AddressDependent => dst_entry.permitted_ips.contains(&src_ext.ip()),
            FilteringBehavior::AddressAndPortDependent => dst_entry.permitted_eps.contains(&src_ext),
            FilteringBehavior::NoReturnTraffic => false,
        };
        if !allowed {
            return None;
        }
        let delivered_from = match self.spec.hairpin {
            HairpinBehavior::None => return None,
            HairpinBehavior::ExternalSource => src_ext,
            HairpinBehavior::InternalSource => src_internal,
            HairpinBehavior::ExternalIpOnly => SocketAddr::new(src_ext.ip(), ring_wrap(i64::from(src_ext.port()) + 5)),
            HairpinBehavior::OtherSource => SocketAddr::new(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 250)), 44444),
        };
        Some((dst_internal, delivered_from))
    }

    /// True if `addr` is an active external mapping owned by this NAT layer.
    pub fn owns_external(&self, addr: SocketAddr) -> bool {
        self.by_ext.contains_key(&addr)
    }
}

/// Rewrite a plain `MAPPED-ADDRESS` (0x0001) attribute in-place to `internal`, simulating a
/// classic binary NAT ALG while leaving `XOR-MAPPED-ADDRESS` (0x0020) intact.
fn mangle_stun_plain_mapped(payload: &mut [u8], internal: SocketAddr) {
    let Ok(msg) = decode(payload) else { return };
    if msg.kind != BINDING_SUCCESS {
        return;
    }
    let replacement = write_address(internal, None);
    let mut off = 20usize;
    while off + 4 <= payload.len() {
        let attr = u16::from_be_bytes([payload[off], payload[off + 1]]);
        let len = usize::from(u16::from_be_bytes([payload[off + 2], payload[off + 3]]));
        let val_start = off + 4;
        if val_start + len > payload.len() {
            break;
        }
        if attr == MAPPED_ADDRESS && len == replacement.len() {
            payload[val_start..val_start + len].copy_from_slice(&replacement);
            break;
        }
        off = val_start + len + ((4 - len % 4) % 4);
    }
}
