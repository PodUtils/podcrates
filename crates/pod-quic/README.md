# pod-quic

QUIC for the crates of podcrates and their binaries, on noq 1.4 (call
C43): the settings of the transport, the reset key and the token key of an
endpoint in pure Rust, an endpoint on a UDP socket of the system or on any
`pod_core::Datagrams`, a connection within the handshake's wait, a race of
handshakes, the probe of UDP by a Version Negotiation, and `Datagrams` over
a link that has only async send and receive (the userspace stack under
WARP).

The sources are podssh's `quick/quic.rs`, `quick/probe.rs` and the
`TokenKey` of `serve/quic.rs` (podssh-relay), and podnet's
`src/warp/quic.rs` (`WarpUdpSocket`, at the fix of PodUtils/podnet@T-225). noq has no
default feature here (`runtime-tokio`, `rustls`): no `ring` and no `aws-lc`
in the stack, as the keys are this crate's and the TLS provider is the
caller's.

## API

| Module | What |
| --- | --- |
| (root) | `Settings` (the transport and the handshake's wait; `Default` is cloudflared's), the constants `HANDSHAKE_WAIT`, `SILENCE`, `KEEPALIVE`, `FIRST_PACKET_V4`, `FIRST_PACKET_V6`, `MAX_STREAMS`, `DATAGRAMS`; `over` (`endpoint::over`); `noq` (the stack, re-exported) |
| `Settings` | `handshake_wait`, `silence`, `keepalive`, `first_packet_v4`, `first_packet_v6`, `max_streams`, `datagram_buffer`, `mtu_ceiling`; `first_packet(v6)`, `transport(v6)` (a `noq::TransportConfig`) |
| `keys` | `ResetKey` (`noq::crypto::HmacKey`), `TokenKey` (`noq::crypto::HandshakeTokenKey`): HMAC-SHA-256 of 32 random bytes; `random()`, `from_entropy(&mut dyn pod_core::entropy::Entropy)` |
| `endpoint` | `config()` (an `EndpointConfig` with a `ResetKey`), `server_config(crypto)` (a `ServerConfig` with a `TokenKey`), `on_socket(std UdpSocket, Option<ServerConfig>)`, `bind(addr)`, `system(v6)`, `server(config, addr)`, `over(Arc<dyn Datagrams>, Option<ServerConfig>)` |
| `connect` | `tls(provider, roots, alpn)` (TLS 1.3 alone), `client_config(tls, &Settings, v6)`, `connect(&endpoint, addr, server_name, tls, &Settings)`, `handshake(&[addr], server_name, tls, &Settings)` (each address at once; the first that answers and its time) |
| `probe` | `RESERVED`, `PADDED`, `WAIT`, `packet(&dcid)`, `answers(datagram, &dcid)`, `probe(&[addr], wait)` |
| `queued` | `Link` (async `send_to`, `recv_from`, `local_addr`), `QueuedDatagrams::{new, with_queue, counts}` (a `pod_core::Datagrams`), `SEND_QUEUE` |
| `socket` | `DatagramSocket::new(Arc<dyn Datagrams>)`: noq's `AsyncUdpSocket` with a `UdpSender` for each task of noq |

Each function returns a `pod_core::Failure`: the stage `Quic` (an endpoint,
a connection, a handshake), `Tls` (a configuration of TLS for QUIC) or
`Connect` (a UDP socket, the probe), and the class of the cause (`Timeout`
for a silence, `Refused` for a close of the peer, `Protocol` for an error of
the transport, and the class of an I/O error).

Each endpoint and each `QueuedDatagrams` must be made in a tokio runtime,
which drives it.

## Features

None.

## Settings

The crate reads no variable of the environment and keeps no process
state.

| Value | Where |
| --- | --- |
| The TLS provider and the roots | `connect::tls(provider, roots, alpn)`, or the caller's own `rustls::ClientConfig` to `connect::client_config` |
| The TLS of a server | the caller's `noq::crypto::ServerConfig` to `endpoint::server_config` |
| The waits, the keepalive, the first packets, the streams, the datagram buffer, the ceiling of the MTU | `Settings` |
| The way of the datagrams (the system, WARP, UDP in a stream) | `endpoint::over(Arc<dyn Datagrams>, ..)`; a link with no poll interface through `queued::QueuedDatagrams` |
| The size of the send queue of a link | `QueuedDatagrams::with_queue` (`SEND_QUEUE`, 1024, by default) |
| The random bytes of the keys | `ResetKey::from_entropy`, `TokenKey::from_entropy` (`random()` reads the system) |

## Parity

Against podssh `crates/podssh-relay` at `af214a6`:

| Capability of podssh | Here |
| --- | --- |
| `quick/quic.rs`: `HANDSHAKE_WAIT`, `SILENCE`, `KEEPALIVE`, `FIRST_PACKET_V4`, `FIRST_PACKET_V6`, `MAX_STREAMS`, `DATAGRAMS`, `transport(v6)` | [x] `Settings` and its default, `Settings::transport` |
| `ResetKey`, `endpoint_config()` | [x] `keys::ResetKey`, `endpoint::config()` |
| `endpoint(v6)` | [x] `endpoint::system(v6)`, with its words ("no UDP socket for QUIC", "no QUIC endpoint") |
| `tls(roots)` on podssh's provider | [x] `connect::tls(provider, roots, alpn)`: the provider and the ALPN are the caller's |
| `connect(..)` within the handshake's wait, "no answer over QUIC within 5 s" | [x] `connect::connect(.., &Settings)` |
| `handshake(found, tls)`: each region's first address of each family at once, the close at once, `CLOSE_WAIT` | [x] `connect::handshake(&[addr], ..)`; the resolution of the regions of an edge stays in podssh |
| `server_name`, `alpn` (data of `pod-services`), `connect_edge`, `EdgeConnection` | stay in podssh until `pod-cloudflare` |
| `quick/probe.rs`: `packet`, `answers`, `probe`, `RESERVED`, `PADDED`, `WAIT` | [x] `probe`; the words "said nothing within 300 ms" and the refusal of a closed port stay |
| `probe::edge` (the regions of an edge) | stays in podssh |
| `serve/quic.rs`: `TokenKey` | [x] `keys::TokenKey`; the role's server stays in podssh until `pod-relay` |

Against podnet `src/warp/quic.rs` at `2fed3ff`:

| Capability of podnet | Here |
| --- | --- |
| `WarpUdpSocket`: a bounded send queue (1024) and a writer task, a reader task into a queue, a drop that stops both | [x] `queued::QueuedDatagrams` over a `Link`; the queue is a setting (`with_queue`) |
| The send poller that keeps its waker between two polls (PodUtils/podnet@T-225) | [x] `SendReady` of `queued.rs`, with its tests |
| A closed link fails a send at once and each receive | [x] a receive now reads each datagram that came before the end, then fails |
| `endpoint(stack)` with `set_default_client_config`, `client_config()` (podnet's trust, the ceiling of the MTU of its tunnel) | podnet keeps both; `Settings::mtu_ceiling` holds the ceiling for a caller of `Settings` |

Differences, with the choice:

* noq makes one sender for each task (the endpoint, and each connection);
  a `Datagrams`, tokio's UDP socket and podnet's `SendReady` keep one waker.
  `socket.rs` gives the way one shared waker, the fan-out, which wakes each
  sender that waits. The same holds for podnet on quinn: quinn 0.11 makes a
  poller for each connection, and the pollers of one `WarpUdpSocket` share
  one `AtomicWaker`, so two connections on one endpoint (the HA connections
  of the WARP leg) can lose a wake-up of a full queue. podnet fixed it
  in its own code (PodUtils/podnet@T-228, T-159) with a list of wakers.
* The errors are `pod_core::Failure` in place of `String` (podssh) and
  `anyhow` (podnet); the texts are the same, except the probe's: "UDP got
  no answer to a QUIC packet" (podssh: "UDP to the edge got ...").
* A transmit of several segments goes into the queue whole or not at all.

## Adoption map

### podssh

| Old | New |
| --- | --- |
| `quick/quic.rs`: `HANDSHAKE_WAIT`, `SILENCE`, `KEEPALIVE`, `FIRST_PACKET_V4`, `FIRST_PACKET_V6`, `MAX_STREAMS` | `pub use pod_quic::{..}` |
| `quick/quic.rs`: `transport`, `ResetKey`, `endpoint_config`, `endpoint`, `connect`, `tls` | glue over `pod_quic::{Settings, endpoint::config, endpoint::system, connect::connect, connect::tls}` with `podssh_ws::crypto::provider()` and the ALPN of the edge |
| `quick/quic.rs`: `handshake(found, tls)` | the resolution of the regions, then `pod_quic::connect::handshake` |
| `quick/probe.rs`: `RESERVED`, `PADDED`, `WAIT`, `packet`, `answers`, `probe` | `pub use pod_quic::probe::{..}`; `probe` maps the `Failure` to its text; `edge` stays |
| `serve/quic.rs`: `TokenKey`, `Endpoint::new(endpoint_config, ..)` | `pod_quic::keys::TokenKey`, `pod_quic::endpoint::on_socket` |
| `tests/quick_probe.rs` | `tests/probe.rs` here |
| `podssh-relay/Cargo.toml`: feature `quick` | adds `dep:pod-quic` (tag `v0.14.0`) |

`adoption/podssh/pod-quic.patch` (at podssh `c5636c9`) is the hand-off.

### podnet

podnet moves from quinn 0.11 to noq 1.4 through this crate. Each user of
quinn moves: `src/cloudflare/{mod,quic,rpc}.rs`, `src/proxy.rs`,
`src/tunnel.rs`, `src/warp/{h3,l4,quic,session}.rs`, `src/ws.rs`; the
names of the API are the same (`quinn::` becomes `noq::`).

| Old | New |
| --- | --- |
| `quinn = "0.11"` | `noq = { version = "1.4", default-features = false, features = ["runtime-tokio", "rustls"] }` and `pod-quic` (tag `v0.14.0`) |
| `src/warp/quic.rs`: `WarpUdpSocket`, `SendPoller`, `SendReady` | `pod_quic::queued::QueuedDatagrams` over a `Link` of the stack's `UdpConn`, and `pod_quic::over` |
| `quinn::Endpoint::client(addr)` (`cloudflare/quic.rs`, `warp/h3.rs`) | `pod_quic::endpoint::bind(addr)` |
| `quinn::Endpoint::server(quinn::ServerConfig::with_crypto(c), addr)` (tests of `cloudflare/quic.rs`, `warp/l4.rs`, `warp/session.rs`) | `pod_quic::endpoint::server(pod_quic::endpoint::server_config(c)?, addr)` |
| `EndpointConfig::default()` (ring's reset key) | `pod_quic::endpoint::config()` (in `over`, `bind`, `server`) |
| `RecvStream::read_chunk(max, true)` and `chunk.bytes` (`warp/{h3,l4,session}.rs`) | `RecvStream::read_chunk(max)`, which reads in order and gives the `Bytes` |
| `ConnectionStats::path.rtt`, `path.lost_packets` (`tunnel.rs`) | `Connection::rtt(noq::PathId::ZERO)`, `ConnectionStats::lost_packets` (noq has paths) |
| the tests `send_poller_reports_ready_when_the_queue_has_room`, `a_drain_after_pending_wakes_the_send_poller` | `src/queued_tests.rs` here |

`adoption/podnet/pod-quic.patch` (at podnet `5465007`) is the hand-off.

## Tests

`cargo test -p pod-quic`: the unit tests of each module, and on the
loopback: `tests/probe.rs` (podssh's probe against stand-ins of a Version
Negotiation and a closed port), `tests/handshake.rs` (podssh's three tests
of the doctor's handshake, and an untrusted server), `tests/datagrams.rs`
(a stream of 1 MiB each way and a burst of datagrams over the system's
`Datagrams`, over a queued link, and over a queued link of two
datagrams). The stand-in server runs TLS on ring with a certificate of
pod-tls's tests. The fuzz target `quic_probe_answer` reads `probe::answers`.

## Sources

`SOURCES` lists each imported file with its blob.
