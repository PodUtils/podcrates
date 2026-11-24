//! Deterministic virtual-time simulator world, packet capture and `drive_sim` harness.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use pod_core::entropy::{Entropy, NoRandom};

use super::gateway::SimGateway;
use super::nat::{NatLayer, NatSpec};
use super::server::SimStunServer;
use crate::engine::{Input, Output, ProbeEvent, ProbeMachine};
use crate::error::NatError;
use crate::report::{Family, Report, SockId, Tick};
use crate::seams::LocalEnv;
use crate::settings::Settings;

/// Deterministic PRNG implementing [`Entropy`] for reproducible simulations.
#[derive(Clone, Debug)]
pub struct SeededEntropy(u64);

impl SeededEntropy {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x517c_c1b7_2722_0a95)
    }
}

impl Entropy for SeededEntropy {
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), NoRandom> {
        for chunk in buf.chunks_mut(8) {
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^= z >> 31;
            let bytes = z.to_le_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
        }
        Ok(())
    }
}

/// One captured datagram emitted by the probe machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedPacket {
    pub tick: Tick,
    pub sock: SockId,
    pub src: SocketAddr,
    pub dst: SocketAddr,
    pub bytes: Vec<u8>,
}

/// Full wire capture recorded during [`drive_sim`].
#[derive(Clone, Debug, Default)]
pub struct Capture {
    pub sent: Vec<CapturedPacket>,
}

impl Capture {
    /// True if any captured STUN datagram contained a `CHANGE-REQUEST` (0x0003) attribute.
    pub fn has_change_request(&self) -> bool {
        self.sent.iter().any(|p| pod_turn::stun::decode(&p.bytes).ok().and_then(|m| m.change_request()).is_some())
    }

    /// Count of hairpin probe datagrams emitted.
    pub fn hairpin_count(&self) -> usize {
        self.sent.iter().filter(|p| crate::engine::decode_hairpin(&p.bytes).is_some()).count()
    }
}

/// Simulated DNS64 responder configuration.
#[derive(Clone, Debug, Default)]
pub struct SimDns64 {
    pub aaaa: Vec<Ipv6Addr>,
    pub has_a: bool,
    pub resolver_is_doh: bool,
    pub failed: bool,
}

/// Complete virtual network world: host environment, NAT stack (inner -> outer), STUN servers,
/// optional LAN gateway, DNS64 responder and link impairment (RTT, uniform packet loss).
#[derive(Clone, Debug)]
pub struct SimWorld {
    pub local_env: LocalEnv,
    pub nat_stack: Vec<NatLayer>,
    pub servers: Vec<SimStunServer>,
    pub gateway: Option<SimGateway>,
    pub dns64: SimDns64,
    pub online_ok: bool,
    /// One-way propagation delay in microseconds.
    pub one_way_us: u64,
    /// Uniform packet loss percentage in `0..=100`.
    pub loss_percent: u8,
    pub next_ephemeral_port: u16,
    rng: u64,
}

impl SimWorld {
    /// Build a world with a local private IP, NAT stack (innermost first), and STUN servers.
    pub fn new(local_v4: Ipv4Addr, nat_specs: Vec<NatSpec>, servers: Vec<SimStunServer>, seed: u64) -> Self {
        let local_env = LocalEnv {
            route_source_v4: Some(local_v4),
            route_iface_v4: Some("eth0".to_string()),
            route_source_v6: None,
            v6_stack_available: false,
            v6_no_route: true,
            v6_wkp_routable: None,
            local_ips: vec![IpAddr::V4(local_v4)],
            gateway_v4: None,
        };
        let nat_stack = nat_specs
            .into_iter()
            .enumerate()
            .map(|(i, s)| NatLayer::new(s, seed.wrapping_add(i as u64 * 17)))
            .collect();
        Self {
            local_env,
            nat_stack,
            servers,
            gateway: None,
            dns64: SimDns64::default(),
            online_ok: true,
            one_way_us: 20_000,
            loss_percent: 0,
            next_ephemeral_port: 40_000,
            rng: seed ^ 0xd1b5_4a32_d192_ed03,
        }
    }

    fn drop_packet(&mut self) -> bool {
        if self.loss_percent == 0 {
            return false;
        }
        if self.loss_percent >= 100 {
            return true;
        }
        self.rng = self.rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((z ^ (z >> 31)) % 100) < u64::from(self.loss_percent)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ScheduledEvent {
    at: Tick,
    seq: u64,
    kind: SimEv,
}

/// An event of the world. `Inbound` is a reply of a server at the outer NAT layer: the stack
/// filters it at its arrival.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SimEv {
    Datagram { sock: SockId, from: SocketAddr, bytes: Vec<u8> },
    Inbound { from: SocketAddr, to: SocketAddr, bytes: Vec<u8> },
    Dns64,
    HttpGet { id: u32, url: String },
    HttpSoap { id: u32, url: String, action: String },
    Online,
}

/// Drive [`ProbeMachine`] to completion against `world` in pure deterministic virtual time.
pub fn drive_sim(settings: Settings, world: &mut SimWorld, seed: u64) -> Result<(Report, Capture), NatError> {
    let mut machine = ProbeMachine::new(settings, world.local_env.clone(), Box::new(SeededEntropy::new(seed)))?;
    let mut capture = Capture::default();
    let mut sockets: BTreeMap<SockId, SocketAddr> = BTreeMap::new();
    let mut by_local: BTreeMap<SocketAddr, SockId> = BTreeMap::new();
    let mut queue: BinaryHeap<Reverse<ScheduledEvent>> = BinaryHeap::new();
    let mut seq = 0u64;
    let mut now: Tick = 0;

    machine.start(now);

    let mut steps = 0usize;
    loop {
        steps += 1;
        if steps > 100_000 {
            return Err(NatError::ReportInvariant("the simulator did more than 100000 steps".to_string()));
        }
        while let Some(out) = machine.poll_output() {
            match out {
                Output::Bind { sock, spec } => {
                    let ip = spec.local_ip.unwrap_or(match spec.family {
                        Family::V4 => {
                            world.local_env.route_source_v4.map_or(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)), IpAddr::V4)
                        }
                        Family::V6 => {
                            world.local_env.route_source_v6.map_or(IpAddr::V6(Ipv6Addr::LOCALHOST), IpAddr::V6)
                        }
                    });
                    let port = spec.local_port.unwrap_or_else(|| {
                        let p = world.next_ephemeral_port;
                        world.next_ephemeral_port = if p >= 65_000 { 40_000 } else { p + 1 };
                        p
                    });
                    let local = SocketAddr::new(ip, port);
                    sockets.insert(sock, local);
                    by_local.insert(local, sock);
                    machine.handle_input(now, Input::Bound { sock, local });
                }
                Output::Close { sock } => {
                    if let Some(addr) = sockets.remove(&sock) {
                        by_local.remove(&addr);
                    }
                }
                Output::Send { sock, dest, bytes } => {
                    let Some(&src_local) = sockets.get(&sock) else { continue };
                    capture.sent.push(CapturedPacket {
                        tick: now,
                        sock,
                        src: src_local,
                        dst: dest,
                        bytes: bytes.clone(),
                    });
                    route_outbound(now, world, src_local, dest, &bytes, &by_local, &mut queue, &mut seq);
                }
                Output::QueryDns64 { .. } => {
                    seq += 1;
                    queue.push(Reverse(ScheduledEvent { at: now + 5_000, seq, kind: SimEv::Dns64 }));
                }
                Output::HttpGet { id, url } => {
                    seq += 1;
                    queue.push(Reverse(ScheduledEvent { at: now + 5_000, seq, kind: SimEv::HttpGet { id, url } }));
                }
                Output::HttpSoap { id, url, action, .. } => {
                    seq += 1;
                    queue.push(Reverse(ScheduledEvent {
                        at: now + 5_000,
                        seq,
                        kind: SimEv::HttpSoap { id, url, action },
                    }));
                }
                Output::CheckOnline => {
                    seq += 1;
                    queue.push(Reverse(ScheduledEvent { at: now + 10_000, seq, kind: SimEv::Online }));
                }
            }
        }
        while let Some(ev) = machine.poll_event() {
            if let ProbeEvent::Final(report) = ev {
                return Ok((*report, capture));
            }
        }

        let next_timeout = machine.poll_timeout();
        let next_ev_at = queue.peek().map(|r| r.0.at);
        match (next_ev_at, next_timeout) {
            (Some(et), Some(tt)) if et <= tt => {
                let Some(Reverse(ev)) = queue.pop() else { continue };
                now = ev.at;
                deliver_event(now, ev.kind, world, &by_local, &mut machine);
            }
            (_, Some(tt)) => {
                now = tt;
                machine.handle_timeout(now);
            }
            (Some(et), None) => {
                let Some(Reverse(ev)) = queue.pop() else { continue };
                now = et;
                deliver_event(now, ev.kind, world, &by_local, &mut machine);
            }
            (None, None) => {
                return Err(NatError::ReportInvariant("the simulator has no event and no timer".to_string()));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn route_outbound(
    now: Tick,
    world: &mut SimWorld,
    src_local: SocketAddr,
    dest: SocketAddr,
    bytes: &[u8],
    by_local: &BTreeMap<SocketAddr, SockId>,
    queue: &mut BinaryHeap<Reverse<ScheduledEvent>>,
    seq: &mut u64,
) {
    // 1. Gateway traffic (NAT-PMP/PCP/SSDP).
    if let Some(ref gw) = world.gateway {
        let is_gw = (dest.ip() == gw.ip && (dest.port() == gw.pmp_port || dest.port() == gw.ssdp_port))
            || dest == crate::wire::ssdp::SSDP_MULTICAST;
        if is_gw {
            let replies = gw.handle_udp(dest, bytes);
            if let Some(&sock) = by_local.get(&src_local) {
                for (from, body) in replies {
                    *seq += 1;
                    queue.push(Reverse(ScheduledEvent {
                        at: now + 2_000,
                        seq: *seq,
                        kind: SimEv::Datagram { sock, from, bytes: body },
                    }));
                }
            }
            return;
        }
    }

    if world.drop_packet() {
        return;
    }

    // 2. Hairpin check on outermost NAT layer (for IPv4 NAT stacks).
    if dest.is_ipv4() && !world.nat_stack.is_empty() {
        let outer_idx = world.nat_stack.len() - 1;
        if world.nat_stack[outer_idx].owns_external(dest) {
            // Walk inner layers first if stacked.
            let mut cur = src_local;
            for layer in &mut world.nat_stack[..outer_idx] {
                let Some(ext) = layer.translate_outbound(now, cur, dest) else { return };
                cur = ext;
            }
            if let Some((mut dst_addr, delivered_from)) = world.nat_stack[outer_idx].try_hairpin(now, cur, dest) {
                let mut payload = bytes.to_vec();
                for layer in world.nat_stack[..outer_idx].iter_mut().rev() {
                    let Some(inner) = layer.translate_inbound(now, delivered_from, dst_addr, &mut payload) else {
                        return;
                    };
                    dst_addr = inner;
                }
                if let Some(&dst_sock) = by_local.get(&dst_addr) {
                    *seq += 1;
                    queue.push(Reverse(ScheduledEvent {
                        at: now + 5_000,
                        seq: *seq,
                        kind: SimEv::Datagram { sock: dst_sock, from: delivered_from, bytes: payload },
                    }));
                }
            }
            return;
        }
    }

    // 3. Outbound through NAT stack (v4 only unless layer explicitly handles v6).
    let mut cur_src = src_local;
    if dest.is_ipv4() {
        for layer in &mut world.nat_stack {
            let Some(ext) = layer.translate_outbound(now, cur_src, dest) else { return };
            cur_src = ext;
        }
    }

    // 4. Deliver to matching STUN server.
    let Some(srv) = world.servers.iter().find(|s| s.owns(dest)).cloned() else { return };
    let Some((reply_src, reply_dst, reply_bytes)) = srv.handle(dest, cur_src, bytes) else { return };

    if world.drop_packet() {
        return;
    }

    // 5. The reply reaches the outer NAT layer one round trip later. The stack translates and
    // filters it then, with the mappings and the pinholes of that time.
    *seq += 1;
    queue.push(Reverse(ScheduledEvent {
        at: now + 2 * world.one_way_us,
        seq: *seq,
        kind: SimEv::Inbound { from: reply_src, to: reply_dst, bytes: reply_bytes },
    }));
}

/// Translate an inbound datagram through the NAT stack, outer layer first, at `now`. Returns
/// the internal socket and the payload (an ALG can change it), or `None` when a layer drops it.
fn route_inbound(
    now: Tick,
    world: &mut SimWorld,
    from: SocketAddr,
    to: SocketAddr,
    mut bytes: Vec<u8>,
    by_local: &BTreeMap<SocketAddr, SockId>,
) -> Option<(SockId, Vec<u8>)> {
    let mut cur_dst = to;
    if to.is_ipv4() {
        for layer in world.nat_stack.iter_mut().rev() {
            cur_dst = layer.translate_inbound(now, from, cur_dst, &mut bytes)?;
        }
    }
    by_local.get(&cur_dst).map(|&sock| (sock, bytes))
}

fn deliver_event(
    now: Tick,
    ev: SimEv,
    world: &mut SimWorld,
    by_local: &BTreeMap<SocketAddr, SockId>,
    machine: &mut ProbeMachine,
) {
    match ev {
        SimEv::Datagram { sock, from, bytes } => {
            machine.handle_input(now, Input::Datagram { sock, from, bytes });
        }
        SimEv::Inbound { from, to, bytes } => {
            if let Some((sock, bytes)) = route_inbound(now, world, from, to, bytes, by_local) {
                machine.handle_input(now, Input::Datagram { sock, from, bytes });
            }
        }
        SimEv::Dns64 => {
            machine.handle_input(
                now,
                Input::Dns64Answer {
                    aaaa: world.dns64.aaaa.clone(),
                    has_a: world.dns64.has_a,
                    resolver_is_doh: world.dns64.resolver_is_doh,
                    failed: world.dns64.failed,
                },
            );
        }
        SimEv::HttpGet { id, url } => {
            let body = world.gateway.as_ref().map_or(Err("no gw".to_string()), |g| g.handle_http_get(&url));
            machine.handle_input(now, Input::HttpReply { id, body });
        }
        SimEv::HttpSoap { id, url, action } => {
            let body = world.gateway.as_ref().map_or(Err("no gw".to_string()), |g| g.handle_http_soap(&url, &action));
            machine.handle_input(now, Input::HttpReply { id, body });
        }
        SimEv::Online => {
            machine.handle_input(now, Input::OnlineResult { ok: world.online_ok });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::server::SimStunServer;
    use pod_turn::stun::{CHANGE_PORT, NatRequestBuilder};

    /// A pinhole that opens while a reply is on the wire lets the reply in: the stack filters
    /// a reply when it arrives, not when the server sends it.
    #[test]
    fn sim_filters_a_reply_at_its_arrival() {
        let pp: SocketAddr = "203.0.113.1:3478".parse().expect("pp");
        let aa: SocketAddr = "203.0.113.2:3479".parse().expect("aa");
        let pa = SocketAddr::new(pp.ip(), aa.port());
        let wan = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10));
        let local: SocketAddr = "192.168.1.10:40000".parse().expect("local");
        let mut world = SimWorld::new(
            Ipv4Addr::new(192, 168, 1, 10),
            vec![NatSpec::home_eim_apdf(wan)],
            vec![SimStunServer::rfc5780("s1", pp, aa)],
            1,
        );
        let by_local = BTreeMap::from([(local, SockId(0))]);
        let mut queue = BinaryHeap::new();
        let mut seq = 0;
        let ask = NatRequestBuilder::binding([1; 12]).change_request(CHANGE_PORT).build().expect("request");
        route_outbound(0, &mut world, local, pp, &ask, &by_local, &mut queue, &mut seq);
        // The reply from PA is on the wire; the client then sends to PA.
        let plain = NatRequestBuilder::binding([2; 12]).build().expect("request");
        route_outbound(1_000, &mut world, local, pa, &plain, &by_local, &mut queue, &mut seq);
        let mut delivered = Vec::new();
        while let Some(Reverse(ev)) = queue.pop() {
            if let SimEv::Inbound { from, to, bytes } = ev.kind
                && let Some((sock, _)) = route_inbound(ev.at, &mut world, from, to, bytes, &by_local)
            {
                delivered.push((sock, from));
            }
        }
        assert_eq!(delivered, vec![(SockId(0), pa), (SockId(0), pa)], "both replies come from PA and pass");
    }
}
