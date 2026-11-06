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
