# The plan of podcrates

Status: draft for review. The owner reads it, rules the questions of
section 12, and gives the go-ahead. After the go-ahead, step M0 writes
this plan into the record `TODO/`, and `docs/design.md` keeps the rules
of sections 3 to 7. Then this file is removed.

## 1. Goal

`PodUtils/podcrates` holds the Rust crates that the PodUtils projects
share. The consumers are:

| Consumer | Repository | Takes from podcrates |
| --- | --- | --- |
| podssh | `PodUtils/podssh` (now `Azathothas/podssh`) | each library crate; it keeps its CLI, its SSH client (`podssh-ssh`, on its patched russh and ssh-key), its IRC and `podssh-probe` |
| podnet | `PodUtils/podnet` | TLS, DNS, proxy, WebSocket, relay, the layer, WARP, Cloudflare, transfer; it keeps `podnet`, `podnet-core`, `podnet-netns`, `podnet-vnet`, `podnet-serve` |
| podvm | (formerly podbox) | `pod-relay` through its blocking facade (no async runtime), `pod-term` |

The goal is reached when:

1. Each crate of section 5 exists, passes its checks, links static on
   each target, and has a released version (a tag, or crates.io: Q1).
2. Each crate has the tests of its source code, and they pass.
3. No feature or capability of the source code is lost. A crate with two
   sources has a parity list of each capability of each source, and each
   item is ticked.
4. Each crate passed an adoption trial for each consumer of its first
   column (section 8, step 6), and its README has the adoption map.

podcrates does not edit podssh, podnet or podvm. Each consumer replaces
its own modules in its own repository, in its own session. podssh and
podnet do not edit podcrates.

## 2. Inputs

* The design agreed by the podssh and podnet sessions on 2026-11-05 (the
  reply of the podssh session; podssh `TODO/PROGRESS.md` Q101 and
  `TODO/repo.md` T-684 at `1915a9f`).
* podnet at `b7682c5`: `docs/decisions.md` ("Shared crates", "podcrates
  design", "Order", "Crypto crates", "File size"), `TODO/crates.md`
  (T-049, T-050, T-052, T-137), `docs/research/podnet/podssh-copy.md`,
  `docs/research/podnet/architecture.md` (section 4).
* podssh at `1915a9f`: `AGENTS.md`, `docs/decisions.md` (2026-11-05: the
  move, podvm, "No compatibility with the released binaries, for now").

## 3. Rules for each crate (the library contract)

The owner's ruling of 2026-11-05 for podcrates: "we use whatever is
necessary, including c, zig, modern rust, no backwards compat. our users
& clients do not care about building the tool, they care about
downloading & using it. the only rule is: everything must be statically
compiled". Thus the only hard rule is rule 1. Rules 2 to 11 are the
design of the crates. `pod-todo` and `pod-testkit` are tools: rules 3, 4
and 7 do not apply to them.

1. **Static.** Each crate links into a static binary on each target of
   section 7: no `NEEDED` entry and no program interpreter on Linux
   (musl, cargo-zigbuild with zig cc for the cross targets); the C
   runtime linked in on Windows (`+crt-static`; only the DLLs of the
   system); only `libSystem` on macOS (Apple does not allow more). A C
   dependency is allowed when it links static on each target. CI builds
   the `examples/` and the test binaries of each crate for each target
   and checks them (`scripts/check-static.sh`); a plant proves that the
   check fails.
2. **Modern Rust, no backwards compatibility.** Edition 2024. The
   `rust-version` is the current stable Rust (1.97 on 2026-11-05); a
   session can raise it to a newer stable at any time. A public API, a
   name or a format of the wire can change in any version.
3. **The binary chooses the crypto provider.** A crate that makes a TLS
   or QUIC configuration takes an `Arc<rustls::crypto::CryptoProvider>`
   from its caller, so that one binary has one provider. The tests use
   ring. `pod-tls` keeps podssh's pure-Rust provider (feature `pure`).
4. **The caller gives the way out.** A crate that opens a connection
   takes an `Arc<dyn Dial>` (section 6). It does not read `HTTPS_PROXY`,
   `ALL_PROXY` or `NO_PROXY` by itself; `pod-proxy` has a function that
   reads them when the caller asks.
5. **No environment, no files of a consumer.** A crate does not read an
   environment variable unless the caller asks for it, and does not
   choose a state directory. The consumer passes settings and paths. A
   variable such as `PODSSH_REPLAY_BUFFER` becomes a field of a settings
   type, and the consumer keeps the variable.
6. **No output.** A crate does not print to stdout or stderr and does
   not exit the process. Diagnostics go to `tracing` (C3). A message for
   the user is a typed event or a returned value.
7. **Names of the wire.** An application identifier (a magic-wormhole
   AppID, a user agent, a client name) is a parameter. A transport
   constant can have a default.
8. **Each target of each consumer.** The crate builds and links static
   for each target of section 7. Code for one OS is behind `cfg`, and the
   crate still builds on the other targets.
9. **The source of the crates.** As Q1 rules: a git dependency on a tag
   of podcrates, or crates.io.
10. **Tests come with the code.** The tests of the source move with it
    and keep their names. A test uses loopback only and starts with the
    `need_loopback!` guard of `pod-testkit` when it binds. A test of a
    live service is `#[ignore]` and runs in `live.yml`. Each check and
    each test that guards a fix has a planted defect (section 9).
11. **Size.** A source file has at most N lines (Q2: 500 or 800). A file
    that is longer is split at import. There is no allowlist.

## 4. Repository

| Path | What |
| --- | --- |
| `Cargo.toml` | virtual workspace; `[workspace.package]` (edition, license 0BSD, repository); `[workspace.dependencies]` |
| `crates/pod-*` | one directory for each crate: `Cargo.toml`, `README.md`, `LICENSE`, `SOURCES`, `src/`, `tests/` |
| `TODO/` | the record (the podssh model), checked by `pod-todo` |
| `docs/` | `design.md`, `decisions.md`, `development.md`, `maintenance.md` |
| `scripts/` | `agent/` (check, watch-issues, watch-runs), `check-size.py`, `check-static.sh`, `check-sources.py`, `check-github-numbers.py`, `check-workflows.py`, `plant.sh`, `plant-each.sh`, `plants/`, `release.sh` |
| `fuzz/` | the fuzz targets of the moved code (its own workspace) |
| `examples/` | small binaries for the live checks (ring provider; `publish = false`) |
| `.github/` | `workflows/` (section 10), `dependabot.yml`, `secrets-allow.txt` |
| `AGENTS.md`, `CLAUDE.md` | the rules for each session (`CLAUDE.md` imports `AGENTS.md`) |
| `.claude/` | `settings.json`, `session-start.sh` (identity, branch `main`) |

Toolchain:

* Edition 2024, `rust-version = "1.97"` in `[workspace.package]` (the
  stable Rust of 2026-11-05), the same for each crate. The moved code
  goes from edition 2021 to 2024 at step 2 of section 8 (`cargo fix
  --edition`, then a review).
* cargo-zigbuild and zig for the static cross builds; the version of zig
  is pinned, from the mirrors of podssh `scripts/fetch-zig.sh`.
* smoltcp 0.14, in `pod-netstack` only. The other crates use its types
  through `pod-netstack`.
* `rustfmt.toml`: `max_width = 120`, `use_small_heuristics = "Max"`,
  `style_edition = "2024"`.
* `.cargo/config.toml`: `[alias] todo = "run -q -p pod-todo --"`; `[env]
  SMOLTCP_IFACE_MAX_ADDR_COUNT = "8"` (podnet's stack has more than two
  addresses).
* `deny.toml`: the targets of section 7; `multiple-versions = "warn"`;
  git sources as Q1 rules (only the fork sources of `vendor/` and
  podcrates itself); one ignored advisory, RUSTSEC-2023-0071 (`rsa` 0.9,
  feature `rsa` of `pod-tls`: verify only; the advisory is a timing
  attack on operations with the private key). cargo-deny runs with all
  features.
* `vendor/`: the patched forks that a crate needs (tailscale-rs for
  `pod-ts`, iroh-relay for `pod-iroh`), each with its patches and its
  upstream commit, as podssh `vendor/README.md` does.

## 5. The crates

"Lines" are of `.rs` files with comments and tests, from the agreed
design. "Base" is the source whose code the crate starts from; the other
source's capabilities and tests come in at step 4 of section 8.

### M0: the foundation (wave 1, the tools)

| Crate | Source | Lines | Base | Consumers |
| --- | --- | --- | --- | --- |
| `pod-todo` | podssh `crates/podssh-todo`; podnet `crates/podnet-todo` (its adapted copy) as configuration | 3,600 | podssh | podssh, podnet, podcrates |
| `pod-testkit` | podnet `tests/test_host/mod.rs` (`need_loopback!`, `need_localhost!`); podssh `crates/podssh-cli/tests/ssh_harness/` (feature `ssh`, russh 0.65); later waves add the stand-ins (section 5, M2 to M4) | 2,200 | both | dev-dependency only |

### M1: wave 1

| Crate | Source | Lines | Base | Pod deps |
| --- | --- | --- | --- | --- |
| `pod-identity` | podssh-relay `identity/`; `Entropy`, `OsEntropy` of `session/`; the private-file helpers of `cache.rs`, `cache/` | 1,200 | podssh | none |
| `pod-term` | podssh `crates/podssh-terminal` (consumers: podssh, podvm) | 3,700 | podssh | none |
| `pod-proxy` | podssh-ws `dial.rs`, `proxy.rs`, `proxy_auth.rs`, `socks5.rs`, `mptcp.rs`, `race.rs`, `names.rs`, `text.rs`, `cut.rs`, `lowat.rs`, `write_limit.rs`, `stun.rs` (the UDP probe); podnet `src/egress.rs` (CONNECT with Basic and Digest, SOCKS5 and 5h, `unix:`, `+exec:`, `NO_PROXY`, RFC 8305), `src/proxy_auth.rs`, `src/race.rs`. The `Dial`, `Resolve` and `Datagrams` traits | 4,000 | merge: podnet's proxy chain, podssh's dial | none |
| `pod-tls` | podssh-ws `crypto/` (feature `pure`), `tls.rs`, `tls/`, `stores/`, `tls_server.rs`, `tls_server/`, `tls_wrap.rs`, `bundle.rs`; podssh-relay `tls_groups.rs`; podnet `src/trust.rs` (roots, pins), `src/tls_fragment.rs` | 5,300 | merge: podssh | none |
| `pod-dns` | podssh-ws `resolve.rs`, `resolve/`; podnet `src/resolve.rs` (pins, system, DoH by address and by name through the proxy) | 2,800 | merge: podssh | pod-proxy, pod-tls |
| `pod-ws` | podssh-ws `client.rs`, `frame.rs`, `handshake.rs`, `session.rs`, `url.rs`, `close.rs`, `error.rs`, `plain.rs`, `http.rs`, `http/`, `server.rs`, `clock.rs`, `wake.rs`; podnet `src/ws.rs`, `src/transport/websocket.rs` | 4,500 | merge: podssh | pod-tls, pod-proxy, pod-dns |

### M2: wave 2

| Crate | Source | Lines | Base | Pod deps |
| --- | --- | --- | --- | --- |
| `pod-session` | podssh-relay `session/` (with `handover/`), `udp/` | 7,800 | podssh | pod-identity |
| `pod-e2e` | podssh-relay `e2e/` (Noise XX on snow 0.10) | 3,400 | podssh | pod-identity, pod-session |
| `pod-netstack` | podnet `src/warp/ip.rs`, `src/vnet/device.rs`, `src/vnet/framing.rs`; the TCP of podssh-warp as the smoltcp 0.14 reference | 2,600 | podnet | none |
| `pod-wg` | podnet `src/warp/wg.rs` (on ring, as now) | 1,100 | podnet | pod-proxy (`Datagrams`) |
| `pod-relay` | podssh-relay `relay.rs`, `open.rs`, `pool.rs`, `pool/`, `failover.rs`, `failures.rs`, `gate.rs`, `rank*.rs`, `token.rs`, `health.rs`, `addresses.rs`, `answer.rs`, `keyed.rs`, `forward_layer/`, `pair.rs`, `pair/`, `grant/`, `reverse/`, `http_road/`, `quic_road/`, `h2_road.rs`, `cleartext.rs`, `blocking/` (feature `blocking`, podvm), the relay hosts of `cache/`; podssh-ws `h2_road/`, `cleartext/`; podnet `src/ws_relay.rs`, `src/relay_pool.rs`, `src/relay_reverse.rs`; the relay stand-in into `pod-testkit` | 18,000 | merge: podssh | pod-ws, pod-tls, pod-dns, pod-proxy, pod-identity, pod-session, pod-e2e |

### M3: wave 3

| Crate | Source | Lines | Base | Pod deps |
| --- | --- | --- | --- | --- |
| `pod-wormhole` | podssh-relay `code/` (mailbox client, SPAKE2 on spake2 0.4, SecretBox phases, the code words); the mailbox harness into `pod-testkit` | 1,800 | podssh | pod-ws, pod-identity, pod-session |
| `pod-transit` | podssh-relay `transit/`; the transit server into `pod-testkit` | 1,000 | podssh | pod-ws, pod-session, pod-proxy |
| `pod-warp` | podnet `src/warp/` (registration, MASQUE over HTTP/2 and HTTP/3, the WireGuard leg, TCP and UDP inside, rules, endpoint order and scan, proxy, forward, nc, TUN behind `cfg(target_os = "linux")`); podssh `crates/podssh-warp` retires (its endpoint order and API routes are checked against podnet's); its stand-in edge and API into `pod-testkit`. podnet keeps `up`, `export` and `ssh_config` | 13,500 | podnet | pod-netstack, pod-wg, pod-tls, pod-dns, pod-proxy, pod-relay |
| `pod-cloudflare` | podnet `src/cloudflare/`, `src/tunnel.rs`, `src/edge_h2.rs`, `src/edge_discovery.rs`, `src/quicktunnel.rs`, `src/config.rs`, `src/proxy.rs` (the dispatch of the edge's streams; a trait for podnet's services: C14), the HTTP/2 leg through the relay, the QUIC configuration of the quic+warp leg; podssh-relay `quick/` moves onto it; the pin of cloudflared and its watch | 14,000 | merge: podnet | pod-tls, pod-dns, pod-proxy, pod-relay, pod-warp |

### M4: wave 4

Each crate of M4 starts with a study entry: the API, the C, the fork,
the targets. A study can change its row.

| Crate | Source | Lines | Base | Pod deps |
| --- | --- | --- | --- | --- |
| `pod-derp` | podssh-relay `derp/`; the DERP stand-in | 3,500 | podssh | pod-ws, pod-identity, pod-session, pod-proxy |
| `pod-piping` | podssh-relay `piping/`; the Piping harness | 2,400 | podssh | pod-ws, pod-session, pod-proxy, pod-relay |
| `pod-opentunnel` | podssh-relay `opentunnel/`; the OpenTunnel stand-in | 2,600 | podssh | pod-ws, pod-tls, pod-session, pod-proxy |
| `pod-acme` | podssh-cli `acme/` (keys through the given provider) | 1,400 | podssh | pod-tls, pod-proxy |
| `pod-egress` | podssh-ws `via/`, `vless/`, `wstunnel/`, `wisp/`, `shadowsocks/`, `forwarder/`, `meek.rs`, `meek/`, `webtunnel.rs`, `pt.rs`, `eice/`, `iap/`, `ech/`, `cloudflare.rs` (the Access client). Each road is a `Dial` | 11,000 | podssh | pod-proxy, pod-tls, pod-dns, pod-ws |
| `pod-relay-serve` | podssh-relay `serve/` (with `serve/mailbox/`) | 13,200 | podssh | pod-relay, pod-ws, pod-tls, pod-session, pod-e2e, pod-identity, pod-wormhole, pod-cloudflare |
| `pod-iroh` | podssh `crates/podssh-iroh`; podssh `vendor/iroh-relay` with its patch | 3,600 | podssh | pod-derp, pod-proxy, pod-tls |
| `pod-tor` | podssh `crates/podssh-tor` (arti, with its SQLite linked static) | 900 | podssh | pod-proxy |
| `pod-ts` | podssh `crates/podssh-ts`; podssh `vendor/tailscale-rs` with its patches (C12); the headscale harness | 900 | podssh | pod-proxy |

Names held, with no crate: `pod-wireguard`, `pod-quick`, `pod-udp`,
`pod-mailbox`, `pod-reverse`, `podcrates`, `podutils`, `pod-utils`.

## 6. The shared traits (in `pod-proxy`)

* `Dial`: a target (a name and a port, or an address) to a boxed
  `AsyncRead + AsyncWrite + Send + Unpin` stream; a label for the logs;
  `names_inside()` (the far end resolves the name).
* `Resolve`: a name to addresses. `pod-dns` implements it and takes a
  `Dial` for DoH, so the two crates do not depend on each other in a
  loop.
* `Datagrams`: poll-based send and receive of datagrams, in the shape of
  the `AsyncUdpSocket` of quinn and noq (`poll_send` of several transmits,
  `poll_recv` into several buffers, `local_addr`, one segment for each
  transmit when there is no GSO). Thus QUIC goes over WARP or through the
  relay. The system socket is one implementation; `pod-iroh` can still
  give iroh the system sockets.
* A probe that tells whether UDP leaves the host (STUN, podssh-ws
  `stun.rs`, in `pod-proxy`), so that a consumer uses UDP only after a probe.

The binary makes the chain (direct, a proxy, WARP, the relay, a road of
`pod-egress`). A crate makes a dialer of its own only when the caller
asks for the default (`Direct`).

## 7. Targets

The union of the release targets of the consumers:

| Targets | Consumer |
| --- | --- |
| `x86_64`, `aarch64`, `riscv64gc`, `i686`, `loongarch64`, `powerpc64le` `-unknown-linux-musl`; `armv7-unknown-linux-musleabihf`; `arm-unknown-linux-musleabihf` | podnet (podssh: four of them) |
| `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` | podssh |
| `x86_64-apple-darwin`, `aarch64-apple-darwin` | podssh |
| `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc` | podssh |
| the same as podssh (C16) | podvm |

CI builds the test binaries and the `examples/` of each crate for each
target (cargo-zigbuild for the Linux targets), checks that each one is
static (`scripts/check-static.sh`), and runs the tests on Linux x86_64,
macOS and Windows runners on each push (C7). The tests of the other Linux
targets run under QEMU user mode, each day (as podnet
`scripts/binfmt.sh`).

## 8. The procedure for one crate

Each crate is one record entry, or more for a large merge. Its commits:

1. **Import.** Copy the source files with no change. The commit message
   names each source: `pod-session: import podssh crates/podssh-relay/src/session at 1915a9f`.
   Write `SOURCES` (section 9). The import commit is pushed only
   together with step 2, so that `main` stays green.
2. **Adapt.** Make the crate build under the rules of section 3: the
   module paths, the dialer and the provider, settings in place of
   variables, `tracing` in place of prints, the size limit (split the
   files), rustfmt. Remove dated history from the comments, and keep each
   technical fact. Write in ASD-STE100.
3. **Tests.** Move the tests and the plants of the source; each test
   keeps its name. A test that fails shows a defect of the adaptation:
   repair the code, never the test.
4. **Merge** (a crate with two sources). Add each capability of the
   other source that the base does not have, with the tests of that
   source. Write the parity list in the crate README; tick each item.
5. **Document.** The crate README: purpose, API, features, the provider
   and the dialer, the parity list, the adoption map (each old path of
   each consumer to its new path).
6. **Adoption trial.** In a scratch copy of each consumer (never pushed),
   replace the module with the crate through a path override, and run
   the consumer's tests of that module (podssh: also the integration
   tests of `crates/podssh-cli/tests` that use it). Write the commit of
   the consumer and the counts in the entry. A failure is a defect of the
   crate or of its API: repair it here.
7. **Check and close.** `sh scripts/agent/check.sh`; the CI of the push
   is green (each target, static, deny). Write the Done
   section (date, commits, test counts, CI run). Release the crate
   (section 11).

Order in a wave: the dependency order of section 5. Agents that work in
parallel use local worktrees under `.claude/worktrees/`, one crate each.
At most one build runs at a time (`CARGO_BUILD_JOBS=4`). The main
session merges, builds, tests and pushes. No worktree branch is pushed.

## 9. Checks, plants and drift

* `scripts/agent/check.sh`: `cargo fmt --all --check`, `cargo clippy
  --workspace --all-targets --all-features -- -D warnings`, `cargo test
  --workspace --all-features`, `cargo todo check`,
  `python3 -I scripts/check-size.py`; and on a push, the static builds
  of `ci.yml`.
* Plants (the podssh model): `scripts/plants/NAME.plant` names a check
  and its expected failure, `NAME.patch` is the defect.
  `scripts/plant-each.sh` applies each patch to a scratch copy and fails
  when its check passes. A new check or a test that guards a fix is
  trusted only after its plant fails it.
* Drift: podssh changes each day; podnet holds (podnet, ruling "Order").
  `crates/pod-X/SOURCES` lists each imported file: repository, path,
  commit, SHA-256. `scripts/check-sources.py` compares each source with
  the `main` of its repository (read through `forge.web.pkgforge.dev`)
  and prints each changed file with the commits that changed it.
  `upstream.yml` runs it each day; a change becomes a record entry "port
  podssh change to pod-X". When the consumer has taken the crate, its
  lines leave `SOURCES`.

## 10. Workflows

| Workflow | When | What |
| --- | --- | --- |
| `ci.yml` | each push | fmt, clippy, tests (Linux, macOS, Windows), `cargo todo check`, size, the static build of each target (`scripts/check-static.sh`), `cargo doc`, `cargo package --workspace` |
| `deny.yml`, `secrets.yml` | each push | licences, advisories, sources; secret scan |
| `plants.yml` | each push to `main` that changes a check, and by hand | `scripts/plant-each.sh` |
| `release.yml` | by hand, and when a crate is done | the release (section 11); a dry run with no input |
| `upstream.yml` | each day | `check-sources.py`; the cloudflared pin (with `pod-cloudflare`) |
| `record-issues.yml` | each day | `TODO/github.txt` against GitHub |
| `fuzz.yml` | each week | the fuzz targets |
| `live.yml` | each day, by hand | the `examples/` against WARP, the edge, the relay, the mailbox (from M2) |
| interop jobs | each push that changes the crate | magic-wormhole mailbox and transit relay, `derper`, Pebble, headscale, the servers of `pod-egress` (from M3) |

## 11. Versions and releases

The form depends on Q1.

* **A git tag (the recommendation of Q1).** One version for the
  workspace, from `0.1.0`; the tag `vX.Y.Z` releases each crate. A
  consumer writes `pod-tls = { git = "https://github.com/PodUtils/podcrates", tag = "v0.3.0" }`;
  its `Cargo.lock` holds the commit. A crate in `vendor/` (a fork) is a
  path dependency inside the repository, and cargo follows it through
  the git dependency. `release.yml` makes the tag when a crate is done
  and `main` is green, and writes `docs/releases/vX.Y.Z.md` (the crates
  that changed, each breaking change).
* **crates.io.** Each crate has its own version; `scripts/publish.sh`
  publishes, in dependency order, each crate whose version is not on
  crates.io, and tags `pod-X-vVERSION`. A fork must be published as a
  crate of its own name first.
* In both forms: a change of a public API or of a wire format is a new
  minor version (0.x); a release is unattended when its checks are green.

## 12. Questions for the owner

Each question has a recommendation. Q1 and Q2 block M0.

- **Q1. How do the consumers get the crates: a git tag of podcrates, or
  crates.io?** The rulings of 2026-11-05 in podnet and podssh said
  "crates.io only" and reserved names; the new ruling says "the only
  rule is: everything must be statically compiled". Recommendation: a
  git dependency on a tag of podcrates now (section 11), crates.io later
  if wanted. Reasons: (a) `pod-ts` needs the patched tailscale-rs fork
  (40 crates, 29 patches) and `pod-iroh` the patched iroh-relay; a crate
  on crates.io cannot carry a fork or a `[patch]`, a git dependency can;
  (b) no token, no publish order, no rate limit for new crates; (c) no
  question of name squatting (the crates.io usage policy forbids it, and
  26 crates plus 8 held names with empty 0.0.0 versions are at risk).
  Cost: each consumer adds `https://github.com/PodUtils/podcrates` to
  `allow-git` of its `deny.toml`, and cargo fetches the whole repository.
  If the owner rules crates.io: a crates.io API token with the scope
  `pod-*` and the rights `publish-new` and `publish-update`, as the
  secret `CARGO_REGISTRY_TOKEN` of a GitHub environment `crates-io`, for
  the first version of each crate; then trusted publishing, and the token
  is removed; `pod-ts` and `pod-iroh` wait for published forks.
- **Q2. File size.** podssh's rule is 500 lines (for agents that read
  the code); podnet's is 800. Recommendation: 500, as both sessions
  recommend. podssh's code fits; podnet's longer files split when they
  move (`src/warp/ip.rs`, `src/egress.rs`, `src/proxy.rs`,
  `src/edge_h2.rs`, `src/tunnel.rs`, `src/warp/wg.rs` and more).

## 13. Calls made without a ruling

Each call goes into `docs/decisions.md`, and the work goes on with it
until the owner rules otherwise:

- **C1.** Edition 2024; `rust-version` = the current stable Rust (1.97).
- **C2.** rustfmt of podssh (`max_width = 120`, `Max`).
- **C3.** `tracing` for the diagnostics of each crate (podnet uses it;
  podssh adds it with the first crate that it takes).
- **C4.** The version scheme follows Q1 (section 11).
- **C5.** `pod-todo` reads the facts of a repository (milestones, areas,
  categories, the top directories, the documents, the first GitHub number)
  from `TODO/todo.toml`. A consumer keeps a small member crate (for
  example `podnet-todo`) whose `main` calls `pod_todo::main()`, so that
  `cargo todo` and the lock stay the same. podssh's `interop` command
  stays in podssh's member crate, through an extension hook of `pod-todo`.
- **C6.** The record of podcrates starts at T-001. Milestones: M0
  (foundation), M1 to M4 (the waves), M5 (maintenance: ports of source
  changes, the reports of the consumers).
- **C7.** Tests on macOS and Windows runners on each push (podssh ships
  there).
- **C8.** `pod-wg` keeps podnet's code on ring.
- **C9.** `pod-testkit` has russh 0.65 from crates.io behind its feature
  `ssh`, a dev-dependency only.
- **C10.** QUIC (quinn 0.11 in podnet, noq 1.4 in podssh): one stack, by
  measurement, in the first entry of M3.
- **C11.** A C dependency is allowed in each crate when it links static
  on each target of section 7 (cargo-zigbuild for Linux). The static
  build of CI proves it for each crate and each target.
- **C12.** The forks that a crate needs go into `vendor/` with their
  patches, their upstream commit and a weekly check of upstream (podssh
  `scripts/check-vendored.py`): tailscale-rs for `pod-ts`, iroh-relay for
  `pod-iroh`. The study entry of `pod-ts` compares the fork with
  `tailscale` 0.6.1 of crates.io.
- **C13.** snow 0.10, spake2 0.4 and crypto_secretbox 0.1 stay (podnet,
  ruling "Crypto crates" of 2026-11-05).
- **C14.** podnet `src/proxy.rs` and `src/config.rs` go into
  `pod-cloudflare` (podnet's architecture study, section 4.1), not into
  `pod-egress`.
- **C16.** podvm: `pod-relay` (feature `blocking`) and `pod-term` build
  for each target of section 7. podssh's record does not give podvm's
  targets; podvm is a container runtime, so Linux is certain.
- **C15.** An adoption trial for each crate and each consumer (section
  8, step 6), in a scratch copy that is never pushed.

## 14. Owner actions

| When | Action |
| --- | --- |
| Before M0 | Q1 and Q2 |
| If Q1 is crates.io, before the first publish | the token of Q1 |
| After each released crate | Tell the consumer sessions to take it (podssh T-684, podnet T-137) |

## 15. Order of M0 (after the go-ahead)

1. The core documents: `AGENTS.md`, `CLAUDE.md`, `.claude/`, `README.md`,
   `LICENSE`, `SECURITY.md`, `docs/decisions.md` (written before the
   go-ahead).
2. The workspace: `Cargo.toml`, `rustfmt.toml`, `.cargo/config.toml`,
   `deny.toml`, `.gitignore`.
3. `pod-todo`: import, adapt (C5), tests. `TODO/RULES.md`,
   `TODO/todo.toml`, and the record with an entry for each step of this
   plan.
4. The checks: `scripts/agent/check.sh`, `check-size.py`,
   `check-static.sh`, `check-sources.py`, the plant
   scripts, each check with its plant.
5. The workflows: `ci.yml`, `deny.yml`, `secrets.yml`, `plants.yml`,
   `record-issues.yml`, `upstream.yml`, `release.yml` (dry run).
6. `pod-testkit` (the guards; the feature `ssh`).
7. The first release of `pod-todo` and `pod-testkit` (Q1).
8. Then M1.
