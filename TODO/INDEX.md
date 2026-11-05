# Index

Each entry of the work record, one row each, sorted by id. The entry is in
the area file that its row links. It closes there, with its Prove run and
the result written.

`TODO/PROGRESS.md` has the only work order. `TODO/RULES.md` gives the rules
of the record. `TODO/issues.md` maps each GitHub issue to its entries.

`cargo todo set` and `cargo todo counts` derive the counts from the rows.
`cargo todo check` makes sure that the counts, the rows and the entries
agree.

## Values

**Priority.**

- **P0**: correctness fails, data is lost, or the process stops.
- **P1**: a crate that a consumer waits for, or a check that the rules
  need.
- **P2**: worth doing; nothing is wrong without it.
- **P3**: worth recording, so that nobody must find it again.

**Effort.** S is less than one day. M is some days. L is one week. There
is no XL: make two entries.

**Status.** `open`, `partial`, `blocked` or `done` (`TODO/RULES.md`).

**Milestone.** `M0` to `M5`, as in `TODO/ROADMAP.md`: M0 Foundation, M1
Transport, M2 Relay, M3 Adapters, M4 Roads, M5 Upkeep.

**Category.** `defect`, `feature`, `measurement`, `release`, `chore`,
`research` or `docs`.

## Entries

**41 entries: 29 open, 0 partial, 1 blocked, 11 done.**

| Priority | open | partial | blocked | done | total |
| --- | --- | --- | --- | --- | --- |
| P0 | 0 | 0 | 0 | 0 | 0 |
| P1 | 16 | 0 | 0 | 10 | 26 |
| P2 | 9 | 0 | 1 | 1 | 11 |
| P3 | 4 | 0 | 0 | 0 | 4 |
| **All** | 29 | 0 | 1 | 11 | 41 |

| ID | Priority | Effort | Milestone | Category | Status | Item |
| --- | --- | --- | --- | --- | --- | --- |
| [T-001](foundation.md) | P1 | S | M0 | chore | done | `pod-todo`: one checker for the records of podssh, podnet and podcrates |
| [T-002](foundation.md) | P1 | S | M0 | docs | done | The record, the design and the procedures of podcrates |
| [T-003](foundation.md) | P1 | M | M0 | chore | done | The checks of each push, each with its plant |
| [T-004](foundation.md) | P1 | M | M0 | chore | done | The static probe on each target |
| [T-005](foundation.md) | P1 | M | M0 | feature | done | `pod-core`: the traits, the errors, the events and the helpers that each crate shares |
| [T-006](foundation.md) | P1 | S | M0 | chore | done | `pod-testkit`: the guards of a test host |
| [T-007](foundation.md) | P1 | M | M0 | chore | done | The workflows of podcrates |
| [T-008](foundation.md) | P1 | M | M0 | chore | done | Adoption trials in CI, and the trial of `pod-todo` in podssh and podnet |
| [T-009](foundation.md) | P1 | S | M0 | release | done | The first release |
| [T-010](transport.md) | P1 | S | M1 | feature | done | `pod-identity`: the Ed25519 identity, the key files and the entropy |
| [T-011](transport.md) | P2 | S | M1 | feature | done | `pod-term`: the line discipline and the grammar of a terminal's output |
| [T-012](transport.md) | P1 | L | M1 | feature | open | `pod-tls`: trust, the TLS configurations, the pure-Rust provider and ECH |
| [T-013](transport.md) | P1 | M | M1 | feature | open | `pod-proxy`: the dial, the proxies, the race of addresses and the HTTP client (the base of podssh) |
| [T-014](transport.md) | P1 | M | M1 | feature | open | `pod-proxy`: podnet's proxy chain, Digest and fetch |
| [T-015](transport.md) | P1 | M | M1 | feature | open | `pod-dns`: the resolver: system, DoH in both forms, pins, HTTPS records |
| [T-016](transport.md) | P1 | M | M1 | feature | open | `pod-ws`: the WebSocket client and server, HTTP/2 and cleartext roads |
| [T-017](transport.md) | P2 | S | M1 | chore | open | The fuzz workspace and its weekly workflow |
| [T-018](relay.md) | P1 | M | M2 | research | open | One QUIC stack: quinn against noq, measured |
| [T-019](relay.md) | P1 | M | M2 | feature | open | `pod-quic`: the QUIC endpoint, the probe and QUIC over datagrams |
| [T-020](relay.md) | P1 | L | M2 | feature | open | `pod-session`: the resumable layer and UDP in a stream |
| [T-021](relay.md) | P1 | M | M2 | feature | open | `pod-e2e`: the end-to-end channel (Noise XX) |
| [T-022](relay.md) | P1 | S | M2 | feature | open | `pod-wormhole`: the mailbox client, SPAKE2 and the code words |
| [T-023](relay.md) | P1 | M | M2 | feature | open | `pod-netstack`: the userspace TCP and UDP stack on smoltcp 0.14 |
| [T-024](relay.md) | P2 | S | M2 | feature | open | `pod-wg`: the WireGuard initiator |
| [T-025](relay.md) | P1 | L | M2 | feature | open | `pod-relay`: the relay client, the pool, the roads and the reverse road (the base of podssh) |
| [T-026](relay.md) | P1 | M | M2 | feature | open | `pod-relay`: podnet's relay client and its plain reverse mode |
| [T-027](adapters.md) | P2 | S | M3 | feature | open | `pod-transit`: the transit relay carrier |
| [T-028](adapters.md) | P1 | L | M3 | feature | open | `pod-warp`: the WARP adapter |
| [T-029](adapters.md) | P1 | L | M3 | feature | open | `pod-cloudflare`: the edge client, the tunnel and the quick tunnels (the base of podnet) |
| [T-030](adapters.md) | P1 | M | M3 | feature | open | `pod-cloudflare`: podssh's quick tunnels onto the edge client |
| [T-031](roads.md) | P2 | M | M4 | feature | open | `pod-derp`: the DERP client and the DERP carrier of a node |
| [T-032](roads.md) | P3 | S | M4 | feature | open | `pod-piping`: the Piping Server carrier |
| [T-033](roads.md) | P3 | S | M4 | feature | open | `pod-opentunnel`: the OpenTunnel carrier |
| [T-034](roads.md) | P2 | S | M4 | feature | open | `pod-acme`: the ACME client |
| [T-035](roads.md) | P2 | L | M4 | feature | open | `pod-egress`: the other roads out (via, VLESS, wstunnel, Wisp, Shadowsocks and more) |
| [T-036](roads.md) | P2 | L | M4 | feature | open | `pod-relay-serve`: the relay role and its mailbox |
| [T-037](roads.md) | P2 | M | M4 | feature | open | `pod-iroh`: the iroh road |
| [T-038](roads.md) | P3 | S | M4 | feature | open | `pod-tor`: the Tor road through arti |
| [T-039](roads.md) | P2 | L | M4 | feature | open | `pod-ts`: a Tailscale node in the process |
| [T-040](upkeep.md) | P2 | S | M5 | chore | blocked | The trial of podvm |
| [T-041](transport.md) | P3 | S | M1 | chore | open | `pod-testkit`: the SSH test server of podssh (the feature `ssh`) |
