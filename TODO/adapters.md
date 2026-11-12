This file holds the entries of milestone M3: the transit carrier, the WARP
adapter and the Cloudflare adapter. Each crate follows the procedure of
`docs/design.md`, section 8.

# T-027: `pod-transit`: the transit relay carrier

**Source:** `docs/design.md`, section 4 (M3).
**Category:** feature
**Milestone:** M3
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

A wormhole needs a direct or relayed stream after its rendezvous.

## Premise

- Read: `PodUtils/podssh:crates/podssh-relay/src/transit/` (1,013,
  without `link.rs`, which goes to `pod-session`); tests `transit` and
  `transit_server/`.

## Approach

The procedure of `docs/design.md`, section 8, with the transit server of
the tests.

## Prove

```sh
cargo test -p pod-transit
sh scripts/agent/check.sh
```

## Start condition

T-020 and T-022 are done.

# T-028: `pod-warp`: the WARP adapter

**Source:** `docs/design.md`, section 4 (M3); podnet's ruling "Shared
crates" of 2026-11-05.
**Category:** feature
**Milestone:** M3
**Priority:** P1
**Effort:** L
**Status:** partial

## Problem

podnet's WARP adapter (registration, MASQUE over HTTP/2 and HTTP/3, the
WireGuard leg, TCP and UDP inside, rules, endpoint order and scan, proxy,
forward, nc, TUN) moves to podcrates. podssh's WARP road retires onto it.

## Premise

- Read: podnet `PodUtils/podnet:src/warp/` (14,501 lines, 21 files);
  without `ip.rs`, `wg.rs`, `quic.rs` (M2), `up.rs`, `export.rs`,
  `ssh_config.rs` and the backends of `warp run` (they stay in podnet):
  about 10,000 lines.
- `src/warp/` uses podnet's `exit` (23 times), `decisions` (15), `fetch`
  (14), `state`, `instances` and `hop`: the failures, the observer and
  the HTTP client of `pod-core` and `pod-proxy` take their place.
- Read: podssh `PodUtils/podssh:crates/podssh-warp` (5,469; its stand-in
  edge and API stand-in go to `pod-testkit`).
- podnet holds `src/warp/` to fixes from `7c5f87e`, but `wg.rs` and `wg/`
  (taken as `pod-wg`) and `quic.rs`, `up.rs`, `export.rs` and
  `ssh_config.rs` (they stay in podnet) (PodUtils/podnet#449). podnet
  accepts the cut of `docs/design.md`.

## Approach

The procedure of `docs/design.md`, section 8; the stand-ins give
loopback tests of the registration, the proxy, the forward and nc; the
live checks move to `live.yml`.

The identity of another uid (PodUtils/podnet#413, ask 9;
PodUtils/podnet@T-173): podnet refuses a WARP identity that another uid
owns, with exit 78. The identity loader of the crate reads such an identity
only with the consent of the caller (podnet's variable
`PODNET_FOREIGN_IDENTITY=allow`, given as a setting: the crate reads no
variable) and only at a path that the caller names; it keeps the check of
the mode and the owner. A test for each case.

A pinned front door (PodUtils/podnet#413, ask 10;
PodUtils/podnet@T-169): podnet tries the IPv6 front door first on a host
with no IPv4 default route. The door selection of the crate takes a pinned
`HOST:PORT` from the caller (podnet's `--masque-endpoint` and its variable)
and skips the order, so that an IPv6-only host pays no failed IPv4 connect.
A test of the pinned door and of the order without it.

A WARP session as a network of the caller (PodUtils/podnet#449;
PodUtils/podnet@T-057): the crate gives a session as a `Dial` (TCP streams
and names inside) and as `Datagrams` (UDP), so that podnet's registry,
router, groups and `--egress` take WARP as one more network. About twenty
podnet entries wait for it (among them PodUtils/podnet@T-095,
PodUtils/podnet@T-097 and PodUtils/podnet@T-101). The `Dial` and the
`Datagrams` of `pod-core` are enough: podnet implements its own
`podnet_core::network` traits over the types of the crate
(PodUtils/podnet#452).

The lint of shadowed rules (PodUtils/podnet#449, PodUtils/podnet#433;
PodUtils/podnet@T-242): `warp proxy -r` and `--rules` call the same lint
as podnet's netns rules (`podnet_netns::rules::shadow::check`): a shadowed
`reject` is an error (exit 64) unless the caller allows it. The crate
cannot depend on a podnet crate: the rules of `src/warp/rules.rs` take the
lint as a function of the caller, and podnet passes its
`shadow::check` (PodUtils/podnet#452: the lint stays in podnet).

## Prove

```sh
cargo test -p pod-warp --all-features
sh scripts/agent/check.sh
```

## Start condition

T-019, T-023, T-024 and T-026 are done.

## Done

2026-11-12, partial: the first part. Commits `601571e` (the import of
PodUtils/podnet:src/warp at `e046560` and
PodUtils/podssh:crates/podssh-warp at `562751d`), `39cbdde` (the edition), `133f97a` (the adaptation),
`1f184f3` (the static probe and the fuzz target), `c732690` (the plants);
calls C135 to C142. The rest is T-335 (podnet's stack of `ip.rs`), T-336
(the relay leg), T-337 (HTTP/3, WireGuard, the carry), T-338 (the proxy,
the forward, nc, the L4 door) and T-339 (`Live`, TUN, routes, the live
step of `live.yml`, the adoption trials and the hand-off of the freeze).

- `crates/pod-warp`: the registration through the caller's
  `pod_proxy::Client` (each base in turn, a `POST` final), the account
  calls, the device hints; podssh's pure-Rust P-256 identity in podnet's
  file form; the identity file with its lock, its record `created`, the
  check of the owner and the mode, and the consent of the caller for a
  file of another uid (ask 9); the doors in order on the caller's routes,
  or a pinned `HOST:PORT` (ask 10); MASQUE over HTTP/2 through the
  caller's `Dial` and provider, with the key of the front door pinned and
  the record read again on a new key; `WarpNet`, the stack of
  `pod-netstack` over a session as a `Dial` (names inside the tunnel) and
  as `Datagrams`; the rules of the proxy with the lint as a function of
  the caller. The README has the API, the settings, the parity list with
  podnet and podssh, and the adoption map.
- `pod-testkit`: `stand::warp` (the stand-in API) and the feature `masque`
  (the HTTP/2 and capsule part of the stand-in edge).
- `cargo test -p pod-warp --all-features`: 57 unit tests, 6 of
  `tests/net.rs`, 8 of `tests/register.rs` (as root, with the foreign
  owner of a real `chown`), 1 live test ignored; `net` and `register`
  passed 5 runs in a loop. Each item of the Approach and its test is in
  the table "The tests" of `crates/pod-warp/README.md`.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo fmt --all --check`, `python3 -I scripts/check-size.py`,
  `scripts/check-probe.py`, `scripts/check-design-paths.py`: pass. The
  static probe for `x86_64-unknown-linux-musl` builds and
  `scripts/check-static.sh` passes; its line reads "pod-warp: an SPKI of
  124 characters, a packet of 6 bytes, 1 rules, a session unavailable".
- Plants: `warp-pin-unchecked`, `warp-foreign-owner-taken`,
  `warp-pinned-door-ordered`, `warp-post-retried`; each failed its test
  here.
- The fuzz target `warp_capsule` with 4 seeds:
  `cargo test --manifest-path fuzz/Cargo.toml --lib`, 9 passed (each
  target on its seeds and their mutations).
- Not run here: the live test (the egress of this sandbox reaches no WARP
  endpoint; T-339 adds its step), the CI of the push, the adoption trials.
- Before the release: the main session reserves the name `pod-warp` on
  crates.io if it is not reserved.

# T-029: `pod-cloudflare`: the edge client, the tunnel and the quick tunnels (the base of podnet)

**Source:** `docs/design.md`, section 4 (M3); podnet's ruling "Shared
crates" of 2026-11-05.
**Category:** feature
**Milestone:** M3
**Priority:** P1
**Effort:** L
**Status:** open

## Problem

podnet's Cloudflare tunnel (QUIC, HTTP/2, Cap'n Proto RPC, the legs, the
race, ingress, the quick tunnels) is one adapter of podnet and moves to
podcrates.

## Premise

- Read: podnet `PodUtils/podnet:src/cloudflare/` (3,956),
  `src/tunnel.rs` (1,599), `src/race.rs` (257), `src/edge_h2.rs` (1,736),
  `src/edge_discovery.rs` (314), `src/quicktunnel.rs` (892),
  `src/config.rs` (1,207), `src/proxy.rs` (2,034), the edge side of
  `src/ws.rs`: about 11,700 lines.
- `src/proxy.rs` dispatches the streams of the edge to podnet's
  services: a `ServiceDispatch` trait.
- Read podnet at `a026974` or later (PodUtils/podnet#418): its layer cuts
  made `src/cloudflare/rpc.rs`, `src/config.rs` and `src/quicktunnel.rs`
  re-export items that moved to podnet's own modules (`hop::Refused`,
  `ingress::RULE_KEYS`, `report::Refusal`), and the origin dial of
  `src/proxy.rs` is `origin::dial::dial`.
- A test of `src/cloudflare/http2.rs` reads `tools/header-oracle.txt` of
  podnet; the pin of cloudflared and its watch move with the crate.
- podnet holds `src/cloudflare/`, `src/tunnel.rs`, `src/race.rs`,
  `src/edge_h2.rs`, `src/edge_discovery.rs`, `src/quicktunnel.rs`,
  `src/config.rs`, `src/proxy.rs` and the edge side of `src/ws.rs`
  (`serve_websocket`, `forward_path`) to fixes from `7c5f87e`
  (PodUtils/podnet#449). podnet accepts the cut of `docs/design.md`.
- A fix in the frozen files to port at the import (PodUtils/podnet#460,
  podnet `28da113`): a comment of a test in `mod tests` of
  `src/tunnel.rs`, near the assert of `limit(LegKind::QuicDirect)`, names
  `PodUtils/podnet:docs/tunnels.md` in place of `README.md`.
- podnet's `tunnel --inspect` (PodUtils/podnet#435,
  PodUtils/podnet@T-244) captures each exchange beside the copy helpers of
  `forward_http` in `src/proxy.rs`: a ring, redaction by default, a replay
  over a socket of mode 0600.
- PodUtils/podnet#482 (after podnet's take of `pod-relay`): podnet's
  `ws_relay::meter` takes the limits of the pool document; the edge leg of
  the tunnel (`PodUtils/podnet:src/edge_h2.rs`, frozen for this crate)
  still counts the constants. The import takes the limits of the relay
  (`RelayBudget::meter_for` of `pod-relay`) for the edge leg; podnet sends
  no fix of the frozen file for it.

## Approach

The procedure of `docs/design.md`, section 8, with podnet's files only.
The copy of `forward_http` gives the caller a hook on each exchange, so
that podnet's `tunnel --inspect` stays podnet's (or the crate takes the
capture, if podnet's code of it is in the cut at the import).

## Prove

```sh
cargo test -p pod-cloudflare --all-features
sh scripts/agent/check.sh
```

## Start condition

T-019, T-026 and T-028 are done.

# T-030: `pod-cloudflare`: podssh's quick tunnels onto the edge client

**Source:** `docs/design.md`, section 8, step 5.
**Category:** feature
**Milestone:** M3
**Priority:** P1
**Effort:** M
**Status:** open

## Problem

podssh makes quick tunnels for the WebSocket visitors of a node with its
own code over HTTP/2 and noq (`PodUtils/podssh:crates/podssh-relay/src/quick/`,
6,180 lines).

## Premise

- Read: `quick/` changed 36 times in the last 50 commits of podssh.
- Tests: 12 files `PodUtils/podssh:crates/podssh-relay/tests/quick_*.rs`
  and `http_road_quick`.

## Approach

Each capability of `quick/` that the base does not have, with its tests;
the parity list; the trials of podssh and podnet.

## Prove

```sh
cargo test -p pod-cloudflare --all-features
sh scripts/agent/check.sh
```

## Start condition

T-029 is done.

# T-113: `pod-cloudflare`: the connector of the fleet, part 1: remote config, readiness, drain, revoked token, Access JWT, colos (GitHub #35)

**Source:** GitHub #35 (A-N-02, A-N-03, A-N-06, A-C-01, A-C-02, A-C-07)
**Category:** feature
**Milestone:** M3
**Priority:** P1
**Effort:** L
**Status:** open

## Problem

The clauses of A-N-02, A-N-03, A-N-06, A-C-01, A-C-02 and A-C-07 are in the code that `pod-cloudflare`
holds. The owner, 2026-11-09: they wait until after the first release on crates.io.

## Premise

- Read: PodUtils/podnet `src/edge_h2.rs` acks a push and applies nothing; `src/metrics.rs` counts;
  `--metrics`, `--grace-period` and `--post-quantum` are refused in `src/cli/argv.rs`.
- Not measured: the clauses.

## Approach

Apply a remote config (HTTP/2 push, QUIC `UpdateConfiguration`) and refuse it with a local config. State
for `/ready` and `/metrics` (the listener is T-103). A drain at SIGTERM. A revoked token gives exit class
Revoked (T-101). Honour `originRequest.access` (a JWT against the team keys, else 403). Four connections in
two colos or more on TCP 443, and a lost one replaced.

## Prove

```sh
cargo test -p pod-cloudflare --all-features
```

## Start condition

T-029 is done, and the first release on crates.io is done (the owner, 2026-11-09).

# T-114: `pod-cloudflare`: the connector of the fleet, part 2: several tunnels, pools, a Workers runtime ingress, VPC allow list, rate and meter, post-quantum (GitHub #35)

**Source:** GitHub #35 (A-X-27, A-N-14, A-X-06, A-C-06, A-X-26, A-N-16)
**Category:** feature
**Milestone:** M3
**Priority:** P2
**Effort:** L
**Status:** open

## Problem

The clauses of A-X-27, A-N-14, A-X-06, A-C-06, A-X-26 and A-N-16 are in the code that `pod-cloudflare`
holds. The owner, 2026-11-09: they wait until after the first release on crates.io.

## Premise

- Read: PodUtils/podnet `src/cli/argv.rs` refuses `--post-quantum`. rustls gives X25519MLKEM768 through
  aws-lc-rs; aws-lc-sys on i686, arm, loongarch64 and powerpc64le musl is unproven (`docs/design.md`,
  section 7).
- Not measured: the clauses.

## Approach

Several tunnels in one process, each with its own token and metrics, and an upgrade with no downtime. A
pool service in the ingress (several origins, a health check, one retry). The Host header form, the path
rewrite and the Unix origins of a Workers runtime. A Workers VPC allow list of local targets. A rate limit
and a byte meter for each hostname. A hybrid post-quantum group on the edge legs, with a switch.

## Prove

```sh
cargo test -p pod-cloudflare --all-features
```

## Start condition

T-029 is done, and the first release on crates.io is done (the owner, 2026-11-09).

# T-115: `pod-warp`: the rate of `quic+warp`, team enrolment with a service token, a Mesh node in user space, posture (GitHub #35)

**Source:** GitHub #35 (A-N-10, A-N-08, A-C-03, A-C-05, A-C-11)
**Category:** feature
**Milestone:** M3
**Priority:** P2
**Effort:** L
**Status:** open

## Problem

The clauses of A-N-10, A-N-08, A-C-03, A-C-05 and A-C-11 are in the code that `pod-warp` holds. cfb
measured `quic+warp` at 37 % (GET) and 22 % (POST) of direct `quic`.

## Premise

- Read: the asks. PodUtils/podnet@T-003 is blocked: it waits for a captured exchange of the official
  client.
- Not measured: a Mesh node.

## Approach

One MASQUE session for each edge connection and a faster receive path (GET and POST at 50 % or more of
direct `quic`). Enrolment in a Zero Trust team with a service token. A Mesh node in user space with no TUN
and no route change. Device posture results. The live steps wait for test accounts (ruling of 2026-11-09);
each clause keeps its offline test.

## Prove

```sh
cargo test -p pod-warp --all-features
```

## Start condition

T-028 is done, and the captured exchange of PodUtils/podnet@T-003 exists.

# T-116: The live and bench workflows of the tunnel with a test account (GitHub #35)

**Source:** GitHub #35 (A-N-01, A-N-09, A-N-15, A-C-12, A-X-23)
**Category:** measurement
**Milestone:** M3
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

cfb accepts the tunnel only from live runs with a test account, and wants a weekly bench of each leg.

## Premise

- Read: the asks. The owner, 2026-11-09: no test account now.
- Not measured: a live run.

## Approach

The steps of A-N-01, A-N-09, A-C-01 and A-C-06 in `live.yml`. A weekly bench of each leg against
`cloudflared`, with `bench.json` holding leg, Mbit/s, CPU percent and date.

## Prove

```sh
gh workflow run live.yml
```

## Start condition

T-029 is done, and the owner gives test accounts as CI secrets (not now, 2026-11-09).

# T-359: `pod-warp`: the stand-ins of each push and one live run each week

**Source:** `docs/live-calls.md` ("The crates of the import and of the
plan"); call C150.
**Category:** feature
**Milestone:** M3
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

`pod-warp` (T-028) talks to WARP's API (api.cloudflareclient.com), to the
MASQUE door (consumer-masque.cloudflareclient.com:443) and to the
WireGuard door (UDP 2408). podnet runs its live WARP checks on each push
and each day (PodUtils/podnet:.github/workflows/live.yml). The tests of
the crate must not reach WARP on each push: a fault of WARP or of the
runner's network makes the gate red.

## Premise

- Read (2026-11-12): podnet's `live.yml` (the job `live` on each push,
  each day and by hand); podnet's `tests/warp_register.rs` (a fake API on
  the loopback) and `src/warp/wg/standin.rs` (the responder of
  `pod-wg`, feature `standin`); `crates/pod-wg/Cargo.toml` (the feature
  `standin`); T-028 (podssh's stand-in edge and API stand-in go to
  `pod-testkit`).
- Not measured: the kernel's WireGuard on a runner (`ip link add type
  wireguard` as root in a network namespace).

## Approach

1. Each push: the API stand-in (registration, the lock, the resume, the
   PATCH), the MASQUE stand-in and the responder of `pod-wg`, all on the
   loopback, in `pod-testkit`.
2. An independent peer: the kernel's WireGuard in a network namespace as
   root, in a step as `scripts/root-netns-tests.sh`, so the handshake of
   `pod-wg` meets code that this repository did not write.
3. `live.yml`, each week and by hand: one registration of a device of the
   run, one MASQUE connect, one GET through it, and the device removed at
   the end. The log masks the device id (as podnet's
   PodUtils/podnet:scripts/live-run.sh). No live call in `ci.yml` (call C150).

## Prove

```sh
cargo test -p pod-warp --all-features
sh scripts/root-netns-tests.sh
gh workflow run live.yml
```

## Start condition

T-028 is done.

# T-360: `pod-cloudflare`: a stand-in edge of each push and one quick tunnel each week

**Source:** `docs/live-calls.md` ("The crates of the import and of the
plan"); call C150.
**Category:** feature
**Milestone:** M3
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

`pod-cloudflare` (T-029) talks to the edge of Cloudflare's tunnels
(region1 and region2.v2.argotunnel.com:7844, QUIC and HTTP/2, Cap'n Proto
RPC) and to api.trycloudflare.com. No server of another party speaks the
edge's side: cloudflared is a client. The tests of the crate must prove
the RPC, the legs and the race on each push with no edge.

## Premise

- Read (2026-11-12): podnet's `live.yml` (a quick tunnel on QUIC and on
  HTTP/2 on each push and each day); `scripts/services/edge-facts.sh` (the
  facts of the edge, each day in `services.yml`);
  `crates/pod-services/services/cloudflare-tunnel-edge.toml`.
- Not measured: whether podnet's tests hold recorded RPC exchanges that a
  stand-in can replay.

## Approach

1. A stand-in edge in `pod-testkit` on the loopback: QUIC (through
   `pod-quic`) and HTTP/2 over TLS with a test root, the registration RPC
   answered from exchanges recorded from the real edge (the schemas of
   cloudflared, Apache-2.0), and the streams of a request to the origin.
2. Each push: the legs, the race, the ingress and the dispatch against
   the stand-in.
3. `live.yml`, each week and by hand: one quick tunnel on each leg, and a
   GET of its URL; the log masks the host. The edge facts stay in
   `services.yml` each day.

## Prove

```sh
cargo test -p pod-cloudflare --all-features
gh workflow run live.yml
```

## Start condition

T-029 is done.

# T-335: `pod-warp`: podnet's stack of `ip.rs` onto `pod-netstack` (congestion control, TCP burst, link counters, the check of the data path)

**Source:** T-028, the first part (`docs/decisions.md`, C138).
**Category:** feature
**Milestone:** M3
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

The `WarpNet` of `pod-warp` runs the default stack of `pod-netstack` over a
session. podnet's `src/warp/ip.rs` (1,680 lines) has more: the congestion
control of `--cc` (`parse_cc`, `set_cc`, `check_cc`), the TCP burst of
`PODNET_WARP_TCP_BURST`, the counters of the link (`link_stats`), the
budget of a relay that renews the session (`renew_due`), and the check
that a packet crossed the session (`proxy::check_data_path`, issue 120)
with the door that carried nothing marked bad (issue 133). podnet's
registry needs each of them before it drops its own stack.

## Premise

- Read: `PodUtils/podnet:src/warp/ip.rs` at `e046560`; `pod-netstack` sets
  `CongestionControl::None` on each socket (`crates/pod-netstack/src/stack/reactor.rs`).
- Read: `Warp::mark_door_bad` exists; nothing calls it after a failed check.
- Not measured: the speed of the stack of `pod-netstack` over a session.

## Approach

A setting of `pod-netstack` for the congestion control and the burst of a
stack (a field of `StackConfig`, with a test there); `WarpNet` settings for
them, the counters of the link, and the check of the data path (a DNS
question to 1.1.1.1 through the stack, with its time) that marks the door
bad on a failure. The tests of `ip.rs` that read these values move with
them.

## Prove

```sh
cargo test -p pod-netstack --all-features
cargo test -p pod-warp --all-features
```

## Done

2026-11-12. Commits `1e5d154` (the work) and `7796b38` (the plants);
calls C170 to C174.

- `pod-netstack`: `StackConfig::congestion_control`, `max_burst` and
  `link_stats` (C170). Fix in place: `LinkDevice::with_stats` after
  `LinkDevice::link` moves the count of the inbound drops to the new
  counters (the test failed on the code before: 0 of 6 drops).
- `pod-warp`: `NetSettings` (`Settings::net`; C171): the MTU, the TCP buffer
  of 512 KiB (`WarpNet` had 64 KiB), `congestion_control` (`parse_cc`),
  `tcp_burst` (`parse_tcp_burst`, 1 to 430), `link_stats`, `check`,
  `renew` (C173); `WarpNet::link_stats`, `renew_due`, `check_data_path`,
  `checked`. `Warp::connect` checks the data path and marks a door that
  carried nothing bad (C172). The test edge answers DNS on UDP port 53,
  notes the largest TCP window of the client and drops the questions that a
  test names. The README has the API, the settings, the tests of T-335, the
  parity rows and the adoption rows.
- Sources: `PodUtils/podnet:src/warp/ip.rs`,
  `PodUtils/podnet:src/warp/proxy.rs` (`check_data_path`, `query_at`) and
  `PodUtils/podnet:crates/podnet-core/src/services.rs` at `e046560`; the blobs are the same at podnet's `main` (`27f2b41`), so no
  fix to port. `SOURCES` of both crates names `ip.rs`; no import commit
  (C174).
- `cargo test -p pod-netstack --all-features`: 67 passed (6 new).
  `cargo test -p pod-warp --all-features`: 61 unit tests (4 new), 8 of
  `tests/stack.rs` (new), 6 of `tests/net.rs`, 8 of `tests/register.rs`, 1
  live test ignored; `tests/stack.rs` passed 8 runs in a loop.
- Plants: `netstack-shared-counters-lose-the-drops`,
  `netstack-controller-not-set`, `netstack-burst-not-set`,
  `warp-burst-not-passed`, `warp-check-without-answer`,
  `warp-check-door-kept`: each caught (`sh scripts/plant-each.sh`).
- New entries for the rest of `ip.rs`: T-393 (the TCP timeout, the
  keepalive, no Nagle), T-394 (the streams of a closed session fail; a
  close by the caller), T-395 (UDP to a family that the device lacks). The
  DNS over TLS of the servers of the check is in the Approach of T-338.
- Not run here: the live test (no WARP endpoint from this sandbox), the CI
  of a push, the adoption trial of podnet (T-339).

# T-336: `pod-warp`: the relay leg and the race of the direct path and the relay, with the leg file

**Source:** T-028, the first part (`docs/decisions.md`, C137).
**Category:** feature
**Milestone:** M3
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

podnet opens a MASQUE session on the direct path first and, after a delay
(3 s, or 0.5 s when the relay worked last), through its WebSocket relay;
the first that opens wins, and the leg that worked goes into
`warp-leg.json` (issues 76 and 126). A session through the relay has a
budget of bytes and time, and is renewed before the relay ends it.
`pod-warp` takes one dial and has no race.

## Premise

- Read: `PodUtils/podnet:src/warp/mod.rs` (`open_session`, `read_leg`,
  `write_leg`, `relay_first_from`) and `session.rs` (`relay_budget`) at
  `e046560`; the meter of `pod-relay` (podnet 83e0c93).
- Not measured: a live session through the relay.

## Approach

`Settings` takes a second dial (the relay of `pod-relay`) and the delays;
`Warp::open_session` races the two as podnet does; the leg is a value that
the caller reads and writes (`relay_first_from` and the text of a leg file
as pure functions, as `door_from`). The budget of a relay session is given
to `WarpNet` (T-335). Loopback tests with two dials of the test, one slow.

## Prove

```sh
cargo test -p pod-warp --all-features
```

## Done

2026-11-12. Commit `5358f67` (the work and the plant), and the commit of
this record; calls C193 to C196.

- `Settings::relay: Option<leg::Relay>` (`dial`, `name`, `meter`,
  `delay` 3 s, `delay_known` 0.5 s); `Warp::open_session` races the
  direct path and the relay: the relay starts after its delay, or at once
  when the direct path failed (C194); the first session that opens wins
  and the other try is dropped; a failure of both legs names both causes
  (C196).
- The leg is a value of the caller (C195): `leg::relay_first_from`,
  `leg::leg_text` (podnet's `warp-leg.json`), `Warp::with_relay_first`,
  `Warp::relay_first`, `Warp::leg`, `Warp::take_leg_change`.
- The budget of a relay session (C193): `session::Meter` in `Relay::meter`
  and `Way::meter`, `MasqueSession::relay_budget`; `WarpNet::start` takes
  it before `NetSettings::renew`.
- Tests: `tests/leg.rs` (`the_direct_path_wins_when_it_is_fast`,
  `the_relay_wins_when_the_direct_path_hangs`,
  `the_relay_goes_first_when_the_leg_says_so`,
  `a_failure_of_both_legs_names_both_causes`) and `leg::tests` (the
  text of the leg file reads back). Plant
  `warp-race-relay-waits-for-the-direct-path` (caught by hand: the test
  of a hung direct path fails after 20 s).
- Finding, fixed in the record: HTTP/3 and WireGuard take no relay leg;
  T-337 has it in its Approach.

# T-337: `pod-warp`: MASQUE over HTTP/3, the WireGuard leg and the carried device

**Source:** T-028, the first part.
**Category:** feature
**Milestone:** M3
**Priority:** P1
**Effort:** L
**Status:** open

## Problem

podnet's session has three transports: HTTP/2 (in `pod-warp`), HTTP/3 on
UDP 443 (`src/warp/h3.rs`, 622 lines, issue 157) and WireGuard on UDP
(the driver of `src/warp/wg.rs` over `pod-wg`, issue 152), and a second
WireGuard device carried inside the first session (issues 154 and 155,
`CARRY_MTU`). `pod-warp` has HTTP/2 alone.

## Premise

- Read: `PodUtils/podnet:src/warp/h3.rs`, `session.rs` (`connect_h3`,
  `connect_wg`, `connect_wg_inside`) and `mod.rs` (`connect_carried`) at
  `e046560`; the endpoint of `pod-quic` over any `Datagrams`; the sans-IO
  initiator of `pod-wg`.
- Not measured: none of them here.

## Approach

`MasqueTx` and `MasqueRx` with the three transports as podnet has them;
HTTP/3 on `pod-quic` over the caller's datagrams; the WireGuard driver
(`connect`, `WgTx`, `WgRx`, `WgKeepalive`) on `pod-wg` over a `Datagrams`
(a system socket, or the datagrams of a `WarpNet`, for the carry). HTTP/3
and WireGuard take no relay leg (podnet issues 152 and 157: the relay
carries TCP): `Warp::open_session` skips `Settings::relay` for them. The
tests `a_session_over_http3_carries_datagrams_and_capsules` and
`http3_refuses_a_proxy_and_a_wrong_key` of podnet, and a loopback tunnel
of WireGuard against the stand-in of `pod-wg`.

## Prove

```sh
cargo test -p pod-warp --all-features
```

# T-338: `pod-warp`: the proxy, the forward, nc and the L4 door, with loopback tests

**Source:** T-028, its Approach (the stand-ins give loopback tests of the
proxy, the forward and nc).
**Category:** feature
**Milestone:** M3
**Priority:** P1
**Effort:** L
**Status:** done

## Problem

podnet's `warp proxy` (SOCKS5 and HTTP, the rules, the names, the bound of
DNS: `src/warp/proxy.rs`, `proxy/names.rs`, `proxy/dns_bound.rs`, 2,130
lines), `warp forward` (`forward.rs`), `warp nc` (`nc.rs`) and the L4 door
of `--l4` (`l4.rs`) are not in `pod-warp`. The rules of the proxy are
(`rules.rs`, with the lint of the caller); the reload of a rules file is
the caller's (C141).

## Premise

- Read: those files at `e046560`. They use podnet's `decisions` (the
  observer of `pod-core` takes its place), `net`, `services` and
  `test_host`; the SOCKS5 datagram path is `pod_proxy::udp` (C23).
- Not measured: none of them here.

## Approach

Each module over `WarpNet` (a `Dial`), with the counters of
`rules::Rules`; the loopback tests of podnet's files against the stand-in
edge of `crates/pod-warp/tests/common/edge.rs`. The resolvers of the proxy
(`Resolver`, UDP and DNS over TLS) are also the servers of the check of the
data path: `DataPathCheck` of T-335 takes UDP servers alone, and podnet's
`check_data_path` asks a resolver of `--resolver` over TLS too.

## Prove

```sh
cargo test -p pod-warp --all-features
```

## Done

2026-11-12, commit `e114433` (the code) and the commit of the record
after it (calls C200 to C204). The L4 door goes to T-406 (call C204: it
needs the HTTP/3 of T-337).

- `pod-proxy`: `serve` (the server face of a SOCKS5 or HTTP proxy) and
  `udp::unwrap`, from the server half of podnet-netns's `socks.rs`
  (C200); `tests/serve.rs` 10 passed.
- `pod-warp`: `resolver` (UDP and DNS over TLS inside the tunnel), `proxy`
  (`Proxy`, `Tunnel`, `serve_client`, the rules with their counters, the
  names and the pins, the UDP associations, the DNS forwarder with its
  bound, the limit of clients, `health`), `forward`, `nc`, each over a
  `Dial` (C201 to C203). `DataPathCheck` takes resolvers on UDP or over
  TLS, with the TLS client of the caller (C202). The loopback tests of
  podnet's files keep their names; the tests of the bind of a listener
  and of the SSH configuration stay in podnet with that code.
- The Prove: `cargo test -p pod-warp --all-features` passes: the unit
  tests 93 passed (the dispatch, the credentials, the limit, the UDP
  client, the resolvers, the DNS bound, the names and the pins, the
  forward, nc), `tests/proxy.rs` 11 passed (the resolvers against the
  stand-in edge, a lost question sent again, the check over TLS against a
  server of RFC 7858 behind a route of the edge, the DNS forwarder, the
  UDP associations, the rules of the proxy over the session, the forward
  and the UDP pipe of nc), and each other target as before;
  `cargo test -p pod-proxy --all-features` passes. Clippy, fmt and the
  docs with `-D warnings` of both crates: clean.
- Plants: `warp-proxy-rule-not-applied` and `warp-dns-bound-not-held` are
  caught; `warp-check-without-answer` now patches the UDP loop of
  `resolver.rs` and is caught; `sh scripts/plant-each.sh --apply`: 0
  stale.
- T-407: the fuzz targets of the server face.

# T-339: `pod-warp`: the live session as a value, TUN and routes, the live step in `live.yml`, and the adoption trials of podnet and podssh

**Source:** T-028, the first part.
**Category:** feature
**Milestone:** M3
**Priority:** P1
**Effort:** L
**Status:** open

## Problem

podnet's `Live` (a session replaced when it closes, `live.rs`,
`live/instance.rs`), `pod.rs`, `find.rs`, the TUN device and its routes
(`tun.rs`, `tun/`, `route.rs`, `route/resolv.rs`, Linux only) and the map
of `exit.rs` are not in `pod-warp`. The live test
`live_register_connect_and_trace` of `crates/pod-warp/tests/live.rs` is
not a step of `live.yml`. No trial patch of podnet or podssh exists
(`docs/design.md`, section 8, step 7).

## Premise

- Read: those files at `e046560`; `.github/workflows/live.yml`.
- Not measured: the live test (the egress of a session reaches no WARP
  endpoint).

## Approach

The modules behind `cfg(target_os = "linux")` where they need it; a step
of `live.yml` that runs `cargo test -p pod-warp --test live -- --ignored`;
adoption/podnet/pod-warp.patch and adoption/podssh/pod-warp.patch
with their runs; then the COORD issue of the freeze to podnet and podssh.

## Prove

```sh
cargo test -p pod-warp --all-features
cargo test -p pod-warp --test live -- --ignored
```

# T-393: `pod-netstack`: the TCP timeout, the keepalive and no Nagle of each socket as settings, for `WarpNet` (podnet issue 125, podssh)

**Source:** T-335 (the parts of podnet's `ip.rs` that the stack of
`pod-netstack` lacks).
**Category:** feature
**Milestone:** M3
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podnet's WARP stack (`PodUtils/podnet:src/warp/ip.rs`, `Driver::tcp_socket`)
gives each TCP socket a timeout of 60 s (a connect, data with no ACK, or a
keepalive with no answer aborts it), a keepalive each 20 s (issue 125), and
no Nagle (podssh: a keystroke of SSH goes at once, with no wait for the ACK
of the data before it, one round trip through WARP). The sockets of
`pod-netstack` have no timeout and no keepalive, and Nagle is on until the
app calls `TcpStream::set_nodelay`. `WarpNet` sets none of them: a stream
to a peer that went away waits for ever, and a small write waits for an
ACK.

## Premise

- Read: `crates/pod-netstack/src/stack/reactor.rs` (`new_tcp_socket`
  sets the buffers and the controller only), `crates/pod-warp/src/net.rs`
  (`connect_addr`); podnet's tests `a_tcp_socket_has_a_timeout_and_a_keepalive`
  and `the_tcp_sockets_have_no_nagle` at `e046560`.
- Not measured: what the reactor gives the app when smoltcp aborts a socket
  at its timeout (a reset is the expectation).

## Approach

`StackConfig` fields `tcp_timeout`, `tcp_keepalive` (both `Option<Duration>`,
none by default) and `nodelay` (false by default), set in `new_tcp_socket`
for each connect and each accepted socket. `NetSettings` of `pod-warp` sets
60 s, 20 s and true, as podnet. Tests: the settings on a socket (podnet's two
tests keep their names); a connect to a peer that never answers ends with an
error after a timeout of 1 s; a stream whose peer is gone ends after the
keepalive and the timeout. A plant for the timeout.

## Prove

```sh
cargo test -p pod-netstack --all-features
cargo test -p pod-warp --all-features
```

## Done

2026-11-12. Commit `d32dd06` (the work, the plants) and the commit of
this record; calls C189, C190 and C191.

- `pod-netstack`: `StackConfig::tcp_timeout`, `tcp_keepalive` (each
  `Option<Duration>`, none by default) and `nodelay` (false by default).
  `new_tcp_socket` sets them on each socket, so each connect and each
  listen socket (and so each accepted stream) takes them;
  `TcpStream::nodelay` says the setting (C189). `StackConfig` moves to
  `stack/config.rs` (`stack.rs` had 474 lines).
- Measured (the premise): smoltcp closes a socket at its timeout and sends
  a RST; before, the app read "connection reset by the peer" on an open
  stream and `ConnectionRefused` on a connect. Now it reads `TimedOut`
  ("no answer of the peer in the TCP timeout of N ms"). The reactor reads
  the head of each inbound RST (`Head::Rst`, `Core::note_reset`): a RST of
  the peer that closed the socket stays `ConnectionReset`, or
  `ConnectionRefused` before the handshake (C190).
- `pod-warp`: `NetSettings::tcp_timeout` (`TCP_TIMEOUT`, 60 s),
  `tcp_keepalive` (`TCP_KEEPALIVE`, 20 s), `nodelay` (true), as podnet's
  `ip.rs`. Fix in place: `NetSettings::tcp_linger` (`TCP_LINGER`, 30 s,
  podnet's linger of issue 125); the stack of a session had the default of
  60 s (C191).
- Tests: `pod-netstack` `stack/tests/timeout.rs` (6 new:
  `a_tcp_socket_has_a_timeout_and_a_keepalive` and
  `the_tcp_sockets_have_no_nagle` with podnet's names; a connect with no
  answer ends with `TimedOut` after a timeout of 1 s; a stream whose peer
  is gone ends at each end; the keepalive holds an idle stream to a live
  peer past the timeout; a RST of the peer stays a reset).
  `cargo test -p pod-netstack --all-features`: 77 passed. `pod-warp`
  `net::settings::tests::the_stack_has_the_tcp_timeout_the_keepalive_and_no_nagle_of_podnet`.
- Plants: `netstack-tcp-timeout-not-set` and
  `netstack-peer-reset-read-as-timeout`: each caught (run by hand in the
  tree with the shared target). The patches of
  `netstack-controller-not-set`, `netstack-dropped-stream-never-lingers`
  and `warp-burst-not-passed` are made again on the changed files.

# T-394: `pod-warp`: the streams of a `WarpNet` fail at once when its session closes, and a close by the caller (podnet issues 125, 112, 185)

**Source:** T-335 (the parts of podnet's `ip.rs` that `WarpNet` lacks).
**Category:** feature
**Milestone:** M3
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

podnet's driver (`PodUtils/podnet:src/warp/ip.rs`, `Driver::run`) fails
each pending operation and aborts each connection when its session closes
(issue 125: "the operations of a dead session fail now. Before, a read
waited for data that could not come"); its supervisor marks the stack
closed when the driver stops, also by a panic (issue 112); and
`WarpStack::close_session(why)` closes a session on purpose (issue 185:
`--session-max-age`), so that `Live` opens a new one. `WarpNet` marks
itself closed when a pump ends, and refuses a new dial, but its stack runs
on: an open stream reads nothing and waits for ever, and the caller has no
close.

## Premise

- Read: `crates/pod-warp/src/net.rs` (`Closed::mark`, `pumps`), the
  reactor of `pod-netstack` (`run` clears its connections at a shutdown:
  the app reads the end of the stream, not an error); podnet's tests
  `a_dead_session_fails_the_pending_operations` and
  `a_panic_of_the_driver_closes_the_stack` at `e046560`.
- Not measured: a stream of a `WarpNet` after the edge ends its session.

## Approach

A command of `NetStack` that resets each connection and each UDP socket
with a reason (the app reads `ConnectionReset` with it, not a clean end),
called by `WarpNet` when the first cause of the close is marked;
`WarpNet::close(why)`; the stack of a closed session marks the cause of a
stopped reactor. Loopback tests: the edge ends a session, and a read of an
open stream fails with the cause; a close by the caller does the same.

## Prove

```sh
cargo test -p pod-netstack --all-features
cargo test -p pod-warp --all-features
```

## Done

2026-11-12. Commits `428db75` (the work) and the commit of the plants
and of this record (this commit); calls C180 and C181.

- `pod-netstack`: `NetStack::reset_all(reason)` and `resetter() ->
  StackReset` (a handle that a task keeps) end each open stream and each
  UDP socket at once: the app reads its bytes, then `ConnectionReset` with
  the reason; a write, a UDP receive and a UDP send fail the same way. A
  listener stays (C180). The stop of the reactor (a shutdown, a drop, a
  panic) ends each open socket with its cause, not a clean end;
  `stop_reason` and `stopped()` give the cause, and a new command fails
  with it. Module `stack/reset.rs`.
- Fix in place: the drop of a connection did not wake a write that waited
  for room in the channel from the app. After a reap, a shutdown or a
  reset, the write waited for ever (the test failed on the code before).
- `pod-warp`: the first cause of the close of a `WarpNet` resets its stack
  (`the WARP session is closed: CAUSE`); a task watches
  `NetStack::stopped()` (podnet's `supervise`); `WarpNet::close(why)`
  (podnet's `close_session`) stops the tasks of the session; a new UDP
  socket of a closed session fails as a new dial does (C181). The test
  edge has `Edge::end_sessions` (a shutdown of the TCP connection of each
  session).
- Tests: `pod-netstack` `stack/tests/reset.rs` (4 new: a reset of each
  socket, a write that waits, a shutdown, a panic of the reactor through
  a `GatewayService`); `pod-warp` `tests/net.rs` (2 new: the edge ends a
  session, the caller closes it; each with a read of a stream and a UDP
  receive that wait). `cargo test -p pod-netstack --all-features`: 71
  passed. `cargo test -p pod-warp --all-features`: 61 unit tests, 8 of
  `tests/net.rs`, 8 of `tests/stack.rs`, 8 of `tests/register.rs`, 1 live
  test ignored. `cargo test -p pod-hold --all-features` (a consumer of
  the stack): 59 passed.
- Plants: `netstack-reset-wakes-no-write`, `netstack-stop-reads-a-clean-end`
  and `warp-close-resets-nothing`: each caught (run by hand in the tree
  with the shared target). The patches of `netstack-udp-over-the-limit-goes`
  and `netstack-udp-send-loss-not-counted` are made again on the changed
  files; each still catches.

# T-395: `pod-warp`: a UDP datagram of `WarpNet` to a family that the device lacks is refused (podnet issue 112)

**Source:** T-335 (the tests of podnet's `ip.rs` that `WarpNet` lacks).
**Category:** defect
**Milestone:** M3
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podnet's stack refuses a datagram to IPv6 on a tunnel with IPv4 alone,
and to IPv4 on a tunnel with IPv6 alone ("the tunnel has no IPv6 address";
issue 112). `WarpNet` gives the family that the device lacks a
documentation address (`192.0.2.2`, `2001:db8::2`, C138) and refuses a TCP
dial to it, but `WarpNet::datagrams` binds a UDP socket on each address:
a datagram to the other family leaves into the session with the
documentation address as its source, and no answer comes.

## Premise

- Read: `crates/pod-warp/src/net.rs` (`family_failure` is called by the
  dial only; `datagrams` and `bind_udp`); podnet's tests
  `udp_to_ipv6_on_an_ipv4_tunnel_is_refused`,
  `udp_on_a_dual_stack_tunnel_takes_the_source_of_each_family` and
  `udp_to_ipv4_on_an_ipv6_tunnel_is_refused` at `e046560`.
- Not measured: the packet that leaves (read from the code).

## Approach

The `Datagrams` of `WarpNet` refuse a send to a family that the device
lacks (`InvalidInput`, with podnet's words), and the check of the data path
never asks such a server. podnet's three tests keep their names, over the
stand-in edge.

## Prove

```sh
cargo test -p pod-warp --all-features
```

## Done

2026-11-12. Commit `d32dd06` (the work, the plant) and the commit of this
record; call C192.

- `pod-warp`: `WarpNet::datagrams` gives a `WarpDatagrams`
  (`net/datagrams.rs`): a send to a family that the device lacks is
  `InvalidInput`, "the tunnel has no IPv6 address for TO" (podnet's
  words), and no packet leaves. The check of the data path refuses such a
  server before it binds a socket; `DataPathCheck::servers_for` already
  left it out.
- The stand-in edge notes the source and the destination of each UDP
  datagram and the count of the packets of each family (`Watch::udp`,
  `Watch::families`). Fix in place: its DNS answered a UDP datagram that
  holds no question with a panic of its task (an index out of range), and
  so stopped the far end; it now sends an empty answer.
- Tests: `tests/net.rs` (3 new, podnet's names):
  `udp_to_ipv6_on_an_ipv4_tunnel_is_refused` (with a check that names an
  IPv6 server first: no IPv6 packet leaves),
  `udp_on_a_dual_stack_tunnel_takes_the_source_of_each_family`,
  `udp_to_ipv4_on_an_ipv6_tunnel_is_refused`; the unit test
  `net::datagrams::tests::a_family_that_the_device_lacks_is_refused`.
  `cargo test -p pod-warp --all-features`: 63 unit tests, 11 of
  `tests/net.rs`, 8 of `tests/stack.rs`, 8 of `tests/register.rs`, 1 live
  test ignored.
- Plant: `warp-udp-to-a-family-the-device-lacks`: caught (run by hand).

# T-406: `pod-warp`: the L4 door of `--l4` and its proxy, after HTTP/3

**Source:** T-338 (call C204: the L4 door needs the HTTP/3 of T-337).
**Category:** feature
**Milestone:** M3
**Priority:** P1
**Effort:** M
**Status:** open

## Problem

podnet's `warp proxy --l4` (issue 140: `src/warp/l4.rs`, 272 lines, and
`proxy::run_l4`) is not in `pod-warp`. Each TCP client of the proxy is one
HTTP/3 CONNECT stream (RFC 9114, section 4.4) to the L4 door of WARP, on
one QUIC connection with the client certificate and the pinned key of the
device; UDP is not carried. The rules apply with no lookup of a name
(`Table::decide_static`). The door needs the QUIC connection and the
frames of `h3.rs`, which come with T-337.

## Premise

- Read: `PodUtils/podnet:src/warp/l4.rs` and `proxy.rs` (`run_l4`) at
  `e046560`; `l4_sni` reads the endpoint `masque-l4` of `pod-services`.
- Not measured: the L4 door of WARP.

## Approach

`l4::L4` (the door, its QUIC connection made again when it closes, `dial`
of a `Target`) over the HTTP/3 of T-337 and the `Datagrams` of the
caller; `connect_headers_form` with podnet's `PODNET_WARP_L4_FORM` as a
setting (`plain`, `extended`, `header`); `L4` as a `pod_core::Dial`, so
that `Proxy` serves it with its `direct` dial, and the rules of
`decide_static` for a proxy with no resolver in the tunnel. The tests
`a_flow_through_an_l4_door_on_the_loopback` and `the_connect_request` of
podnet; the line of `SOURCES`.

## Prove

```sh
cargo test -p pod-warp --all-features
```

## Start condition

T-337 is done (the HTTP/3 of `h3.rs` is in `pod-warp`).
