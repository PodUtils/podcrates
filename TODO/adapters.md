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
- A test of `src/cloudflare/http2.rs` reads `tools/header-oracle.txt` of
  podnet; the pin of cloudflared and its watch move with the crate.

## Approach

The procedure of `docs/design.md`, section 8, with podnet's files only.

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
