This file holds the entries of milestone M2: the QUIC stack, the layer,
the end-to-end channel, the code words, the userspace stack, WireGuard and
the relay client. Each crate follows the procedure of `docs/design.md`,
section 8.

# T-018: One QUIC stack: quinn against noq, measured

**Source:** `docs/decisions.md`, call C10.
**Category:** research
**Milestone:** M2
**Priority:** P1
**Effort:** M
**Status:** open

## Problem

podnet uses quinn 0.11 (the edge of Cloudflare, the quic+warp leg, MASQUE
over HTTP/3, QUIC over WARP through `WarpUdpSocket`). podssh uses noq 1.4,
the fork of quinn by n0 (the quick tunnel, the QUIC road, iroh). One
binary must carry one QUIC stack.

## Premise

- Read: podnet `PodUtils/podnet:src/warp/quic.rs`,
  `PodUtils/podnet:src/cloudflare/quic.rs`; podssh
  `PodUtils/podssh:crates/podssh-relay/src/quick/quic.rs`,
  `PodUtils/podssh:crates/podssh-relay/src/quic_road/`.
- noq is the stack of iroh, which `pod-iroh` keeps.

## Approach

1. List each use of each stack: datagrams, a custom UDP socket, the
   transport configuration, the provider.
2. Port the smallest use to the other stack, both ways, and measure the
   build size and the throughput on the loopback.
3. Write the choice in `docs/decisions.md`.

## Prove

```sh
cargo test -p pod-quic
```

The entry gives the measurements of both stacks.

## Start condition

T-016 is done.

# T-019: `pod-quic`: the QUIC endpoint, the probe and QUIC over datagrams

**Source:** `docs/design.md`, section 4 (M2).
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** M
**Status:** open

## Problem

`pod-relay`, `pod-warp` and `pod-cloudflare` share the QUIC endpoint, the
QUIC probe and QUIC over `Datagrams`.

## Premise

- Read: podssh `quick/quic.rs`, `quick/probe.rs`; podnet
  `PodUtils/podnet:src/warp/quic.rs` (the client configuration is an
  argument: podnet's edge 10 of its architecture study).

## Approach

The procedure of `docs/design.md`, section 8, on the stack of T-018.

## Prove

```sh
cargo test -p pod-quic
sh scripts/agent/check.sh
```

## Start condition

T-018 is done.

# T-020: `pod-session`: the resumable layer and UDP in a stream

**Source:** `docs/design.md`, section 4 (M2).
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** L
**Status:** open

## Problem

podssh's resumable layer (64-bit offsets each way, resume through any
road, replay, heartbeat, the move before the limits of the relay,
handover to a new process) is the base of the relay roads of podssh and
of the transfer of podnet.

## Premise

- Read: `PodUtils/podssh:crates/podssh-relay/src/session/` (7,468, with
  `handover/`), `udp/` (352), `answer.rs`, `transit/link.rs`.
- Tests: 21 files `PodUtils/podssh:crates/podssh-relay/tests/session_*.rs`
  and `udp_framing.rs`.
- `session/` changed 38 times in the last 50 commits of podssh: the
  freeze of `docs/decisions.md` (Who edits) applies from the import.

## Approach

The procedure of `docs/design.md`, section 8. The variables
(`PODSSH_REPLAY_BUFFER` and the others) become fields of the settings.

## Prove

```sh
cargo test -p pod-session
sh scripts/agent/check.sh
```

## Start condition

T-010 and T-016 are done.

# T-021: `pod-e2e`: the end-to-end channel (Noise XX)

**Source:** `docs/design.md`, section 4 (M2).
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** M
**Status:** open

## Problem

The relay sees plain bytes unless the end-to-end channel runs above the
layer.

## Premise

- Read: `PodUtils/podssh:crates/podssh-relay/src/e2e/` (3,376, on snow
  0.10); tests `e2e`, `e2e_faults`, `e2e_layered`, `e2e_net`,
  `e2e_harness/`.

## Approach

The procedure of `docs/design.md`, section 8.

## Prove

```sh
cargo test -p pod-e2e
sh scripts/agent/check.sh
```

## Start condition

T-020 is done.

# T-022: `pod-wormhole`: the mailbox client, SPAKE2 and the code words

**Source:** `docs/design.md`, section 4 (M2).
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** open

## Problem

podssh pairs with a code of words through a magic-wormhole mailbox;
podnet's transfer needs the same rendezvous.

## Premise

- Read: `PodUtils/podssh:crates/podssh-relay/src/code/` (1,824):
  `mailbox.rs`, `wormhole.rs`, `phrase.rs`, `words.rs`, `mod.rs`; the
  pairing exchange (`exchange.rs`, `run.rs`) uses the pairs of the relay
  and goes to `pod-relay`.
- The AppID is a parameter (`docs/design.md`, section 1, rule 10).

## Approach

The procedure of `docs/design.md`, section 8, with the mailbox harness.

## Prove

```sh
cargo test -p pod-wormhole
sh scripts/agent/check.sh
```

## Start condition

T-016 is done.

# T-023: `pod-netstack`: the userspace TCP and UDP stack on smoltcp 0.14

**Source:** `docs/design.md`, section 4 (M2).
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** M
**Status:** open

## Problem

WARP and the virtual networks of podnet carry TCP and UDP in user space,
on smoltcp 0.12 now.

## Premise

- Read: podnet `PodUtils/podnet:src/warp/ip.rs` (1,680),
  `PodUtils/podnet:src/vnet/device.rs` (639; it calls the recorder of
  `podnet-vnet`), `PodUtils/podnet:src/vnet/framing.rs` (253); the TCP of
  podssh-warp on smoltcp 0.14.
- smoltcp 0.14 has each feature that podnet uses.

## Approach

The procedure of `docs/design.md`, section 8; the move from smoltcp 0.12
to 0.14; a `PacketTap` trait for the recorder.

## Prove

```sh
cargo test -p pod-netstack
sh scripts/agent/check.sh
```

## Start condition

T-005 is done.

# T-024: `pod-wg`: the WireGuard initiator

**Source:** `docs/design.md`, section 4 (M2); `docs/decisions.md`, call C8.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

podnet's WireGuard leg of WARP has its own initiator, sans-IO, on ring.

## Premise

- Read: podnet `PodUtils/podnet:src/warp/wg.rs` (1,078): it imports only
  ring.

## Approach

The procedure of `docs/design.md`, section 8.

## Prove

```sh
cargo test -p pod-wg
sh scripts/agent/check.sh
```

## Start condition

T-003 is done.

# T-025: `pod-relay`: the relay client, the pool, the roads and the reverse road (the base of podssh)

**Source:** `docs/design.md`, section 4 (M2).
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** L
**Status:** open

## Problem

podssh's relay client (hosts, pool, rank, failover, tokens, the roads to
a relay, the reverse road, the pairs, the blocking facade of podvm) is
the base of the relay crate of the three consumers.

## Premise

- Read: the files of `PodUtils/podssh:crates/podssh-relay/src/` of
  `docs/design.md`, section 4 (about 18,000 lines), and podssh-ws
  `h2_road/` and `cleartext/`.
- Cycles of the source: `reverse/wire.rs` names `serve::reverse::carrier::Door`
  (a trait of `pod-relay`); `quic_road/` uses `quick::{probe, quic}`
  (`pod-quic`); `e2e/legs.rs` uses `answer::offers` (`pod-session`).
- `reverse/` changed 34 times in the last 50 commits of podssh.

## Approach

The procedure of `docs/design.md`, section 8, with podssh's files only;
the blocking facade behind the feature `blocking`.

## Decision

The owner, 2026-11-06 (Q103): podvm takes the feature `blocking` of
`pod-relay` from podcrates by a tag; podssh's test of two clients at once
comes with the facade.

## Prove

```sh
cargo test -p pod-relay --all-features
sh scripts/agent/check.sh
```

## Start condition

T-019, T-021 and T-022 are done.

# T-026: `pod-relay`: podnet's relay client and its plain reverse mode

**Source:** `docs/design.md`, section 8, step 5.
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** M
**Status:** open

## Problem

podnet's relay code (the mint, the token cache, `RelayBudget`, the race
of hosts, the reverse mode on `/v1/node/NAME` with 32-hex ids) must keep
each capability in `pod-relay`.

## Premise

- Read: podnet `PodUtils/podnet:src/ws_relay.rs` (995; `active()` is a
  process global), `PodUtils/podnet:src/relay_pool.rs` (380; a copy of
  podssh's pool), `PodUtils/podnet:src/relay_reverse.rs` (1,139).

## Approach

Each capability of podnet that the base does not have, with its tests;
the plain reverse mode as a mode of the API of the reverse road; the
parity list; the trials of podssh, podnet and podvm.

## Prove

```sh
cargo test -p pod-relay --all-features
sh scripts/agent/check.sh
```

## Start condition

T-025 is done.

# T-110: `pod-session`: the held link to a server that is not a podssh relay (GitHub #35)

**Source:** GitHub #35 (A-C-08, A-S-05)
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** M
**Status:** open

## Problem

The node agent of cfb needs one WebSocket to a cfb URL: pings each 25 s, reconnect with jitter, resume by
sequence and a replay window, a CONNECT proxy and a private CA, upgrade headers from files or from a
command, and a local Unix socket of frames.

## Premise

- Read: the ask of A-C-08. podssh has the WebSocket roads and the resumable layer, with no link to a server
  that is not a podssh relay.
- Not measured: the link.

## Approach

A `Link` over `pod-ws` with the layer of `pod-session`: pings at a set interval, reconnect with jitter,
resume by sequence and a replay window, upgrade headers from files or from a command, a frame API for a
local socket. The wire is T-120. The command `podssh link` stays in podssh.

## Prove

```sh
cargo test -p pod-session --all-features
```

A test drops 3 % of the frames for a long run. No frame is lost or repeated.

## Start condition

T-016 and T-020 are done.

# T-111: `pod-relay`: a lease header, Access headers from files on each WebSocket road, a heartbeat that a relay offers (GitHub #35)

**Source:** GitHub #35 (A-S-14, A-C-04, A-S-05)
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

A-S-14, A-C-04 and A-S-05 change what each relay road sends: a lease in place of a token, Access service
token headers from files, and a heartbeat that lets a Durable Object relay hibernate.

## Premise

- Read: the asks. podssh sends `X-Relay-Token` only, and pings each 10 s.
- Not measured: a relay that offers the heartbeat.

## Approach

A lease header from a file. Access headers from files on each WebSocket road, with exit class Refused and
a name of the missing variable. The heartbeat that a `hello` offer sets: WebSocket Ping, or 60 s or more.

## Prove

```sh
cargo test -p pod-relay --all-features
```

## Start condition

T-025 is done.

# T-120: The wire documents of the layer, the reverse dialect and the role's extensions, with test vectors (GitHub #35)

**Source:** GitHub #35 (A-S-15, A-X-04, A-C-08)
**Category:** docs
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

A port of the relay to wasm (`podrelay-worker`, A-X-04) and the link of A-C-08 need the wire as a
document with test vectors. The owner, 2026-11-09: the wire document comes first.

## Premise

- Read: the prose in `PodUtils/podssh:docs/relay.md` and `PodUtils/podssh:docs/design.md`.
- Not measured: a vector test.

## Approach

One wire document for `pod-session` and one for `pod-relay`, each with vector files. A test reads each
vector. The Python stand-in of podssh reads the same vectors. podssh can write the documents first, and
the import takes them.

## Prove

```sh
cargo test -p pod-session --all-features
cargo test -p pod-relay --all-features
```

## Start condition

T-020 and T-025 are done.

# T-121: `pod-relay`: renew a pair (PodUtils/podnet#413)

**Source:** PodUtils/podnet#413, ask 1 (PodUtils/podnet@T-161, PodUtils/podnet#358)
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

A pair of the reverse road ends at its TTL. The published relay contract has no call that
extends it, so a node cannot stay paired for months.

## Premise

- Read: the ask of podnet; the relay contract of `pod-relay` (T-025, T-047).
- Not read: a renew endpoint on the relay server; the owner of the relay adds it.

## Approach

A call `POST /v1/pair/NAME/renew`, authorized by the node token only, that extends the TTL and
keeps the pair id. The node renews at half the TTL and again in the first warning band. A test
with the stand-in relay of `pod-testkit`.

## Prove

```sh
cargo test -p pod-relay --all-features
cargo todo check
```

## Start condition

T-025 or T-026 is done, and the relay server has the renew endpoint.

# T-122: `pod-relay`: a health memory of the relay hosts (PodUtils/podnet#413)

**Source:** PodUtils/podnet#413, ask 2 (PodUtils/podnet@T-162, PodUtils/podnet#359)
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

The alternate relay hosts are chosen once at the start. A stalled primary costs the stagger of the
race (2 s) on each dial.

## Premise

- Read: the ask of podnet.
- Not measured: the cost over a day of dials.

## Approach

Count the failures and the lost races of each host. Demote a host after 2; restore it on a
success; keep the order stable otherwise. Tests with planted failures.

## Prove

```sh
cargo test -p pod-relay --all-features
```

## Start condition

T-025 or T-026 is done.

# T-123: `pod-relay`: the session budget from the pool document (PodUtils/podnet#413)

**Source:** PodUtils/podnet#413, ask 3 (PodUtils/podnet@T-163, PodUtils/podnet#360)
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

The budget of a relay session (60 MiB, 11.5 h) is a constant. The default relay publishes `limits`
in `/relays.json`.

## Premise

- Read: the ask of podnet; `crates/pod-services/services/ajam-relay.toml` (the watch of `/limits`).
- Not measured: the values of other relays.

## Approach

Parse `limits` with the published key names, keep them with the cached pool, and give them to the
meter (`pod_core::Metered`). The floor is the current constants; clamp each value; keep the margins
60/64 MiB and 11.5/12 h.

## Prove

```sh
cargo test -p pod-relay --all-features
```

## Start condition

T-025 or T-026 is done.

# T-124: `pod-relay`: failover on the reverse road (PodUtils/podnet#413)

**Source:** PodUtils/podnet#413, ask 4 (PodUtils/podnet@T-200, PodUtils/podnet#397)
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

`pair`, `node` and `stop` of the reverse road reach host 0 of the pool only.

## Premise

- Read: the ask of podnet.
- Not measured: a node socket on host B that serves a pair minted on host A.

## Approach

Measure the cross-host case first. Then one host loop for `connect`, `websocket` and `post_json`
of the reverse road, with the health memory of T-122.

## Prove

```sh
cargo test -p pod-relay --all-features
```

## Start condition

T-025 or T-026 is done.
