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
| `vendor/` | the patched forks that a crate needs, each with its patches (`vendor/patches/`), its upstream commit and its licence: `iroh-relay` (`crates/pod-iroh/README.md`, "The fork of iroh-relay"); tailscale-rs is planned |
| `adoption/CONSUMER/` | the pinned commit of each consumer, its `todo.toml`, and for each crate the trial patch (section 8, step 7) |
| `TODO/` | the record (the podssh model): `RULES.md`, `ROADMAP.md`, `INDEX.md`, `PROGRESS.md`, `issues.md`, `github.txt`, `todo.toml`, one area file for each milestone |
| `docs/` | `HANDOFF.md` (the state of unfinished work), `decisions.md`, `design.md`, `development.md`, `install.md` (the installer core of each project, its pins and its test), `live-calls.md` (each live call of CI and the tests, and its stand-in), `maintenance.md`, `session-protocol.md` (the talk between sessions), `wire.md` (the wire of the relay's roads, with a version; its vectors are in `crates/pod-relay/tests/vectors/`), `releases/` (the notes of each tag), `research/` (the studies behind the plan), `upstream/` (fixes that the owner sends to the crates of other projects) |
| `scripts/` | `adoption.sh` (the adoption trial); `agent/`; `binfmt.sh`; `ci/` (`root-confine-tests.sh`: the tests of `pod-confine` that need root, as root; `root-unit-tests.sh`: the install of a unit of the system of `pod-unit` on the systemd of the runner, as root, where a skip fails; `root-enter-confine-tests.sh`: the confinement tests and the test of a refused exec of `pod-enter` on the rungs with a chroot, as root; `root-uid1000-tests.sh`: the tests of `pod-extract` and `pod-rootfs` that start a worker of uid 1000, as root, where a skip fails; `userns-tests.sh`: the namespace rung of `pod-enter` and its refused exec, the leg of `pod-hostprobe` through a user namespace, the labs of `pod-lab` and the end of a tree of `pod-supervise` with a PID namespace, as a user with no capabilities, where a skip fails, and the test of the devices of `pod-enter` again with root through `sudo -n` over a `/dev` with no `nosuid` and no `noexec`; `mosquitto-tests.sh`: the MQTT meeting point of `pod-wormhole` against the pinned Eclipse Mosquitto of `images/mosquitto/`; `tracker-tests.sh`: the WebTorrent meeting point of `pod-wormhole` against bittorrent-tracker from the hashed npm lock file of `bittorrent-tracker/`; `ntfy-tests.sh`: the ntfy meeting point of `pod-wormhole` against the pinned release of ntfy; `coredns-tests.sh`: the DNS over HTTPS of `pod-dns` against the pinned release of CoreDNS; `registry-tests.sh`: the client of `pod-oci` against the pinned release of the CNCF distribution registry; `s3-tests.sh`: the s3 carrier of `pod-link` against the pinned release of SeaweedFS; `turn-tests.sh`: the TURN client of `pod-turn` and the STUN codec of `pod-proxy` against the pinned coturn of `images/coturn/`; `piping-tests.sh`: the Piping Server carrier of `pod-piping` against nwtgck's two servers, from the hashed npm lock file of `piping-server/` and the pinned release of piping-server-rust; `mailbox-tests.sh`: the pairing by a code against magic-wormhole's mailbox server, and the transit road of `pod-transit` against its transit relay, from the packages and SHA-256 sums of `wormhole-servers.txt`; `fetch-tool.sh`: one program of a release archive, checked by its SHA-256; `zig-tools.sh`: zig and cargo-zigbuild for a job); `check-consumer-toolchains.py`; `check-consumer-tests.py` (each test target that a consumer's gate names exists); `check-crates-io-names.py`; `check-design-paths.py` (this table and the crate table of `README.md`); `check-github-numbers.py`; `check-history.py` (no narrative history in the documents that hold the current state); `check-loopback.py` (no bind of a fixed loopback port in product code); `check-man.sh` (a man page of `pod-manual` under groff and mandoc, with its plant); `check-private-replace.py` (no rename of a file outside `pod_core::files::replace_private`); `check-probe.py` (the static probes cover each feature); `check-publish-order.py` (`cargo publish --workspace` can order the crates); `check-results.py` (the `RESULTS.md` of each measured crate); `check-schemas.py` (a row of `schemas/CHANGES.md` for each change of a schema, and a new id for each change that needs one); `check-size.py`; `check-sources.py`; `check-static.sh`; `check-strings.py` (the text rules of `pod-manual` over the strings of a release binary); `check-wire-vectors.py`, `wire_check_parts.py`, `wire_ed25519.py` (the second reader of the wire vectors, from `docs/wire.md` alone, with its own Ed25519; `--plant` changes one byte of each file); `check-workflows.py`; `check-zig-mirrors.py`; `cloud-setup.sh` (zig, cargo-zigbuild, cargo-deny, the targets); `codegraph.py`, `codegraph_scan.py` (the code graph: `similar` ranks the public items, the entries, the crates and the scripts by the words of a query, `find`, `crate`, `graph`, `build`; `dups`, the check of repeated work: a function body in two files, two entries with nearly one title, one paragraph in two documents, the pod dependencies of section 4 against each `Cargo.toml`; `new`, the hint of a change against `origin/main`); `dups-baseline.txt` (the known findings of `dups`, each with the entry that removes it); `fetch-zig.sh`; `fuzz.sh`; `install/` (`core.sh`: the POSIX `sh` core of the installer of each project; `core-net.sh`: its network part, with the cosign signature of `SHA256SUMS`; `podlab.sh`: the installer of `podlab`; `pins.py`: the pins, the core inline, the offline bundle; `test.sh`, `test-cases.sh`: the test against `mirror.py`, a mirror and proxy on the loopback, in each shell and lab, on Linux, macOS and Windows); `issue-sync.py`, `issue_sync_test.py` (the issues on GitHub against the record, with the status block of `pod-todo`; its self-test with a fake API); `names-proxy.py` (a CONNECT proxy that takes names alone, for the live tests); `openssl-perl.sh` (a perl of Windows for the OpenSSL of `pod-gsr`); `plant-each.sh`; `plant-shard.py` (the plants of one shard of `plants.yml`, each group whole); `plants/` (the planted defects); `pull-image.sh`; `release-index.py` (the writer of the release index of `pod-release` in CI, with its self-test); `retry.sh`; `root-netns-tests.sh` (the netns tests of `pod-hold` as root; a skip fails); `secrets-report.py`; `services/` (the measurements of the services of other parties by hand, with their `SOURCES`: `edge-facts.sh`); `tor-net.sh` (the private Tor network of the tests of `pod-tor`: chutney at a pinned commit); `zig-mirrors.txt` |
| `scripts/agent/` | `check.sh` (the checks before a push); `check-windows.sh` (the tests compile for Windows); `check-macos.sh` (the workspace and its tests compile for macOS); `watch.py`, `watch_test.py` (the one watcher of each session of PodUtils: issues, pull requests, comments and Actions runs; its self-test); `runs.py`, `watch-runs.sh` (the runs of a commit); `secrets_like.py` (two TruffleHog detectors over files); `togit.sh` (path dependencies to a git tag) |
| `fuzz/` | the fuzz targets of the moved code (cargo-fuzz, its own workspace), their seeds (`fuzz/seeds/`) and their stable test; `fuzz/README.md` lists each target and the targets that wait for their crate; `scripts/fuzz.sh` runs them |
| `examples/` | `static-probe`, `static-probe-lean` (section 7) and the binaries of the live workflows (ring provider; `publish = false`) |
| `images/` | the build and test images, pinned by digest; `release/`: the images of the release binaries on ghcr.io (`FROM scratch`, call C789) |
| `schemas/` | the JSON Schemas (`*.schema.json`) that go out with each tag, each written from its types by a test, and `CHANGES.md`: one row for each change of a schema (`scripts/check-schemas.py`) |
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
  made safe to print), `names` (name checks);
* `http1`: the head of an HTTP/1.1 message (its bounds, the parse of a request line,
  a status line and the header fields, the write), with no I/O, for each
  reader of a crate (T-882).

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
| `pod-testkit` | podnet `tests/test_host/mod.rs` (`need_loopback!`, `need_localhost!`); podssh `crates/podssh-cli/tests/ssh_harness/` (feature `ssh`, russh 0.65 on ring). Later waves add the stand-ins, each behind a feature; a stand-in speaks bytes and HTTP only and takes no type of a pod crate, so that a crate can dev-depend on it, except the feature `githome`, which speaks the source wire of `pod-job` (call C786). The fake GitHub of a consumer's CI and its binary `pod-fake-github` (feature `github`, T-921, call C785): the App, the JIT runners, their broker after podrunner `crates/podrunner-broker-fake` at `d001608` (`SOURCES`), the `workflow_job` webhooks and a control API. The fake of githome's source of jobs for podrunner's CI and its binary `pod-fake-githome` (feature `githome`, T-948, call C786): `serve` holds the jobs, the claims, the credentials, the reports and the refusals with a control API; `source` is a program of the source wire of `pod_job::source`; the rules after podrunner `crates/podrunner-fake/src/source.rs` at `b1a1a28` (`SOURCES`); `serve --github URL` queues each job in a `pod-fake-github` too and mints its JIT runner there, so the feature `githome` takes the feature `github` (T-952, call C788). The conformance vectors of the roads (`vectors`, no feature): SIP004's answers of Shadowsocks, Trojan's requests and VMess's AEAD fixture as data, each with its source, and a runner for each that takes the codec as a trait (VEC-1 to VEC-5 of PodUtils/podssh#224) | 5,700 | pod-hostprobe (the one detection of an emulator); pod-manual (the lookup of a program in `PATH`, `host::tool`); pod-time (feature `github`, the UTC times of the webhooks); pod-job (feature `githome`, the source wire and the job offer) |

### M1: wave 1, the transport (in this order)

| Crate | Source | Lines | Pod deps |
| --- | --- | --- | --- |
| `pod-identity` | podssh-relay `identity/` (the entropy of `session/` and the private files of `cache` go to `pod-core`) | 900 | pod-core |
| `pod-term` | podssh `crates/podssh-terminal` | 3,700 | none |
| `pod-tls` | podssh-ws `crypto/` (feature `pure`), `tls.rs`, `tls/`, `stores/`, `tls_server.rs`, `tls_server/`, `tls_wrap.rs`, `bundle.rs`, `ech/` (feature `ech`; the HTTPS record through `Resolve`); podssh-relay `tls_groups.rs`; podnet `src/trust.rs` (roots, pins, `SSL_CERT_FILE` as a setting; the edge roots of Cloudflare come from `pod-cloudflare` as extra roots; no `init_crypto`), `src/tls_fragment.rs`; new for podnet's link (T-304): TLS between two peers that each pin the Ed25519 key of `pod-identity` (`peer`) | 6,300 | pod-core, pod-identity |
| `pod-proxy` | podssh-ws `dial.rs`, `proxy.rs`, `proxy_auth.rs`, `socks5.rs`, `mptcp.rs`, `race.rs` (the race of addresses), `cut.rs`, `lowat.rs`, `write_limit.rs`, `stun.rs` (now `pod-turn`'s, re-exported), `http.rs` and `http/` (the HTTP/1.1 heads), the GET client of `client.rs` (`https_get_accepting`); podnet `src/egress.rs`, `src/proxy_auth.rs` (Digest), `src/fetch.rs` (the HTTP client under DoH, the mint of the relay and the API of the quick tunnels), the server face of `crates/podnet-netns/src/socks.rs` (`serve`, `udp::unwrap`; T-338). Dial, race and SOCKS take `&dyn Resolve`; the bypass of a pinned loopback moves to the caller | 5,500 | pod-core, pod-tls, pod-turn (feature `stun`) |
| `pod-dns` | podssh-ws `resolve.rs`, `resolve/` (RFC 8484 wire over HTTP/2, HTTPS and SVCB records); podnet `src/resolve.rs` (DoH JSON, pins, the query by name through a proxy). Both DoH forms stay | 2,800 | pod-core, pod-tls, pod-proxy, pod-services |
| `pod-ws` | podssh-ws `client.rs`, `frame.rs`, `handshake.rs`, `session.rs`, `wire.rs`, `url.rs`, `close.rs`, `error.rs`, `plain.rs`, `server.rs` (its WebTunnel test holds the bytes of podssh's client: no trait until `pod-egress`), `h2_road/` (the pool a value of the caller; `shape.rs`, the client's HTTP/2 shape as data: SETTINGS and their order, the connection's window, the header order, the PRIORITY frames and the padding, with `Shape::browser` opt-in, and a CONNECT answered 4xx falls back to the upgrade of HTTP/1.1), `cleartext/`, `wake.rs`, the stream of `via/http2.rs`; podnet `src/ws.rs` (`bridge`, `dial`, `connect_bridge`, `accept_key`, the close codes, `close_hint`; podnet edges 8 and 18). Each connection goes through the caller's `Way` (C41); the headers of the caller for a host (a credential of a gate in front of the relay) are a field of the `Way`, which each road writes after the token, in cleartext only where the caller allows it (C775). The limits are settings (`Limits`: podssh's by default, 16 MiB a message; podnet's, the defaults of tokio-tungstenite) | 4,700 | pod-core, pod-tls, pod-proxy, pod-identity |
| `pod-turn` | GitHub #100 (T-377; PodUtils/podnet#485, PodUtils/podssh@T-621; calls C160 to C164): the codec of STUN of `pod-proxy` (podssh `stun.rs`, with the integrity of T-370) moved here with its tests and its plants, and its own MD5, behind the feature `stun` (no async, no TLS; `pod_proxy::stun` is a re-export); new: the SHA-256 credentials (PASSWORD-ALGORITHM(S), MESSAGE-INTEGRITY-SHA256), and a TURN client of RFC 8656 behind the feature `turn` over UDP (the caller's socket source), plain TCP and TLS on TCP (the caller's dial and TLS context): Allocate, Refresh on its timer, CreatePermission, ChannelBind, Send and Data indications, ChannelData, the 401 and 438 answers, and the TCP relay of RFC 6062 (T-501: Connect, ConnectionBind, ConnectionAttempt, and the framed `RelayedStream`); no DTLS | 4,100 | none (`stun`); pod-core, pod-tls (`turn`) |
| `pod-services` | the owner's ruling of 2026-11-05: the relays and the services of other parties that podssh and podnet hold in their code (relay lists, DoH resolvers, STUN, DERP and iroh relays, piping and mailbox servers, Cloudflare and WARP endpoints, and the rest), with the code, the scripts and the workflows that measure or watch them. T-048 takes the inventory of each consumer; T-049 moves it. The library holds the lists, each with its source and the date it was read; the watches run in the workflows of podcrates | (T-048) | pod-core |

### M2: wave 2, the relay

| Crate | Source | Lines | Pod deps |
| --- | --- | --- | --- |
| `pod-quic` | noq 1.4 (T-018, C43; `docs/research/quic/stack.md`). Sources: podssh-relay `quick/quic.rs`, `quick/probe.rs` (the settings of the transport, the endpoint, the race of handshakes and the probe; the regions of an edge stay in podssh), the `TokenKey` of `serve/quic.rs`; podnet `src/warp/quic.rs` (`WarpUdpSocket`: `QueuedDatagrams` over a `Link`; the client configuration is an argument, podnet edge 10). An endpoint runs over any `Datagrams`, with a waker for each sender of noq. The TLS provider and the roots are the caller's. `migrate` rotates the local connection IDs on a jittered schedule (`Schedule` as data, `RotatingIds` as noq's `cid_lifetime`) and moves a live connection to a new local address (`Endpoint::rebind`, RFC 9000 section 9); a peer that refuses the new path gives a new connection and a resumed session through `Carried::reopen` (PodUtils/podnet#486) | 1,500 | pod-core |
| `pod-session` | podssh-relay `session/` (with `handover/`), `udp/` (the resolver of a flow is the caller's `Resolve`), `answer.rs`, `transit/link.rs` (`ws_link::Link` over a session of `pod-ws`). No environment and no global: the settings, the relay's limits (`Settings::pair_window`, `move_bytes`, `move_age`) and the sink of the budget's lines (`Keeper::with_tell`) are the caller's; a target moves at a handover by `Movable` or by the caller's function (`Keeper::hand_over_with`). The test of the close table (`session_policy.rs`) goes with `pod-relay`, and the test of the role (`session_relay_resume.rs`) with `pod-relay-serve`; the handover test with the channel is `pod-e2e`'s | 8,400 | pod-core, pod-ws |
| `pod-e2e` | podssh-relay `e2e/` (Noise XX on snow 0.10): the channel and its carry, the handshake, the frames, the word of a proof (`Ask`), the two ends (`ends`), the gate of a node's person (`person`; the question goes to the caller's `Asker`), the net's document (`net`; the caller's `Docs`), the tokens of one leg (`legs`), the handover of a node's channel (`hold`). No process state: the documents of the net and the holds of a handover are fields of the caller's `Node` (`Node::docs`, `Node::holds`), and a node's lines are events `e2e.*`. The tests `e2e.rs`, `e2e_faults.rs`, `e2e_net.rs`, `routes.rs`, `person.rs` and the channel's test of `session_handover.rs` come with it; `e2e_layered.rs` goes with `pod-relay`; new in podcrates: the exporter of the handshake and the record layer (`exporter`, `record`: a suite agreed after the verdict, a rekey, the padding to buckets, a replay window, a single-use RESUME; T-301, calls C246, C255 to C258) | 5,000 | pod-core, pod-identity, pod-session |
| `pod-shape` | new code for issue #493 (the record layer of `pod-e2e`, T-301): a padding profile as data for a page load, a video call, a terminal and a file transfer (its bucket distribution, its rule, its first-flight target, its pace and its entropy band, with its source date and cover version); a seeded shaper that chooses a bucket for each record, never repeats a short cycle and counts the padded bytes; the first flight from the measured rate; a pacer with jittered keepalives; entropy control per window with one printable run per 64 KiB, and a nonce salted by a coarse timestamp with a four-minute window; the declared and observed shape with the states holds, fails and unseen, where a divergence is a warning. The record layer of `pod-e2e` takes the chosen body (`Records::seal_with`) and counts it | 1,200 | none |
| `pod-wormhole` | podssh-relay `code/` `mailbox.rs`, `wormhole.rs`, `phrase.rs`, `words.rs`, `mod.rs` (`Failed`, hex), and of `run.rs` the rendezvous (`Ends`, `opened`, `joined`); the mailbox harness (a stand-in in Rust, magic-wormhole's mailbox server and its Python peer). The AppID, the client name, the mailbox URLs and the way out are the caller's (C45). The pairing exchange (`exchange.rs`, the rest of `run.rs`) goes to `pod-relay` | 1,200 | pod-core, pod-proxy, pod-ws |
| `pod-netstack` | the cut of PodUtils/podnet#427: podnet `crates/podnet-vnet/src/stack.rs` and `stack/` without `proof.rs` (the stack, its sockets, its reactor; the `Acl` of `StackConfig` becomes the trait `Admit`, its resolver the trait `GatewayService`), `device.rs` (the recorder becomes the trait `PacketTap`), of `addr.rs` only `VirtualSubnet` (as `Subnet`); on smoltcp 0.14. `framing.rs`, the switch, the proxy, the dialer and the proof stay in podnet; `src/warp/ip.rs` goes to `pod-warp`, over this crate; the router of `podnet-netns` moves onto it with PodUtils/podnet@T-249 (T-170, step 2) | 2,900 | pod-core |
| `pod-wg` | the cut of PodUtils/podnet#447: podnet `src/warp/wg.rs` without its driver (sans-IO, on ring, call C8): the primitives (BLAKE2s, HMAC, the KDF, X25519, ChaCha20-Poly1305, TAI64N), `Keys`, `Initiation`, mac1, `Session` with its window of counters, the timers of `State` as `Tunnel` on the caller's clock; new: the cookie reply and mac2, and timestamps that never go back (`Stamps`); later the preshared key (T-561) and a responder for a list of peers (`Responder`, T-562, for `podnet wg serve`). `src/warp/wg/standin.rs` is the feature `standin` (a responder for the tests of a consumer). `Peer` with `from_exported`, the socket, `connect`, `connect_inside`, `connect_on`, `WgTx`, `WgRx`, `WgKeepalive` and the lock stay in podnet. With no feature, no tokio | 1,850 | pod-core |
| `pod-relay` | podssh-relay `relay.rs`, `open.rs`, `pool.rs`, `pool/`, `failover.rs`, `failures.rs`, `gate.rs`, `rank*.rs`, `token.rs`, `health.rs`, `addresses.rs`, `keyed.rs`, the token entries of `cache.rs` (its files and `cache/` are `pod-core`'s), `forward_layer/`, `pair.rs`, `pair/`, `code/exchange.rs`, `code/run.rs`, `grant/`, `reverse/` (the carrier `Door` of `serve` is the trait `reverse::wire::Door`), `http_road/`, `quic_road/`, `h2_road.rs`, `cleartext.rs`, `blocking/` (feature `blocking`, podvm), with their tests (`tests/e2e_layered.rs` and `tests/session_policy.rs` among them; `tests/session_relay_resume.rs` runs the role, so it goes with `pod-relay-serve`). Each call takes the caller's `Context`: the way out, the places of the cache, the `Settings` (no environment), the events and the state of a run (no global). podnet `src/ws_relay.rs` (`active()` becomes a handle that the caller gives), `src/relay_pool.rs`, `src/relay_reverse.rs`; the relay stand-in. The pool, the rank and the reverse road of podssh are the base; podnet's reverse mode (plain `/v1/node/NAME`, 32-hex ids) is merged last, as settings of the reverse road and its handler of a TCP or Unix target (T-026, call C74) | 19,800 | pod-core, pod-tls, pod-proxy, pod-dns, pod-ws, pod-quic, pod-identity, pod-session, pod-e2e, pod-wormhole, pod-services |
| `pod-link` | new code for podnet's link (T-300; PodUtils/podnet#466, row E0-2, and #467, rows E1-7 and E1-8; calls C116, C117, C120 to C124): many streams over one carrier byte stream (odd ids for the dialer, even ids for the listener; `OPEN`, `DATA`, `WINDOW`, `CLOSE`, `RESET`, `PING`, `PONG`, `GOAWAY`), a credit window for each stream and for the link (16 KiB at the start, at most 8 MiB), 256 streams by default and 4096 at most, frames of at most 1 MiB, a keepalive and the end of a silent link; the trait of a carrier with its `available()`, the unix socket carrier (the bind of mode 0600 and the claim of a path of podnet's `podnet-netns`) and the memory carrier. The registry of the carriers stays in podnet (C117). It runs over any byte stream, and over the application side of a session of `pod-session` for a resume (C120). The ARQ of a carrier that loses frames is T-310. The tcp carrier (T-396, PodUtils/podnet#513; calls C177 to C179), behind the feature `tcp`: TLS 1.3 over a dial of the caller's way out, each side pinning the Ed25519 key of the other by `pod-tls`'s `peer`, its listener with concurrent handshakes, and the key of the peer at each accept; the carriers `tcp`, `ws` (on `pod_ws::serve`), `h2`, `poll` (the long poll, SSE, a chunked stream, the simplex poll), `quic` (on `pod-quic`), `udp` (KCP of the crate `kcp`), `dns` (the ARQ over DNS queries and TXT answers of `pod-dns`, a simplex carrier) `icmp` (the ARQ over ICMP echo messages, a raw socket on unix) and `s3` (objects of a bucket of the S3 API, with SigV4; its stand-in on the loopback for the tests of a consumer is the feature `s3-standin`, T-627), each with TLS and the pin of an Ed25519 key; the selective-repeat ARQ `arq` over a lossy datagram carrier (T-396, T-420, T-421, T-422, T-423, T-424, T-310, T-495, T-493, T-497, T-494, T-627) | 5,900 | pod-core, pod-identity; pod-tls (feature `tcp`); pod-ws (features `ws`, `h2`, `poll`); pod-quic (feature `quic`); pod-dns (feature `dns`) |
| `pod-natprobe` | new code (T-802, call C922; GitHub #212, PodUtils/podcrates#256): a probe of the NAT on the path, with no I/O in its core (`ProbeMachine`): the mapping and the filtering of RFC 5780 on separate sockets, with positive controls, and with two plain servers where no server has `OTHER-ADDRESS`; the port allocation (preserved, sequential, a counter, random) on the ring of ports 1024 to 65535; the IP pooling; hairpinning; CGNAT and double NAT; ALG; IPv6 and NAT64 (RFC 7050, RFC 6052); the gateway (NAT-PMP, PCP, UPnP IGD, with no change of a mapping by default); the lifetime of a mapping (`Deep`); a traversal strategy for two peers; a profile of 20 bytes for the wire. Features: `run` (the runner on tokio, on the caller's sockets, clock, entropy and resolver), `report` (JSON), `sim` (a NAT simulator in virtual time). With no feature, no tokio | 9,900 | pod-core, pod-turn (feature `stun`); pod-services (feature `run`) |

### M3: wave 3, the adapters

| Crate | Source | Lines | Pod deps |
| --- | --- | --- | --- |
| `pod-transit` | podssh-relay `transit/` (without `link.rs`, which is `pod_session::ws_link`) at `b9c04657` (PodUtils/podssh@T-684, T-027): the words of a pair, the meeting with its proof, the client, the node; the transit server of the tests is magic-wormhole's own relay. The relay, the way out, the trust and the lines are the caller's `Config`; the variable `PODSSH_TRANSIT` stays in podssh (calls C598 to C600) | 1,100 | pod-core, pod-ws, pod-session, pod-services |
| `pod-warp` | podnet `src/warp/` without the sans-IO part of `wg.rs` (`pod-wg`; its driver comes here), `quic.rs` (`pod-quic`), `up.rs`, `export.rs`, `ssh_config.rs` (those stay in podnet); its `ip.rs` runs on `pod-netstack` (PodUtils/podnet#427); and without the backends of `warp run` (`podnet-netns`, stays in podnet); TUN and routes behind `cfg(target_os = "linux")`; `eprintln!` and `process::exit` become events; podssh `crates/podssh-warp` retires (its endpoint order and API routes are checked against podnet's); its stand-in edge and API into `pod-testkit`. The SOCKS5 datagram path of podnet `podnet-netns/src/upstream.rs` (UDP ASSOCIATE, the command 0x83, the frames) is not here: it is `pod_proxy::udp` (C23, T-083), and `pod-warp` calls it. `pod-session` keeps its own UDP-in-a-stream adapter (T-020). One HTTP/3 path carries CONNECT-IP, CONNECT-UDP (RFC 9298, the URI template and the DATAGRAM capsule) and a stream CONNECT; `provider` splits the enrolment into a trait, an adapter of Cloudflare and a local adapter, with `EnrolError` apart from a transport failure; `h3::shape` and `h3::server` are the selectable server shape (`h3`, the control stream, SETTINGS, the profile `cubic`, `newreno` or `bbr`) with `ShapeReport` for `doctor` (PodUtils/podnet#486) | 10,000 | pod-core, pod-dns, pod-proxy, pod-quic, pod-netstack, pod-wg, pod-services |
| `pod-cloudflare` | podnet `src/cloudflare/`, `src/tunnel.rs`, `src/race.rs` (the race of the legs), `src/edge_h2.rs`, `src/edge_discovery.rs`, `src/quicktunnel.rs`, `src/config.rs`, `src/proxy.rs` (the dispatch of the edge's streams to a `ServiceDispatch` trait that podnet implements), the edge side of `src/ws.rs` (`serve_websocket`, `forward_path`; edges 16 and 17), the HTTP/2 leg through the relay, the edge roots, the pin of cloudflared and its watch; podssh-relay `quick/` moves onto it | 17,900 | pod-core, pod-tls, pod-dns, pod-proxy, pod-ws, pod-quic, pod-relay, pod-services |
| `pod-devtunnel` | new code for GitHub #94 and PodUtils/podssh#216 (T-378): the client side of Microsoft Dev Tunnels, from the vendor's client microsoft/dev-tunnels (MIT) at `37956c63`: the connect information of a tunnel from the management API (by name, or by id on the host of its cluster; the scope `connect`; with the user's token, a tunnel access token or no token, T-489), the relay WebSocket with the subprotocol `tunnel-relay-client` through the caller's `pod_ws::Way`, SSH (russh) with the user `tunnel` and `none` and a host key of the tunnel, `direct-tcpip` channels as streams, a bounded reconnect with a fresh connect token, the named steps of a failure, and the stand-in (feature `standin`; calls C165 to C169). Each value of the protocol is a fact of `microsoft-devtunnel` of `pod-services`. The creation, the list and the delete of tunnels, the device-code logins and the token store are the second part of T-378 | 2,700 | pod-core, pod-proxy, pod-tls, pod-ws, pod-services |

### M4: wave 4, the other roads

Each crate of M4 starts with a study entry: the API, the C on each
target, the fork. A study can change its row.

| Crate | Source | Lines | Pod deps |
| --- | --- | --- | --- |
| `pod-derp` | podssh-relay `derp/`; the DERP stand-in (feature `standin`) | 3,500 | pod-core, pod-ws, pod-identity, pod-session, pod-proxy, pod-services |
| `pod-piping` | podssh-relay `piping/` at `b9c0465` (T-032): the paths, the hello and the marks of a pair, the streamed and the numbered link, HTTP/1.1 and HTTP/2, the client and the node across the public servers of `pod-services`, the probe of the upgrade; the dial through the caller's `pod_ws::Way`, the user agent of the caller; the Piping harness (nwtgck's two servers on the loopback, `scripts/ci/piping-tests.sh`) | 2,400 | pod-core, pod-tls, pod-ws, pod-session, pod-proxy, pod-relay, pod-services |
| `pod-opentunnel` | podssh-relay `opentunnel/` at `b9c0465` (T-033): the API of OpenTunnel and its retries, the record of a tunnel in the caller's places, the provision, the bridge and its messages, the visitors' connections, the node with its renewal and the client by TLS by name; the dials through the caller's `pod_ws::Way`, the node's TLS with the caller's provider, the user agent of the caller; the OpenTunnel stand-in, and OpenTunnel's own server in `opentunnel-interop.yml` | 2,600 | pod-core, pod-tls, pod-ws, pod-session, pod-proxy, pod-services |
| `pod-acme` | podssh-cli `acme/` and the `Url` of `update/fetch.rs` at `b9c04657` (PodUtils/podssh@T-684, T-034): the account, the order by TLS-ALPN-01 through the `Served` of `pod-tls`, the CSR, the state and the renewal. The keys are P-256 keys of `p256` (pure Rust), as `pod-tls`'s server signs: the pure provider of `pod-tls` loads no private key (call C601). The HTTPS client (`pod_proxy::Client`), the trust, the places of the state and the lines are the caller's; Let's Encrypt's directory is a fact of `pod-services`. podssh's Pebble interop runs its binary and stays there | 1,700 | pod-core, pod-tls, pod-proxy, pod-services |
| `pod-egress` | podssh-ws `via/`, `forwarder/`, `wstunnel/`, `wisp/`, `meek.rs`, `meek/`, `webtunnel.rs`, `pt.rs`, `cloudflare.rs` (the Access client) at `2ffb7cd` (T-035, part 1): a feature for each road; the shared parts (the carrier of a WebSocket session, the end of input, the notes); the dials through the caller's `pod_ws::Way`; each road that takes its target from the client is a `Dial`; the built-in forwarders of `pod-services`; `forwarder-live.yml`. Part 2 at the same commit: `vless/`, `vmess/`, `trojan/`, `shadowsocks/` (VLESS over a WebSocket and XHTTP, VMess's AEAD form, Trojan, Shadowsocks's AEAD form and SIP022), each a `Dial` with a typed failure; `vmess-interop.yml`. The shared parts of a road's first write and of early data: `coalesce` (one write of 3,333 bytes, padded when the head and the data are short, once for each session, in the WebSocket, XHTTP and HTTP/1.1 paths) and `replay` (`ReplayWindow`, a nonce of early data kept 10 minutes; a repeat keeps the session at 1-RTT). Part 3: `eice/` (AWS's EC2 Instance Connect Endpoint, SigV4) and `iap/` (Google Cloud's IAP for TCP, its carrier with its events and typed ends), each a feature, not a `Dial` | 10,000 | pod-core, pod-proxy, pod-tls, pod-ws, pod-services |
| `pod-relay-serve` | podssh-relay `serve/` (with `serve/mailbox/`) | 13,200 | pod-core, pod-relay, pod-ws, pod-tls, pod-session, pod-e2e, pod-identity, pod-wormhole, pod-cloudflare |
| `pod-iroh` | podssh `crates/podssh-iroh` at `0370b0b` (iroh 1.4 on ring, `tls-ring`; PodUtils/podssh@T-684, T-037): QUIC between keys, through an iroh relay whose dial takes the caller's proxy (`pod_proxy::ProxyChoice`), the caller's resolver and provider; a node's record of pkarr with each of its roads; each request of the crate through the caller's `pod_ws::Way`; `vendor/iroh-relay` 1.4.2 with podssh's patch (SOCKS5), by `[patch.crates-io]` | 6,100 | pod-core, pod-session, pod-identity, pod-derp, pod-opentunnel, pod-proxy, pod-tls, pod-ws, pod-services |
| `pod-tor` | podssh `crates/podssh-tor` at `0370b0b` (arti 0.48 with SQLite built in; PodUtils/podssh@T-684, T-038): a stream through Tor, each first hop and each transport of a bridge (meek_lite, WebTunnel of `pod-egress`) through the caller's `pod_ws::Way`, and an onion service | 1,850 | pod-core, pod-tls, pod-ws, pod-egress |
| `pod-ts` | podssh `crates/podssh-ts`; `vendor/tailscale-rs` with its patches; the headscale harness | 900 | pod-core, pod-proxy |
| `pod-gsr` | the GSRN relay of gsocket: the owner's prototype `pod-gsr` 0.3.0 (`pod-gsr.zip` of GitHub #24) and its data file `hackerschoice-gsr` for `pod-services`; the protocol of `hackerschoice/gsocket` at `5950647f` (`SOURCES`). TLS with SRP on a static OpenSSL (C34) | 2,200 | pod-core, pod-services (its live test also takes the proxy crate) |

### The packet sidecar (PodUtils/podnet#495)

The sidecar sits below the stream and above the link. It is new code, and
no consumer takes it in a default build: every part is behind the feature
`sidecar`, which is off by default (rules SYN-1 and LIM-2). The raw socket
of the probe checks the host at run time and reports a named skip.

| Crate | Source | Lines | Pod deps |
| --- | --- | --- | --- |
| `pod-sidecar` | new code for PodUtils/podnet#495: the five primitives (drop, tamper, duplicate, fragment, coalesce) and the strategy file with its date; a deterministic local TCP-like middlebox simulator with the handshake, the data phase, the window, the reassembly, the checksum, the retransmission and the FIN rules, and the divergence of a run; a genetic search over strategies against the simulator alone; the disagreement probe with `Disagrees`, `Agrees` and `Unknown`, and its raw socket behind the runtime check; one split library with one configuration format (split at an offset, disorder, fake TTL, tampered checksum, padded first segment) and its defeats; fake TCP message framing with its costs; a per-flow bond of links; and the limits page with the condition and the detection of each limit | 4,310 | none |

### M8: the fleet (the asks of GitHub #35 and #36)

The new crates for the asks of cfb and githome. podcrates gives libraries,
schemas and shared CI scripts. Each command line, unit file and live step
stays in its product, and a broker binary is a thin `main` in the product
that hosts it (call C27). The JSON Schemas live in `schemas/` and go out
with each tag (the owner, 2026-11-09). `pod-egress` keeps the roads of M4;
the egress broker is `pod-broker` (call C25).

| Crate | Source | Lines | Pod deps |
| --- | --- | --- | --- |
| `pod-broker` | the egress broker: per-job policy (allow-list, or open with fixed denies), one resolve and a deny file, the upstream CONNECT chain, a TLS end with a CA for each job, placeholder credentials swapped and scrubbed, a JSON audit line, a Unix-socket listener, a drop at the TTL. Mostly new code; the vocabulary of PodUtils/podvm#184, the rules of `podnet pod` and `warp proxy`, podssh `exit --listen` and `pipe http-listen:` | (T-099) | pod-core, pod-proxy, pod-control (the control of a broker, T-1000), pod-tls (the TLS end of T-100), pod-acme (the P-256 keys and the PEM of the CA of a job); the resolver comes from the caller as a `pod_core::Resolve` |
| `pod-unit` | hardened service units with no secret in them (T-102, calls C859 to C861): the model `Unit` (the program, its words, its variables, its secret files, `Scope::User` or `Scope::System`, the start with `sd_notify`, the restart with `RestartPreventExitStatus=64 78`, the drain at a stop, the hardening); the renderers of a user unit and a unit of the system of systemd (`User=` or `DynamicUser=yes`, `LoadCredential=`, each line of the hardening, the lines of seccomp alone for a user unit), a LaunchAgent and a LaunchDaemon, a task at the logon and a Windows service of `sc.exe`, the lines of the crontab; the check of a secret in a unit (the place and the rule, never the word); the audit of the hardening of a unit; the install and the removal of a unit of the user, and of a unit of the system as root or an administrator (T-1027, call C885: systemd, a LaunchDaemon, a Windows service of `sc.exe`; the privilege, the manager, the account and the files of the secrets checked before any write), through the caller's `Host`. Sources: podssh `crates/podssh-cli/src/node_install/` (base) at `db0db54`, podnet `crates/podnet-core/src/autostart.rs` at `f4ad31d`, podrunner's `print_unit` (`SOURCES`) | 2,400 | pod-core |
| `pod-control` | one versioned JSON control API on a Unix socket or a Windows named pipe (T-103, calls C842 to C844): the catalogue of the methods with the wire name, the route, the tier and the summary of each (`version`, `methods` and `auth` local; `status`, `metrics`, `reload`, `drain` and `stop` delegated to the host's trait `Host`; the host's own `Extra` rows), the tiers read, operate and master with one answer for a name that is not there and a method above the tier, the duplex line frame (a cap, UTF-8, a deadline of each line, calls back by id with a budget for each request and each connection), the check of the peer user, the snapshot of `status`, the client, the JSON Schema `control.v1` and the conformance check of a program's own server; std threads, no tokio. New code, with the JSON of podnet `src/status.rs`, podssh `status` and podrunner's `crates/podrunner/src/control.rs` | 3,100 | pod-core |
| `pod-job` | the records of the job of a CI runner (PodUtils/podrunner#17): the job offer, the unit capability, the run report and the public report, their ids and refusal codes, the `check` of each record, the status of a host (`status.v1`) and the source wire (`source-request.v1`, `source-reply.v1`), and their JSON Schemas in `schemas/`, generated from the types (Q-A8; call C50). Source: podrunner `crates/podrunner-schema` and `schemas/` at `9ab7fd3`; `status.v1` and the source wire at `ca8c308` (T-922). The credential files (`secret.rs`) stay in podrunner; the wire holds its own `Minted` | 1,500 | none |
| `pod-actions-expr` | the expression language of GitHub Actions (`${{ }}`) for podrunner's local mode (T-251, PodUtils/podrunner@T-046, Q11): the lexer and the parser of the runner's grammar, the evaluator with GitHub's loose equality, coercion and case rules and the object filter, the functions, the templates and the implicit `success()` of an `if:`; the contexts and the status are the caller's; no I/O (`hashFiles` fails by name). New code from the documents of GitHub | 3,000 | none |
| `pod-actions-workflow` | a workflow file into a typed model (T-251): the YAML of saphyr-parser into a tree with lines (call C105), the keys of the workflow schema, an unknown key kept as data with a warning, a reusable workflow named and not expanded; the expansion: the graph of `needs` (a missing need and a cycle by name), the matrix with `include` and `exclude`, the concrete jobs, each skip with its reason; the filters of `on:` and the decision whether a workflow takes an event (T-572, PodUtils/podrunner#97); the calls of reusable workflows, with their inputs, secrets, outputs and limits (T-576, PodUtils/podrunner#98). New code from the documents of GitHub | 4,600 | pod-actions-expr |
| `pod-actions-results` | the ArtifactService of the results API of GitHub Actions (T-565, PodUtils/podrunner#95): the twirp JSON calls `CreateArtifact`, `FinalizeArtifact`, `ListArtifacts`, `GetSignedArtifactURL` and `DeleteArtifact`, and the block blob calls of the signed URLs, over one store for each run; a random signature in each URL, a name unique in a run, the size and the SHA-256 checked at the finalize, a limit of bytes for each run. The store of a rerun replaces an artifact of the same name. The CacheService of the same API (T-588, PodUtils/podrunner#99): the entries by key and version in the scopes of a ref, the restore keys, a random entry id, `Range` downloads, eviction at the limit. No HTTP server: the caller gives each request to `ArtifactStore::answer` or `CacheStore::answer`. Source: podrunner `crates/podrunner-broker-fake/src/artifacts.rs`, `src/cache.rs` at `872b43f` (`SOURCES`) | 1,900 | pod-core (no default feature) |
| `pod-actions-store` | the actions of a workflow by tag (T-564, PodUtils/podrunner#94): each remote `uses:` of a workflow and of each composite action that it names, resolved to a commit by a forge (the `Forge` trait; `Git`: `git ls-remote` with the peeled tag, a shallow fetch, `git archive`), the store of tarballs `OWNER_REPO/SHA.tar.gz`, and the lock file (`OWNER/REPO@REF COMMIT sha256:HEX`, sorted) with the digest of the uncompressed tar; the modes online, frozen and offline; the checkouts of other repositories as trees of the same lock (`HEAD` with no ref), and the images of `container:`, `services:` and `docker://` by tag, pinned by digest through the consumer's `ImageSource` (T-587, PodUtils/podrunner#99). Source: podrunner `crates/podrunner-local/src/lock.rs`, `src/store.rs`, `src/images.rs` at `872b43f` (`SOURCES`) | 1,500 | pod-actions-workflow |
| `pod-lab` | the classes of hostile hosts as profiles (T-692, PodUtils/podrunner#180, PodUtils/podvm#424): each profile as data (its parts, the rows of `pod-hostprobe` that each part refuses with an errno or allows, the mechanism of each part), `check` of a profile against the findings or the JSON report of `pod-hostprobe`, and `run` of a command in a lab of the parts that a host with no root holds (a seccomp filter through `pod-confine`, Landlock, a tmpfs in a user namespace, inherited descriptors), checked from inside before the command; the static binary `podlab` (feature `cli`). New code. The Actions step, the machine tier of podvm and the network labs of podnet are part 2 | 2,600 | pod-core, pod-hostprobe, pod-confine, pod-manual, pod-tls |
| `pod-manual` | the help and the manual of each command line (T-700, PodUtils/podnet#584; call C681): the model that a program fills (the commands, other names, flags with a kind and a status against another tool, arguments, rules between options, topics, typed facts: variables in groups, exit codes with the code of the payload, files, the keys of a configuration file, a program's own tables), a reader of `clap` (feature `clap`), the writers (plain text, man(7), JSON, `llms.txt`), the help of each command, `CLI man SECTION`, the pager, the text rules of the ruling "The text of a command line", the checks that a program calls from its tests (a generated file, the variables, the exit codes, the flags against the parser) and the form of an error. Sources: podnet `src/manual/` and `scripts/gen-llms.py` at `78a3911` (base); the generic part of podssh `crates/podssh-cli/src/man/` and `pager.rs` at `cfc3ae3` (`SOURCES`). `podlab` takes it first | 4,000 | none (pod-core with the feature `core`) |
| `pod-release` | the release index (`release-index.v1`), its writer for CI (`scripts/release-index.py`, call C793) and its verifier: the keyless Sigstore bundle of `SHA256SUMS` with the caller's identity (the workflow URI and the OIDC issuer), the check of Rekor's promise (v1) or of the inclusion proof with its checkpoint (v2) and of an RFC 3161 timestamp (T-964, calls C820 and C821), the SHA-256 of each file (T-104); the self-update of a running binary: the fetch through the caller's `pod_proxy::Client` (feature `fetch`), the trial run, the replace, the kept binary and the rollback (T-880); the static binary `pod-release` (feature `cli`, T-965, calls C804 to C807): `verify` of a release directory by the signer that the caller names, `sums` before the signature, `index`, an asset and an image of each release. Sources: podssh `crates/podssh-cli/src/update/` at `5975bb0` (base) and podnet `src/update.rs`, `src/update/` at `613a94b` (`SOURCES`). Not yet: minisign, the rollback data of an installer (T-830) | 3,300 | pod-core, pod-tls; pod-proxy (feature `fetch`); pod-manual (feature `cli`) |
| `pod-detach` | the detach of a command by a re-exec of the binary, its state file under the lock of `pod_core::lock` (the ruling "Shared code", COORD-131): podnet's module and podssh's near copy | (T-879) | pod-core |
| `pod-time` | the zone files of the system (TZif, versions 1 to 3, with the POSIX TZ string of the footer), the UTC offset of a zone at an instant, and the cron of `on.schedule` with its `timezone` (call C779; PodUtils/podrunner#291); std alone, no C library | (T-907) | none |
| `pod-cover` | new code for PodUtils/podnet#491 and PodUtils/podssh#225 (podssh T-764 to T-766): the cover site with name routing, an unknown name's own 404, no traversal and a plausible `Server` header; the cover script as TOML with its date, cover version, record histogram, timing and split policy; the HMAC-SHA-256 switch over a timestamp, a nonce and the TLS exporter, with a 240-second window, a replay set and the fall-through to the site; the multiplexer of a page load and a session over one budget, cover first; and the application-layer Ed25519 credential against the same chain and name. Sans-IO: no socket, no clock and no global state. `serde` and `toml` for the script, `hmac` and `sha2` for the proof, `ed25519-dalek` for the credential | 2,000 | pod-core |
| `pod-harness` | the verification harness of PodUtils/podnet#501: three deterministic offline classifiers (a field matcher for the SNI, the ALPN, the JA3 and JA4 hashes and the TLS record sizes, a flow classifier over size and timing distributions, and a first-N-byte classifier over a byte n-gram table), each with a model from JSON and a score in 0..1 with a named reason; a capture format; a local middlebox simulator with no raw socket, its own fault model and a decision record; a road by profile matrix that renders a table with skipped rows and the benign false-positive rate; a relocatable zip with the fixtures, the models, the simulator, the expected scores and a POSIX `sh` `run.sh`; the exit codes of `pod_core::Class`; and the documentation claim check. New code | (PodUtils/podnet#501) | pod-core |

`pod-todo` (an acceptance registry, T-105) and `pod-testkit` (the stand-ins
as programs, T-106) grow in M8; `pod-core` (T-101), `pod-services` (T-112),
`pod-hostprobe` (T-107), `pod-session` (T-110, T-120), `pod-relay` (T-111)
and the adapters (T-113 to T-117) take clauses in their own waves. The
products of the fleet (`podrunner`, `podnode`, `podinit`, `podlog`,
`podstat`, `podkey`, `podsnap`, `podrepl`, `podrelay-worker`, `podforge`
and the installer `pods`) are repositories of their own.

### M9: the no-network carriers

The carriers and the stores of a host with no network
(PodUtils/podnet#499). A host with no path has a serial port, a radio and
a disk; each one needs a session layer that uses no network.

| Crate | Source | Lines | Pod deps |
| --- | --- | --- | --- |
| `pod-offline` | new code for PodUtils/podnet#499 (SERIAL-1 to 3, HF-1 to 4, SPOOL-1 and 2, DISC-1 and 2, BUNDLE-1, SINK-1, PAIR-1 and 2, REPL-1 and 2): the address `serial:PATH[,BAUD]` and the raw 8N1 open with `O_NOCTTY`, `TIOCEXCL` and the settings back at each end; AX.25 frames in KISS framing with a 256-byte maximum and a window of 1 to 7 with acknowledgements and a timeout from the baud rate; an ARDOP or VARA modem driver with an ASCII command set, 300 to 3000 bit/s, a busy signal, a turnaround delay and half-duplex, and a session ARQ that defers to the ARQ of the modem; a spool of files with a checksum, a sequence and the sender; a signed announcement in a DNS-SD TXT record and one radio interface; an offline bundle with a signed manifest and a delta from a named base; a capture format merged by time with redaction at write time; pairing by an eight-word single-use code bound to a key of `pod-identity`; an append-only replication log with last-writer-wins for each key and a vector clock | 2,100 | pod-identity |

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
| `pod-hostprobe` | podvm `crates/podvm-probe` (part: one disposable child for each probe, the errno and never a boolean, "could not run" as a third state, the raw system calls) and `crates/podvm-image/src/{space.rs,exec_probe.rs}`; podnet `crates/podnet-netns/src/doctor.rs` probes the same way | 7,200 | pod-core, pod-beneath |
| `pod-oci` | podvm `crates/podvm-image` (part: the reference, the digest checked while it is written, the OCI types, the platform, the logins of docker and podman at the caller's paths, the registry client over the caller's HTTP client and trusts, the insecure policy from the caller's hosts, the host bundle and the roots, the resolve and the fetch pool into the caller's `Blobs`, the constants of the OCI layout); the content store, the pull into it, `AcceptAny` and docker's codes stay in podvm | 5,000 | pod-core, pod-tls, pod-proxy |
| `pod-extract` | podvm `crates/podvm-extract`: the rootless apply of the layers (the order, the whiteouts on the basename, the containment of each path, an ownership sidecar in place of chown, the digest of each blob while it is read, the completion marker); the caller gives each layer as a blob path with its digest and media type, and the place; the layout of the store stays in podvm | 2,800 | pod-core, pod-hostprobe |
| `pod-rootfs` | podvm `crates/podvm-complete`: the completion of a root file system from the host, each write held in the root (device nodes or shims, the `/dev` links, the resolver, hosts and mtab, the accounts and nsswitch, the walls of pacman, apt and zypper, the CA bundle and the CA directory of the host, `https` package sources); the caller gives its names, the host bundle and the probe of a mirror; the banner, the record ids and the HTTPS request stay in podvm | 4,300 | pod-core, pod-hostprobe, pod-extract |
| `pod-enter` | podvm `crates/podvm-enter`: the entry of a payload (the chroot sequence, the namespace rung, the entry with no chroot, the launch ladder: memfd, FUSE, tmpfs, run directory, cache; the device hand-over; the reader of ELF files); the names are the caller's (`Names`), and docker's 125 stays in podvm; the confinement of a payload (`Plan.confine`, T-108) | 8,000 | pod-core, pod-hostprobe, pod-confine, pod-beneath |
| `pod-supervise` | podvm `crates/podvm-supervise` and the `Lock` of `crates/podvm-image`: detached payloads (launcher state, pidfds, logs, signals, the lifecycle); the entry of a payload is the caller's | 2,300 | pod-core, pod-hostprobe |
| `pod-guest` | podvm `crates/podvm-windows`: a disposable QEMU guest (the boot plan on KVM or TCG, the overlay, the FAT16 mailbox, the `cmd.exe` and DOS agents, QMP, the bounded stop, the store of a base image with its pinned digest); the download, the base caches and docker's exit range stay in podvm. New: a read-only squashfs reader with xz and the x86 BCJ filter, and the module set of a guest kernel from its modloop (T-206, PodUtils/podvm#260) | 5,300 | pod-core, pod-hostprobe |
| `pod-beneath` | the one rule of podvm `crates/podvm/src/userns/host.rs` (`beneath`), `crates/podvm-dispatch/src/vols.rs` (`beneath`), `crates/podvm-interpose/src/map/base.rs` (`pin`) and the host side of `pod-enter` `ns_volume.rs` (`--podvm-volume-base`): a host path under a base opens with `openat2(RESOLVE_BENEATH \| RESOLVE_NO_SYMLINKS)`, a refusal for each case; new code, `no_std`, no allocation, the system calls of the caller (T-997, call C834; PodUtils/podvm#611) | 900 | none |

### M7: the widened scope of podvm

The owner, 2026-11-06: "podvm will make everything that sand-net does,
but without root, in the environment we target". Each crate of M7 starts
with a study entry, as M4 does.

| Crate | Topic | Pod deps |
| --- | --- | --- |
| `pod-microvm` | fast disposable microVMs from an OCI image, on KVM or TCG (kernel, initramfs, virtio devices, boot): PodUtils/podvm#168 | pod-oci, pod-guest |
| `pod-hold` | the hold of each connect of a payload, with its level (`enforced`, `advisory`, `none`) and a proof: the generic core of podnet `crates/podnet-netns` (T-166, call C48): the seccomp filter, its start, the receive of a notification, the memory of the task, the reply, the table of the sockets with the flow budget, the netns entry and its wire (the TUN, else a veth pair with a packet socket: T-201), and with the feature `syn-hold` the hold of each SYN of podnet's router on `pod-netstack` (dial first, then an accept or a RST: T-170, calls C90, C91, C100). The handlers, the rules, the upstreams, the dials, the relays, the device of the router with its DNS and its UDP flows, and `pod doctor` stay in podnet (PodUtils/podnet#414, #422, #451) | pod-core, pod-hostprobe; pod-netstack (`syn-hold`) |
| `pod-sud` | an in-process interceptor of system calls on Syscall User Dispatch, with paths resolved in a root (`openat2(RESOLVE_IN_ROOT)`); no ptrace, no privilege: PodUtils/podvm#109, PodUtils/podvm#169 | pod-core |
| `pod-confine` | confinement with no privilege (T-108, PodUtils/podvm#259): Landlock rules for the paths and the TCP ports, the Landlock scopes, a seccomp deny filter (with a closed UDP and no MPTCP under a TCP list, T-209; the socket families that Landlock does not check, the rights of ABI 9 and 10, T-213) and `no_new_privs`, compiled before the fork and applied in the child with no allocation; new code, except `loader` (the program of a dynamic loader, T-998): podvm `crates/podvm-interpose/src/exec/ldso.rs` at `cdbcaa6` (`SOURCES`). The rlimits are `pod-enter`'s (T-176), and `pod-hold` has the seccomp filter of a hold: PodUtils/podvm#170 | pod-core, pod-hostprobe |
| `pod-netlab` | network labs in a guest that is root (T-207, PodUtils/podvm#260): NAT44, NAT66, NAT64 with DNS64, CGNAT, IPv6 only, firewall policy, impairment: the knobs and the layout of podvm's issue 178, the 27 offline scenarios, the steps that build and remove a lab, the expected verdict of each probe, and the judge of the measurements and of the nftables counters before and after (calls C145 to C149). The probe program, the world's services, a run on a kernel, the VPN scenarios and a lab on a user-space stack are T-345 to T-349 | pod-core (T-349 adds the user-space stack) |
| `pod-pcap` | packet capture, counters and connection logs with no `CAP_NET_RAW`; a pcap writer: PodUtils/podvm#182 | pod-core |
| `pod-sandprobe` | the security probe inside a payload, and the compare of two reports against a list of expected differences: PodUtils/podvm#186 | pod-core, pod-hostprobe |
| `pod-incus` | a client of the REST API of incus on its Unix socket, with no `incus` binary (T-179, PodUtils/podvm#253): the server facts, the import of an image from a stream, the instances and their state, exec with `record-output` or with its websockets, the console log, the files, the operations and their wait; the floor is incus 6.0 LTS. New code from the API of lxc/incus at `63377d9b`. The back end, the conversion of an OCI image and the start of a user's `incusd` stay in podvm | pod-core, pod-proxy, pod-ws |
| `pod-udf` | a reader of UDF (ECMA-167, UDF 1.02 to 2.60, with the metadata partition) that gives the byte ranges of a file in an ISO, so that a client fetches one file of a Windows ISO by Range requests (T-591, PodUtils/podvm#350). No I/O of its own: the caller gives a `ReadAt`. New code from the specifications | none |
| `pod-qga` | a blocking client of the QEMU Guest Agent over the Unix socket of its virtio-serial channel (T-594, PodUtils/podvm#350 item 5): the sync `guest-sync-delimited` after a byte 0xFF on connect and after a call with no answer, `guest-ping`, `guest-exec` and `guest-exec-status` with a wait, `guest-file-open`, `-read`, `-write`, `-close` with base64 bodies and the copy of a whole file, `guest-shutdown`; a bound on each call, a size limit on each answer, and a typed error with the words of the agent; the readers of the answers are pure, on each target. New code from the schema `qga/qapi-schema.json` of QEMU | pod-core |

Names held, with no crate: `pod-wireguard`, `pod-quick`, `pod-udp`,
`pod-mailbox`, `pod-reverse`, `podcrates`, `podutils`, `pod-utils`,
`pod-probe`; for podrunner (T-146): `pod-dockerapi` (phase 3),
`pod-actions-proto` and `pod-actions-exec` (the native runner, Q11); the
owner's name of 2026-11-11: `pod-mask` (T-180); for the asks of podswarm
(T-430 to T-437, the owner's ruling "podswarm"): `pod-proc`, `pod-tty`,
`pod-fuse`, `pod-cas`, `pod-view`, `pod-attest`, `pod-ssh`.

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
  the freeze starts by a COORD issue of podcrates `(to CONSUMER)`
  (`docs/maintenance.md`, "Talk between sessions").


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
  node's person (in `pod-e2e`), `Door` (in `pod-relay`), `Carrier` and
  its `Listener` (in `pod-link`): the traits that cut the edges to the
  code of a consumer.


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
  calls one function of each (`fuzzing` is out). The static job also
  builds `podlab` of `pod-lab`, `pod-fake-github` and `pod-fake-githome` of
  `pod-testkit` and `pod-release` of `pod-release` on each Linux target and
  checks them; `scripts/agent/check.sh` checks `pod-release` on x86_64. `examples/static-probe-lean`
  builds the crates that need no tokio (pod-core and pod-tls with no
  default feature, pod-wg, pod-confine, the STUN codec of pod-turn,
  pod-udf, pod-beneath, pod-control, pod-unit).
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
   with the next ones, so that each push is green. Run the scans of the
   workspace (the tests of `pod-tls`, among them `no_permissive_verifier`,
   and the scripts `check-*.py`) on the crate before it lands: an imported
   file can break a rule of another crate.
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
   checkout of the push (each crate, and each fork of `vendor/` that a
   `[patch]` of the consumer names), and runs: the tests of the replaced module; the
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
  --workspace --all-features`, `cargo todo check`, `cargo doc`, the lint
  scripts of `ci.yml` (among them `check-size.py`, `check-history.py` and
  `codegraph.py dups`), `plant-each.sh --apply`, shellcheck of the
  installer, the static probe for `x86_64-unknown-linux-musl`, and the
  hint of `codegraph.py new`.
* Repeated work: `scripts/codegraph.py dups` fails on a new clone of a
  function body, a new twin of an entry, a paragraph in two documents and
  a row of section 4 whose pod dependencies differ from its `Cargo.toml`.
  `scripts/dups-baseline.txt` holds each known finding with the entry that
  removes it; a key that no longer occurs fails the check too.
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
| `ci.yml` | each push | fmt, clippy, tests (Linux x86_64; the netns tests of `pod-hold` also as root, `scripts/root-netns-tests.sh`; the tests of `pod-extract` and `pod-rootfs` with a worker of uid 1000 as root, `scripts/ci/root-uid1000-tests.sh`; `pod-wormhole` against Eclipse Mosquitto, `scripts/ci/mosquitto-tests.sh`; `pod-wormhole` against webtorrent/bittorrent-tracker, `scripts/ci/tracker-tests.sh`; `pod-wormhole` against ntfy, `scripts/ci/ntfy-tests.sh`; `pod-piping` against nwtgck's Piping Servers, `scripts/ci/piping-tests.sh`; `pod-dns` against CoreDNS, `scripts/ci/coredns-tests.sh`; `pod-link` against SeaweedFS, `scripts/ci/s3-tests.sh`; `pod-turn` and `pod-proxy` against coturn, `scripts/ci/turn-tests.sh`), `cargo todo check`, size, the static probes on each target, `cargo doc`, `scripts/check-probe.py`, `scripts/check-workflows.py`, `scripts/check-wire-vectors.py`; the installer core in each shell and in the labs of `pod-lab` (`scripts/install/test.sh --strict`) |
| `nightly.yml` | each night, before a release | tests on macOS, Windows and the other Linux targets (QEMU); the installer core on macOS and Windows (the job `install-os`, `scripts/install/test.sh --strict`) |
| `live.yml` | each day (01:53 UTC), by hand | the ignored live tests: `live_handshake` of `pod-tls`, `mptcp` of `pod-proxy`, `live` of `pod-oci`, `modloop_live` of `pod-guest` (the modloop of Alpine 3.22); the tests of `pod-wormhole` and `pair_code` of `pod-relay` against magic-wormhole's own mailbox server; on Thursdays and by hand, `live_doh` of `pod-dns` (with `scripts/names-proxy.py`), `live` of `pod-wormhole` and `pair_code_live` of `pod-relay` (the public mailboxes, ntfy servers, Nostr relays, WebTorrent trackers and MQTT brokers); `piping_live` of `pod-piping` (a session through ppng.io; the rates through each public Piping Server by hand); `opentunnel_live` of `pod-opentunnel` (a tunnel of opentunnel.xyz); `live` of `pod-natprobe` (a fast probe of the NAT of the runner against the STUN servers of `pod-services`, on Thursdays and by hand); by hand (the job `relay`, its input `relay`), one live test of the relay client of `pod-relay` |
| `services.yml` | each day (02:17 UTC), by hand (a service, a kind, or a measurement; with neither, no `forward-url`: call C89) | `pod-services-watch`: each `[[watch]]` of the data of `pod-services`; a fact that moved fails the run with the file, the value, the new value and the source. The measurements of `scripts/services/` (`edge-facts.sh`) |
| `deny.yml`, `secrets.yml` | each push, each day | licences, advisories, sources; secret scan |
| `plants.yml` | each push that changes a check, by hand | `scripts/plant-each.sh`, in six shards (`scripts/plant-shard.py`) |
| `adoption.yml` | each push that changes a crate with a trial, by hand | section 8, step 7 |
| `release.yml` | when a crate is done, by hand; then on the tag | section 10; a dry run with no input; the assets of `podlab` (static musl for x86_64 and aarch64, `podlab-install.sh` with its pins, the offline bundle, the release index `release-index.json` of `pod-release`, `SHA256SUMS`; `docs/install.md`) and of `pod-fake-github` and `pod-fake-githome` of `pod-testkit` and `pod-release` of `pod-release` (static musl for x86_64 and aarch64, in `SHA256SUMS`), also in the dry run, and `pod-release sums --all dist` over them; the job `image` builds an image of `podlab`, `pod-fake-github`, `pod-fake-githome` and `pod-release` for `linux/amd64` and `linux/arm64` from these files (`images/release/Dockerfile`), and compares each binary with its asset, also in the dry run; with a version it makes the tag and starts itself on the tag, which signs `SHA256SUMS` (cosign, keyless), checks it with `pod-release verify --all` (a changed file and another tag give 77) and makes the release; on the tag, the job `image` pushes one index for each binary to `ghcr.io/podutils/NAME:TAG`, compares each pulled platform with its asset and attests the provenance of the index in the registry, and the job `notes` adds the digests to the body of the release (call C789) |
| `upstream.yml` | each day | `check-sources.py`; `check-zig-mirrors.py`; the job `cloudflared`: `crates/pod-cloudflare/tools/check-upstream.py` (the pin of cloudflared, with a planted sum) and `crates/pod-cloudflare/tools/check-header-oracle.sh` (the header encoding against cloudflared's Go code) |
| `tor.yml` | each push of `crates/pod-tor`, each Tuesday, by hand | the road through Tor of `pod-tor` and an onion service against a private Tor network of C tor (chutney, `scripts/tor-net.sh`) on the runner |
| `record-issues.yml` | each day | `TODO/github.txt` against GitHub |
| `issue-sync.yml` | each push to `main`, each day, by hand (the input `dry_run`); each project calls it at a tag | `scripts/issue-sync.py`: the dry run, then the status block of each issue, its close and its open from the record |
| `fuzz.yml` | each week | the fuzz targets |
| `guest.yml` | each week, by hand | the tests of `pod-guest` with a real QEMU (TCG; KVM where `/dev/kvm` opens), one guest at a time, and a check that no emulator stays |
| `opentunnel-interop.yml` | each push that changes `pod-opentunnel`, by hand | the OpenTunnel carrier of `pod-opentunnel` against OpenTunnel's own server at v0.6.1, built from its source, with MySQL 8 and its local CA, through squid; then a planted CSR that must fail |
| `forwarder-live.yml` | each day (04:17 UTC), by hand | the built-in forwarders of `pod-egress`, live: GitHub's version line through each, one at a time (podssh's rulings Q56 and Q65), then a planted host that must fail; first the Wisp client against wisp-server-python on the loopback. podssh's `via` and `wstunnel` interop drive its binary and stay in podssh |
| `vmess-interop.yml` | each day (05:23 UTC), each push that changes the VMess road of `pod-egress`, by hand | the VMess road of `pod-egress` against Xray-core of the pinned release (its SHA-256 checked) on the loopback, over a WebSocket and TLS: each cipher, a pipe, a clock 5 minutes off, another ID, another path. podssh's `vless`, `trojan` and `outline` interop and `proxy-nodes-live` drive its binary and stay in podssh |
| `guest-windows.yml` | by hand | the keep-alive mode of `pod-guest` on a real Windows Server 2019 guest under KVM: an unattended install from the evaluation ISO of `microsoft-windows-eval`, six cold boots of `run` with the evidence of each one that times out (T-640), two next commands on new mailbox media through QMP, two controls (T-602) |
| the other live and interop workflows of the moved code | each day, by hand | podssh's `relay-live`, `relay-layer-live`, `warp-live`, `quick-tunnel-*`, `transit-live`, `pair-code`, `tor-live`, `ts-headscale`, `acme-interop`, `reverse-drops`; podnet's live steps of WARP and of the edge. Each moves with its crate |


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

