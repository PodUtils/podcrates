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

**Milestone.** `M0` to `M8`, as in `TODO/ROADMAP.md`: M0 Foundation, M1
Transport, M2 Relay, M3 Adapters, M4 Roads, M5 Upkeep, M6 The crates of
podvm, M7 The widened scope of podvm, M8 The fleet.

**Category.** `defect`, `feature`, `measurement`, `release`, `chore`,
`research` or `docs`.

## Entries

**322 entries: 92 open, 5 partial, 5 blocked, 220 done.**

| Priority | open | partial | blocked | done | total |
| --- | --- | --- | --- | --- | --- |
| P0 | 0 | 0 | 0 | 2 | 2 |
| P1 | 18 | 2 | 0 | 63 | 83 |
| P2 | 38 | 1 | 5 | 110 | 154 |
| P3 | 36 | 2 | 0 | 45 | 83 |
| **All** | 92 | 5 | 5 | 220 | 322 |

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
| [T-016](transport.md) | P1 | M | M1 | feature | done | `pod-ws`: the WebSocket client and server, HTTP/2 and cleartext roads |
| [T-017](transport.md) | P2 | S | M1 | chore | done | The fuzz workspace and its weekly workflow |
| [T-018](relay.md) | P1 | M | M2 | research | done | One QUIC stack: quinn against noq, measured |
| [T-019](relay.md) | P1 | M | M2 | feature | done | `pod-quic`: the QUIC endpoint, the probe and QUIC over datagrams |
| [T-020](relay.md) | P1 | L | M2 | feature | done | `pod-session`: the resumable layer and UDP in a stream |
| [T-021](relay.md) | P1 | M | M2 | feature | done | `pod-e2e`: the end-to-end channel (Noise XX) |
| [T-022](relay.md) | P1 | S | M2 | feature | done | `pod-wormhole`: the mailbox client, SPAKE2 and the code words |
| [T-023](relay.md) | P1 | M | M2 | feature | done | `pod-netstack`: the userspace TCP and UDP stack on smoltcp 0.14 |
| [T-024](relay.md) | P2 | S | M2 | feature | done | `pod-wg`: the WireGuard initiator |
| [T-025](relay.md) | P1 | L | M2 | feature | done | `pod-relay`: the relay client, the pool, the roads and the reverse road (the base of podssh) |
| [T-026](relay.md) | P1 | M | M2 | feature | done | `pod-relay`: podnet's relay client and its plain reverse mode |
| [T-027](adapters.md) | P2 | S | M3 | feature | open | `pod-transit`: the transit relay carrier |
| [T-028](adapters.md) | P1 | L | M3 | feature | partial | `pod-warp`: the WARP adapter |
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
| [T-047](foundation.md) | P2 | S | M2 | chore | done | The weekly watch of the relay contract, in `pod-relay` |
| [T-048](transport.md) | P1 | S | M1 | research | done | `pod-services`: the inventory of the relays and services of other parties in podssh and podnet |
| [T-049](transport.md) | P1 | L | M1 | feature | done | `pod-services`: the schema, the data of each service and the library |
| [T-050](transport.md) | P1 | L | M1 | feature | done | `pod-services`: the watches, their workflows and the measurement scripts |
| [T-051](foundation.md) | P1 | S | M0 | chore | done | The fallback channel between the sessions, tested (GitHub #2) |
| [T-052](transport.md) | P1 | S | M1 | defect | done | The tests of a closed port of `pod-proxy` race with the other tests |
| [T-053](foundation.md) | P2 | S | M6 | chore | done | The names of the crates of podvm on crates.io (GitHub #1) |
| [T-054](transport.md) | P2 | S | M1 | research | done | `pod-services`: the inventory of podvm |
| [T-055](podvm.md) | P2 | M | M6 | feature | done | `pod-hostprobe`: the host probes of podvm |
| [T-056](podvm.md) | P2 | M | M6 | feature | done | `pod-oci`: the OCI registry client of podvm |
| [T-057](podvm.md) | P2 | M | M6 | feature | done | `pod-extract`: the rootless apply of a layer |
| [T-058](podvm.md) | P2 | M | M6 | feature | done | `pod-rootfs`: the completion of a root file system |
| [T-059](podvm.md) | P2 | M | M6 | feature | done | `pod-enter`: the entry of a payload |
| [T-060](podvm.md) | P2 | M | M6 | feature | done | `pod-supervise`: detached payloads |
| [T-061](podvm.md) | P2 | M | M6 | feature | done | `pod-guest`: a disposable QEMU guest |
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
| [T-082](transport.md) | P2 | S | M1 | defect | done | `pod-proxy`: the `Host` header of an IPv6 literal on port 443 (GitHub #20) |
| [T-083](transport.md) | P2 | M | M1 | feature | done | `pod-proxy`: the SOCKS5 datagram path (UDP ASSOCIATE and command 0x83) (GitHub #21) |
| [T-084](foundation.md) | P3 | S | M0 | docs | done | The record of `SOURCES` names a git blob id (GitHub #22) |
| [T-085](foundation.md) | P2 | S | M0 | defect | done | `check-sources.py`: the default branch of a source repository (GitHub #23) |
| [T-086](roads.md) | P2 | L | M4 | feature | done | Import `pod-gsr` and `hackerschoice-gsr.toml` (GitHub #24) |
| [T-087](roads.md) | P2 | S | M4 | feature | done | `pod_gsr::Stream` records how it ended (GitHub #25) |
| [T-088](roads.md) | P2 | M | M4 | feature | done | `pod-gsr`: the synchronous face (GitHub #26) |
| [T-089](transport.md) | P2 | S | M1 | defect | done | `pod-services`: `tailscale-derp` watches the hosts and holds the four missing servers (GitHub #27) |
| [T-090](transport.md) | P2 | S | M1 | defect | done | `pod-services`: `ajam-relay` watches its seven hosts and the pool (GitHub #28) |
| [T-091](transport.md) | P2 | S | M1 | defect | done | `pod-dns`: `Way::lookup` refuses a name that is not DNS-safe (GitHub #29) |
| [T-092](foundation.md) | P2 | M | M0 | chore | done | The live workflow `live.yml` (GitHub #30) |
| [T-093](foundation.md) | P3 | S | M0 | docs | done | README: the state and the table of crates (GitHub #31) |
| [T-094](foundation.md) | P3 | S | M0 | docs | done | `docs/design.md`: the repository table matches the tree (GitHub #32) |
| [T-095](foundation.md) | P2 | S | M0 | chore | done | The static probe turns on `pod-proxy/blocking` and the builds with no tokio (GitHub #33) |
| [T-096](transport.md) | P3 | S | M1 | docs | done | `pod-proxy` README: the six variables of `Rules::http_proxy_vars` (GitHub #34) |
| [T-097](upkeep.md) | P3 | S | M5 | chore | done | Ports and snapshot moves of the drift of `pod-core` and `pod-services` |
| [T-098](upkeep.md) | P1 | M | M5 | chore | done | The asks of cfb (GitHub #35) and githome (GitHub #36) go to their owners |
| [T-099](fleet.md) | P1 | L | M8 | feature | done | `pod-broker`: the egress broker: per-job policy, one resolve, the upstream chain, the audit and the drop (GitHub #35, #36) |
| [T-100](fleet.md) | P1 | L | M8 | feature | open | `pod-broker`: TLS end with a CA for each job, placeholder credentials and the scrub of answers (GitHub #35, #36) |
| [T-101](foundation.md) | P1 | S | M1 | feature | done | `pod-core`: the classes Revoked, Policy and Unsupported, one exit code for each class, one JSON log schema (GitHub #35) |
| [T-102](fleet.md) | P1 | M | M8 | feature | open | `pod-unit`: hardened service units for systemd, launchd and Windows, with no secret in a unit (GitHub #35) |
| [T-103](fleet.md) | P1 | M | M8 | feature | open | `pod-control`: one JSON control API on a Unix socket or a named pipe (GitHub #35, #36) |
| [T-104](fleet.md) | P1 | M | M8 | feature | open | `pod-release`: the release index, its writer for CI and its verifier (GitHub #35, #36) |
| [T-105](foundation.md) | P1 | M | M8 | feature | open | `pod-todo`: an acceptance registry of ask ids and their tests (GitHub #35) |
| [T-106](fleet.md) | P2 | M | M8 | feature | open | `pod-testkit`: the stand-ins of the edge, the relay and the WARP API as binaries for a consumer's CI (GitHub #35, #36) |
| [T-107](podvm.md) | P2 | S | M6 | feature | open | `pod-hostprobe`: a JSON report with a published schema (GitHub #35) |
| [T-108](podvm.md) | P1 | M | M7 | feature | done | `pod-confine`: Landlock, a seccomp deny filter, no_new_privs and rlimits from one profile (GitHub #35) |
| [T-109](podvm.md) | P2 | S | M7 | research | done | Research: the hold of each connect of a payload, from the `podnet-netns` of podnet, for podvm (GitHub #35, #36) |
| [T-110](relay.md) | P1 | M | M2 | feature | open | `pod-session`: the held link to a server that is not a podssh relay (GitHub #35) |
| [T-111](relay.md) | P2 | M | M2 | feature | open | `pod-relay`: a lease header, Access headers from files on each WebSocket road, a heartbeat that a relay offers (GitHub #35) |
| [T-112](transport.md) | P1 | S | M1 | feature | open | `pod-services`: the party of each service as a filter, and the effective set as JSON (GitHub #35) |
| [T-113](adapters.md) | P1 | L | M3 | feature | open | `pod-cloudflare`: the connector of the fleet, part 1: remote config, readiness, drain, revoked token, Access JWT, colos (GitHub #35) |
| [T-114](adapters.md) | P2 | L | M3 | feature | open | `pod-cloudflare`: the connector of the fleet, part 2: several tunnels, pools, a Workers runtime ingress, VPC allow list, rate and meter, post-quantum (GitHub #35) |
| [T-115](adapters.md) | P2 | L | M3 | feature | open | `pod-warp`: the rate of `quic+warp`, team enrolment with a service token, a Mesh node in user space, posture (GitHub #35) |
| [T-116](adapters.md) | P2 | M | M3 | measurement | open | The live and bench workflows of the tunnel with a test account (GitHub #35) |
| [T-117](roads.md) | P2 | M | M4 | feature | open | `pod-egress`: the Access TCP client as a local listener (GitHub #35) |
| [T-118](foundation.md) | P2 | M | M5 | chore | open | Supply chain: an SBOM for each release and a planted advisory that fails `deny.yml` (GitHub #35) |
| [T-119](podvm.md) | P3 | L | M7 | feature | open | `pod-netlab`: the CI matrix of restricted hosts (GitHub #35) |
| [T-120](relay.md) | P2 | M | M2 | docs | open | The wire documents of the layer, the reverse dialect and the role's extensions, with test vectors (GitHub #35) |
| [T-121](relay.md) | P2 | M | M2 | feature | open | `pod-relay`: renew a pair (PodUtils/podnet#413) |
| [T-122](relay.md) | P2 | S | M2 | feature | done | `pod-relay`: a health memory of the relay hosts (PodUtils/podnet#413) |
| [T-123](relay.md) | P2 | S | M2 | feature | done | `pod-relay`: the session budget from the pool document (PodUtils/podnet#413) |
| [T-124](relay.md) | P2 | M | M2 | feature | blocked | `pod-relay`: failover on the reverse road (PodUtils/podnet#413) |
| [T-125](transport.md) | P2 | S | M1 | feature | done | `pod-dns`: one kept connection for each DoH server (PodUtils/podnet#413) |
| [T-126](transport.md) | P3 | S | M1 | feature | done | `pod-dns`: a pin table that the caller can replace (PodUtils/podnet#413) |
| [T-127](transport.md) | P3 | S | M1 | feature | done | `pod-services`: the file host of PyPI (PodUtils/podnet#413) |
| [T-128](podvm.md) | P1 | S | M6 | defect | done | `pod-hostprobe`: `waitid` and `ppoll` read the kernel layouts of a 64-bit target on a 32-bit target |
| [T-129](roads.md) | P1 | S | M4 | defect | done | `pod-gsr`: the tests over `AF_UNIX` do not build on Windows |
| [T-130](transport.md) | P2 | M | M1 | feature | done | `pod-services`: the public Wisp servers, a kind `wisp-public` (PodUtils/podssh#175) |
| [T-131](roads.md) | P1 | M | M4 | defect | open | `pod-gsr`: the handshake tests crash on Windows ARM64 |
| [T-132](podvm.md) | P1 | S | M6 | defect | done | `pod-hostprobe`: the id calls are the 16-bit forms on x86 and arm |
| [T-133](transport.md) | P3 | M | M1 | feature | done | `pod-services`: the public proxy lists and nodes, the kinds `proxy-list` and `proxy-node` (PodUtils/podssh#177) |
| [T-134](podvm.md) | P1 | S | M6 | defect | done | `pod-hostprobe`, `pod-enter`: the open flags are the x86 values on arm, aarch64 and powerpc |
| [T-135](transport.md) | P3 | S | M1 | feature | done | `pod-services`: Pinggy's SSH inside TLS, its host key and its facts (PodUtils/podssh#178) |
| [T-136](podvm.md) | P2 | S | M6 | chore | done | Port of PodUtils/podvm@T-1646 into `pod-enter`: the library path of the interposer |
| [T-137](podvm.md) | P3 | S | M6 | chore | done | `pod-enter`: the notes of podvm's PodUtils/podvm@T-1649 and PodUtils/podvm@T-1650 (PodUtils/podvm#220) |
| [T-138](podvm.md) | P3 | M | M6 | chore | done | `pod-guest`: the run of a real guest in CI |
| [T-139](podvm.md) | P1 | M | M6 | defect | done | `pod-enter`: classify and stage a payload over 128 MiB with bounded memory (PodUtils/podvm#222) |
| [T-140](podvm.md) | P1 | S | M6 | defect | done | `pod-oci`: the test of the host name fails where Rust and OCI say the same word |
| [T-141](podvm.md) | P1 | S | M6 | defect | done | `pod-extract`: port podvm's fix of the whiteout escape and its tests; remake the patch on podvm's main |
| [T-142](podvm.md) | P3 | S | M6 | measurement | done | `pod-oci`: a live pull from a public registry |
| [T-143](foundation.md) | P1 | S | M5 | defect | done | `plants.yml`: four plants are stale and two miss |
| [T-144](podvm.md) | P2 | M | M6 | feature | done | `pod-enter`: the volume pairs in the maps table; the workdir and the first program through a guest-to-host resolver (PodUtils/podvm#224) |
| [T-145](podvm.md) | P2 | S | M6 | feature | done | `pod-hostprobe`: `Statfs::f_flags` (PodUtils/podvm#224) |
| [T-146](fleet.md) | P2 | L | M8 | research | open | `podrunner`: run GitHub CI and other CI on any machine (the plan, GitHub #47) |
| [T-147](podvm.md) | P3 | S | M5 | chore | done | `pod-services`: podvm took `pod-oci`: the SOURCES block of podvm reads the take |
| [T-148](podvm.md) | P2 | S | M6 | defect | done | `pod-hostprobe`: `fstatat` has no fallback to `statx` (loongarch64 before Linux 6.11) |
| [T-149](podvm.md) | P3 | S | M6 | defect | done | `pod-enter`: three tests fail under qemu-user only |
| [T-150](relay.md) | P3 | S | M2 | feature | done | `pod-relay`: a handler target for the relay node (PodUtils/podnet#413) |
| [T-151](transport.md) | P2 | M | M1 | feature | open | `pod-services`: the watch scripts of podssh and podnet that the watches replace leave them |
| [T-152](transport.md) | P3 | M | M1 | feature | done | `pod-services-watch`: `proxy-banner` speaks no VLESS over XHTTP |
| [T-153](transport.md) | P2 | S | M1 | feature | done | `pod-services`: the services `serveo` and `ssh-j` (PodUtils/podssh#182) |
| [T-154](foundation.md) | P2 | S | M5 | defect | done | Timed tests under an emulator: one bound of the test kit and a sweep |
| [T-155](podvm.md) | P3 | S | M6 | feature | done | `pod-enter`: the refusal of a missing `-w` names its volume; the reserved guest paths and the bound of pairs in `Maps::new` (PodUtils/podvm#226) |
| [T-156](transport.md) | P2 | S | M1 | feature | done | `pod-services`: the service `croc` (PodUtils/podssh#183) |
| [T-157](transport.md) | P2 | S | M1 | defect | done | `pod-services`: four public Trojan nodes fail the live check |
| [T-158](transport.md) | P2 | S | M1 | defect | done | `pod-services`: the watch of `croc` upgrades with no relay, and the bridge refuses it |
| [T-159](relay.md) | P2 | S | M2 | defect | done | podnet: two QUIC connections on one WARP socket share one waker of the full send queue |
| [T-160](fleet.md) | P2 | S | M8 | feature | done | `pod-broker`: a listener bound to one job, with no token in the unit (PodUtils/podvm#227) |
| [T-161](fleet.md) | P3 | M | M8 | feature | done | `pod-broker`: send the name to an upstream that takes names only (PodUtils/podvm#227) |
| [T-162](podvm.md) | P2 | M | M8 | feature | done | `pod-supervise`: kill the tree, a wall timeout, rusage and the count of residual processes (PodUtils/podvm#227) |
| [T-163](fleet.md) | P2 | M | M8 | feature | done | Schemas and types: job offer, unit capability, run report and public report (PodUtils/podvm#227) |
| [T-164](transport.md) | P3 | S | M8 | feature | done | `pod-services`: the Actions control-plane hosts and the pin of `actions/runner`, with watches (PodUtils/podvm#227) |
| [T-165](transport.md) | P3 | M | M8 | feature | done | `pod-tls`: mutual TLS with pinned keys (PodUtils/podvm#227) |
| [T-166](podvm.md) | P2 | L | M1 | feature | done | `pod-hold`: the hold of each connect of a payload, from podnet's `podnet-netns` (PodUtils/podnet#422) |
| [T-167](relay.md) | P2 | S | M2 | defect | done | `pod-session`: a link meets its cap under qemu-user in `a_session_moves_before_each_cap` |
| [T-168](upkeep.md) | P3 | S | M5 | defect | done | Tests that bind a Unix socket under the temporary directory fail when its path is long |
| [T-169](podvm.md) | P2 | M | M1 | feature | done | `pod-hold`: the dial state of a held `connect` of the seccomp hold, from podnet's `connect.rs` |
| [T-170](podvm.md) | P2 | M | M1 | feature | done | `pod-hold`: the netns hold of a SYN (dial first, then accept or RST), from podnet's netns router |
| [T-171](transport.md) | P2 | S | M1 | feature | done | `pod-services`: the services `localhost-run`, `localtunnel` and `bore` (PodUtils/podssh#186) |
| [T-172](transport.md) | P3 | S | M2 | feature | done | `pod-session`: `Rewound::new`, a public constructor (PodUtils/podssh#186) |
| [T-173](podvm.md) | P2 | M | M6 | feature | done | Fuzz targets of four parsers of untrusted input: `pod-oci` (two), `pod-extract`, `pod-enter` (PodUtils/podvm#241) |
| [T-174](transport.md) | P3 | S | M1 | feature | done | `pod-services`: a public Trojan node of the lists that passes the live check |
| [T-175](podvm.md) | P1 | S | M6 | defect | done | `pod-supervise`: private modes for the table, the logs and each container directory (PodUtils/podvm#244) |
| [T-176](podvm.md) | P1 | M | M6 | feature | done | `pod-enter`: resource limits and `no_new_privs` in the child before the exec (PodUtils/podvm#245) |
| [T-177](fleet.md) | P2 | S | M8 | defect | done | One rule for the version field of each schema of `schemas/`: the log line and the broker policy |
| [T-178](fleet.md) | P2 | S | M8 | defect | done | `pod-job`: the `policy` of an offer is a `broker-policy.v1` policy |
| [T-179](fleet.md) | P2 | M | M7 | feature | done | `pod-incus`: a client of the incus REST API (the owner's name, 2026-11-11) |
| [T-180](fleet.md) | P3 | M | M8 | feature | open | `pod-mask`: the confidential execution of pmask as a crate (the owner's name, 2026-11-11) |
| [T-181](transport.md) | P3 | S | M1 | feature | done | `pod-services`: the host key of `ssh-j` as a fact (podssh's message, 2026-11-11) |
| [T-182](podvm.md) | P2 | S | M6 | defect | done | `pod-enter`: `lib_dirs` takes `/usr/local/lib` and the image's loader configuration (PodUtils/podvm#247) |
| [T-183](fleet.md) | P2 | S | M8 | defect | done | `pod-broker`: a raw byte that is not visible ASCII in a path is read as Latin-1 |
| [T-184](relay.md) | P3 | S | M1 | defect | done | `pod-wormhole`: the words of a mailbox can end with a space |
| [T-185](transport.md) | P3 | S | M1 | feature | done | `pod-services`: the host key of `localhost-run` as a fact (podssh's message, 2026-11-11) |
| [T-186](transport.md) | P3 | S | M1 | feature | done | `pod-services`: the service `sandhole`, the demo of Sandhole (podssh's message, 2026-11-11) |
| [T-187](transport.md) | P3 | S | M1 | feature | done | `pod-services`: the kind `bare-node` and the node `openbare-xyz` (podssh's message, 2026-11-11) |
| [T-188](relay.md) | P2 | S | M2 | measurement | done | `pod-netstack`: Reno and CUBIC of smoltcp 0.14 grow by one segment for each round trip on a link that gives one ACK for each round trip |
| [T-189](transport.md) | P3 | S | M1 | feature | open | `pod-proxy`: a TCP congestion controller on request for each dialed socket, read back (PodUtils/podnet#437) |
| [T-190](podvm.md) | P3 | M | M6 | feature | open | `pod-oci`: one large blob as parallel ranges, kept only by the digest of the whole blob (PodUtils/podnet#437) |
| [T-191](fleet.md) | P2 | M | M8 | feature | open | `pod-broker`: a listener that takes a plain request in origin form and opens verified TLS to its `Host` (PodUtils/podnet#437) |
| [T-192](foundation.md) | P3 | M | M1 | feature | open | `pod-core`: one replace of a private file with a durable commit and three outcomes (PodUtils/podnet#437) |
| [T-193](foundation.md) | P3 | S | M1 | feature | open | `pod-core`: a bounded tail of a log that starts at a line (PodUtils/podnet#437) |
| [T-194](fleet.md) | P2 | S | M8 | feature | open | `pod-broker`: a hash of the policy in the load answer, the list answer and each audit line (PodUtils/podnet#437) |
| [T-195](foundation.md) | P2 | S | M1 | defect | done | `pod-core`: two threads that store one private file at once share one temporary name |
| [T-196](fleet.md) | P2 | S | M8 | defect | done | `pod-broker`: `bind_unix` leaves a window with the mode of the umask (PodUtils/podnet#444) |
| [T-197](relay.md) | P3 | S | M2 | measurement | open | `pod-netstack`: the reactor gives one ACK for the segments that one poll takes |
| [T-198](transport.md) | P3 | S | M1 | feature | done | `pod-services`: the WebSocket of `openbare-xyz` fails (podssh's messages, 2026-11-11) |
| [T-199](upkeep.md) | P3 | S | M5 | chore | open | `pod-e2e`: snow on the line of `chacha20poly1305` 0.11 |
| [T-200](transport.md) | P3 | S | M1 | feature | open | `pod-services`: a public Bare node whose WebSocket passes (PodUtils/podssh@T-740) |
| [T-201](podvm.md) | P3 | M | M1 | feature | done | `pod-hold`: the veth wire of the netns hold when the namespace has no `/dev/net/tun` (PodUtils/podnet#446) |
| [T-202](podvm.md) | P3 | S | M1 | measurement | done | `pod-supervise`: a payload that kills its launcher leaves a `Dead` record with no count of 0 (podrunner's T-063) |
| [T-203](transport.md) | P3 | S | M1 | feature | done | `pod-services`: the TCP relay of ZeroTier (PodUtils/podssh#190) |
| [T-204](relay.md) | P2 | S | M2 | defect | open | `pod-session`: `a_session_moves_while_both_ends_send_at_full_speed` lost its link one time in a run of the workspace's tests |
| [T-205](podvm.md) | P2 | S | M1 | defect | done | `pod-supervise`, `pod-enter`: a payload outlives its launcher; no code sets `PR_SET_PDEATHSIG` |
| [T-206](podvm.md) | P2 | M | M6 | feature | done | `pod-guest`: the module set of a guest kernel from its modloop (PodUtils/podvm#260) |
| [T-207](podvm.md) | P3 | L | M6 | feature | partial | `pod-netlab`: the network lab of podvm's guest (PodUtils/podvm#260, issue 178) |
| [T-208](podvm.md) | P1 | S | M6 | defect | done | `pod-extract`: a root directory entry with an empty name refuses its layer (PodUtils/podvm#261) |
| [T-209](podvm.md) | P2 | S | M7 | feature | done | `pod-confine`: the Landlock scopes (signals, abstract Unix sockets) and UDP stay open to a confined payload |
| [T-210](upkeep.md) | P2 | S | M5 | chore | done | A plant whose file changed goes stale, and only `plants.yml` finds it |
| [T-211](transport.md) | P2 | S | M1 | feature | done | `pod-services`: gcr.io, mirror.gcr.io and the NIC modules of podvm's guest (PodUtils/podvm#263) |
| [T-212](relay.md) | P2 | M | M2 | chore | open | `pod-relay`: the lines for the user as events, and the words of podssh's commands as the caller's |
| [T-213](podvm.md) | P2 | M | M7 | defect | done | `pod-confine`: under a TCP list or a closed UDP, a payload still makes sockets that Landlock and the filter do not see |
| [T-214](relay.md) | P2 | L | M2 | feature | open | `pod-wormhole`, `pod-transit`: the file transfer of magic-wormhole (PodUtils/podnet#450) |
| [T-215](relay.md) | P3 | M | M2 | feature | open | `pod-wormhole`: the mailbox server (PodUtils/podnet#450) |
| [T-216](podvm.md) | P2 | S | M6 | defect | done | `pod-enter`: `a_descriptor_passed_onto_its_own_number_reaches_the_payload` failed one time in a run of the workspace's tests |
| [T-217](roads.md) | P2 | M | M4 | research | blocked | Research: the base of `pod-git`, a git server in Rust with no installed git (PodUtils/podnet#453) |
| [T-218](roads.md) | P2 | L | M4 | feature | blocked | `pod-git`: upload-pack (v0, v1, v2) and the pack writer (PodUtils/podnet#453) |
| [T-219](roads.md) | P2 | L | M4 | feature | blocked | `pod-git`: receive-pack, the policy and the hook (PodUtils/podnet#453) |
| [T-220](roads.md) | P2 | M | M4 | feature | blocked | `pod-git`: stateless smart HTTP, the repository calls, the budgets and podnet's trial (PodUtils/podnet#453) |
| [T-221](podvm.md) | P2 | S | M7 | defect | done | `pod-confine`: a payload that runs as root keeps its capabilities, and reads the environment of its unconfined ancestors past the Landlock domain |
| [T-222](relay.md) | P1 | M | M2 | feature | done | `pod-netstack`: any IP, a TCP listener and a UDP socket on any address, and the checksums of a link device (PodUtils/podnet#454) |
| [T-223](relay.md) | P3 | S | M2 | defect | done | `pod-netstack`: a SYN sent again can reach a newer listen socket of the pool of a listener |
| [T-224](transport.md) | P2 | M | M1 | feature | done | `pod-services`: the facts of an SSH forward relay, and a reader of its banner across chunks (PodUtils/podnet#455) |
| [T-225](transport.md) | P1 | M | M1 | feature | done | `pod-services-watch`: a watch that reads the banner of each SSH forward relay and holds its URL rule |
| [T-226](fleet.md) | P2 | S | M8 | feature | done | `pod-job`: the time to the first step, the job images and the exit reason `limit` in a report (podrunner's T-075) |
| [T-227](podvm.md) | P1 | M | M6 | defect | done | `pod-enter`: a rule of `Plan.confine` misses a path that the child mounts after the compile (PodUtils/podvm#267) |
| [T-228](relay.md) | P2 | S | M2 | chore | done | `pod-relay`: the live tests of the relay client in `live.yml` |
| [T-229](relay.md) | P2 | M | M2 | defect | done | `pod-relay`: a test of the crate for each of six plants of podssh whose tests stay in podssh |
| [T-230](relay.md) | P1 | S | M2 | defect | done | `pod-netstack`: a listener frees its listen sockets, its port and its address at its close and at its drop (PodUtils/podnet#457) |
| [T-231](relay.md) | P1 | S | M2 | defect | done | `pod-netstack`: a dropped or closed stream sends its queued bytes, then the FIN, and frees its socket (PodUtils/podnet#457) |
| [T-232](relay.md) | P1 | M | M2 | defect | done | `pod-netstack`: a burst of SYNs to one port gets no RST: a listener adds a listen socket for each SYN (PodUtils/podnet#457) |
| [T-233](transport.md) | P2 | S | M1 | feature | done | `pod-services`: the registry mcr.microsoft.com of podvm's profile playwright-chromium (PodUtils/podvm#269) |
| [T-234](relay.md) | P2 | M | M2 | feature | done | `pod-relay`: a relay host as an IPv6 address in brackets, and plain `ws://` to a loopback relay on the forward road |
| [T-240](podvm.md) | P2 | S | M6 | defect | done | `pod-enter`: `release_tmpfs` leaves a busy tmpfs mounted and its directory in the store |
| [T-241](podvm.md) | P3 | M | M7 | feature | open | `pod-confine`: the permissions and the quiet flags of Landlock ABI 11 and 12 |
| [T-242](fleet.md) | P2 | S | M8 | chore | done | Docs: the names of the renames of 2026-11-12 (`pods`, `podstat`, `podforge`) and the stale text of `pod-git` and of the podrunner plan (GitHub #78) |
| [T-243](upkeep.md) | P3 | S | M5 | chore | done | Dependabot of 2026-11-12: trufflehog 3.99.8, toml 1.2.1, and foreign-types 0.5.0 declined (GitHub #79, #80, #81) |
| [T-244](transport.md) | P2 | S | M1 | feature | done | `pod-services`: the public TCP relays of Tox on port 443, with the key of each node and a watch of the list (PodUtils/podssh#196) |
| [T-245](relay.md) | P2 | S | M2 | defect | done | `pod-ws`, `pod-relay`: the `Host` of an HTTPS request to an IPv6 relay on port 443 has no brackets |
| [T-246](relay.md) | P3 | S | M2 | defect | done | `pod-relay`: two relay hosts can have one name of a cache file |
| [T-250](podvm.md) | P2 | S | M6 | defect | done | `pod-supervise`: the test of a payload that kills its launcher raced the launcher's word of the start |
| [T-251](fleet.md) | P3 | L | M8 | feature | done | `pod-actions-expr` and `pod-actions-workflow`: the expressions and the workflow files of GitHub Actions, for podrunner's local mode (GitHub #84) |
| [T-252](transport.md) | P3 | S | M1 | chore | done | podnet's change of `src/doctor.rs` for the lanes of A-C-10 (PodUtils/podnet#462) |
| [T-253](transport.md) | P2 | S | M1 | feature | done | `pod-services`: a watch of the relay contract that `pod-relay` implements (PodUtils/podnet#463) |
| [T-254](transport.md) | P2 | S | M1 | feature | done | `pod-services`: the npm archive of workerd for podvm's profile workerd-test (PodUtils/podvm#274) |
| [T-255](transport.md) | P2 | S | M1 | feature | done | `pod-services`: the release archive of wasmtime for podvm's profile wasmtime-wasi (PodUtils/podvm#275) |
| [T-256](transport.md) | P2 | S | M1 | feature | done | `pod-services`: the public wss:// peers of Yggdrasil on port 443 (PodUtils/podssh#203) |
| [T-257](relay.md) | P2 | S | M2 | feature | done | `pod-netstack`: UDP counters for a datagram lost at a slow app, at a full queue or buffer, and over the size that the stack carries (PodUtils/podnet#464) |
| [T-258](relay.md) | P2 | S | M2 | defect | done | `pod-netstack`: a UDP send over what the stack carries is `Ok` and lost; the IPv4 fragmentation of smoltcp as a feature (PodUtils/podnet#464) |
| [T-259](relay.md) | P1 | S | M2 | defect | done | `pod-netstack`: the reactor loses a datagram that comes while the channel to the app is full (PodUtils/podnet#464) |
| [T-260](transport.md) | P2 | S | M1 | feature | done | `pod-services`: the public bootstrap nodes of HyperDHT (PodUtils/podssh#204) |
| [T-261](relay.md) | P2 | M | M2 | feature | done | `pod-wormhole`: a meeting point of the pairing over public MQTT brokers on 443 (PodUtils/podssh#205) |
| [T-262](podvm.md) | P2 | S | M7 | defect | done | `pod-confine`: a TCP list is refused on a target with `socketcall(2)` (PodUtils/podvm#276) |
| [T-263](relay.md) | P3 | S | M2 | defect | done | `pod-wormhole`: no workflow runs the tests against magic-wormhole's own mailbox server and Python library |
| [T-266](podvm.md) | P1 | S | M6 | defect | done | `pod-guest`: the provision writes its image with `qemu-img convert`; `qemu-img commit` takes no output file (PodUtils/podvm#277) |
| [T-267](transport.md) | P2 | M | M1 | feature | done | `pod-services-watch`: `forward-url` proves the URL of the tunnel with a request through it |
| [T-271](transport.md) | P2 | S | M1 | chore | done | `pod-services`: the VLESS node `node-xhttp-78-17-39-129` leaves (PodUtils/podssh#207) |
| [T-272](transport.md) | P2 | S | M1 | defect | done | `pod-services-watch`: `forward-url` tries keyboard-interactive, which Serveo's anonymous tunnel takes |
| [T-276](transport.md) | P2 | S | M1 | feature | done | `pod-services`: the public Syncthing relays on port 443, with their device IDs (PodUtils/podssh#209) |
| [T-277](relay.md) | P2 | S | M2 | feature | done | `pod-relay`: the runs of a code meet at the MQTT brokers after the mailboxes (PodUtils/podssh#210) |
| [T-278](transport.md) | P3 | S | M1 | feature | done | `pod-identity`: a display form of a fingerprint: base32 in five groups with a checksum of 4 bytes (PodUtils/podnet#479) |
| [T-279](relay.md) | P3 | S | M2 | feature | done | `pod-session`: the resume window and the single use of a resume token as settings (PodUtils/podnet#479) |
| [T-280](fleet.md) | P3 | M | M8 | measurement | open | `pod-actions-expr` and `pod-actions-workflow` against GitHub: a live run of the conformance vectors |
| [T-290](relay.md) | P3 | M | M2 | feature | open | `pod-relay`: the resumable layer and the end-to-end channel as a feature, so that a plain client builds without them |
| [T-300](relay.md) | P3 | L | M2 | feature | partial | `pod-link`: the stream multiplexer with credit flow control, the carriers and the ARQ, as a shared crate (PodUtils/podnet#479) |
| [T-301](relay.md) | P3 | S | M2 | feature | open | `pod-e2e`: the handshake hash and the transport keys as an interface (PodUtils/podnet#479) |
| [T-302](roads.md) | P3 | M | M4 | feature | open | `pod-egress`: the accept side of the SIP004 (Shadowsocks AEAD) and Trojan codecs (PodUtils/podnet#479) |
| [T-303](relay.md) | P3 | S | M2 | chore | done | `pod-relay`: research: the relay contract and an exchange of NAT traversal candidates between two peers (PodUtils/podnet#479) |
| [T-304](transport.md) | P3 | S | M1 | feature | open | `pod-tls`: a verifier that pins an Ed25519 identity of the peer (PodUtils/podnet#479) |
| [T-305](transport.md) | P3 | S | M1 | feature | open | `pod-dns`: a reader of TXT and CNAME answers with EDNS0 and the TC bit, for DNS carriers (PodUtils/podnet#479) |
| [T-306](upkeep.md) | P1 | S | M5 | chore | done | `pod-services`: six podvm files of its block changed after `2be31b2` (`upstream.yml` red since 2026-11-10) |
| [T-307](upkeep.md) | P1 | S | M5 | defect | open | The nightly at `0edd703` is red: a test of `pod-guest` with no Linux gate, the TCP-list tests on targets with `socketcall(2)`, a bound of 1 s under riscv64 |
| [T-308](upkeep.md) | P1 | S | M5 | chore | done | `pod-core` and `pod-relay`: three podssh files of their blocks changed (`upstream.yml` at `c553207`) |
| [T-309](fleet.md) | P2 | S | M8 | feature | done | `pod-actions-workflow`: a `Run` that owns its workflow (podrunner's ask) |
| [T-310](relay.md) | P3 | L | M2 | feature | open | `pod-link`: a selective-repeat ARQ for a carrier that loses frames, with a test at 3 % loss (PodUtils/podnet#480) |
| [T-311](relay.md) | P3 | M | M2 | feature | done | `pod-link`: a test of a link over a resumable session of `pod-session` across the loss of its carrier (PodUtils/podnet#480) |
| [T-312](foundation.md) | P2 | S | M0 | defect | open | `pod-todo`: a code fence that is not closed in a file of the record is an error by name |
| [T-320](podvm.md) | P1 | S | M6 | defect | done | `pod-extract`: a correct plain-tar layer is refused: the hash stops at the end marker (PodUtils/podvm#281) |
| [T-321](podvm.md) | P1 | S | M6 | defect | done | `pod-enter`: `libc_beside` misses the aarch64 glibc of a Debian image: the search is depth-first from the end (PodUtils/podvm#282) |
| [T-322](fleet.md) | P2 | S | M8 | feature | done | `pod-actions-expr`: a public view of the parts of a `Template` (podrunner's ask) |
| [T-323](upkeep.md) | P1 | S | M5 | defect | done | `services.yml` at `613058c` is red: Serveo's banner names its console, a text watch of shiftr names an endpoint, no RDAP for `.me`, and openbare.xyz in 66 days |
| [T-324](upkeep.md) | P2 | M | M5 | defect | done | `pod-services-watch`: no offline test checks the arguments of each watch of the data |
| [T-325](upkeep.md) | P1 | S | M5 | defect | done | The nightly at `613058c` is red: the stage of a socket does not fit on macOS, and four bounds under riscv64 are not scaled |
| [T-326](podvm.md) | P1 | L | M6 | feature | done | `pod-relay`: the trial of the blocking facade in podvm, for PodUtils/podvm@T-1678 (PodUtils/podvm#283) |
| [T-327](relay.md) | P1 | M | M5 | defect | done | `pod-relay`: podnet's `relay connect` hangs on a runner with a direct egress after its take (PodUtils/podnet#502) |
| [T-328](podvm.md) | P2 | S | M6 | research | open | GNU `tar -xz -C DIR` fails with "Cannot open" right after `mkdir -p DIR` in a podvm unit (podrunner's note) |
| [T-329](transport.md) | P2 | S | M1 | feature | done | `pod-services`: the services `smee-io` and `hostc` (PodUtils/podssh#213) |
| [T-330](podvm.md) | P2 | M | M6 | feature | open | `pod-relay`: podvm's members that make, ask about and stop a pair, in place of the curl of its drives |
| [T-335](adapters.md) | P1 | M | M3 | feature | open | `pod-warp`: podnet's stack of `ip.rs` onto `pod-netstack` (congestion control, TCP burst, link counters, the check of the data path) |
| [T-336](adapters.md) | P1 | M | M3 | feature | open | `pod-warp`: the relay leg and the race of the direct path and the relay, with the leg file |
| [T-337](adapters.md) | P1 | L | M3 | feature | open | `pod-warp`: MASQUE over HTTP/3, the WireGuard leg and the carried device |
| [T-338](adapters.md) | P1 | L | M3 | feature | open | `pod-warp`: the proxy, the forward, nc and the L4 door, with loopback tests |
| [T-339](adapters.md) | P1 | L | M3 | feature | open | `pod-warp`: the live session as a value, TUN and routes, the live step in `live.yml`, and the adoption trials of podnet and podssh |
| [T-345](podvm.md) | P3 | M | M6 | feature | open | `pod-netlab`: the probe program of the guest, which measures each probe of issue 178 |
| [T-346](podvm.md) | P3 | M | M6 | feature | open | `pod-netlab`: the world's services (`world.py`) over `pod-dns` and `pod-proxy` |
| [T-347](podvm.md) | P3 | M | M6 | feature | open | `pod-netlab`: each offline scenario on a real kernel as root, with the planted flush |
| [T-348](podvm.md) | P3 | M | M6 | feature | open | `pod-netlab`: the VPN and the live scenarios of issue 178 (12 of 39) |
| [T-349](podvm.md) | P3 | L | M7 | feature | open | `pod-netlab`: a lab on a user-space stack, and a lab with more than one guest |
| [T-350](transport.md) | P1 | M | M5 | defect | done | `pod-dns`: the daily DoH test to the public resolvers fails on Quad9's connect from a runner; CoreDNS on the loopback takes its place |
| [T-351](upkeep.md) | P1 | M | M5 | defect | partial | `services.yml`: a host that gives no answer to one runner fails the daily run |
| [T-352](upkeep.md) | P2 | S | M5 | chore | open | `live.yml` runs each week, and no step of `ci.yml` reaches a host of another party |
| [T-353](podvm.md) | P2 | M | M5 | feature | open | `pod-oci`: a pull from the CNCF distribution registry on the loopback; the live pull from ECR Public each week |
| [T-354](relay.md) | P2 | S | M5 | chore | open | `pod-wormhole`, `pod-relay`: magic-wormhole's mailbox server and transit relay from a hashed requirements file, on each push |
| [T-355](upkeep.md) | P2 | M | M5 | chore | open | The downloads of each job come from a cache, and the tools from their release binaries |
| [T-356](transport.md) | P3 | M | M5 | feature | open | `pod-services-watch`: `forward-url` against sish and sandhole on the loopback |
| [T-357](roads.md) | P3 | S | M5 | feature | open | `pod-gsr`: gs-netcat as the other end, through the stand-in of the relay |
| [T-358](transport.md) | P3 | S | M5 | feature | open | `pod-proxy`: the STUN client against coturn on the loopback |
| [T-359](adapters.md) | P2 | M | M3 | feature | open | `pod-warp`: the stand-ins of each push and one live run each week |
| [T-360](adapters.md) | P2 | M | M3 | feature | open | `pod-cloudflare`: a stand-in edge of each push and one quick tunnel each week |
| [T-361](roads.md) | P3 | M | M4 | feature | open | `pod-relay`: its end-to-end tests against the self-hosted relay of `pod-relay-serve`, then against workerd |
| [T-370](transport.md) | P2 | S | M1 | feature | done | `pod-proxy`: MESSAGE-INTEGRITY, FINGERPRINT and the long-term key in `stun`, for TURN (PodUtils/podssh#214) |
| [T-371](upkeep.md) | P2 | M | M5 | defect | open | `pod-services-watch`: the DNS watches, `stun-binding` and the TLS of the HTTP client give no class "no answer" |
| [T-372](transport.md) | P2 | S | M1 | feature | open | `pod-services`: `github-ssh` names SSH over port 443 (GitHub #95) |
| [T-373](transport.md) | P2 | S | M1 | feature | open | `pod-services`: Sandhole's remote port as a fact, and `Relay::remote_port` (GitHub #97) |
| [T-374](transport.md) | P1 | S | M1 | defect | open | `pod-services`: Serveo's tunnels are under `serveousercontent.com`; the anonymous login (GitHub #98) |
| [T-375](transport.md) | P3 | S | M1 | feature | open | `pod-services`: `tuns-sh`, the hosted SSH tunnel of pico.sh, as data (GitHub #99) |
| [T-376](transport.md) | P3 | S | M1 | feature | open | `pod-services`: QSocket's relay and Portal's public relays as candidates, with their measured limits (GitHub #96) |
| [T-377](transport.md) | P2 | L | M1 | feature | open | `pod-turn`: a TURN client (RFC 8656) over UDP and TLS, on the STUN codec moved out of `pod-proxy` (GitHub #100) |
| [T-378](transport.md) | P3 | L | M1 | feature | open | `pod-devtunnel`: a client of Microsoft Dev Tunnels: the management API and the relay (GitHub #94) |
| [T-380](transport.md) | P2 | M | M1 | feature | open | `pod-turn`: the client against coturn on the loopback, over UDP and TLS |
| [T-381](transport.md) | P3 | M | M1 | feature | open | `pod-turn`: the options of RFC 8489 and RFC 8656 that the client does not use |
