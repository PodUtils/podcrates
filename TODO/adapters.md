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
