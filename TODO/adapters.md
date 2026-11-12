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
**Status:** open

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
