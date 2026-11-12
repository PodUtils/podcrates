# pod-netstack

The userspace TCP and UDP stack, on smoltcp 0.14: the stack of podnet's
virtual network (`PodUtils/podnet:crates/podnet-vnet`), as
PodUtils/podnet#427 cut it.

A `NetStack` owns a private IPv4 and IPv6 subnet and runs the TCP and UDP
state machines of smoltcp for it in a task of tokio. Its link layer is a
pair of packet queues in the process, so the stack makes no `AF_INET` call
of the kernel. On the loopback, the stack talks to itself; over a link, a
pump of the consumer moves its packets to a real byte stream (a unix
socket, a QUIC stream, a WebSocket, a TUN).

The policy is the consumer's: the admit of a target, a UDP service at the
gateway and a tap of each packet are traits that the consumer implements.
The crate reads no environment variable, prints nothing (diagnostics go to
`tracing`) and keeps no global state.

**Build setting.** The stack owns more than the two addresses that smoltcp
allows by default. Each binary that links the crate sets this line in its
`.cargo/config.toml`; Cargo does not read the configuration of a
dependency, and a build with less fails at compile time:

```toml
[env]
SMOLTCP_IFACE_MAX_ADDR_COUNT = "8"
```

## API

| Item | Contents |
| --- | --- |
| `NetStack` | `loopback()`, `loopback_with(config)`, `over_link(config)` (each spawns its reactor: call it in a runtime of tokio); `listen`, `connect`, `connect_from`, `bind_udp` (each takes `impl Into<SocketAddr>`); `listen_any(port)`, `bind_udp_any(port)` (on each address: the section "Any IP"); `link_in`, `link_out` (the queues of a pump); `stats`, `device_stats`, `socket_count` (live and peak); `take_events`; `shutdown`; `Drop` stops the reactor. |
| `StackConfig` | The settings (below). |
| `StackEvent` | `ListenerBound`, `Accepted`, `Connected`, `Refused`, `Closed`, `Datagram`. |
| `NetStats`, `NetStatsSnapshot` | `accepted`, `connected`, `refused`, `closed`, `datagrams`, `dropped` (an inbound SYN that the stack dropped at `max_connections`; the peer sends it again), `udp_rx_dropped`, `udp_tx_dropped`, `udp_oversize` (the section "UDP"). |
| `TcpStream` | `AsyncRead`, `AsyncWrite`; `established()` (the handshake, or `ConnectionRefused`, `ConnectionReset`), `local_addr`, `peer_addr`, `set_nodelay`, `shutdown` (a FIN after the queued data; the read side stays open), `close` (the section "Close"), `abort` (a RST). A RST of the peer reads as `ConnectionReset` after the bytes that came before it. |
| `TcpListener` | `accept`, `local_addr` (`[::]:port` on each address), `close` (the section "Close"). |
| `UdpSocket` | `send_to(buf, remote)`; `send_to_from(buf, remote, source: IpAddr)` (from an address that the stack need not own; the port of the socket); `recv_from(buf) -> (n, from)`; `recv_from_to(buf) -> (n, from, to)` (`to`: the destination of the datagram); `local_addr` (`[::]:port` on each address); `max_datagram(to)`; `into_datagrams`. A remote of the other family than the source, and a datagram over `max_datagram`, are `InvalidInput`. A drop sends the queued datagrams, then frees the port. |
| `UdpDatagrams` | A UDP socket as `pod_core::Datagrams` (QUIC, a DNS client over the stack). `Transmit::src_ip` is the source of the datagram; `RecvMeta::dst_ip` is its destination. `try_send` does not wait: a full queue to the stack drops the datagram (`udp_tx_dropped`). `max_datagram(to)`; `may_fragment` is `IPV4_FRAGMENTATION`. |
| `Clock` | The monotonic clock in milliseconds of smoltcp. |
| `Subnet` | The address plan: `v4`/`v4_prefix`, `v6`/`v6_prefix`, `gateway_v4`, `gateway_v6`, `gateway_for`, `contains`, `prefix_for`. Default `10.66.0.0/24` and `fd00:66::/64`. |
| `device` | `LinkDevice` (`loopback`, `tunnel`, `tunnel_bounded`, `link`, `with_inbound_limit`, `with_stats`, `with_max_burst`, `with_checksum(ChecksumCapabilities)`, `add_local`), its tokens `LinkRx`, `LinkTx`; `PacketQueue` (bounded, with a waker); `DeviceStats`, `DeviceStatsSnapshot`; `LINK_OUTBOUND` (512), `LINK_INBOUND` (4096); `RecordingDevice`, `PacketTap`, `Direction`. |
| `hooks` | `Admit`, `GatewayService`, `GATEWAY_DATAGRAM` (1232). |
| `MAX_ADDRESSES` | `smoltcp::config::IFACE_MAX_ADDR_COUNT` of the build (8 or more). |
| `IPV4_FRAGMENTATION` | True with the feature `ipv4-fragmentation`. |
| `smoltcp` | The re-export of smoltcp, so that a consumer names one version. |

## The hooks

| Trait | Called | podnet |
| --- | --- | --- |
| `Admit::admits(&self, target: SocketAddr) -> bool` | At each listen, connect and UDP bind (a refusal is `PermissionDenied`; a listen or a bind on each address gives `[::]:port`), and at each UDP send (a refused datagram is dropped and counted in `refused`). No admit: each target in the subnet. A closure `Fn(SocketAddr) -> bool` is an `Admit`. | `Acl` (`Acl::allows`) |
| `GatewayService::port(&self) -> u16`, `GatewayService::answer(&self, from: SocketAddr, request: &[u8]) -> Option<Vec<u8>>` | The stack owns the IPv4 gateway, binds `port` there and gives each datagram (at most `GATEWAY_DATAGRAM` bytes; a longer one is dropped and counted in `udp_rx_dropped`) to `answer`, at the packet that brings it; `None` sends nothing. It runs in the task of the stack. An answer that finds the send buffer full (more than `udp_packets` requests in one pass) is dropped and counted in `udp_tx_dropped`. | `Resolver` (port 53, `Resolver::handle`) |
| `PacketTap::packet(&self, direction: Direction, packet: &[u8])` | `RecordingDevice<D, T>` wraps a device and calls the tap for each packet, `Inbound` (link to stack) and `Outbound` (stack to link). A closure `Fn(Direction, &[u8])` is a `PacketTap`. | `Recorder` (the pcap and JSON capture stay in podnet) |

## Settings

`StackConfig`; the default is podnet's.

| Field | Default | What |
| --- | --- | --- |
| `subnet` | `10.66.0.0/24`, `fd00:66::/64` | The private address space; a connect or a bind outside it is `AddrNotAvailable`. |
| `local_v4`, `local_v6` | `10.66.0.2`, `fd00:66::2` | The addresses of the stack. |
| `mtu` | 1280 | The MTU of the link (at least 576). `StackConfig::max_datagram(to)` gives the largest UDP payload for it (the section "UDP"). |
| `tcp_buffer` | 64 KiB | The send and the receive buffer of each TCP socket. |
| `udp_packets`, `udp_payload` | 64, 64 KiB | The buffers of each UDP socket, and the bound of its backlog (the section "UDP"). |
| `max_connections` | 4096 | The TCP connections at one time, open and half-open. A connect past it is `WouldBlock`; a SYN past it waits (the section "Listeners"). |
| `admit` | none | `Option<Arc<dyn Admit>>`. |
| `gateway_service` | none | `Option<Arc<dyn GatewayService>>`. |
| `random_seed` | the time | The seed of smoltcp for its ports and sequence numbers. |
| `max_poll_interval` | 100 ms | The longest sleep of the reactor. |
| `any_ip` | false | Take each packet, also to an address that the stack does not own (`Interface::set_any_ip`). |
| `checksum` | `Checksum::Both` for each protocol | The checksums that the stack computes and verifies (`LinkDevice::with_checksum`). |
| `linger` | 60 s | The longest time that a dropped or closed stream waits for a peer that makes no progress (the section "Close"). |
| `congestion_control` | `CongestionControl::None` | The congestion controller of each TCP socket: none, Reno or CUBIC (the section "The congestion controllers"; podnet's `--cc`). |
| `max_burst` | none | The burst limit of the link in segments (`LinkDevice::with_max_burst`); smoltcp clamps the TCP window that the stack advertises to it (PodUtils/podnet#100; podnet's `PODNET_WARP_TCP_BURST`). |
| `link_stats` | none | `Option<Arc<DeviceStats>>`: the counters of the link, shared with other stacks (`LinkDevice::with_stats`); `device_stats` reads them. None: the counters of this stack. |

## Features

| Feature | What |
| --- | --- |
| `ipv4-fragmentation` | The IPv4 fragmentation of smoltcp (`proto-ipv4-fragmentation`, `fragmentation-buffer-size-65536`): a UDP datagram to IPv4 holds up to 65507 bytes in both directions over each MTU. Each interface of smoltcp in the binary then holds a buffer of 64 KiB for the fragments that it sends. smoltcp puts together one datagram at a time, and a lost fragment holds that place for 60 s; a binary that needs more sets `SMOLTCP_REASSEMBLY_BUFFER_COUNT` in the `[env]` table of its `.cargo/config.toml`. Off by default (call C85). Turn on this feature, not the feature of smoltcp: the stack reads its own feature for `max_datagram`. |

## UDP

T-257 to T-259 (PodUtils/podnet#464):

* **The bound of a slow reader.** The reactor takes each datagram out of
  smoltcp at once, at the packet that brings it, into the backlog of its
  socket; the receive buffer of smoltcp thus never fills and drops a
  datagram with no count. The backlog goes into the channel to the app (64
  datagrams) while the channel has room: a place first, then the datagram.
  A socket whose app reads nothing thus keeps 64 datagrams in the channel
  and at most `udp_packets` datagrams and `udp_payload` bytes in its
  backlog, in order; the next read of the app kicks the reactor
  (`a_burst_with_no_reader_keeps_each_datagram_that_fits`: 128 of 500).
* **The counters.** `udp_rx_dropped`: an inbound datagram past the
  backlog, or a request to the gateway service over `GATEWAY_DATAGRAM`.
  `udp_tx_dropped`: a `UdpDatagrams::try_send` that found the queue to the
  stack full, or an answer of the gateway service that found its send
  buffer full. `udp_oversize`: a datagram over what the stack can carry
  (below). A `UdpSocket::send_to` that finds the send buffer of smoltcp
  full waits (the queue to the stack fills, and the send waits for room):
  it loses nothing. `rx_dropped` of the device counts a packet that the
  inbound queue of the link could not take.
* **The size.** `StackConfig::max_datagram(to)` (also on `UdpSocket` and
  `UdpDatagrams`): IPv6 `mtu - 48` (smoltcp does not fragment IPv6); IPv4
  `mtu - 28`, or 65507 with the feature `ipv4-fragmentation`; each at most
  `udp_payload`. A send over it is `InvalidInput` and counted in
  `udp_oversize` (1252 and 1232 bytes at MTU 1280). Before, the send was
  `Ok` and smoltcp dropped the packet. Inbound: with no reassembly (IPv6;
  IPv4 with no feature), the stack drops each IPv4 fragment before smoltcp
  (smoltcp would read it as a whole packet), and counts the first fragment
  of a datagram to a socket of the stack in `udp_oversize`; smoltcp answers
  an IPv6 fragment with an ICMPv6 error.
* **The drop.** The drop of a `UdpSocket` or a `UdpDatagrams` sends the
  datagrams that it queued, then frees its smoltcp socket and its port. A
  link that takes nothing for `StackConfig::linger` frees it too.

## Listeners

* A listener keeps one free listen socket. The reactor takes the inbound
  packets one at a time (`Interface::poll_ingress_single`) and reads the
  head of each in the queue: before a SYN goes to smoltcp, the listener of
  its port gets a free listen socket. Thus a burst of SYNs to one port gets
  no RST (`a_burst_of_connects_to_one_port_gets_no_reset`: 48 at once).
* The bound is `max_connections`: the open and the half-open connections
  of the stack. A SYN past it waits: the stack drops it, counts it in
  `dropped`, and the peer sends it again (`a_syn_past_the_connection_limit_waits`).
* The queue of a listener holds 64 accepted streams. When it is full, an
  established connection waits in the listener, and the next `accept`
  takes it (`a_burst_past_the_accept_queue_loses_no_connection`: 100).
* A SYN sent again (its SYN-ACK was lost) whose 4-tuple a socket holds does
  not reach a listen socket: smoltcp gives a segment to the first socket of
  its set that takes it, and a newer listen socket in an earlier slot would
  take it (`a_syn_sent_again_reaches_its_own_socket`).

## Close

* `TcpStream::close` and a drop are one graceful close, with no wait in
  the caller: the stack sends each queued byte (in the channel from the
  app and in the socket), then the FIN, and frees the socket at the end.
  The bytes that come in after it are discarded
  (`a_dropped_stream_sends_each_queued_byte_then_the_end`: 16 MiB).
* The bound: a dropped stream that makes no progress (no change of its TCP
  state, of its send queue and of its queued bytes) for
  `StackConfig::linger` is aborted (a RST), and its socket is freed: a dead
  peer, a peer in FIN-WAIT-2 that never sends its FIN, or a peer that reads
  nothing for that time (`a_dropped_stream_to_a_dead_peer_is_freed_after_the_linger`,
  `a_dropped_stream_whose_peer_never_closes_is_freed_after_the_linger`).
* `TcpListener::close` and a drop free the listen sockets, the port and the
  address that the listener made the stack own (when no listener and no
  connection uses it). The streams that it accepted stay. The streams in
  its queue and its established connections close as dropped streams do;
  its half-open sockets go (`dropped_listeners_free_their_sockets_and_their_ports`:
  1,000 ports one after the other).
* `abort` sends a RST: the socket leaves the set after smoltcp sent it
  (`an_abort_sends_a_reset`).

## Any IP

A router answers for each destination of a program (podnet's netns
router, PodUtils/podnet#454; T-222):

* `StackConfig::any_ip` takes each packet, to each address. With it off
  (the default), a packet to an address that the stack does not own is
  dropped, as before.
* `listen_any(port)` listens on each address of both families. The
  `local_addr` of each accepted stream is the destination of its SYN: the
  address that the program dialed.
* `bind_udp_any(port)` binds on each address. `recv_from_to` gives the
  destination of each datagram, and `send_to_from(buf, remote, source)`
  answers from it. The source port is the port of the socket.
* A socket on each address and a socket on one address do not share a
  port (`AddrInUse`, both ways): smoltcp gives a packet to the first socket
  of its set that takes it, so the two would race. The gateway service is
  the first socket of the set for the life of the stack: it keeps its
  address, and a UDP socket on each address of its port takes the other
  destinations. A UDP bind at the address and the port of the service is
  `AddrInUse` (that socket would get no datagram).
* A listener on one address that the stack does not own makes the stack
  own it, also with any IP: smoltcp 0.14's `has_ip_addr` is true for each
  address with any IP, so the stack reads the list of the interface.
* `StackConfig::checksum` (`LinkDevice::with_checksum`): the end of a veth
  pair with its offload on sends partial TCP and UDP sums. podnet's veth
  wire sets `Checksum::Tx` for `ipv4`, `tcp`, `udp` and `icmpv4`: the
  stack computes each sum and verifies none.

## smoltcp 0.14

The crate moves podnet's stack from smoltcp `=0.12.0` to 0.14, with
podnet's features (`std`, `medium-ip`, `proto-ipv4`, `proto-ipv6`,
`socket-tcp`, `socket-udp`, `socket-tcp-reno`, `socket-tcp-cubic`,
`assembler-max-segment-count-32`). The API that the stack uses is the same
in 0.14; no code changed for it. What the consumers rely on, and the test
of each:

* The medium `ip` for each stack: each test.
* The close of a socket after its send and receive queues are empty, and
  `TimeWait` read as closed: `closed_connections_leave_no_socket` (20
  connections whose server drops its handle with its reply queued; each
  socket leaves the set).
* The buffers of 64 KiB of `StackConfig::default`, and a large download at
  full speed with a bounded link: `a_large_download_crosses_a_bounded_link`
  (16 MiB between two stacks over the queues of `LinkDevice::link`; about
  1 s in a debug build). In podnet, the netns backend moved 48 MiB in 2 s
  (`netns_backend_moves_a_large_download`) and the tunnel device filled a
  window of 512 KiB at 40 ms (`tests/tcp_window.rs`, 11 MB/s).
* The stack sets no congestion controller (`CongestionControl::None`) by
  default, as podnet did; `StackConfig::congestion_control` sets Reno or
  CUBIC (`a_download_with_each_controller_crosses_a_bounded_link`). The
  next section gives the rate of each controller.

## The congestion controllers

In slow start, Reno and CUBIC of smoltcp 0.14 add one segment to the
window for each ACK (RFC 5681; 0.12 added each acknowledged byte). A
receiver of smoltcp sends its ACK in the pass of egress: `Interface::poll`
takes each waiting packet first, so one poll gives one ACK for all the
segments that it takes. The rate thus depends on the link:

* A link that delivers each burst at one time (podnet's
  `tests/tcp_window.rs`): one ACK for each round trip, so the window grows
  by one segment for each round trip. Reno and CUBIC stay slow.
* A link that delivers the packets one by one (a serial link): one ACK
  for each two segments (the delayed ACK of smoltcp), so the window grows
  by half for each round trip. After the slow start, the rate is that of
  no controller.

`link_rate` (T-188) measures it: a download of 8 MiB between two
interfaces of smoltcp over `LinkDevice::link`, 20 ms in each direction,
buffers of 512 KiB, on a virtual clock. The burst link has the window as
its limit (512 KiB in 40 ms); the serial link has 100 Mbit/s. In MB/s
(`cargo test -p pod-netstack --lib link_rate -- --nocapture`):

| Link | Limit | None | Reno | CUBIC |
| --- | --- | --- | --- | --- |
| Burst | 13.11 | 11.34 | 1.80 | 1.80 |
| Burst, the second half | 13.11 | 10.20 | 2.98 | 2.98 |
| Serial | 12.50 | 11.00 | 6.72 | 6.72 |
| Serial, the second half | 12.50 | 10.58 | 10.58 | 10.58 |
| Serial, receiver with no ACK delay | 12.50 | | 7.90 | 7.90 |

A change of the receiver is the fix that a consumer can use (Reno, burst
link):

| Receiver | MB/s | ACKs |
| --- | --- | --- |
| `Interface::poll`, ACK delay 10 ms (the default) | 1.80 | 118 |
| `Interface::poll`, `set_ack_delay(None)` | 1.80 | 119 |
| `poll_ingress_single` and `poll_egress` after each packet | 6.36 | 3,388 |
| The same, `set_ack_delay(None)` | 8.22 | 6,818 |

No segment is sent again in a case: the server sends 6,769 packets
(6,766 segments of data). The reactor of `NetStack` takes each waiting
packet with `Interface::poll_ingress_single`, then runs one pass of egress,
as `Interface::poll` does; T-197 has its ACKs.

## Parity

With podnet `crates/podnet-vnet` at `f7ed86b` (the files are the same at
`da6705f`):

* Each item of the cut is here, with its behaviour: the reactor and its
  poll loop (`max_poll_interval`, the kick of a write, the waker of the
  inbound queue), the listeners, the owned address of a
  listener off the stack's own address, the connect with its family, the
  deferred close of a dropped stream, `TimeWait` as closed, the removal of
  each closed socket and its count, UDP, `VnetDatagrams` (as
  `UdpDatagrams`), the link device with its bounds, its hold when full,
  its local addresses and its counters, the record device.
* Changes:
  * The endpoints are `std::net::SocketAddr` (podnet: `VirtAddr`, with the
    abstract names of its shim); a call takes `impl Into<SocketAddr>`, so
    podnet's `VirtAddr` goes in as it is. The subnet is `Subnet` (podnet's
    `VirtualSubnet` without the allocation and the names).
  * The field `acl` is `admit` (a trait), `dns` is `gateway_service` (a
    trait with its port), and the recorder is a `PacketTap`.
  * The stack takes no bytes from a TCP socket while the channel to the
    app is full, and the next read of the app kicks the reactor. podnet
    took the bytes first and dropped them when the channel (64 chunks) was
    full: a reader slower than the link got a stream with a hole
    (`a_slow_reader_gets_each_byte_and_the_end`; the plant
    `netstack-receive-with-no-room`). The end of the stream and the
    handshake are sent again at the next pass when the channel was full.
  * A listen, a connect or a bind that smoltcp refuses removes the socket
    that it added to the set (`a_refused_bind_frees_its_socket`).
  * UDP (T-257 to T-259; the section "UDP"): podnet's reactor took a
    datagram from smoltcp with no room in the channel to the app and lost
    it, counted no lost datagram, sent a datagram over the MTU with `Ok`
    and lost it, lost a datagram that a full send buffer did not take, and
    kept the socket and the port of a dropped UDP socket.
  * A UDP send to an address of the other family than the source is
    `InvalidInput` (`UdpSocket` and `UdpDatagrams`), and the reactor drops
    such a datagram: podnet's reactor gave it to smoltcp, which panicked on
    a packet of two families, and the stack stopped
    (`a_datagram_to_the_other_family_is_refused_and_the_stack_runs`).
  * A poisoned lock of a packet queue does not panic.
  * The close of a stream and of a listener, and the listen sockets of a
    listener (T-230, T-231, T-232; the sections "Listeners" and "Close"):
    podnet's listener kept `accept_pool` listen sockets and answered a SYN
    past them with a RST; a dropped listener kept its sockets and its port;
    a dropped stream closed its socket at once and lost the bytes in the
    channel from the app; an abort sent no RST; a RST of the peer read as a
    clean end.
  * The messages of the errors name the policy and the subnet, not the ACL
    and the virtual subnet.
  * `LinkDevice::with_stats` after `LinkDevice::link` moves the count of
    the drops of the inbound queue to the new counters; the queue counted
    them in the counters that the device had before
    (`shared_counters_take_the_inbound_drops_of_each_link`; the plant
    `netstack-shared-counters-lose-the-drops`, T-335).
  * The congestion controller, the burst limit and the shared counters of
    podnet's WARP stack (`src/warp/ip.rs`) are settings of each stack
    (`congestion_control`, `max_burst`, `link_stats`; T-335, the section
    "Settings").
* Not here (stays in podnet): the abstract names of the shim (`addr.rs`),
  the ACL, the resolver, the recorder and its pcap, the switch and its
  framing, the proxy, the shim, the control protocol, the dialers, the
  door, the proof of `net doctor` (`stack/proof.rs`), wg-quick and the
  Tailscale SSH shims.

## Adoption map

### podnet

podnet took the crate at `d23ce4e` (tag `v0.20.0`, PodUtils/podnet#445;
the `taken` lines of `SOURCES`):

| Old (`crates/podnet-vnet/src/`) | New |
| --- | --- |
| `stack.rs`: `Clock`, `StackConfig`, `StackEvent`, `NetStats`, `NetStatsSnapshot`, `NetStack` | `pod_netstack::stack::*`, given by `stack.rs` at its old path |
| `stack/reactor.rs`, `stack/service.rs` | gone (the reactor of `pod_netstack::stack`) |
| `stack/socket.rs`: `TcpStream`, `TcpListener`, `UdpSocket`, `SocketCount` | `pod_netstack::stack::{..}` |
| `stack/datagrams.rs`: `VnetDatagrams` | `pod_netstack::UdpDatagrams`, re-exported as `VnetDatagrams` |
| `stack.rs`: `net::LOCAL_IP`, the module `proof` | stay in `stack.rs` |
| `StackConfig::acl: Option<Arc<Acl>>` | `StackConfig::admit: Option<Arc<dyn Admit>>`; `impl Admit for Acl` (`policy.rs`) |
| `StackConfig::dns: Option<Arc<Resolver>>` | `StackConfig::gateway_service: Option<Arc<dyn GatewayService>>`; `impl GatewayService for Resolver` (`dns.rs`, port 53) |
| `StackConfig::subnet: VirtualSubnet` | `pod_netstack::Subnet`; `From` both ways in `addr.rs` (`Resolver::from_maps(.., &config.subnet.into())`) |
| `TcpStream::local_addr`, `peer_addr`, `UdpSocket::recv_from`, the events: `VirtAddr` | `SocketAddr` (`VirtAddr::from(..)` where podnet needs it; the dialer takes it as it is) |
| `device.rs`: `DeviceStats`, `DeviceStatsSnapshot`, `PacketQueue`, `LinkDevice`, `LinkRx`, `LinkTx`, `LINK_OUTBOUND`, `LINK_INBOUND` | `pod_netstack::device::*`, given by `device.rs` |
| `device.rs`: `TunnelDevice` | stays, an alias of `LinkDevice` |
| `device.rs`: `RecordingDevice<D>`, `RecordRx`, `RecordTx` | `pod_netstack::device::RecordingDevice<D, Recorder>` (the alias `RecordingDevice<D>`); `impl PacketTap for Recorder` (`record.rs`) |
| `lib.rs`: the build check of `IFACE_MAX_ADDR_COUNT` | in `pod-netstack` |
| tests `clock_is_monotonic`, `udp_loopback_round_trip`, `a_listener_on_another_address_accepts`, `outside_subnet_is_refused`, `tcp_nodelay_is_recorded`, `application_writes_wake_the_reactor`, `ipv6_loopback_round_trip` (`stack/tests.rs`); the nine tests of `device.rs` | here, with the same names |
| tests `virtual_dns_answers_at_the_gateway`, `acl_refuses_connections` | here with stand-ins; they stay in podnet too, over its `Resolver` and `Acl` |
| tests `proof_completes`, `async_loopback_round_trip` | stay in podnet (the proof); `async_loopback_round_trip` is here too, with no proof |
| `smoltcp = "=0.12.0"` (root, `podnet-netns`, `podnet-vnet`) | `smoltcp = "0.14"`, one version in the tree; `futures` leaves `podnet-vnet` |
| `tests/tcp_window.rs` | its sockets set `CongestionControl::None`, as podnet's stacks (T-188) |
| `src/warp/ip.rs`: `set_cc` and `cc`, `tcp_burst` (`LinkDevice::with_max_burst`), `LINK_STATS` (`LinkDevice::with_stats`) | `StackConfig::congestion_control`, `max_burst`, `link_stats`; `pod_warp::NetSettings` sets them (T-335) |
| `.cargo/config.toml`: `SMOLTCP_IFACE_MAX_ADDR_COUNT = "8"` | stays: each binary that links `pod-netstack` needs it |
| the record (`TODO/`): the citations of the removed files | `PodUtils/podcrates:crates/pod-netstack/src/..` |
| each `pod-*` dependency | tag `v0.20.0`; `podnet-vnet` adds `pod-netstack` |

### Later consumers

* `pod-warp` (podnet `src/warp/ip.rs`, PodUtils/podnet#427): the TCP and UDP
  of WARP over the link device; it drives smoltcp 0.14 with it now.
* podnet's netns router (`crates/podnet-netns/src/netns/backend.rs`,
  `udpflow.rs`): PodUtils/podnet@T-249 moved it onto the stack with the
  calls of "Any IP" (T-222) at podnet `a7350b2`; its hold of a SYN is
  `pod_hold::syn_hold` on this stack (T-170, step 2).
* `pod-netlab` (PodUtils/podvm#178): the network labs of podvm.

## Tests

`cargo test -p pod-netstack`, also with `--all-features`: the stack (TCP and UDP on the loopback, IPv6,
a listener off the stack's address, a refused connect, the wake of a
write, the hooks, the close of each socket, a slow reader, a large
download over two bounded links, the refused binds, a UDP send of two
families), the close of a listener (`listener.rs`) and of a stream
(`close.rs`: the queued bytes, the linger, the abort and its RST on the
link, a reset after a full channel), a burst of SYNs (`burst.rs`: 48 at
once, a full accept queue, `max_connections`, a SYN sent again; no RST on
the link), UDP over a link (`udp.rs`: a burst with no reader, the counters,
the size of a datagram for each build, the fragments, the drop of a
socket), any IP between a router and a
program over a link (`any.rs`:
TCP and UDP to each destination, any IP off, a listener off the router's
address, the ports that a socket on each address shares), the checksums
(`checksum.rs`: a partial sum with `Checksum::Tx` and with the default),
the device (the loopback, the tunnel, the bounds, the hold, the waker, the
tap, the checksums), the subnet, and the rate of each congestion
controller over a delay link (`link_rate`, on a virtual clock). The plant
`netstack-receive-with-no-room` (`scripts/plants/`) proves the test of the
slow reader; the plant `netstack-link-rate-no-controller` proves the tests
of the rate; the plants `netstack-any-ip-not-set` and
`netstack-listener-owns-by-has-ip-addr` prove the tests of any IP. The
plants `netstack-dropped-listener-stays`,
`netstack-dropped-stream-closes-at-once`,
`netstack-dropped-stream-never-lingers`, `netstack-reset-with-no-room`,
`netstack-syn-finds-no-listen-socket` and
`netstack-syn-again-to-a-listen-socket` prove the tests of the close and
of the listeners; `netstack-abort-frees-the-socket-at-once` proves the test
of the RST on the link. The plants `netstack-udp-backlog-lost-at-a-full-channel`,
`netstack-udp-backlog-waits-for-the-tick`, `netstack-udp-loss-not-counted`,
`netstack-udp-send-loss-not-counted`, `netstack-udp-over-the-limit-goes` and
`netstack-dropped-udp-socket-stays` prove the tests of UDP.

## Sources

`SOURCES` lists each imported file with its blob.
