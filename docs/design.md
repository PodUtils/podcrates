# Design

The rules of the crates of podcrates, their sources, the shared traits,
the targets, the procedure for one crate, the checks and the releases.
`AGENTS.md` has the rules of each session; `docs/decisions.md` has the
rulings of the owner; `TODO/` has the work.

## 1. Rules for each crate

The ruling of the owner, 2026-11-05: "we use whatever is necessary,
including c, zig, modern rust, no backwards compat. our users & clients
do not care about building the tool, they care about downloading &
using it. the only rule is: everything must be statically compiled".
Later the same day: modern and stable Rust, "even nightly/bleeding edge
if its necessary to maintain performance, security whatever we need".

Rule 1 is the only hard rule. Rules 2 to 12 are the design of the
crates. `pod-todo` and `pod-testkit` are tools: rules 4 to 10 do not
apply to them.

1. **Static.** Each crate links into a static binary on each target of
   section 7. Linux: no `NEEDED` entry and no program interpreter (musl;
   zig cc through cargo-zigbuild for the cross targets). Windows: the C
   runtime linked in (`+crt-static`), only the DLLs of the system.
   macOS: only the libraries of the system (`libSystem`, `libiconv`,
   `CoreFoundation`, `Security`; Apple allows no static `libSystem`). A
   C dependency is allowed when it links static on each target.
2. **Modern Rust.** Edition 2024 and the current stable Rust. The
   `rust-version` is the stable of the day (`rust-toolchain.toml`), and a
   session raises it when a new stable comes out. A nightly feature is
   allowed when it gives a necessary gain (speed, security); the entry
   names the gain and its measurement, and `rust-toolchain.toml` then
   pins the nightly. Each consumer moves its toolchain to match.
3. **No backwards compatibility.** A public API, a name or a format of
   the wire can change in any version. No test against a released
   binary.
4. **The binary chooses the crypto provider.** A crate that makes a TLS
   or QUIC configuration takes an `Arc<rustls::crypto::CryptoProvider>`
   from its caller, so that one binary has one provider. A crate does not
   install a process default. The tests use ring. `pod-tls` keeps
   podssh's pure-Rust provider (feature `pure`).
5. **The caller gives the way out.** A crate that opens a connection
   takes an `Arc<dyn Dial>` (section 6). `pod-proxy` has a function that
   reads `HTTPS_PROXY`, `ALL_PROXY` and `NO_PROXY` when the caller asks;
   it keeps the origin of each value (a `+exec:` proxy only from a value
   that the user gave on purpose, as podnet does now).
6. **Settings, not the environment.** A crate does not read an
   environment variable and does not choose a directory. The consumer
   fills a settings struct once and passes it. Each variable of the
   sources (about 25 of podssh, about 12 of podnet) becomes a field, and
   the consumer keeps the variable. The adoption map lists each one.
7. **No process singletons.** A crate keeps no global state, except a
   cache that is keyed and can be turned off. The globals of the sources
   (podnet `CHOICE`, `DETECTED`, `ROOTS`, `PINS`, `RELAY`; podssh `SAY`,
   the rank atomics, the `KEPT` caches) become fields of a value that the
   consumer owns.
8. **No output.** A crate does not print and does not exit the process.
   Diagnostics go to `tracing`. A line for the user is a typed event
   (`pod_core::Event`) that the consumer shows; podssh's `Say` hooks
   become events.
9. **Typed failures.** An error carries its stage (podnet's `Hop`: the
   proxy, DNS, TLS, the edge, the relay, WARP) and its class (podnet's
   `Fault`: usage, unavailable, config, protocol and the rest). The
   consumer maps the class to its exit code. Counters (podnet's
   `decisions`, `metrics`) go to an `Observer` that the consumer gives.
10. **Names of the consumer are parameters.** An AppID, a user agent
    (podssh builds `podssh/VERSION` in about 15 places), a client name.
11. **Tests come with the code.** The tests, the plants and the fuzz
    targets of the source move with it and keep their names. A test uses
    loopback only and starts with the `need_loopback!` guard of
    `pod-testkit` when it binds. A live test is `#[ignore]` and runs in a
    live workflow. A test does not set a process variable.
12. **Size.** A source file has at most 500 lines (Q2). A file that is
    longer is split at import. There is no allowlist.


## 2. Repository

| Path | What |
| --- | --- |
| `Cargo.toml` | virtual workspace; `[workspace.package]` (edition 2024, `rust-version`, version, license 0BSD, repository); `[workspace.dependencies]`; each `pod-*` dependency has `path` and `version` |
| `rust-toolchain.toml` | the toolchain (stable, or a pinned nightly when an entry needs it), the targets and the components |
| `crates/pod-*` | `Cargo.toml`, `README.md` (purpose, API, features, settings, the parity list, the adoption map), `LICENSE`, `SOURCES`, `src/`, `tests/` |
| `vendor/` | planned: the patched forks that a crate needs (tailscale-rs, iroh-relay), each with its patches, its upstream commit and its licence (as podssh `vendor/README.md`) |
| `adoption/CONSUMER/` | the pinned commit of each consumer, its `todo.toml`, and for each crate the trial patch (section 8, step 7) |
| `TODO/` | the record (the podssh model): `RULES.md`, `ROADMAP.md`, `INDEX.md`, `PROGRESS.md`, `issues.md`, `github.txt`, `todo.toml`, one area file for each milestone |
| `docs/` | `HANDOFF.md` (the state of unfinished work), `decisions.md`, `design.md`, `development.md`, `maintenance.md`, `session-protocol.md` (the talk between sessions), `releases/` (the notes of each tag), `research/` (the studies behind the plan) |
| `scripts/` | `adoption.sh` (the adoption trial); `agent/`; `binfmt.sh`; `ci/` (`root-confine-tests.sh`: the tests of `pod-confine` that need root, as root; `root-enter-confine-tests.sh`: the confinement tests of `pod-enter` on the rungs with a chroot, as root); `check-consumer-toolchains.py`; `check-consumer-tests.py` (each test target that a consumer's gate names exists); `check-crates-io-names.py`; `check-design-paths.py` (this table and the crate table of `README.md`); `check-github-numbers.py`; `check-probe.py` (the static probes cover each feature); `check-publish-order.py` (`cargo publish --workspace` can order the crates); `check-size.py`; `check-sources.py`; `check-static.sh`; `check-workflows.py`; `check-zig-mirrors.py`; `cloud-setup.sh` (zig, cargo-zigbuild, cargo-deny, the targets); `fetch-zig.sh`; `fuzz.sh`; `names-proxy.py` (a CONNECT proxy that takes names alone, for the live tests); `openssl-perl.sh` (a perl of Windows for the OpenSSL of `pod-gsr`); `plant-each.sh`; `plants/` (the planted defects); `pull-image.sh`; `retry.sh`; `root-netns-tests.sh` (the netns tests of `pod-hold` as root; a skip fails); `secrets-report.py`; `services/` (the measurements of the services of other parties by hand, with their `SOURCES`: `edge-facts.sh`); `zig-mirrors.txt` |
| `scripts/agent/` | `check.sh` (the checks before a push); `new-issues.py`, `watch-issues.sh` (new issues of this repository); `org-issues.py`, `watch-org.sh` (the messages to a session in each repository of PodUtils); `runs.py`, `watch-runs.sh` (the runs of a commit); `secrets_like.py` (two TruffleHog detectors over files); `togit.sh` (path dependencies to a git tag) |
| `fuzz/` | the fuzz targets of the moved code (cargo-fuzz, its own workspace), their seeds (`fuzz/seeds/`) and their stable test; `fuzz/README.md` lists each target and the targets that wait for their crate; `scripts/fuzz.sh` runs them |
| `examples/` | `static-probe`, `static-probe-lean` (section 7) and the binaries of the live workflows (ring provider; `publish = false`) |
| `images/` | the build and test images, pinned by digest |
| `schemas/` | the JSON Schemas (`*.schema.json`) that go out with each tag |
| `.github/` | `workflows/` (section 9), `dependabot.yml`, `secrets-allow.txt` |
| `AGENTS.md`, `CLAUDE.md`, `.claude/` | the rules for each session; the settings and the session-start hook |

Toolchain facts:

* `rustfmt.toml`: `max_width = 120`, `use_small_heuristics = "Max"`,
  `style_edition = "2024"`.
* `.cargo/config.toml`: `[alias] todo = "run -q -p pod-todo --"`; `[env]
  SMOLTCP_IFACE_MAX_ADDR_COUNT = "8"`. Cargo does not read the config of
  a dependency: each consumer of `pod-netstack` sets the same line (the
  adoption map says so).
* smoltcp 0.14, in `pod-netstack` only, with the features of podnet
  (`medium-ip`, `proto-ipv4`, `proto-ipv6`, `socket-tcp`, `socket-udp`,
  `socket-tcp-reno`, `socket-tcp-cubic`, `assembler-max-segment-count-32`;
  each exists in 0.14).
* `deny.toml`: the targets of section 7; `multiple-versions = "warn"`;
  `allow-git` holds only the upstreams of the forks when a fork is not
  in `vendor/`; one ignored advisory, RUSTSEC-2023-0071 (`rsa`
  0.9, feature `rsa` of `pod-tls`: verify only; the advisory is a timing
  attack on operations with the private key). It runs with all features.


## 3. `pod-core`: the bottom crate

The agreed list has no crate below `pod-proxy`. The review of the code
found cycles that only a bottom crate breaks: in podssh-ws, `dial.rs:129`
calls `resolve::pinned`, `resolve.rs:151` calls `dial::proxy_from_env`,
`tls.rs:24` takes `error::WsError`, `cut.rs:10` takes
`client::ConnectError`; in podssh-relay, five modules use
`open::backoff`; in podnet, `ws_relay.rs:73` takes `edge_h2::Metered`,
`relay_reverse.rs` calls `warp::register::utc_minutes`, and `warp/`
takes `exit`, `hop`, `decisions` and `fetch` of the core of podnet.
Thus podcrates adds `pod-core` (C17). It has no pod dependency, and it
holds:

* the traits `Dial`, `Resolve`, `Datagrams` (section 6), the `Target`,
  the stream type (`BoxedIo`);
* the `Failure` with its `Stage` and its `Class`, the `Event`s and the
  `Observer` of a `Report`;
* `Metered` (a stream that counts bytes), `backoff` and
  `CONNECT_TIMEOUT`, `clock` (with `utc_minutes`), `text` (untrusted text
  made safe to print), `names` (name checks).

`pod-core` grows with the crate that needs a part first:

* the shared error types of a dial (podssh-ws `ConnectError`, `Cut`,
  `Missed`), with `pod-tls` and `pod-proxy`;
* the private files and the cache directories (podssh `cache.rs` and
  `cache/`: a capped read, a 0600 write, refuse a file that others can
  read, the places of an `App`, `store_file_in_first`, `safe_name`), and
  the entropy source (`Entropy`, `OsEntropy`), with `pod-identity`;
* `Metered`, under the `RelayBudget` of `pod-relay` (call C73: the budget
  goes under the limits of the relay in use, which are `pod-relay`'s).

podssh-ws `wire.rs` writes and reads WebSocket frames: it goes to `pod-ws`.


## 4. The crates

"Lines" are of `.rs` files with comments and tests. "Base" is the source
whose code the crate starts from. For a crate with two sources, the
first commits move the base alone; the capabilities of the other source
come after it, each behind a feature when it changes a behaviour, each
with its tests (section 8, step 5).

The waves are M1 to M4 (the transport to the roads), M6 and M7 (the
crates of podvm) and M8 (the fleet).

### M0: the foundation

| Crate | Source | Lines | Pod deps |
| --- | --- | --- | --- |
| `pod-todo` | podssh `crates/podssh-todo` without `interop/` (base); the adaptations of podnet `crates/podnet-todo` as configuration (C5) | 3,600 | none; no external dependency |
| `pod-core` | section 3 | 1,500 | none |
| `pod-testkit` | podnet `tests/test_host/mod.rs` (`need_loopback!`, `need_localhost!`); podssh `crates/podssh-cli/tests/ssh_harness/` (feature `ssh`, russh 0.65 on ring). Later waves add the stand-ins, each behind a feature; a stand-in speaks bytes and HTTP only and takes no type of a pod crate, so that a crate can dev-depend on it | 2,200 | none |

### M1: wave 1, the transport (in this order)

| Crate | Source | Lines | Pod deps |
| --- | --- | --- | --- |
| `pod-identity` | podssh-relay `identity/` (the entropy of `session/` and the private files of `cache` go to `pod-core`) | 900 | pod-core |
| `pod-term` | podssh `crates/podssh-terminal` | 3,700 | none |
| `pod-tls` | podssh-ws `crypto/` (feature `pure`), `tls.rs`, `tls/`, `stores/`, `tls_server.rs`, `tls_server/`, `tls_wrap.rs`, `bundle.rs`, `ech/` (feature `ech`; the HTTPS record through `Resolve`); podssh-relay `tls_groups.rs`; podnet `src/trust.rs` (roots, pins, `SSL_CERT_FILE` as a setting; the edge roots of Cloudflare come from `pod-cloudflare` as extra roots; no `init_crypto`), `src/tls_fragment.rs` | 6,300 | pod-core |
| `pod-proxy` | podssh-ws `dial.rs`, `proxy.rs`, `proxy_auth.rs`, `socks5.rs`, `mptcp.rs`, `race.rs` (the race of addresses), `cut.rs`, `lowat.rs`, `write_limit.rs`, `stun.rs`, `http.rs` and `http/` (the HTTP/1.1 heads), the GET client of `client.rs` (`https_get_accepting`); podnet `src/egress.rs`, `src/proxy_auth.rs` (Digest), `src/fetch.rs` (the HTTP client under DoH, the mint of the relay and the API of the quick tunnels). Dial, race and SOCKS take `&dyn Resolve`; the bypass of a pinned loopback moves to the caller | 5,500 | pod-core, pod-tls |
| `pod-dns` | podssh-ws `resolve.rs`, `resolve/` (RFC 8484 wire over HTTP/2, HTTPS and SVCB records); podnet `src/resolve.rs` (DoH JSON, pins, the query by name through a proxy). Both DoH forms stay | 2,800 | pod-core, pod-tls, pod-proxy |
| `pod-ws` | podssh-ws `client.rs`, `frame.rs`, `handshake.rs`, `session.rs`, `wire.rs`, `url.rs`, `close.rs`, `error.rs`, `plain.rs`, `server.rs` (its WebTunnel test holds the bytes of podssh's client: no trait until `pod-egress`), `h2_road/` (the pool a value of the caller), `cleartext/`, `wake.rs`, the stream of `via/http2.rs`; podnet `src/ws.rs` (`bridge`, `dial`, `connect_bridge`, `accept_key`, the close codes, `close_hint`; podnet edges 8 and 18). Each connection goes through the caller's `Way` (C41). The limits are settings (`Limits`: podssh's by default, 16 MiB a message; podnet's, the defaults of tokio-tungstenite) | 4,700 | pod-core, pod-tls, pod-proxy |
| `pod-services` | the owner's ruling of 2026-11-05: the relays and the services of other parties that podssh and podnet hold in their code (relay lists, DoH resolvers, STUN, DERP and iroh relays, piping and mailbox servers, Cloudflare and WARP endpoints, and the rest), with the code, the scripts and the workflows that measure or watch them. T-048 takes the inventory of each consumer; T-049 moves it. The library holds the lists, each with its source and the date it was read; the watches run in the workflows of podcrates | (T-048) | pod-core |

### M2: wave 2, the relay

| Crate | Source | Lines | Pod deps |
| --- | --- | --- | --- |
| `pod-quic` | noq 1.4 (T-018, C43; `docs/research/quic/stack.md`). Sources: podssh-relay `quick/quic.rs`, `quick/probe.rs` (the settings of the transport, the endpoint, the race of handshakes and the probe; the regions of an edge stay in podssh), the `TokenKey` of `serve/quic.rs`; podnet `src/warp/quic.rs` (`WarpUdpSocket`: `QueuedDatagrams` over a `Link`; the client configuration is an argument, podnet edge 10). An endpoint runs over any `Datagrams`, with a waker for each sender of noq. The TLS provider and the roots are the caller's | 1,500 | pod-core |
| `pod-session` | podssh-relay `session/` (with `handover/`), `udp/` (the resolver of a flow is the caller's `Resolve`), `answer.rs`, `transit/link.rs` (`ws_link::Link` over a session of `pod-ws`). No environment and no global: the settings, the relay's limits (`Settings::pair_window`, `move_bytes`, `move_age`) and the sink of the budget's lines (`Keeper::with_tell`) are the caller's; a target moves at a handover by `Movable` or by the caller's function (`Keeper::hand_over_with`). The test of the close table (`session_policy.rs`) goes with `pod-relay`, and the test of the role (`session_relay_resume.rs`) with `pod-relay-serve`; the handover test with the channel is `pod-e2e`'s | 8,400 | pod-core, pod-ws |
| `pod-e2e` | podssh-relay `e2e/` (Noise XX on snow 0.10): the channel and its carry, the handshake, the frames, the word of a proof (`Ask`), the two ends (`ends`), the gate of a node's person (`person`; the question goes to the caller's `Asker`), the net's document (`net`; the caller's `Docs`), the tokens of one leg (`legs`), the handover of a node's channel (`hold`). No process state: the documents of the net and the holds of a handover are fields of the caller's `Node` (`Node::docs`, `Node::holds`), and a node's lines are events `e2e.*`. The tests `e2e.rs`, `e2e_faults.rs`, `e2e_net.rs`, `routes.rs`, `person.rs` and the channel's test of `session_handover.rs` come with it; `e2e_layered.rs` goes with `pod-relay` | 5,000 | pod-core, pod-identity, pod-session |
| `pod-wormhole` | podssh-relay `code/` `mailbox.rs`, `wormhole.rs`, `phrase.rs`, `words.rs`, `mod.rs` (`Failed`, hex), and of `run.rs` the rendezvous (`Ends`, `opened`, `joined`); the mailbox harness (a stand-in in Rust, magic-wormhole's mailbox server and its Python peer). The AppID, the client name, the mailbox URLs and the way out are the caller's (C45). The pairing exchange (`exchange.rs`, the rest of `run.rs`) goes to `pod-relay` | 1,200 | pod-core, pod-ws |
| `pod-netstack` | the cut of PodUtils/podnet#427: podnet `crates/podnet-vnet/src/stack.rs` and `stack/` without `proof.rs` (the stack, its sockets, its reactor; the `Acl` of `StackConfig` becomes the trait `Admit`, its resolver the trait `GatewayService`), `device.rs` (the recorder becomes the trait `PacketTap`), of `addr.rs` only `VirtualSubnet` (as `Subnet`); on smoltcp 0.14. `framing.rs`, the switch, the proxy, the dialer and the proof stay in podnet; `src/warp/ip.rs` goes to `pod-warp`, over this crate; the router of `podnet-netns` moves onto it with PodUtils/podnet@T-249 (T-170, step 2) | 2,900 | pod-core |
| `pod-wg` | the cut of PodUtils/podnet#447: podnet `src/warp/wg.rs` without its driver (sans-IO, on ring, call C8): the primitives (BLAKE2s, HMAC, the KDF, X25519, ChaCha20-Poly1305, TAI64N), `Keys`, `Initiation`, mac1, `Session` with its window of counters, the timers of `State` as `Tunnel` on the caller's clock; new: the cookie reply and mac2, and timestamps that never go back (`Stamps`). `src/warp/wg/standin.rs` is the feature `standin` (a responder for the tests of a consumer). `Peer` with `from_exported`, the socket, `connect`, `connect_inside`, `connect_on`, `WgTx`, `WgRx`, `WgKeepalive` and the lock stay in podnet. With no feature, no tokio | 1,850 | pod-core |
| `pod-relay` | podssh-relay `relay.rs`, `open.rs`, `pool.rs`, `pool/`, `failover.rs`, `failures.rs`, `gate.rs`, `rank*.rs`, `token.rs`, `health.rs`, `addresses.rs`, `keyed.rs`, the token entries of `cache.rs` (its files and `cache/` are `pod-core`'s), `forward_layer/`, `pair.rs`, `pair/`, `code/exchange.rs`, `code/run.rs`, `grant/`, `reverse/` (the carrier `Door` of `serve` is the trait `reverse::wire::Door`), `http_road/`, `quic_road/`, `h2_road.rs`, `cleartext.rs`, `blocking/` (feature `blocking`, podvm), with their tests (`tests/e2e_layered.rs` and `tests/session_policy.rs` among them; `tests/session_relay_resume.rs` runs the role, so it goes with `pod-relay-serve`). Each call takes the caller's `Context`: the way out, the places of the cache, the `Settings` (no environment), the events and the state of a run (no global). podnet `src/ws_relay.rs` (`active()` becomes a handle that the caller gives), `src/relay_pool.rs`, `src/relay_reverse.rs`; the relay stand-in. The pool, the rank and the reverse road of podssh are the base; podnet's reverse mode (plain `/v1/node/NAME`, 32-hex ids) is merged last, as settings of the reverse road and its handler of a TCP or Unix target (T-026, call C74) | 19,800 | pod-core, pod-tls, pod-proxy, pod-dns, pod-ws, pod-quic, pod-identity, pod-session, pod-e2e, pod-wormhole, pod-services |

### M3: wave 3, the adapters

| Crate | Source | Lines | Pod deps |
| --- | --- | --- | --- |
| `pod-transit` | podssh-relay `transit/` (without `link.rs`); the transit server | 900 | pod-core, pod-ws, pod-session, pod-proxy |
| `pod-warp` | podnet `src/warp/` without the sans-IO part of `wg.rs` (`pod-wg`; its driver comes here), `quic.rs` (`pod-quic`), `up.rs`, `export.rs`, `ssh_config.rs` (those stay in podnet); its `ip.rs` runs on `pod-netstack` (PodUtils/podnet#427); and without the backends of `warp run` (`podnet-netns`, stays in podnet); TUN and routes behind `cfg(target_os = "linux")`; `eprintln!` and `process::exit` become events; podssh `crates/podssh-warp` retires (its endpoint order and API routes are checked against podnet's); its stand-in edge and API into `pod-testkit`. The SOCKS5 datagram path of podnet `podnet-netns/src/upstream.rs` (UDP ASSOCIATE, the command 0x83, the frames) is not here: it is `pod_proxy::udp` (C23, T-083), and `pod-warp` calls it. `pod-session` keeps its own UDP-in-a-stream adapter (T-020) | 10,000 | pod-core, pod-tls, pod-dns, pod-proxy, pod-quic, pod-netstack, pod-wg, pod-relay |
| `pod-cloudflare` | podnet `src/cloudflare/`, `src/tunnel.rs`, `src/race.rs` (the race of the legs), `src/edge_h2.rs`, `src/edge_discovery.rs`, `src/quicktunnel.rs`, `src/config.rs`, `src/proxy.rs` (the dispatch of the edge's streams to a `ServiceDispatch` trait that podnet implements), the edge side of `src/ws.rs` (`serve_websocket`, `forward_path`; edges 16 and 17), the HTTP/2 leg through the relay, the edge roots, the pin of cloudflared and its watch; podssh-relay `quick/` moves onto it | 17,900 | pod-core, pod-tls, pod-dns, pod-proxy, pod-ws, pod-quic, pod-relay, pod-warp |

### M4: wave 4, the other roads

Each crate of M4 starts with a study entry: the API, the C on each
target, the fork. A study can change its row.

| Crate | Source | Lines | Pod deps |
| --- | --- | --- | --- |
| `pod-derp` | podssh-relay `derp/`; the DERP stand-in | 3,500 | pod-core, pod-ws, pod-identity, pod-session, pod-proxy |
| `pod-piping` | podssh-relay `piping/`; the Piping harness | 2,400 | pod-core, pod-ws, pod-session, pod-proxy, pod-relay |
| `pod-opentunnel` | podssh-relay `opentunnel/`; the OpenTunnel stand-in | 2,600 | pod-core, pod-ws, pod-tls, pod-session, pod-proxy |
| `pod-acme` | podssh-cli `acme/` (keys through the given provider) | 1,400 | pod-core, pod-tls, pod-proxy |
| `pod-egress` | podssh-ws `via/`, `vless/`, `wstunnel/`, `wisp/`, `shadowsocks/`, `forwarder/`, `meek.rs`, `meek/`, `webtunnel.rs`, `pt.rs`, `eice/`, `iap/`, `cloudflare.rs` (the Access client). Each road is a `Dial` | 10,000 | pod-core, pod-proxy, pod-tls, pod-dns, pod-ws |
| `pod-relay-serve` | podssh-relay `serve/` (with `serve/mailbox/`) | 13,200 | pod-core, pod-relay, pod-ws, pod-tls, pod-session, pod-e2e, pod-identity, pod-wormhole, pod-cloudflare |
| `pod-iroh` | podssh `crates/podssh-iroh`; `vendor/iroh-relay` with its patch | 3,600 | pod-core, pod-derp, pod-proxy, pod-tls, pod-ws, pod-dns, pod-session, pod-identity |
| `pod-tor` | podssh `crates/podssh-tor` (arti, SQLite linked static) | 900 | pod-core, pod-proxy, pod-tls, pod-egress |
| `pod-ts` | podssh `crates/podssh-ts`; `vendor/tailscale-rs` with its patches; the headscale harness | 900 | pod-core, pod-proxy |
| `pod-gsr` | the GSRN relay of gsocket: the owner's prototype `pod-gsr` 0.3.0 (`pod-gsr.zip` of GitHub #24) and its data file `hackerschoice-gsr` for `pod-services`; the protocol of `hackerschoice/gsocket` at `5950647f` (`SOURCES`). TLS with SRP on a static OpenSSL (C34) | 2,200 | pod-core, pod-services (pod-proxy for the live test) |

### M8: the fleet (the asks of GitHub #35 and #36)

The new crates for the asks of cfb and githome. podcrates gives libraries,
schemas and shared CI scripts. Each command line, unit file and live step
stays in its product, and a broker binary is a thin `main` in the product
that hosts it (call C27). The JSON Schemas live in `schemas/` and go out
with each tag (the owner, 2026-11-09). `pod-egress` keeps the roads of M4;
the egress broker is `pod-broker` (call C25).

| Crate | Source | Lines | Pod deps |
| --- | --- | --- | --- |
| `pod-broker` | the egress broker: per-job policy (allow-list, or open with fixed denies), one resolve and a deny file, the upstream CONNECT chain, a TLS end with a CA for each job, placeholder credentials swapped and scrubbed, a JSON audit line, a Unix-socket listener, a drop at the TTL. Mostly new code; the vocabulary of PodUtils/podvm#184, the rules of `podnet pod` and `warp proxy`, podssh `exit --listen` and `pipe http-listen:` | (T-099) | pod-core, pod-proxy; pod-tls with T-100; the resolver (`pod-dns`) comes as a `pod_core::Resolve` |
| `pod-unit` | hardened service units: systemd system and user units, launchd agents, Windows services; no secret in a unit; podssh `crates/podssh-cli/src/node_install/` (base), podnet `src/autostart.rs` | (T-102) | pod-core |
| `pod-control` | one versioned JSON control API on a Unix socket or a Windows named pipe (`version`, `status`, `metrics`, `reload`, `drain`, `stop`), its client, its JSON Schema and a conformance test; new code, with the JSON of podnet `src/status.rs` and podssh `status` | (T-103) | pod-core |
| `pod-job` | the records of the job of a CI runner (PodUtils/podrunner#17): the job offer, the unit capability, the run report and the public report, their ids and refusal codes, the `check` of each record, and their JSON Schemas in `schemas/`, generated from the types (Q-A8; call C50). Source: podrunner `crates/podrunner-schema` and `schemas/` at `9ab7fd3`; the credential (`secret.rs`) stays in podrunner | 1,500 | none |
| `pod-release` | the release index: the schema, the writer for CI, the verifier (SHA-256, the Sigstore bundle of `SHA256SUMS`, minisign), the rollback data of an installer; podnet `src/update.rs` (the verify part), podssh `crates/podssh-cli/src/update/sigstore.rs` | (T-104) | pod-core, pod-proxy, pod-tls |

`pod-todo` (an acceptance registry, T-105) and `pod-testkit` (the stand-ins
as programs, T-106) grow in M8; `pod-core` (T-101), `pod-services` (T-112),
`pod-hostprobe` (T-107), `pod-session` (T-110, T-120), `pod-relay` (T-111)
and the adapters (T-113 to T-117) take clauses in their own waves. The
products of the fleet (`podrunner`, `podnode`, `podinit`, `podlog`,
`podstat`, `podkey`, `podsnap`, `podrepl`, `podrelay-worker`, `podforge`
and the installer `pods`) are repositories of their own.

### `pod-services`: the services of other parties

The owner's rulings of 2026-11-05: the lists of relays and services that
podssh and podnet hardcode, and their measurements and watches, move here
(`docs/decisions.md`, Services crate); and "we will soon add other
relay/services, so design in a way it's pluggable, maintainable,
agnostic". So the crate knows no provider in its code:

* **One schema for each service.** A service is one data file,
  `crates/pod-services/services/ID.toml`: its id, its kind (relay, doh,
  stun, derp, iroh-relay, mailbox, transit, piping, opentunnel,
  forwarder, tunnel-edge, warp, acme, ssh-host, and others: a kind is a
  word, so a new kind needs no change of the schema), its party, its
  endpoints (scheme, host, port, path, addresses, SNI, ALPN), its facts
  (keys, pins, limits, versions, as typed values), and for each value its
  source (a URL or a measurement) and the date it was read.
* **No parse at run time.** `build.rs` reads the files, checks each one
  against the schema (each value has a source and a date; each service has
  a watch), and writes Rust constants. The library depends on `pod-core`
  only. A new relay or service is a new file, with no change of code.
* **The consumer reads by id or by kind** (`pod_services::get("ID")`,
  `pod_services::of_kind("doh")`) and can lay its own files of the same
  schema over the built-in set (a setting, rule 6), so that a user's own
  relay needs no new release.
* **Watches are plugins.** Each file names its watches (`[[watch]]`: a
  watch kind and its arguments: an HTTP document and a JSON pointer, a DNS
  SRV name, the SHA-256 of a text, a GitHub release, the RDAP of a domain,
  GitHub's SSH keys of `api.github.com/meta`, a TLS root set). Each watch
  kind is one implementation of a `Watch` trait in the member crate
  `pod-services-watch` (`crates/pod-services-watch`: a library and its
  runner binary, `publish = false`, which use `pod-proxy`, `pod-dns`,
  `pod-tls`, `pod-ws` and russh; calls C42, C88). A new watch kind is one
  implementation, its offline test against a stand-in of `pod-testkit`
  (feature `stand`) and its plant.
* **Workflows of podcrates** run the watches (`services.yml`, each day)
  and the measurements (by hand); a fact that moved fails the run with the
  file, the value, the new value and the source. The measurement scripts
  of the consumers go to `scripts/services/`.
* **What stays out** (the owner's rulings): the release identity of
  `podssh update` and `podnet update`; the cloud endpoints of the user's
  own accounts (GCP IAP and OAuth, AWS EICE), which go with their
  features; the protocol code of each adapter (the Cloudflare edge's
  protocol in `pod-cloudflare`, which reads its hosts, roots and pins as
  data from `pod-services`); the relay contract (`pod-relay`, T-047); the
  supply chain of CI (T-046).

### M6: the crates of podvm

The owner's rulings of 2026-11-06: each generic crate of podvm moves here
with the prefix `pod-`, also where podvm is its only consumer today; a
crate that only podvm needs stays in podvm with the prefix `podvm-`. The
sources are the crates of podvm after its rename from `podbox-*` to
`podvm-*` (PodUtils/podvm#194). `pod-hostprobe` comes first: four of the
others read its raw system calls.

| Crate | Source | Lines | Pod deps |
| --- | --- | --- | --- |
| `pod-hostprobe` | podvm `crates/podvm-probe` (part: one disposable child for each probe, the errno and never a boolean, "could not run" as a third state, the raw system calls) and `crates/podvm-image/src/{space.rs,exec_probe.rs}`; podnet `crates/podnet-netns/src/doctor.rs` probes the same way | 7,200 | pod-core |
| `pod-oci` | podvm `crates/podvm-image` (part: the reference, the digest checked while it is written, the OCI types, the platform, the logins of docker and podman at the caller's paths, the registry client over the caller's HTTP client and trusts, the insecure policy from the caller's hosts, the host bundle and the roots, the resolve and the fetch pool into the caller's `Blobs`, the constants of the OCI layout); the content store, the pull into it, `AcceptAny` and docker's codes stay in podvm | 5,000 | pod-core, pod-tls, pod-proxy |
| `pod-extract` | podvm `crates/podvm-extract`: the rootless apply of the layers (the order, the whiteouts on the basename, the containment of each path, an ownership sidecar in place of chown, the digest of each blob while it is read, the completion marker); the caller gives each layer as a blob path with its digest and media type, and the place; the layout of the store stays in podvm | 2,800 | pod-core, pod-hostprobe |
| `pod-rootfs` | podvm `crates/podvm-complete`: the completion of a root file system from the host, each write held in the root (device nodes or shims, the `/dev` links, the resolver, hosts and mtab, the accounts and nsswitch, the walls of pacman, apt and zypper, the CA bundle and the CA directory of the host, `https` package sources); the caller gives its names, the host bundle and the probe of a mirror; the banner, the record ids and the HTTPS request stay in podvm | 4,300 | pod-core, pod-hostprobe, pod-extract |
| `pod-enter` | podvm `crates/podvm-enter`: the entry of a payload (the chroot sequence, the namespace rung, the entry with no chroot, the launch ladder: memfd, FUSE, tmpfs, run directory, cache; the device hand-over; the reader of ELF files); the names are the caller's (`Names`), and docker's 125 stays in podvm; the confinement of a payload (`Plan.confine`, T-108) | 8,000 | pod-core, pod-hostprobe, pod-confine |
| `pod-supervise` | podvm `crates/podvm-supervise` and the `Lock` of `crates/podvm-image`: detached payloads (launcher state, pidfds, logs, signals, the lifecycle); the entry of a payload is the caller's | 2,300 | pod-core, pod-hostprobe |
| `pod-guest` | podvm `crates/podvm-windows`: a disposable QEMU guest (the boot plan on KVM or TCG, the overlay, the FAT16 mailbox, the `cmd.exe` and DOS agents, QMP, the bounded stop, the store of a base image with its pinned digest); the download, the base caches and docker's exit range stay in podvm. New: a read-only squashfs reader with xz and the x86 BCJ filter, and the module set of a guest kernel from its modloop (T-206, PodUtils/podvm#260) | 5,300 | pod-core, pod-hostprobe |

### M7: the widened scope of podvm

The owner, 2026-11-06: "podvm will make everything that sand-net does,
but without root, in the environment we target". Each crate of M7 starts
with a study entry, as M4 does.

| Crate | Topic | Pod deps |
| --- | --- | --- |
| `pod-microvm` | fast disposable microVMs from an OCI image, on KVM or TCG (kernel, initramfs, virtio devices, boot): PodUtils/podvm#168 | pod-oci, pod-guest |
| `pod-hold` | the hold of each connect of a payload, with its level (`enforced`, `advisory`, `none`) and a proof: the generic core of podnet `crates/podnet-netns` (T-166, call C48): the seccomp filter, its start, the receive of a notification, the memory of the task, the reply, the table of the sockets with the flow budget, the netns entry and its wire (the TUN, else a veth pair with a packet socket: T-201). The handlers, the rules, the upstreams, the relays, the router and `pod doctor` stay in podnet (PodUtils/podnet#414, #422) | pod-core, pod-hostprobe |
| `pod-sud` | an in-process interceptor of system calls on Syscall User Dispatch, with paths resolved in a root (`openat2(RESOLVE_IN_ROOT)`); no ptrace, no privilege: PodUtils/podvm#109, PodUtils/podvm#169 | pod-core |
| `pod-confine` | confinement with no privilege (T-108, PodUtils/podvm#259): Landlock rules for the paths and the TCP ports, the Landlock scopes, a seccomp deny filter (with a closed UDP and no MPTCP under a TCP list, T-209; the socket families that Landlock does not check, the rights of ABI 9 and 10, T-213) and `no_new_privs`, compiled before the fork and applied in the child with no allocation; new code. The rlimits are `pod-enter`'s (T-176), and `pod-hold` has the seccomp filter of a hold: PodUtils/podvm#170 | pod-core, pod-hostprobe |
| `pod-netlab` | network labs in a guest or a namespace: NAT44, NAT66, NAT64 with DNS64, CGNAT, IPv6 only, firewall policy, impairment: PodUtils/podvm#178 | pod-netstack |
| `pod-pcap` | packet capture, counters and connection logs with no `CAP_NET_RAW`; a pcap writer: PodUtils/podvm#182 | pod-core |
| `pod-sandprobe` | the security probe inside a payload, and the compare of two reports against a list of expected differences: PodUtils/podvm#186 | pod-core, pod-hostprobe |
| `pod-incus` | a client of the REST API of incus on its Unix socket, with no `incus` binary (T-179, PodUtils/podvm#253): the server facts, the import of an image from a stream, the instances and their state, exec with `record-output` or with its websockets, the console log, the files, the operations and their wait; the floor is incus 6.0 LTS. New code from the API of lxc/incus at `63377d9b`. The back end, the conversion of an OCI image and the start of a user's `incusd` stay in podvm | pod-core, pod-proxy, pod-ws |

Names held, with no crate: `pod-wireguard`, `pod-quick`, `pod-udp`,
`pod-mailbox`, `pod-reverse`, `podcrates`, `podutils`, `pod-utils`,
`pod-probe`; for podrunner (T-146): `pod-dockerapi` (phase 3),
`pod-actions-proto`, `pod-actions-expr`, `pod-actions-exec` and
`pod-actions-workflow` (the native runner and the local mode, Q11); the
owner's name of 2026-11-11: `pod-mask` (T-180).

Project names, each with the repository of its name under PodUtils
(`podvm-*`: PodUtils/podvm; GitHub #1): `pods`, `podstat`, `podforge`,
`podci`, `podinit`, `podkey`, `podlog`, `podnode`, `podrelay-worker`,
`podrepl`, `podrunner`, `podsnap`, `podvm-probe`, `podvm-image`,
`podvm-ssh`, `podvm-interpose`, `podvm-machine`, `podvm-cage`,
`podvm-notebook`.

The crate `pod-git` is not held: its work goes to PodUtils/podforge
(the owner, 2026-11-12).

What stays in the consumers: podnet `src/transport/websocket.rs` (a
service transport, `podnet-serve`); podnet `hop`, `exit`, `state`,
`instances` and `detach` (they implement the traits of `pod-core` or
give the settings). podssh-ws `wire.rs` goes to `pod-ws`.


## 5. Churn and order

podssh changes each day, most in `serve/`, `session/`, `quick/`,
`reverse/` and `e2e/`, and little in `podssh-warp` and
`podssh-terminal`. Thus:

* The quiet code goes first (`pod-term`, the TLS, DNS and proxy code).
* The window between the import of a crate and its adoption is short:
  each crate is released when its entry is done, and the consumer takes
  it on the day of the release (Q3).
* From the import to the adoption, the consumer changes the module only
  for fixes, and podcrates ports each fix (Q3). `check-sources.py` finds
  each change (section 9). The import tells the consumer session that
  the freeze starts (a direct message, or a COORD issue of podcrates
  `(to CONSUMER)`; `docs/maintenance.md`, "Talk between sessions").


## 6. The shared traits (in `pod-core`)

* `Dial`: a target (a name and a port, or an address) to a connected
  stream with its peer and local addresses; `dial_http` for a request
  of plain `http://` (the proxy of `http_proxy`); `dial_addrs` for a given
  address list (podnet `egress::dial_addrs`); a deadline; typed errors
  (`ConnectError`, `Cut`, `Missed`) that the race and the failover read;
  a label; `names_inside()` (the far end resolves the name).
* `Resolve`: a name to addresses; the pins; the source wait of the race;
  the HTTPS and SVCB records (ECH, the ALPN of HTTP/3).
* `Datagrams`: the shape of the `AsyncUdpSocket` of quinn and noq: the
  source and the destination of each datagram, several transmits for
  each `poll_send`, several buffers for each `poll_recv`, `local_addr`,
  the maximum size, a writable poll, one segment for each transmit with
  no GSO. Implementations: the system socket, a UDP socket of the
  userspace stack (`pod-netstack`), WARP (`pod-warp`), UDP in a stream of
  the relay (`pod-session`, an adapter). `pod-iroh` gives iroh the system
  sockets.
* `Observer`, `Event`: section 1, rules 8 and 9.
* `ServiceDispatch` (in `pod-cloudflare`), `PacketTap`, `Admit` and
  `GatewayService` (in `pod-netstack`), `Docs` and the `Asker` of a
  node's person (in `pod-e2e`), `Door` (in `pod-relay`): the traits that
  cut the edges to the code of a consumer.


## 7. Targets and the static check

| Targets | Consumer | Build |
| --- | --- | --- |
| `x86_64`, `aarch64`, `riscv64gc`, `i686`, `loongarch64`, `powerpc64le` `-unknown-linux-musl`; `armv7-unknown-linux-musleabihf`; `arm-unknown-linux-musleabihf` | podnet (podssh: four of them) | cargo-zigbuild on Linux |
| `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` | podssh | Linux runners |
| `x86_64-apple-darwin`, `aarch64-apple-darwin` | podssh | macOS runners |
| `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc` | podssh | Windows runners (zig cannot target MSVC) |
| the targets of podssh | podvm (C16) | |

The check:

* `examples/static-probe` depends on each crate with each feature and
  calls one function of each (`fuzzing` is out). `examples/static-probe-lean`
  builds the crates that need no tokio (pod-core and pod-tls with no
  default feature, pod-wg, pod-confine).
  `scripts/check-probe.py` checks that each feature of a library crate is in
  a probe or in its allowance list, and that tokio is not in the tree of the
  lean probe. CI builds it in release on each target on
  each push (with a cache), and `scripts/check-static.sh` checks it
  (`readelf` on Linux, `otool -L` on macOS, `dumpbin /dependents` on
  Windows, each against its list of system libraries). A plant links a
  library dynamically and must fail the check.
* The tests run on Linux x86_64 on each push. The tests on macOS,
  Windows and the other Linux targets (QEMU user mode, as podnet
  `scripts/binfmt.sh`) run each night and before a release.
* Known risks, proved or fixed in the study entry of the crate:
  aws-lc-sys on i686, arm, loongarch64 and powerpc64le musl (unproven;
  prefer ring where the upstream allows it); the SQLite of arti on
  loongarch64 and powerpc64le; iroh and netwatch on the targets that only
  zig builds. A crate that cannot link static on a target of a consumer
  is a blocked entry with a question for the owner.


## 8. The procedure for one crate

Each crate is one record entry, or more for a large merge. Its commits:

1. **Import.** Copy the source files with no change. The message names
   each source: `pod-session: import podssh crates/podssh-relay/src/session at COMMIT`.
   Write `SOURCES` (section 9). The import commit is pushed together
   with the next ones, so that each push is green.
2. **Edition.** `cargo fix --edition` to 2024, in its own commit, then a
   review of the risky changes: the drop order of tail expressions and
   of `if let` in async code (the order of locks and guards), the
   capture of lifetimes by `impl Trait`, `unsafe_op_in_unsafe_fn`,
   `unsafe extern`, `static_mut_refs`, `env::set_var` in tests, `gen` as
   a name.
3. **Adapt.** Make the crate follow section 1: the paths, `pod-core`,
   the dialer and the provider, the settings, the events, the typed
   failures, no globals, the size limit (split the files), rustfmt.
   Remove dated history from the comments, and keep each technical fact.
   Write in ASD-STE100.
4. **Tests.** Move the tests, the plants and the fuzz targets of the
   source (each crate gets a `fuzzing` feature, as podnet
   `src/fuzzing.rs`). Each test keeps its name. A test that fails shows a
   defect of the adaptation: repair the code, never the test.
5. **Merge** (two sources). Add each capability of the other source,
   with its tests, behind a feature when it changes a behaviour. Write the
   parity list; tick each line. Known differences, with the choice:
   * `socks5://` resolves the name locally and `socks5h://` sends it (the
     rule of curl, and podnet now); podssh sends the name for both, and
     changes.
   * DoH: both forms (JSON, RFC 8484 wire); the timeouts are settings.
   * WebSocket limits: settings; the default is podssh's.
   * The race of addresses (250 ms, both) is in `pod-proxy`; the race of
     the legs of the tunnel (podnet `race.rs`, 2 s) is in
     `pod-cloudflare`.
   * The reverse road: podssh's (the layer, e2e) is the base; podnet's
     plain mode is ported last, as a mode of the same API.
6. **Document.** The crate README: purpose, API, features, settings,
   the parity list, the adoption map (each old path, variable and
   `.cargo/config.toml` line of each consumer to its new form).
7. **Adoption trial.** `adoption.yml` runs in CI: a checkout of a
   consumer and its `target/` do not fit the disk of a session. It
   checks out the consumer at the commit of `adoption/CONSUMER/commit`
   (or of `adoption/CONSUMER/CRATE.commit`, for a patch on a later commit),
   applies `adoption/CONSUMER/pod-X.patch` (the glue: the paths, the
   settings, the events), points the git source of podcrates to the
   checkout of the push, and runs: the tests of the replaced module; the
   consumer's integration tests that use it (podssh
   `crates/podssh-cli/tests`; podnet `tests/egress.rs`, `tests/tls.rs`,
   `tests/relay_reverse.rs` and the exit codes of `tests/cli.rs`); the
   static check of the consumer binary on its targets; a build with the
   consumer's features (feature unification, one provider). The patch is
   the hand-off to the consumer session. The trials of podvm are T-040.
8. **Check, close, release.** `sh scripts/agent/check.sh`; the CI of the
   push is green; write the Done section (date, commits, test counts,
   CI runs, the trial run); release (section 10).

Agents that work in parallel use local worktrees under
`.claude/worktrees/`, one crate each. At most one cargo build runs at a
time (`CARGO_BUILD_JOBS=4`). The main session merges, builds, tests and
pushes. No worktree branch is pushed.


## 9. Checks, plants, drift and workflows

* `scripts/agent/check.sh`: `cargo fmt --all --check`, `cargo clippy
  --workspace --all-targets --all-features -- -D warnings`, `cargo test
  --workspace --all-features`, `cargo todo check`,
  `python3 -I scripts/check-size.py`, the static probe for
  `x86_64-unknown-linux-musl`.
* Plants (the podssh model): `scripts/plants/NAME.plant` names a check
  and its expected failure; `NAME.patch` is the defect;
  `scripts/plant-each.sh` applies each one to a scratch copy and fails
  when its check passes. A new check, and a test that guards a fix, is
  trusted only after its plant fails it.
* Drift: `crates/pod-X/SOURCES` lists each imported directory and file:
  the repository, the path, the commit, the git blob id (SHA-1, 40 hex digits) at that commit, and
  the local paths that the file became. `git ls-tree COMMIT -- PATH` in
  the source repository gives the blob id of a file. `scripts/check-sources.py` reads
  each directory of a source at the default branch of its repository
  (`main`, `master` or another name; through `forge.web.pkgforge.dev`,
  or a shallow clone when that fails) and
  prints each changed, new, removed or renamed file, with its commits. A
  change becomes a record entry "port". The port uses a 3-way merge from
  the imported blob. The lines of a consumer leave `SOURCES` at its
  adoption.

| Workflow | When | What |
| --- | --- | --- |
| `ci.yml` | each push | fmt, clippy, tests (Linux x86_64; the netns tests of `pod-hold` also as root, `scripts/root-netns-tests.sh`), `cargo todo check`, size, the static probes on each target, `cargo doc`, `scripts/check-probe.py`, `scripts/check-workflows.py` |
| `nightly.yml` | each night, before a release | tests on macOS, Windows and the other Linux targets (QEMU) |
| `live.yml` | each day (01:53 UTC), by hand | the ignored live tests: `live_handshake` of `pod-tls`, `live_doh` of `pod-dns` (with `scripts/names-proxy.py`), `mptcp` of `pod-proxy`, `live` of `pod-oci`, `modloop_live` of `pod-guest` (the modloop of Alpine 3.22); the tests of `pod-wormhole` and `pair_code` of `pod-relay` against magic-wormhole's own mailbox server; on Thursdays and by hand, `live` of `pod-wormhole` and `pair_code_live` of `pod-relay` (the public mailboxes); by hand (the job `relay`, its input `relay`), one live test of the relay client of `pod-relay` |
| `services.yml` | each day (02:17 UTC), by hand (a service, a kind, or a measurement; with neither, no `forward-url`: call C89) | `pod-services-watch`: each `[[watch]]` of the data of `pod-services`; a fact that moved fails the run with the file, the value, the new value and the source. The measurements of `scripts/services/` (`edge-facts.sh`) |
| `deny.yml`, `secrets.yml` | each push, each day | licences, advisories, sources; secret scan |
| `plants.yml` | each push that changes a check, by hand | `scripts/plant-each.sh` |
| `adoption.yml` | each push that changes a crate with a trial, by hand | section 8, step 7 |
| `release.yml` | when a crate is done, by hand | section 10; a dry run with no input |
| `upstream.yml` | each day | `check-sources.py`; `check-zig-mirrors.py` |
| `record-issues.yml` | each day | `TODO/github.txt` against GitHub |
| `fuzz.yml` | each week | the fuzz targets |
| `guest.yml` | each week, by hand | the tests of `pod-guest` with a real QEMU (TCG; KVM where `/dev/kvm` opens), one guest at a time, and a check that no emulator stays |
| the other live and interop workflows of the moved code | each day, by hand | podssh's `relay-live`, `relay-layer-live`, `warp-live`, `quick-tunnel-*`, `transit-live`, `pair-code`, `piping`, `via`, `vless` and `wstunnel` interop, `forwarder-live`, `opentunnel-*`, `tor-live`, `ts-headscale`, `acme-interop`, `reverse-drops`; podnet's live steps of WARP and of the edge. Each moves with its crate |


## 10. Versions and releases

The consumers take the crates by a git tag (Q1).

* One version for the workspace, from `0.1.0`. The tag `vX.Y.Z` releases
  each crate. A consumer writes
  `pod-tls = { git = "https://github.com/PodUtils/podcrates", tag = "v0.3.0" }`,
  adds `https://github.com/PodUtils/podcrates` to `allow-git` in its
  `deny.toml`, and its `Cargo.lock` holds the commit.
* A fork in `vendor/` is a path dependency inside the repository, and
  cargo follows it through the git dependency. A `[patch]` of podcrates
  does not reach a consumer; a `[patch.crates-io]` of a consumer reaches
  the crates.io dependencies of podcrates.
* `release.yml` tags when a crate is done and `main` is green, and
  writes `docs/releases/vX.Y.Z.md`: the crates that changed and each
  breaking change. A release is unattended. A change of a public API or
  of a wire format raises the minor version (0.x).
* Each `pod-*` dependency has `path` and `version`, so that a publish to
  crates.io works. The owner reserved the names on crates.io (0.0.0
  placeholders).
* `publish.yml` is the publish to crates.io, apart from `release.yml`
  (the ruling "publish.yml everywhere"). crates.io trusts this file name
  in the environment `crates-io` (only a tag `v*` deploys to it): no
  token is stored, and `rust-lang/crates-io-auth-action` gets a token for
  30 minutes. A tag push starts it, and `release.yml` dispatches it on its
  tag (a push with `GITHUB_TOKEN` starts no workflow). Each run does the
  dry run (`cargo publish --workspace --dry-run`); the publish runs only
  when the switch `crates-io` of `[workspace.metadata.podcrates]` in
  `Cargo.toml` is true at the tag. It stays false while Q1 stands.
* A new crate name is reserved by the owner on crates.io before its first
  publish; trusted publishing does not work for a name that is not
  reserved. `pod-*` names are of podcrates only.

