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
| `NetStack` | `loopback()`, `loopback_with(config)`, `over_link(config)` (each spawns its reactor: call it in a runtime of tokio); `listen`, `connect`, `connect_from`, `bind_udp` (each takes `impl Into<SocketAddr>`); `link_in`, `link_out` (the queues of a pump); `stats`, `device_stats`, `socket_count` (live and peak); `take_events`; `shutdown`; `Drop` stops the reactor. |
| `StackConfig` | The settings (below). |
| `StackEvent` | `ListenerBound`, `Accepted`, `Connected`, `Refused`, `Closed`, `Datagram`. |
| `NetStats`, `NetStatsSnapshot` | `accepted`, `connected`, `refused`, `closed`, `datagrams`, `dropped` (an inbound connection that the accept queue could not take). |
| `TcpStream` | `AsyncRead`, `AsyncWrite`; `established()` (the handshake, or `ConnectionRefused`, `ConnectionReset`), `local_addr`, `peer_addr`, `set_nodelay`, `shutdown` (a FIN after the queued data), `abort` (a RST). A dropped stream asks the reactor to close: the socket closes when nothing is owed in either direction. |
| `TcpListener` | `accept`, `local_addr`. |
| `UdpSocket` | `send_to`, `recv_from`, `local_addr`, `into_datagrams`. |
| `UdpDatagrams` | A UDP socket as `pod_core::Datagrams` (QUIC, a DNS client over the stack). |
| `Clock` | The monotonic clock in milliseconds of smoltcp. |
| `Subnet` | The address plan: `v4`/`v4_prefix`, `v6`/`v6_prefix`, `gateway_v4`, `gateway_v6`, `gateway_for`, `contains`, `prefix_for`. Default `10.66.0.0/24` and `fd00:66::/64`. |
| `device` | `LinkDevice` (`loopback`, `tunnel`, `tunnel_bounded`, `link`, `with_inbound_limit`, `with_stats`, `with_max_burst`, `add_local`), its tokens `LinkRx`, `LinkTx`; `PacketQueue` (bounded, with a waker); `DeviceStats`, `DeviceStatsSnapshot`; `LINK_OUTBOUND` (512), `LINK_INBOUND` (4096); `RecordingDevice`, `PacketTap`, `Direction`. |
| `hooks` | `Admit`, `GatewayService`, `GATEWAY_DATAGRAM` (1232). |
| `MAX_ADDRESSES` | `smoltcp::config::IFACE_MAX_ADDR_COUNT` of the build (8 or more). |
| `smoltcp` | The re-export of smoltcp, so that a consumer names one version. |

## The hooks

| Trait | Called | podnet |
| --- | --- | --- |
| `Admit::admits(&self, target: SocketAddr) -> bool` | At each listen, connect and UDP bind (a refusal is `PermissionDenied`), and at each UDP send (a refused datagram is dropped and counted in `refused`). No admit: each target in the subnet. A closure `Fn(SocketAddr) -> bool` is an `Admit`. | `Acl` (`Acl::allows`) |
| `GatewayService::port(&self) -> u16`, `GatewayService::answer(&self, from: SocketAddr, request: &[u8]) -> Option<Vec<u8>>` | The stack owns the IPv4 gateway, binds `port` there and gives each datagram (at most `GATEWAY_DATAGRAM` bytes) to `answer`; `None` sends nothing. It runs in the task of the stack. | `Resolver` (port 53, `Resolver::handle`) |
| `PacketTap::packet(&self, direction: Direction, packet: &[u8])` | `RecordingDevice<D, T>` wraps a device and calls the tap for each packet, `Inbound` (link to stack) and `Outbound` (stack to link). A closure `Fn(Direction, &[u8])` is a `PacketTap`. | `Recorder` (the pcap and JSON capture stay in podnet) |

## Settings

`StackConfig`; the default is podnet's.

| Field | Default | What |
| --- | --- | --- |
| `subnet` | `10.66.0.0/24`, `fd00:66::/64` | The private address space; a connect or a bind outside it is `AddrNotAvailable`. |
| `local_v4`, `local_v6` | `10.66.0.2`, `fd00:66::2` | The addresses of the stack. |
| `mtu` | 1280 | The MTU of the link (at least 576). |
| `tcp_buffer` | 64 KiB | The send and the receive buffer of each TCP socket. |
| `udp_packets`, `udp_payload` | 64, 64 KiB | The buffers of each UDP socket. |
| `accept_pool` | 8 | The listening sockets of a listener (its half-open limit). |
| `max_connections` | 4096 | The open connections (a connect past it is `WouldBlock`). |
| `admit` | none | `Option<Arc<dyn Admit>>`. |
| `gateway_service` | none | `Option<Arc<dyn GatewayService>>`. |
| `random_seed` | the time | The seed of smoltcp for its ports and sequence numbers. |
| `max_poll_interval` | 100 ms | The longest sleep of the reactor. |

No feature.

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
* The stack sets no congestion controller (`CongestionControl::None`), as
  podnet did. The next section gives the rate of each controller.

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
(6,766 segments of data). The reactor of `NetStack` runs
`Interface::poll`; T-197 has its ACKs.

## Parity

With podnet `crates/podnet-vnet` at `f7ed86b` (the files are the same at
`da6705f`):

* Each item of the cut is here, with its behaviour: the reactor and its
  poll loop (`max_poll_interval`, the kick of a write, the waker of the
  inbound queue), the listeners with their pool, the owned address of a
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
  * A poisoned lock of a packet queue does not panic.
  * The messages of the errors name the policy and the subnet, not the ACL
    and the virtual subnet.
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
| `.cargo/config.toml`: `SMOLTCP_IFACE_MAX_ADDR_COUNT = "8"` | stays: each binary that links `pod-netstack` needs it |
| the record (`TODO/`): the citations of the removed files | `PodUtils/podcrates:crates/pod-netstack/src/..` |
| each `pod-*` dependency | tag `v0.20.0`; `podnet-vnet` adds `pod-netstack` |

From the import, the files of the cut take fixes only in podnet, until
podnet takes the patch.

### Later consumers

* `pod-warp` (podnet `src/warp/ip.rs`, PodUtils/podnet#427): the TCP and UDP
  of WARP over the link device; it drives smoltcp 0.14 with it now.
* podnet's netns router (`crates/podnet-netns/src/netns/backend.rs`,
  `udpflow.rs`): after podnet's T-174 (the veth wire, `medium-ethernet`),
  with step 2 of T-170.
* `pod-netlab` (PodUtils/podvm#178): the network labs of podvm.

## Tests

`cargo test -p pod-netstack`: the stack (TCP and UDP on the loopback, IPv6,
a listener off the stack's address, a refused connect, the wake of a
write, the hooks, the close of each socket, a slow reader, a large
download over two bounded links, the refused binds), the device (the
loopback, the tunnel, the bounds, the hold, the waker, the tap), the
subnet, and the rate of each congestion controller over a delay link
(`link_rate`, on a virtual clock). The plant
`netstack-receive-with-no-room` (`scripts/plants/`) proves the test of the
slow reader; the plant `netstack-link-rate-no-controller` proves the tests
of the rate.

## Sources

`SOURCES` lists each imported file with its blob.
