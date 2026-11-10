# One QUIC stack: quinn against noq (T-018)

This page is the reading for call C43 (`docs/decisions.md`). The numbers
come from the probe in section 3, run on 2026-11-10.

## 1. The uses

| Use | Consumer | Stack | Calls that the code needs |
| --- | --- | --- | --- |
| The QUIC edge of Cloudflare (`src/cloudflare/quic.rs`) | podnet | quinn 0.11.12, default features (ring) | `Endpoint::client`, `connect_with`, `ClientConfig`, `QuicClientConfig::try_from(rustls)`, `TransportConfig` (idle, keep-alive, `initial_mtu`, `max_concurrent_bidi_streams`), `open_bi`, `accept_bi`, `ReadToEndError` |
| QUIC over WARP (`src/warp/quic.rs`) | podnet | quinn | `AsyncUdpSocket` (`WarpUdpSocket`), `UdpPoller`, `Endpoint::new_with_abstract_socket`, `MtuDiscoveryConfig::upper_bound`, `RecvMeta`, `Transmit` |
| MASQUE over HTTP/3 (`src/warp/h3.rs`) | podnet | quinn | Raw `Connection`: `open_uni`, `open_bi`, `send_datagram`, `read_datagram`, `datagram_*_buffer_size`, `SendDatagramError::TooLarge`. The HTTP/3 frames, SETTINGS, capsules and QPACK are podnet code. podnet has no `h3` or `h3-quinn` dependency |
| Stream dispatch of the edge (`src/proxy.rs`, `src/tunnel.rs`, `src/cloudflare/rpc.rs`) | podnet | quinn | `SendStream`, `RecvStream`, `ReadError::Reset`, `VarInt`, `ConnectionError::LocallyClosed` |
| The quick tunnel over QUIC (`quick/quic.rs`, `quick/quic_serve.rs`) | podssh | noq 1.4.0, no default feature (`runtime-tokio`, `rustls`) | The same client calls as podnet's edge, a system `UdpSocket` for `Endpoint::new`, `wait_idle`, datagram receive buffer |
| The QUIC road (`quic_road/`, `serve/quic.rs`) | podssh | noq | Client and server `Endpoint`, `ServerConfig::new` with a token key, `Incoming`, `HandshakeData`, `TransportConfig` |
| The iroh road (`podssh-iroh`) | podssh | noq through iroh 1.4.1 (iroh pins `noq`, `noq-proto`, `noq-udp`) | `noq_proto::congestion` (`Bbr3Config`, `CubicConfig`, `NewRenoConfig`), controller factory, multipath (`PathId`, `open_path`), NAT traversal, address discovery, `UdpSender` |

| Fact | quinn (podnet) | noq (podssh) |
| --- | --- | --- |
| Version | 0.11.12 (`quinn-proto` 0.11.19, `quinn-udp` 0.5.16) | 1.4.0 (`noq-proto`, `noq-udp` 1.4.0) |
| Crypto provider | rustls 0.23 with `ring` | rustls 0.23; no `ring` and no `aws-lc` feature; the provider of `podssh-ws` |
| Keys of the endpoint | `ring` HMAC by default | podssh writes `HmacKey` (reset key) and `HandshakeTokenKey` (token key) over HMAC-SHA-256 in pure Rust (about 50 lines each) |
| Custom UDP socket | `AsyncUdpSocket` with `create_io_poller`, `try_send(&self)`, `poll_recv(&self)` | `AsyncUdpSocket` with `create_sender`, `poll_recv(&mut self)`; `UdpSender::poll_send` |
| Datagrams | yes | yes |
| HTTP/3 | podnet's own frames over the `Connection` | none (podssh uses h2 and WebSocket) |

## 2. The API gap

* The calls of podnet's edge, WARP and MASQUE code exist in noq with the
  same names: `TransportConfig` methods, `MtuDiscoveryConfig::upper_bound`,
  `send_datagram`, `read_datagram`, `max_datagram_size`,
  `SendDatagramError`, `ReadToEndError`, `ConnectionError::LocallyClosed`,
  `export_keying_material`, `stopped`, `set_priority`, `wait_idle`. The
  probe compiled with a one-line change of the crate name.
* The differences that a port meets: `AsyncUdpSocket` (a sender object in
  place of a poller; `poll_recv` takes `&mut self`; the socket goes in a
  `Box`, not an `Arc`; `max_transmit_segments` moves to the sender);
  `RecvMeta` is `#[non_exhaustive]` (use `Default`) and has the new
  fields `interface_index` and `timestamp`.
* `h3` and `h3-quinn` exist for quinn, and there is no noq binding
  (`cargo search h3-noq` finds none). podnet does not use `h3`, so podnet
  loses nothing. A crate that wants `h3` must write the binding to
  `Connection`, or keep its own frames as podnet does.
* quinn has none of what iroh needs: multipath (`PathId`, `open_path`),
  NAT traversal updates, observed external address, `UdpSender`. iroh 1.4.1
  pins noq; a quinn-based `pod-quic` cannot give iroh its connection type.
  A binary with `pod-iroh` and a quinn-based `pod-quic` (podssh with the
  feature `iroh`) carries both stacks.
* noq's rustls glue uses RustCrypto `aes-gcm` for the packets of the
  Initial space. It adds 24 crates (section 3).

## 3. The probe

Two scratch crates outside the tree use one source (`body.rs`), with
`use quinn as q` and `use noq as q`. Each crate has a client with a custom
UDP socket (the shape of `WarpUdpSocket`: a bounded channel to a writer
task, a queue from a reader task, a `PollSender` for each sender) and a
stock socket. The server uses the stock socket. Both stacks use rustls 0.23
with `ring`, TLS 1.3, and one process on the loopback (4 vCPU). The
settings are those of podnet's edge: `initial_mtu` 1232, keep-alive 1 s,
256 bidi streams, 4 MiB datagram buffers.

Commands (`T` is a target):

```sh
cargo zigbuild --release --target T -p qn -p nq   # opt-level s, lto, strip, panic abort
./qn stock stream 1024; ./nq custom stream 1024   # 1 GiB, 3 runs each
./qn stock datagram 5;  ./nq custom datagram 5    # 1000-byte datagrams, 5 s, 3 runs
cargo tree -p qn --target x86_64-unknown-linux-musl | wc -l
```

### Build: static on each target

| Target | quinn (bytes) | noq (bytes) | Static |
| --- | --- | --- | --- |
| x86_64 musl | 1,857,400 | 2,077,344 | yes (no `PT_INTERP`) |
| aarch64 musl | 1,615,048 | 1,806,048 | yes |
| i686 musl | 1,550,396 | 1,762,868 | yes |
| armv7 musleabihf | 1,499,032 | 1,696,056 | yes |
| arm musleabihf | 1,539,104 | 1,750,768 | yes |
| riscv64gc musl | 1,427,504 | 1,647,392 | yes (no `PT_INTERP`, no `NEEDED`) |
| powerpc64le musl | 1,710,424 | 1,941,512 | yes |
| loongarch64 musl | 1,595,064 | 1,802,400 | yes (no `PT_INTERP`, no `NEEDED`) |

noq is 196 to 231 KB larger (11 to 14 percent of the whole probe). Both
stacks build with cargo-zigbuild on each target of the consumers, with
`ring` as the C part. Windows and macOS were not built here: the
consumers build them on their own runners (`docs/design.md`, section 7);
noq with `runtime-tokio` and `rustls` has no C dependency of its own.

### Dependencies (x86_64 musl)

| | quinn | noq |
| --- | --- | --- |
| `cargo tree \| wc -l` | 130 | 185 |
| unique crates, normal edges | 60 | 84 |

The 24 extra crates of noq are `aes-gcm` with its parts (`aead`, `aes`,
`cipher`, `ctr`, `ghash`, `polyval`, `generic-array`, `typenum`, and
others), `derive_more`, `enum-assoc`, `convert_case`, `identity-hash`,
`sorted-index-buffer`, `tokio-stream` and `syn` 2. All are pure Rust.

### Throughput (x86_64 musl, MiB/s of a 1 GiB stream)

| Socket | quinn (3 runs) | noq (3 runs) | noq / quinn |
| --- | --- | --- | --- |
| stock (GSO and GRO) | 323.4, 322.0, 319.0 | 293.0, 286.4, 276.9 | 0.89 |
| custom (one datagram for each call) | 266.8, 252.7, 252.5 | 237.1, 235.7, 228.6 | 0.91 |

### Datagrams (received for each second, 1000 bytes, 5 s)

| Socket | quinn (3 runs) | noq (3 runs) | noq / quinn |
| --- | --- | --- | --- |
| stock | 391,304; 395,228; 409,201 | 321,489; 331,713; 344,222 | 0.83 |
| custom | 279,544; 284,007; 290,067 | 245,648; 250,387; 253,345 | 0.88 |

The two ends share one process and one CPU budget, so the numbers are a
ratio and not a speed of a network. noq is 9 to 17 percent slower on the
loopback in this probe.

## 4. Result

* Both stacks link static on all eight targets, with the same C part
  (`ring`, or none for noq with the provider of the caller).
* noq costs 11 to 14 percent of size, 24 crates and 9 to 17 percent of
  loopback speed. quinn cannot serve iroh.
* The cost of a second stack is the full size of the other stack in each
  binary that has `pod-iroh` next to `pod-cloudflare` or `pod-relay`
  (podssh `iroh` builds). One stack removes that.

Recommendation: noq for `pod-quic`, as call C43 says. `pod-quic` takes
the TLS provider from the caller (`docs/design.md`, section 1, rule 4), as podssh's
road does, and implements the reset key and the token key over HMAC in
pure Rust, so that noq needs no `ring` feature.
