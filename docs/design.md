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
   `rust-version` is the stable of the day (1.97 on 2026-11-05), and a
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
| `vendor/` | the patched forks that a crate needs (tailscale-rs, iroh-relay), each with its patches, its upstream commit and its licence (as podssh `vendor/README.md`) |
| `adoption/CONSUMER/` | the pinned commit of each consumer, its `todo.toml`, and for each crate the trial patch (section 8, step 7) |
| `TODO/` | the record (the podssh model): `RULES.md`, `ROADMAP.md`, `INDEX.md`, `PROGRESS.md`, `issues.md`, `github.txt`, `todo.toml`, one area file for each milestone |
| `docs/` | `design.md`, `decisions.md`, `development.md`, `maintenance.md`, `releases/` |
| `scripts/` | `agent/` (`check.sh`, `watch-issues.sh`, `watch-runs.sh`); `cloud-setup.sh` (zig, cargo-zigbuild, cargo-deny, the targets); `fetch-zig.sh` with `zig-mirrors.txt`; `retry.sh`; `binfmt.sh`; `check-size.py`; `check-static.sh`; `check-sources.py`; `check-vendored.py`; `check-workflows.py`; `check-github-numbers.py`; `plant.sh`, `plant-each.sh`, `plants/`; `release.sh`; the stand-in servers of the moved tests (`fake-*.py`); `check-relay-spec.py` |
| `fuzz/` | the fuzz targets of the moved code (cargo-fuzz, its own workspace); `fuzz.sh`, `fuzz-seeds.py` |
| `examples/` | `static-probe` (section 7) and the binaries of the live workflows (ring provider; `publish = false`) |
| `images/` | the build and test images, pinned by digest |
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
  read, `read_dirs`, `store_file_in_first`, `safe_name`), with the
  directory given by the caller, with `pod-identity`;
* `RelayBudget`, with `pod-netstack` and `pod-relay`.

podssh-ws `wire.rs` writes and reads WebSocket frames: it goes to `pod-ws`.


## 4. The crates

"Lines" are of `.rs` files with comments and tests. "Base" is the source
whose code the crate starts from. For a crate with two sources, the
first commits move the base alone; the capabilities of the other source
come after it, each behind a feature when it changes a behaviour, each
with its tests (section 8, step 5).

### M0: the foundation

| Crate | Source | Lines | Pod deps |
| --- | --- | --- | --- |
| `pod-todo` | podssh `crates/podssh-todo` without `interop/` (base); the adaptations of podnet `crates/podnet-todo` as configuration (C5) | 3,600 | none; no external dependency |
| `pod-core` | section 3 | 1,500 | none |
| `pod-testkit` | podnet `tests/test_host/mod.rs` (`need_loopback!`, `need_localhost!`); podssh `crates/podssh-cli/tests/ssh_harness/` (feature `ssh`, russh 0.65 on ring). Later waves add the stand-ins, each behind a feature; a stand-in speaks bytes and HTTP only and takes no type of a pod crate, so that a crate can dev-depend on it | 2,200 | none |

### M1: wave 1, the transport (in this order)

| Crate | Source | Lines | Pod deps |
| --- | --- | --- | --- |
| `pod-identity` | podssh-relay `identity/`; `Entropy`, `OsEntropy` of `session/` | 900 | pod-core |
| `pod-term` | podssh `crates/podssh-terminal` | 3,700 | none |
| `pod-tls` | podssh-ws `crypto/` (feature `pure`), `tls.rs`, `tls/`, `stores/`, `tls_server.rs`, `tls_server/`, `tls_wrap.rs`, `bundle.rs`, `ech/` (feature `ech`; the HTTPS record through `Resolve`); podssh-relay `tls_groups.rs`; podnet `src/trust.rs` (roots, pins, `SSL_CERT_FILE` as a setting; the edge roots of Cloudflare come from `pod-cloudflare` as extra roots; no `init_crypto`), `src/tls_fragment.rs` | 6,300 | pod-core |
| `pod-proxy` | podssh-ws `dial.rs`, `proxy.rs`, `proxy_auth.rs`, `socks5.rs`, `mptcp.rs`, `race.rs` (the race of addresses), `cut.rs`, `lowat.rs`, `write_limit.rs`, `stun.rs`, `http.rs` and `http/` (the HTTP/1.1 heads), the GET client of `client.rs` (`https_get_accepting`); podnet `src/egress.rs`, `src/proxy_auth.rs` (Digest), `src/fetch.rs` (the HTTP client under DoH, the mint of the relay and the API of the quick tunnels). Dial, race and SOCKS take `&dyn Resolve`; the bypass of a pinned loopback moves to the caller | 5,500 | pod-core, pod-tls |
| `pod-dns` | podssh-ws `resolve.rs`, `resolve/` (RFC 8484 wire over HTTP/2, HTTPS and SVCB records); podnet `src/resolve.rs` (DoH JSON, pins, the query by name through a proxy). Both DoH forms stay | 2,800 | pod-core, pod-tls, pod-proxy |
| `pod-ws` | podssh-ws `client.rs`, `frame.rs`, `handshake.rs`, `session.rs`, `wire.rs`, `url.rs`, `close.rs`, `error.rs`, `plain.rs`, `server.rs` (the WebTunnel hook as a trait), `h2_road/`, `cleartext/`, `wake.rs`; podnet `src/ws.rs` (`bridge`, `dial`, `connect_bridge`, `accept_key`, the close codes, `close_hint`; podnet edges 8 and 18). The limits are settings (podssh: 16 MiB a message; podnet now: the defaults of tokio-tungstenite) | 4,700 | pod-core, pod-tls, pod-proxy, pod-dns |

### M2: wave 2, the relay

| Crate | Source | Lines | Pod deps |
| --- | --- | --- | --- |
| `pod-quic` | the first entry of M2 measures quinn 0.11 (podnet) against noq 1.4 (podssh) and keeps one (C10). Then: podssh-relay `quick/quic.rs`, `quick/probe.rs` (the QUIC endpoint and the probe); podnet `src/warp/quic.rs` (`WarpUdpSocket`: QUIC over `Datagrams`; the client configuration is an argument, podnet edge 10) | 1,500 | pod-core, pod-tls |
| `pod-session` | podssh-relay `session/` (with `handover/`), `udp/`, `answer.rs`, `transit/link.rs` (`Link`) | 8,200 | pod-core, pod-identity |
| `pod-e2e` | podssh-relay `e2e/` (Noise XX on snow 0.10) | 3,400 | pod-core, pod-identity, pod-session |
| `pod-wormhole` | podssh-relay `code/` `mailbox.rs`, `wormhole.rs`, `phrase.rs`, `words.rs`, `mod.rs` (the pairing exchange, `exchange.rs` and `run.rs`, goes to `pod-relay`); the mailbox harness | 1,200 | pod-core, pod-ws, pod-identity |
| `pod-netstack` | podnet `src/warp/ip.rs`, `src/vnet/device.rs` (the recorder of `podnet-vnet` becomes a `PacketTap` trait), `src/vnet/framing.rs`; the TCP of podssh-warp as the smoltcp 0.14 reference | 2,600 | pod-core |
| `pod-wg` | podnet `src/warp/wg.rs` (sans-IO, on ring) | 1,100 | none |
| `pod-relay` | podssh-relay `relay.rs`, `open.rs`, `pool.rs`, `pool/`, `failover.rs`, `failures.rs`, `gate.rs`, `rank*.rs`, `token.rs`, `health.rs`, `addresses.rs`, `keyed.rs`, the token and pool parts of `cache.rs` and `cache/`, `forward_layer/`, `pair.rs`, `pair/`, `code/exchange.rs`, `code/run.rs`, `grant/`, `reverse/` (the carrier `Door` of `serve` becomes a trait of `pod-relay`), `http_road/`, `quic_road/`, `h2_road.rs`, `cleartext.rs`, `blocking/` (feature `blocking`, podvm); podnet `src/ws_relay.rs` (`active()` becomes a handle that the caller gives), `src/relay_pool.rs`, `src/relay_reverse.rs`; the relay stand-in. The pool, the rank and the reverse road of podssh are the base; podnet's reverse mode (plain `/v1/node/NAME`, 32-hex ids) is merged last | 19,800 | pod-core, pod-tls, pod-proxy, pod-dns, pod-ws, pod-quic, pod-identity, pod-session, pod-e2e, pod-wormhole |

### M3: wave 3, the adapters

| Crate | Source | Lines | Pod deps |
| --- | --- | --- | --- |
| `pod-transit` | podssh-relay `transit/` (without `link.rs`); the transit server | 900 | pod-core, pod-ws, pod-session, pod-proxy |
| `pod-warp` | podnet `src/warp/` without `ip.rs`, `wg.rs`, `quic.rs`, `up.rs`, `export.rs`, `ssh_config.rs` (those stay in podnet), and without the backends of `warp run` (`podnet-netns`, stays in podnet); TUN and routes behind `cfg(target_os = "linux")`; `eprintln!` and `process::exit` become events; podssh `crates/podssh-warp` retires (its endpoint order and API routes are checked against podnet's); its stand-in edge and API into `pod-testkit` | 10,000 | pod-core, pod-tls, pod-dns, pod-proxy, pod-quic, pod-netstack, pod-wg, pod-relay |
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

Names held, with no crate: `pod-wireguard`, `pod-quick`, `pod-udp`,
`pod-mailbox`, `pod-reverse`, `podcrates`, `podutils`, `pod-utils`.

What stays in the consumers: podnet `src/transport/websocket.rs` (a
service transport, `podnet-serve`); podnet `hop`, `exit`, `state`,
`instances` and `detach` (they implement the traits of `pod-core` or
give the settings). podssh-ws `wire.rs` goes to `pod-ws`.


## 5. Churn and order

podssh changes each day. In its last 50 commits: `serve/` 100 file
changes, `session/` 38, `quick/` 36, `reverse/` 34, `e2e/` 27;
`podssh-warp` 2, `podssh-terminal` 1. podnet holds (its ruling
"Order"). Thus:

* The quiet code goes first (`pod-term`, the TLS, DNS and proxy code).
* The window between the import of a crate and its adoption is short:
  each crate is released when its entry is done, and the consumer takes
  it on the day of the release (Q3).
* From the import to the adoption, the consumer changes the module only
  for fixes, and podcrates ports each fix (Q3). `check-sources.py` finds
  each change (section 9). The import tells the consumer session (a
  message, or an issue in the consumer's repository written by the
  owner) that the freeze starts.


## 6. The shared traits (in `pod-core`)

* `Dial`: a target (a name and a port, or an address) to a connected
  stream with its peer and local addresses; `dial_addrs` for a given
  address list (podnet `egress::dial_addrs`); a deadline; typed errors
  (`ConnectError`, `Cut`, `Missed`) that the race and the failover read;
  a label; `names_inside()` (the far end resolves the name).
* `Resolve`: a name to addresses; the pins; the source wait of the race;
  the HTTPS and SVCB records (ECH, the ALPN of HTTP/3).
* `Datagrams`: the shape of the `AsyncUdpSocket` of quinn and noq: the
  source and the destination of each datagram, several transmits for
  each `poll_send`, several buffers for each `poll_recv`, `local_addr`,
  the maximum size, a writable poll, one segment for each transmit with
  no GSO. Implementations: the system socket, WARP (`pod-warp`), UDP in
  a stream of the relay (`pod-session`, an adapter). `pod-iroh` gives
  iroh the system sockets.
* `Observer`, `Event`: section 1, rules 8 and 9.
* `ServiceDispatch` (in `pod-cloudflare`), `PacketTap` (in
  `pod-netstack`), `Door` (in `pod-relay`): the traits that cut the
  edges to the code of a consumer.


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
  calls one function of each. CI builds it in release on each target on
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
   checks out the consumer at the commit of `adoption/CONSUMER/commit`,
   applies `adoption/CONSUMER/pod-X.patch` (the glue: the paths, the
   settings, the events), points the git source of podcrates to the
   checkout of the push, and runs: the tests of the replaced module; the
   consumer's integration tests that use it (podssh
   `crates/podssh-cli/tests`; podnet `tests/egress.rs`, `tests/tls.rs`,
   `tests/relay_reverse.rs` and the exit codes of `tests/cli.rs`); the
   static check of the consumer binary on its targets; a build with the
   consumer's features (feature unification, one provider). The patch is
   the hand-off to the consumer session. The trial of podvm waits for its
   repository (owner action).
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
  the repository, the path, the commit, the SHA-256 at that commit, and
  the local paths that the file became. `scripts/check-sources.py` reads
  each directory of a source at the `main` of its repository (through
  `forge.web.pkgforge.dev`, or a shallow clone when that fails) and
  prints each changed, new, removed or renamed file, with its commits. A
  change becomes a record entry "port". The port uses a 3-way merge from
  the imported blob. The lines of a consumer leave `SOURCES` at its
  adoption.

| Workflow | When | What |
| --- | --- | --- |
| `ci.yml` | each push | fmt, clippy, tests (Linux x86_64), `cargo todo check`, size, the static probe on each target, `cargo doc` |
| `nightly.yml` | each night, before a release | tests on macOS, Windows and the other Linux targets (QEMU) |
| `deny.yml`, `secrets.yml` | each push, each day | licences, advisories, sources; secret scan |
| `plants.yml` | each push that changes a check, by hand | `scripts/plant-each.sh` |
| `adoption.yml` | each push that changes a crate with a trial, by hand | section 8, step 7 |
| `release.yml` | when a crate is done, by hand | section 10; a dry run with no input |
| `upstream.yml` | each day | `check-sources.py`; `check-vendored.py`; the cloudflared pin |
| `record-issues.yml` | each day | `TODO/github.txt` against GitHub |
| `fuzz.yml` | each week | the fuzz targets |
| the live and interop workflows of the moved code | each day, by hand | podssh's `doh-live`, `relay-live`, `relay-layer-live`, `warp-live`, `quick-tunnel-*`, `transit-live`, `pair-code`, `piping`, `via`, `vless` and `wstunnel` interop, `forwarder-live`, `opentunnel-*`, `tor-live`, `ts-headscale`, `acme-interop`, `reverse-drops`; podnet's live steps of WARP and of the edge. Each moves with its crate |


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
* Each `pod-*` dependency has `path` and `version`, so that a later
  publish to crates.io stays possible. podcrates publishes nothing to
  crates.io now.

