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

**98 entries: 50 open, 1 partial, 0 blocked, 47 done.**

| Priority | open | partial | blocked | done | total |
| --- | --- | --- | --- | --- | --- |
| P0 | 0 | 0 | 0 | 2 | 2 |
| P1 | 14 | 0 | 0 | 21 | 35 |
| P2 | 26 | 1 | 0 | 21 | 48 |
| P3 | 10 | 0 | 0 | 3 | 13 |
| **All** | 50 | 1 | 0 | 47 | 98 |

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
| [T-012](transport.md) | P1 | L | M1 | feature | done | `pod-tls`: trust, the TLS configurations, the pure-Rust provider and ECH |
| [T-013](transport.md) | P1 | M | M1 | feature | done | `pod-proxy`: the dial, the proxies, the race of addresses and the HTTP client (the base of podssh) |
| [T-014](transport.md) | P1 | M | M1 | feature | done | `pod-proxy`: podnet's proxy chain, Digest and fetch |
| [T-015](transport.md) | P1 | M | M1 | feature | done | `pod-dns`: the resolver: system, DoH in both forms, pins, HTTPS records |
| [T-016](transport.md) | P1 | M | M1 | feature | open | `pod-ws`: the WebSocket client and server, HTTP/2 and cleartext roads |
| [T-017](transport.md) | P2 | S | M1 | chore | done | The fuzz workspace and its weekly workflow |
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
| [T-040](upkeep.md) | P2 | S | M5 | chore | partial | The trial of podvm |
| [T-041](transport.md) | P3 | S | M1 | chore | open | `pod-testkit`: the SSH test server of podssh (the feature `ssh`) |
| [T-042](foundation.md) | P1 | S | M0 | chore | done | `publish.yml`: the publish to crates.io, apart from the release |
| [T-043](foundation.md) | P2 | S | M0 | chore | done | The names of the crates on crates.io and their trusted publishers (GitHub #1) |
| [T-044](transport.md) | P2 | S | M1 | chore | done | The tests of the system stores of `pod-tls` with a root in the store, on Windows and macOS |
| [T-045](transport.md) | P2 | M | M1 | chore | done | The adoption trials check the static binaries of the consumer and build each feature |
| [T-046](foundation.md) | P2 | S | M0 | chore | done | The shared CI scripts: the zig fetch, its mirrors and their weekly check |
| [T-047](foundation.md) | P2 | S | M2 | chore | open | The weekly watch of the relay contract, in `pod-relay` |
| [T-048](transport.md) | P1 | S | M1 | research | done | `pod-services`: the inventory of the relays and services of other parties in podssh and podnet |
| [T-049](transport.md) | P1 | L | M1 | feature | done | `pod-services`: the schema, the data of each service and the library |
| [T-050](transport.md) | P1 | L | M1 | feature | open | `pod-services`: the watches, their workflows and the measurement scripts |
| [T-051](foundation.md) | P1 | S | M0 | chore | done | The fallback channel between the sessions, tested (GitHub #2) |
| [T-052](transport.md) | P1 | S | M1 | defect | done | The tests of a closed port of `pod-proxy` race with the other tests |
| [T-053](foundation.md) | P2 | S | M6 | chore | done | The names of the crates of podvm on crates.io (GitHub #1) |
| [T-054](transport.md) | P2 | S | M1 | research | done | `pod-services`: the inventory of podvm |
| [T-055](podvm.md) | P2 | M | M6 | feature | done | `pod-hostprobe`: the host probes of podvm |
| [T-056](podvm.md) | P2 | M | M6 | feature | open | `pod-oci`: the OCI registry client of podvm |
| [T-057](podvm.md) | P2 | M | M6 | feature | open | `pod-extract`: the rootless apply of a layer |
| [T-058](podvm.md) | P2 | M | M6 | feature | open | `pod-rootfs`: the completion of a root file system |
| [T-059](podvm.md) | P2 | M | M6 | feature | open | `pod-enter`: the entry of a payload |
| [T-060](podvm.md) | P2 | M | M6 | feature | open | `pod-supervise`: detached payloads |
| [T-061](podvm.md) | P2 | M | M6 | feature | open | `pod-guest`: a disposable QEMU guest |
| [T-062](podvm.md) | P3 | L | M7 | research | open | The studies of the crates of the widened scope of podvm |
| [T-063](transport.md) | P2 | M | M1 | feature | done | The synchronous face for podvm: `pod-tls` with no tokio, a blocking `pod-proxy` |
| [T-064](foundation.md) | P1 | S | M0 | defect | done | `watch-org.sh` reads no repository but the last on Windows (PodUtils/podvm#195) |
| [T-065](transport.md) | P0 | S | M1 | defect | done | A vectored write through `pod_proxy::Stream` sends only its first buffer |
| [T-066](transport.md) | P2 | M | M1 | feature | done | `pod-services`: the data files of podvm |
| [T-067](transport.md) | P1 | S | M1 | defect | done | Two tests fail on macOS and Windows in the nightly |
| [T-068](transport.md) | P2 | S | M1 | defect | done | The hint of an unknown issuer names a CA that replaces the roots, which podvm has not |
| [T-069](transport.md) | P2 | M | M1 | feature | done | `pod-proxy`: a streamed body for `blocking::Client` (and the async one) |
| [T-070](transport.md) | P2 | S | M1 | defect | done | `pod-proxy`: the drop of a `blocking::Dialer` waits for a name lookup that the dial left |
| [T-071](transport.md) | P3 | S | M1 | feature | done | `pod-proxy`: a read limit for `blocking::Stream::Exec` |
| [T-072](podvm.md) | P2 | S | M6 | defect | done | `pod-hostprobe`: `sys::statfs` gives wrong values on 32-bit targets |
| [T-073](transport.md) | P2 | S | M1 | feature | done | `pod-dns` names the DoH server that answered in its diagnostics |
| [T-074](transport.md) | P2 | S | M1 | defect | done | `pod-proxy`: the words of a failed dial name the target twice |
| [T-075](transport.md) | P2 | S | M1 | feature | done | `pod-proxy`: `HTTP_PROXY` for a target of `http://` |
| [T-076](transport.md) | P3 | M | M1 | feature | done | `pod-proxy`: redirects in the client |
| [T-077](transport.md) | P3 | M | M1 | feature | done | `pod-proxy`: keep-alive connections in the client |
| [T-078](transport.md) | P2 | S | M1 | feature | done | `pod-services`: a second public Piping Server |
| [T-079](transport.md) | P2 | S | M1 | feature | done | `pod-services`: Tailscale's main DERP fleet after tailcat's |
| [T-080](podvm.md) | P0 | S | M6 | defect | done | `pod-hostprobe`: a failed system call reads as a success on a 32-bit target |
| [T-081](foundation.md) | P2 | S | M1 | feature | done | `pod-core`: a `Failure` gives its I/O cause, and "not found" is not "unavailable" |
| [T-082](transport.md) | P2 | S | M1 | defect | open | `pod-proxy`: the `Host` header of an IPv6 literal on port 443 (GitHub #20) |
| [T-083](transport.md) | P2 | M | M1 | feature | open | `pod-proxy`: the SOCKS5 datagram path (UDP ASSOCIATE and command 0x83) (GitHub #21) |
| [T-084](foundation.md) | P3 | S | M0 | docs | open | The record of `SOURCES` names a git blob id (GitHub #22) |
| [T-085](foundation.md) | P2 | S | M0 | defect | open | `check-sources.py`: the default branch of a source repository (GitHub #23) |
| [T-086](roads.md) | P2 | L | M4 | feature | open | Import `pod-gsr` and `hackerschoice-gsr.toml` (GitHub #24) |
| [T-087](roads.md) | P2 | S | M4 | feature | open | `pod_gsr::Stream` records how it ended (GitHub #25) |
| [T-088](roads.md) | P2 | M | M4 | feature | open | `pod-gsr`: the synchronous face (GitHub #26) |
| [T-089](transport.md) | P2 | S | M1 | defect | open | `pod-services`: `tailscale-derp` watches the hosts and holds the four missing servers (GitHub #27) |
| [T-090](transport.md) | P2 | S | M1 | defect | open | `pod-services`: `ajam-relay` watches its seven hosts and the pool (GitHub #28) |
| [T-091](transport.md) | P2 | S | M1 | defect | open | `pod-dns`: `Way::lookup` refuses a name that is not DNS-safe (GitHub #29) |
| [T-092](foundation.md) | P2 | M | M0 | chore | open | The live workflow `live.yml` (GitHub #30) |
| [T-093](foundation.md) | P3 | S | M0 | docs | open | README: the state and the table of crates (GitHub #31) |
| [T-094](foundation.md) | P3 | S | M0 | docs | open | `docs/design.md`: the repository table matches the tree (GitHub #32) |
| [T-095](foundation.md) | P2 | S | M0 | chore | open | The static probe turns on `pod-proxy/blocking` and the builds with no tokio (GitHub #33) |
| [T-096](transport.md) | P3 | S | M1 | docs | open | `pod-proxy` README: the six variables of `Rules::http_proxy_vars` (GitHub #34) |
| [T-097](upkeep.md) | P3 | S | M5 | chore | open | Ports and snapshot moves of the drift of `pod-core` and `pod-services` |
| [T-098](upkeep.md) | P1 | M | M5 | chore | open | The asks of cfb (GitHub #35) and githome (GitHub #36) go to their owners |
