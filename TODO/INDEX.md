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

**800 entries: 147 open, 26 partial, 7 blocked, 620 done.**

| Priority | open | partial | blocked | done | total |
| --- | --- | --- | --- | --- | --- |
| P0 | 0 | 0 | 0 | 2 | 2 |
| P1 | 10 | 8 | 0 | 183 | 201 |
| P2 | 70 | 8 | 4 | 315 | 397 |
| P3 | 67 | 10 | 3 | 120 | 200 |
| **All** | 147 | 26 | 7 | 620 | 800 |

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
| [T-027](adapters.md) | P2 | S | M3 | feature | done | `pod-transit`: the transit relay carrier |
| [T-028](adapters.md) | P1 | L | M3 | feature | partial | `pod-warp`: the WARP adapter |
| [T-029](adapters.md) | P1 | L | M3 | feature | done | `pod-cloudflare`: the edge client, the tunnel and the quick tunnels (the base of podnet) |
| [T-030](adapters.md) | P1 | M | M3 | feature | open | `pod-cloudflare`: podssh's quick tunnels onto the edge client |
| [T-031](roads.md) | P2 | M | M4 | feature | done | `pod-derp`: the DERP client and the DERP carrier of a node |
| [T-032](roads.md) | P3 | S | M4 | feature | done | `pod-piping`: the Piping Server carrier |
| [T-033](roads.md) | P3 | S | M4 | feature | done | `pod-opentunnel`: the OpenTunnel carrier |
| [T-034](roads.md) | P2 | S | M4 | feature | done | `pod-acme`: the ACME client |
| [T-035](roads.md) | P2 | L | M4 | feature | done | `pod-egress`: the other roads out (via, VLESS, wstunnel, Wisp, Shadowsocks and more) |
| [T-036](roads.md) | P2 | L | M4 | feature | open | `pod-relay-serve`: the relay role and its mailbox |
| [T-037](roads.md) | P2 | M | M4 | feature | done | `pod-iroh`: the iroh road |
| [T-038](roads.md) | P3 | S | M4 | feature | done | `pod-tor`: the Tor road through arti |
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
| [T-106](fleet.md) | P2 | M | M8 | feature | partial | `pod-testkit`: the stand-ins of the edge, the relay and the WARP API as binaries for a consumer's CI (GitHub #35, #36) |
| [T-107](podvm.md) | P2 | S | M6 | feature | done | `pod-hostprobe`: a JSON report with a published schema (GitHub #35) |
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
| [T-120](relay.md) | P2 | M | M2 | docs | done | The wire documents of the layer, the reverse dialect and the role's extensions, with test vectors (GitHub #35) |
| [T-121](relay.md) | P2 | M | M2 | feature | open | `pod-relay`: renew a pair (PodUtils/podnet#413) |
| [T-122](relay.md) | P2 | S | M2 | feature | done | `pod-relay`: a health memory of the relay hosts (PodUtils/podnet#413) |
| [T-123](relay.md) | P2 | S | M2 | feature | done | `pod-relay`: the session budget from the pool document (PodUtils/podnet#413) |
| [T-124](relay.md) | P2 | M | M2 | feature | open | `pod-relay`: failover on the reverse road (PodUtils/podnet#413) |
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
| [T-192](foundation.md) | P3 | M | M1 | feature | done | `pod-core`: one replace of a private file with a durable commit and three outcomes (PodUtils/podnet#437) |
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
| [T-300](relay.md) | P3 | L | M2 | feature | done | `pod-link`: the stream multiplexer with credit flow control, the carriers and the ARQ, as a shared crate (PodUtils/podnet#479) |
| [T-301](relay.md) | P2 | M | M2 | feature | done | `pod-e2e`: the handshake hash and an exporter, and the record layer (PodUtils/podnet#479) |
| [T-302](roads.md) | P3 | M | M4 | feature | open | `pod-egress`: the accept side of the SIP004 (Shadowsocks AEAD) and Trojan codecs (PodUtils/podnet#479) |
| [T-303](relay.md) | P3 | S | M2 | chore | done | `pod-relay`: research: the relay contract and an exchange of NAT traversal candidates between two peers (PodUtils/podnet#479) |
| [T-304](transport.md) | P2 | S | M1 | feature | done | `pod-tls`: a verifier that pins an Ed25519 identity of the peer (PodUtils/podnet#479) |
| [T-305](transport.md) | P3 | S | M1 | feature | done | `pod-dns`: a reader of TXT and CNAME answers with EDNS0 and the TC bit, for DNS carriers (PodUtils/podnet#479) |
| [T-306](upkeep.md) | P1 | S | M5 | chore | done | `pod-services`: six podvm files of its block changed after `2be31b2` (`upstream.yml` red since 2026-11-10) |
| [T-307](upkeep.md) | P1 | S | M5 | defect | done | The nightly at `0edd703` is red: a test of `pod-guest` with no Linux gate, the TCP-list tests on targets with `socketcall(2)`, a bound of 1 s under riscv64 |
| [T-308](upkeep.md) | P1 | S | M5 | chore | done | `pod-core` and `pod-relay`: three podssh files of their blocks changed (`upstream.yml` at `c553207`) |
| [T-309](fleet.md) | P2 | S | M8 | feature | done | `pod-actions-workflow`: a `Run` that owns its workflow (podrunner's ask) |
| [T-310](relay.md) | P2 | L | M2 | feature | done | `pod-link`: a selective-repeat ARQ for a carrier that loses frames, with a test at 3 % loss (PodUtils/podnet#480) |
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
| [T-328](podvm.md) | P2 | S | M6 | research | done | GNU `tar -xz -C DIR` fails with "Cannot open" right after `mkdir -p DIR` in a podvm unit (podrunner's note) |
| [T-329](transport.md) | P2 | S | M1 | feature | done | `pod-services`: the services `smee-io` and `hostc` (PodUtils/podssh#213) |
| [T-330](podvm.md) | P2 | M | M6 | feature | open | `pod-relay`: podvm's members that make, ask about and stop a pair, in place of the curl of its drives |
| [T-335](adapters.md) | P1 | M | M3 | feature | done | `pod-warp`: podnet's stack of `ip.rs` onto `pod-netstack` (congestion control, TCP burst, link counters, the check of the data path) |
| [T-336](adapters.md) | P1 | M | M3 | feature | done | `pod-warp`: the relay leg and the race of the direct path and the relay, with the leg file |
| [T-337](adapters.md) | P1 | L | M3 | feature | done | `pod-warp`: MASQUE over HTTP/3, the WireGuard leg and the carried device |
| [T-338](adapters.md) | P1 | L | M3 | feature | done | `pod-warp`: the proxy, the forward, nc and the L4 door, with loopback tests |
| [T-339](adapters.md) | P1 | L | M3 | feature | partial | `pod-warp`: the live session as a value, TUN and routes, the live step in `live.yml`, and the adoption trials of podnet and podssh |
| [T-345](podvm.md) | P3 | M | M6 | feature | open | `pod-netlab`: the probe program of the guest, which measures each probe of issue 178 |
| [T-346](podvm.md) | P3 | M | M6 | feature | open | `pod-netlab`: the world's services (`world.py`) over `pod-dns` and `pod-proxy` |
| [T-347](podvm.md) | P3 | M | M6 | feature | open | `pod-netlab`: each offline scenario on a real kernel as root, with the planted flush |
| [T-348](podvm.md) | P3 | M | M6 | feature | open | `pod-netlab`: the VPN and the live scenarios of issue 178 (12 of 39) |
| [T-349](podvm.md) | P3 | L | M7 | feature | open | `pod-netlab`: a lab on a user-space stack, and a lab with more than one guest |
| [T-350](transport.md) | P1 | M | M5 | defect | done | `pod-dns`: the daily DoH test to the public resolvers fails on Quad9's connect from a runner; CoreDNS on the loopback takes its place |
| [T-351](upkeep.md) | P1 | M | M5 | defect | partial | `services.yml`: a host that gives no answer to one runner fails the daily run |
| [T-352](upkeep.md) | P2 | S | M5 | chore | done | `live.yml` runs each week, and no step of `ci.yml` reaches a host of another party |
| [T-353](podvm.md) | P2 | M | M5 | feature | done | `pod-oci`: a pull from the CNCF distribution registry on the loopback; the live pull from ECR Public each week |
| [T-354](relay.md) | P2 | S | M5 | chore | done | `pod-wormhole`, `pod-relay`: magic-wormhole's mailbox server and transit relay from a hashed requirements file, on each push |
| [T-355](upkeep.md) | P2 | M | M5 | chore | done | The downloads of each job come from a cache, and the tools from their release binaries |
| [T-356](transport.md) | P3 | M | M5 | feature | open | `pod-services-watch`: `forward-url` against sish and sandhole on the loopback |
| [T-357](roads.md) | P3 | S | M5 | feature | open | `pod-gsr`: gs-netcat as the other end, through the stand-in of the relay |
| [T-358](transport.md) | P3 | S | M5 | feature | done | `pod-proxy`: the STUN client against coturn on the loopback |
| [T-359](adapters.md) | P2 | M | M3 | feature | open | `pod-warp`: the stand-ins of each push and one live run each week |
| [T-360](adapters.md) | P2 | M | M3 | feature | open | `pod-cloudflare`: a stand-in edge of each push and one quick tunnel each week |
| [T-361](roads.md) | P3 | M | M4 | feature | open | `pod-relay`: its end-to-end tests against the self-hosted relay of `pod-relay-serve`, then against workerd |
| [T-370](transport.md) | P2 | S | M1 | feature | done | `pod-proxy`: MESSAGE-INTEGRITY, FINGERPRINT and the long-term key in `stun`, for TURN (PodUtils/podssh#214) |
| [T-371](upkeep.md) | P2 | M | M5 | defect | open | `pod-services-watch`: the DNS watches, `stun-binding` and the TLS of the HTTP client give no class "no answer" |
| [T-372](transport.md) | P2 | S | M1 | feature | done | `pod-services`: `github-ssh` names SSH over port 443 (GitHub #95) |
| [T-373](transport.md) | P2 | S | M1 | feature | done | `pod-services`: Sandhole's remote port as a fact, and `Relay::remote_port` (GitHub #97) |
| [T-374](transport.md) | P1 | S | M1 | defect | done | `pod-services`: Serveo's tunnels are under `serveousercontent.com`; the anonymous login (GitHub #98) |
| [T-375](transport.md) | P3 | S | M1 | feature | done | `pod-services`: `tuns-sh`, the hosted SSH tunnel of pico.sh, as data (GitHub #99) |
| [T-376](transport.md) | P3 | S | M1 | feature | done | `pod-services`: QSocket's relay and Portal's public relays as candidates, with their measured limits (GitHub #96) |
| [T-377](transport.md) | P2 | L | M1 | feature | done | `pod-turn`: a TURN client (RFC 8656) over UDP and TLS, on the STUN codec moved out of `pod-proxy` (GitHub #100) |
| [T-378](transport.md) | P2 | L | M1 | feature | done | `pod-devtunnel`: a client of Microsoft Dev Tunnels: the management API and the relay (GitHub #94) |
| [T-380](transport.md) | P2 | M | M1 | feature | done | `pod-turn`: the client against coturn on the loopback, over UDP and TLS |
| [T-381](transport.md) | P3 | M | M1 | feature | done | `pod-turn`: the options of RFC 8489 and RFC 8656 that the client does not use |
| [T-385](transport.md) | P3 | S | M1 | defect | done | `pod-ws`: a client upgrade checks the subprotocol of the answer |
| [T-386](transport.md) | P2 | S | M1 | measurement | open | `pod-devtunnel`: the live run against the service with the owner's own token |
| [T-387](transport.md) | P2 | S | M1 | release | done | `pod-devtunnel`: the name on crates.io before the release |
| [T-390](relay.md) | P2 | S | M2 | feature | done | `pod-identity`: a limit of entries for `Allowlist`, and a line in the display form (PodUtils/podnet#512 A11) |
| [T-391](relay.md) | P2 | S | M2 | feature | done | `pod-session`: a typed cause in `End::GaveUp` (PodUtils/podnet#512 A12) |
| [T-392](relay.md) | P3 | S | M2 | research | done | `pod-session`: the single use of the resume secret as a setting (PodUtils/podnet#512 A13) |
| [T-393](adapters.md) | P2 | S | M3 | feature | done | `pod-netstack`: the TCP timeout, the keepalive and no Nagle of each socket as settings, for `WarpNet` (podnet issue 125, podssh) |
| [T-394](adapters.md) | P1 | M | M3 | feature | done | `pod-warp`: the streams of a `WarpNet` fail at once when its session closes, and a close by the caller (podnet issues 125, 112, 185) |
| [T-395](adapters.md) | P2 | S | M3 | defect | done | `pod-warp`: a UDP datagram of `WarpNet` to a family that the device lacks is refused (podnet issue 112) |
| [T-396](transport.md) | P2 | M | M2 | feature | done | `pod-link`: a TCP carrier with TLS 1.3 that pins the Ed25519 key of the peer, and its accept side (PodUtils/podnet#513) |
| [T-403](transport.md) | P3 | M | M1 | feature | done | `pod-turn`: the SHA-256 credentials against a TURN server of another party |
| [T-406](adapters.md) | P1 | M | M3 | feature | done | `pod-warp`: the L4 door of `--l4` and its proxy, after HTTP/3 |
| [T-407](transport.md) | P2 | S | M3 | chore | open | `pod-proxy`: fuzz targets of the server face of a proxy |
| [T-409](adapters.md) | P2 | S | M3 | feature | open | `pod-warp`: HTTP/3 on a socket of the system takes the UDP socket of noq (GSO, GRO) |
| [T-410](adapters.md) | P2 | S | M3 | feature | open | `pod-warp`: the L4 door reads the device record again on a new key, and tries the other door |
| [T-412](adapters.md) | P1 | L | M3 | feature | done | `pod-warp`: the adoption trial of podnet (its src/warp on the crate) |
| [T-413](adapters.md) | P2 | L | M3 | feature | done | `pod-warp`: the adoption trial of podssh (crates/podssh-warp retires onto the crate) |
| [T-416](transport.md) | P2 | M | M1 | feature | done | `pod-services`: the kind `file-host` and four public file hosts, for PodUtils/podssh@T-738 (PodUtils/podssh#220) |
| [T-417](relay.md) | P2 | S | M2 | feature | done | `pod-wormhole`: a board of retained messages over `mqtt::Point`, for PodUtils/podssh@T-738 (PodUtils/podssh#220) |
| [T-418](transport.md) | P2 | S | M1 | docs | done | Tell podssh the terms of the file hosts that the catalog records (PodUtils/podssh#220) |
| [T-419](transport.md) | P3 | S | M1 | feature | open | `pod-services`: the facts of the file hosts that podcrates did not read, from podssh's first live run |
| [T-420](relay.md) | P2 | M | M2 | feature | done | `pod-link`: the WebSocket carrier with its server half (PodUtils/podnet#515) |
| [T-421](relay.md) | P2 | M | M2 | feature | done | `pod-link`: the HTTP/2 carrier, one bidirectional POST (PodUtils/podnet#515) |
| [T-422](relay.md) | P2 | L | M2 | feature | done | `pod-link`: the HTTP/1.1 long-poll carrier, and the SSE and chunked-stream carrier (PodUtils/podnet#515) |
| [T-423](relay.md) | P2 | M | M2 | feature | done | `pod-link`: the QUIC carrier (PodUtils/podnet#515) |
| [T-424](relay.md) | P2 | L | M2 | feature | done | `pod-link`: the UDP carrier with KCP (PodUtils/podnet#515) |
| [T-425](fleet.md) | P2 | M | M8 | chore | done | podswarm: record each ask of #107 and each file of the spec of #108 as entries, when the index of #107 is complete |
| [T-430](fleet.md) | P2 | L | M8 | feature | open | `pod-proc`: a spawner with keepers, the containment rungs, a fence and the tree kill on each target (GitHub #113) |
| [T-431](fleet.md) | P3 | M | M8 | feature | open | `pod-proc`: the exec takeover of a new process (`PTRACE_SEIZE`, the stop at the exec, the stub) (GitHub #113) |
| [T-432](fleet.md) | P2 | M | M8 | feature | open | `pod-tty`: the guard of the caller's terminal (raw mode, size, restore, escape line) on Unix and Windows (GitHub #111) |
| [T-433](fleet.md) | P2 | L | M8 | feature | open | `pod-fuse`: a FUSE server with no libfuse, with the write opcodes, `READDIRPLUS` and invalidation (GitHub #110) |
| [T-434](fleet.md) | P3 | L | M8 | feature | open | `pod-cas`: a BLAKE3 content store with resume, placement, sweep and tree manifests (GitHub #109) |
| [T-435](fleet.md) | P3 | L | M8 | feature | open | `pod-view`: a live view of a directory (records, a frame codec, an ordered write-back log) (GitHub #114) |
| [T-436](fleet.md) | P3 | L | M8 | feature | open | `pod-attest`: the check of an SEV-SNP report and the expected measurement of a guest set (GitHub #115) |
| [T-437](fleet.md) | P3 | L | M8 | feature | open | `pod-ssh`: an SSH client library on `russh` (host key policy, auth chain, exec channel) (GitHub #112) |
| [T-438](fleet.md) | P2 | S | M8 | release | done | The names `pod-proc`, `pod-tty`, `pod-fuse`, `pod-cas`, `pod-view`, `pod-attest` and `pod-ssh` on crates.io (GitHub #1) |
| [T-439](transport.md) | P3 | M | M1 | feature | done | `pod-ws`: a module `serve` with an accept loop, stage deadlines and caps for each address (GitHub #116) |
| [T-440](transport.md) | P3 | S | M8 | feature | open | `pod-tls`: an enrolment mode of the client verifier of `peer` that admits any Ed25519 key and reports it (GitHub #117) |
| [T-441](transport.md) | P2 | S | M8 | feature | open | `pod-proxy`: `ChildIo::spawn_args` with no shell, a stderr pipe and a wait future (GitHub #120) |
| [T-442](relay.md) | P2 | M | M8 | feature | open | `pod-link`: a first stream whose frames go before the data of the others, and a change of the windows on a live link (GitHub #88) |
| [T-443](podvm.md) | P2 | M | M8 | feature | open | `pod-hostprobe`: a map from the findings to capabilities, and a cache across runs keyed by the kernel, the boot and the probe binary (GitHub #82) |
| [T-444](podvm.md) | P2 | M | M6 | feature | open | `pod-hostprobe`: the probe rows that `PROBES` lacks (user and PID namespace, cgroup2 delegation, FUSE in a user namespace, and more) (GitHub #82) |
| [T-445](podvm.md) | P3 | M | M8 | feature | open | `pod-hold`: the marker, the calls that fail with `ENOSYS`, and the reads of a path and an argv of a stopped task (GitHub #121) |
| [T-446](podvm.md) | P3 | M | M8 | feature | open | `pod-hold`: a plan that the child applies before `execve` (`PreExec` and `Program::with_plan`) (GitHub #121) |
| [T-447](podvm.md) | P3 | M | M7 | feature | open | `pod-guest`: a Linux flavour of the plan (kernel and initramfs, one virtio-serial socket, one raw disk, mem-lock) and an SNP variant (GitHub #122) |
| [T-448](podvm.md) | P3 | S | M6 | docs | open | `pod-enter`: the text of `fuse.rs` says that `mount(2)` of FUSE fails in a user namespace; it works since Linux 4.18 (GitHub #123) |
| [T-449](fleet.md) | P2 | S | M8 | feature | open | `pod-job`: `LimitMode::Polled`, a limit that a keeper measures and enforces by a kill (GitHub #118) |
| [T-450](foundation.md) | P2 | S | M8 | feature | open | `pod-core`: `Stage::Task` for a failure of a task that a tool runs for its user (GitHub #119) |
| [T-470](adapters.md) | P3 | S | M3 | feature | open | `pod-warp`: a change of the settings of a `Warp` keeps the state of its doors and legs |
| [T-475](adapters.md) | P3 | S | M3 | feature | done | `pod-warp`: the rules `port:N=WAY` and `tcp:HOST:PORT=WAY` in the reader of the rules (PodUtils/podnet#518) |
| [T-476](relay.md) | P2 | M | M2 | feature | done | `pod-relay`: the invariants 6 to 10 of the HTTP road, client and server, with a test of lost acknowledgements (PodUtils/podssh#221) |
| [T-477](relay.md) | P1 | S | M2 | defect | done | `pod-relay`: the serial form of the HTTP road deadlocks when the bytes down block the bytes up (PodUtils/podssh#222) |
| [T-478](relay.md) | P3 | S | M2 | defect | done | `pod-relay`: the client of the HTTP road never sends `close`, so a caller that drops its stream leaves the session at the role |
| [T-480](adapters.md) | P2 | S | M3 | feature | done | `pod-warp`: the API sends a team's token with a registration and names the base that answered |
| [T-481](adapters.md) | P2 | M | M3 | feature | open | `pod-warp`: a session to a connect-ip server of the user's own over HTTP/3 (RFC 9484) |
| [T-482](relay.md) | P2 | S | M2 | defect | open | `pod-netstack`: a stream dropped before its handshake ends finishes the handshake later |
| [T-485](podvm.md) | P2 | S | M6 | feature | done | `pod-hostprobe`: a reader of the report beside its writer, so that `podvm run` reads its probe cache (PodUtils/podvm#291) |
| [T-486](podvm.md) | P2 | M | M6 | feature | done | `pod-enter`: an overlay root in the namespace rung, the image read-only under a tmpfs upper layer (PodUtils/podvm#294) |
| [T-488](upkeep.md) | P1 | S | M5 | defect | done | The nightly at `613058c` fails to compile on Windows: `reverse_plain.rs` of `pod-relay` binds a Unix socket with no `cfg(unix)` |
| [T-489](transport.md) | P2 | S | M1 | feature | done | `pod-devtunnel`: a client with no user token, for a tunnel with anonymous access or with a connect token |
| [T-490](transport.md) | P3 | L | M8 | feature | open | `pod-tls`: the server half of ECH, a ClientHello in the shape of a browser, and the measure of the handshake (GitHub #130, deferred) |
| [T-491](podvm.md) | P2 | S | M6 | feature | done | `pod-enter`: a fresh proc on the `/proc` of the image in the namespace rung (PodUtils/podvm#296) |
| [T-492](podvm.md) | P2 | M | M6 | feature | done | `pod-enter`: an entry that joins the mount namespace of a running payload, for exec (PodUtils/podvm#297) |
| [T-493](relay.md) | P2 | M | M2 | feature | done | `pod-link`: the DNS simplex carrier (PodUtils/podnet#521) |
| [T-494](relay.md) | P2 | M | M2 | feature | done | `pod-link`: the object-storage (S3) simplex carrier (PodUtils/podnet#521) |
| [T-495](relay.md) | P2 | M | M2 | feature | done | `pod-link`: the HTTP poll simplex carrier (PodUtils/podnet#521) |
| [T-496](relay.md) | P2 | L | M2 | feature | open | `pod-link`: the DNS stream carrier with a pipelined window of queries (PodUtils/podnet#521) |
| [T-497](relay.md) | P2 | M | M2 | feature | done | `pod-link`: the ICMP echo carrier (PodUtils/podnet#521) |
| [T-498](relay.md) | P2 | M | M2 | feature | open | `pod-link`: a WireGuard peer as a carrier, over `pod-wg` (PodUtils/podnet#521) |
| [T-499](relay.md) | P2 | L | M2 | feature | open | `pod-link`: NAT traversal with a fall back to the relay (PodUtils/podnet#521) |
| [T-500](relay.md) | P2 | M | M2 | feature | done | `pod-link`: one session over an up carrier and a down carrier (PodUtils/podnet#521) |
| [T-501](transport.md) | P2 | M | M1 | feature | open | `pod-turn`: the TCP relay of RFC 6062 (PodUtils/podnet#521) |
| [T-502](transport.md) | P3 | M | M1 | feature | open | `pod-testkit`: a headscale harness for the tests of a Tailscale node (PodUtils/podnet#521) |
| [T-503](podvm.md) | P3 | S | M6 | defect | done | `pod-netlab`: three fixes of the knob flags: an empty `--udp`, the range of a number, the words of a flag (PodUtils/podvm#298) |
| [T-504](podvm.md) | P2 | S | M6 | feature | done | `pod-enter`: a bind of the host's `/dev/pts` on the `/dev/pts` of the image in the namespace rung (PodUtils/podvm#296) |
| [T-505](fleet.md) | P1 | S | M8 | feature | done | `pod-job`: the actor classes `agent` and `bot` in `job-offer.v1` (PodUtils/podrunner#80) |
| [T-506](relay.md) | P2 | S | M2 | feature | done | `pod-link`: the ws carrier with the rules of podnet's WebSocket carrier: a subprotocol, a text message as a protocol error, a Ping each 20 s with a Pong within 10 s (PodUtils/podnet#515) |
| [T-507](upkeep.md) | P1 | S | M5 | defect | done | The tests of `pod-services-watch` fail on Windows: a reset loses the bytes of a stand-in, and a runner with no `SystemRoot` has no Winsock |
| [T-508](adapters.md) | P1 | S | M3 | defect | done | `pod-warp`: QUIC over the datagrams of a session reaches no IPv4 peer: an IPv4-mapped destination leaves as an IPv6 packet (PodUtils/podnet#524) |
| [T-510](relay.md) | P2 | S | M2 | chore | done | `pod-e2e`: a fuzz target of the opener of the records |
| [T-514](relay.md) | P3 | L | M2 | feature | open | `pod-link`: a stream-aware ARQ, in which `arq-timeout` closes one stream and the link lives (PodUtils/podnet#468) |
| [T-515](relay.md) | P2 | M | M2 | feature | open | `pod-link`: a congestion window for the ARQ on a shared path (UDP, WireGuard, NAT traversal) |
| [T-516](relay.md) | P2 | S | M2 | chore | done | `pod-link`: a fuzz target of the datagrams of the ARQ |
| [T-517](relay.md) | P3 | S | M2 | feature | done | `pod-link`: a cap of the bytes that the sessions of a poll listener hold |
| [T-518](relay.md) | P3 | S | M2 | chore | done | `pod-link`: a fuzz target of the HTTP/1.1 of the poll carriers |
| [T-520](relay.md) | P2 | S | M2 | feature | done | `pod-link`: the default of the congestion window of KCP against podnet's gate of 20 % loss (PodUtils/podnet#515) |
| [T-521](relay.md) | P2 | S | M2 | chore | done | `pod-link`: a fuzz target of the datagrams of the udp carrier (the input of KCP) |
| [T-525](relay.md) | P3 | S | M2 | feature | done | `pod-link`: the bytes of a poll listener outside `max_bytes` (the pipes of each session, the connections after their handshake) |
| [T-526](relay.md) | P3 | S | M2 | chore | partial | `pod-link`: the arithmetic of the crate `kcp` wraps by design: no overflow checks for it, and the fix upstream |
| [T-527](fleet.md) | P1 | S | M8 | feature | done | `pod-job`: the CPU time and the peak of resident memory of a job in `run-report.v1` (githome's G-06, #83) |
| [T-528](transport.md) | P2 | M | M1 | defect | done | `pod-services`: three of the four file hosts fail live from GitHub: the download URL of tmpfiles.org, the upload form of litterbox, the busy answer of 0x0.st (PodUtils/podssh#230) |
| [T-529](transport.md) | P2 | S | M1 | defect | done | `pod-services`: the result of the first live round trip of the file hosts; the text of litterbox's 412 |
| [T-531](relay.md) | P2 | S | M2 | chore | done | `pod-netstack`: tests of a far end that closed first and of the packets held while the queue out is full (PodUtils/podssh#231) |
| [T-532](upkeep.md) | P1 | S | M5 | defect | done | The nightly at `d8c8ae6` is red on riscv64: the bounds of the tests of `pod-warp` are not scaled |
| [T-533](upkeep.md) | P2 | S | M5 | chore | done | `scripts/agent/check-macos.sh`: a local check of the build for macOS, as `check-windows.sh` does for Windows |
| [T-534](upkeep.md) | P1 | S | M5 | defect | done | `ci.yml` at `3d6bbf0` is red: the registry of `pod-oci`'s test took a port that a parallel test held |
| [T-535](upkeep.md) | P1 | S | M5 | defect | done | The nightly at `72632b2` is red on riscv64 and armv7: the bounds of the tests of `pod-link` are not scaled under an emulator |
| [T-536](upkeep.md) | P2 | S | M5 | defect | done | `pod-oci`: the test of the distribution registry with a bearer token fails at random: the registry refuses the connection |
| [T-537](upkeep.md) | P2 | S | M5 | defect | done | `services.yml` at `4c2d17c` is red: the tox node `node-3091c6be` gives no TCP relay |
| [T-538](upkeep.md) | P3 | S | M5 | docs | blocked | The rule of attribution of a comment on GitHub: the documents of podcrates and the practice of its sessions differ |
| [T-539](transport.md) | P2 | M | M3 | feature | done | `pod-term`: a line discipline that follows a termios state, for the tty of `podssh serve` with no /dev/ptmx (PodUtils/podssh#232) |
| [T-540](transport.md) | P3 | S | M3 | chore | done | `pod-term`: a test of `termios::Tty` against a pty of the kernel, where one opens |
| [T-541](relay.md) | P1 | S | M2 | defect | done | `pod-link`: the dns and icmp carriers stop at the wrap of the sequence of the ARQ: `dgram_tls` never calls `rekeyed` (PodUtils/podnet#540) |
| [T-542](relay.md) | P2 | S | M2 | feature | done | `pod-link`: one typed marker for "the peer refused the key of this end" on each carrier (PodUtils/podnet#533) |
| [T-543](relay.md) | P2 | S | M2 | defect | done | `pod-link`: the dns carrier and the TC bit: each answer keeps to the size of its query, and a cut answer does not stall the link (PodUtils/podnet#533) |
| [T-544](podvm.md) | P2 | S | M1 | feature | done | `pod-hold`: a seccomp rule that refuses a raw socket and an ICMP socket, with an opt-in (PodUtils/podnet#538) |
| [T-545](relay.md) | P1 | S | M2 | feature | done | `pod-wormhole`: `Wormhole::derive_key`, magic-wormhole's derived key, for the transit key of podnet's transfer (PodUtils/podnet#535) |
| [T-546](relay.md) | P2 | S | M2 | feature | done | `pod-wormhole`: the stand-in mailbox behind the feature `standin`, for the tests of a consumer (PodUtils/podnet#535) |
| [T-547](relay.md) | P2 | S | M2 | feature | done | `pod-wormhole`: the count of words of a new code in `Config`, as magic-wormhole's `--code-length` (PodUtils/podnet#535) |
| [T-548](relay.md) | P2 | S | M2 | feature | done | `pod-services`: magic-wormhole's TCP transit relay as a service of the kind `transit`, with the watch `transit-relay` (PodUtils/podnet#535) |
| [T-549](adapters.md) | P2 | S | M3 | feature | done | `pod-warp`: name the certificate that the front door showed when its key is not the pinned one (PodUtils/podssh#234) |
| [T-550](relay.md) | P3 | M | M2 | feature | done | `pod-wormhole`: Nostr's ephemeral events as a meeting point of the pairing by a code (PodUtils/podssh#235) |
| [T-551](relay.md) | P3 | M | M2 | feature | done | `pod-wormhole`: WebTorrent's trackers as a meeting point of the pairing by a code (PodUtils/podssh#235) |
| [T-552](transport.md) | P2 | S | M1 | feature | done | `pod-services`: the services `srv-us` and `tunnl-gg` of the kind `ssh-tunnel` (PodUtils/podssh#236) |
| [T-553](fleet.md) | P2 | S | M8 | feature | done | `pod-job`: the container route `engine` and the need `container_engine`, for a container engine inside the unit (PodUtils/podrunner#91) |
| [T-554](relay.md) | P3 | M | M2 | feature | done | `pod-wormhole`: an ntfy topic as a meeting point of the pairing where a proxy refuses the WebSocket upgrade (PodUtils/podssh#237) |
| [T-555](upkeep.md) | P1 | S | M5 | defect | done | `adoption.yml` at `7122cc7` is red for podssh: its test pins five tox relays, and T-537 left four |
| [T-556](transport.md) | P2 | S | M1 | feature | done | `pod-services`: serv00's public Piping Server as the third of the kind `piping` (PodUtils/podssh#238) |
| [T-557](relay.md) | P3 | S | M2 | defect | done | `pod-link`: the listener of the dns, icmp, udp and ws carriers gets no `KeyRefused` when the dialer refuses its key |
| [T-558](relay.md) | P2 | S | M6 | defect | done | `pod-hold`: the rule of raw sockets leaves `AF_PACKET` open |
| [T-559](relay.md) | P2 | M | M2 | feature | done | `pod-relay`: the client of the HTTP road's one-POST form (PodUtils/podssh#239) |
| [T-560](relay.md) | P3 | S | M2 | feature | done | `pod-relay`: the one-POST form of the HTTP road over HTTP/2 |
| [T-561](relay.md) | P1 | S | M2 | feature | done | `pod-wg`: the preshared key of a handshake (PodUtils/podnet#547) |
| [T-562](relay.md) | P2 | M | M2 | feature | done | `pod-wg`: a responder sans I/O for a list of peers (PodUtils/podnet#547) |
| [T-563](upkeep.md) | P1 | S | M5 | defect | done | `adoption.yml` at `b0533d0` is red for podssh: `Route::One` of `pod-relay` breaks a match of its role |
| [T-564](fleet.md) | P2 | M | M8 | feature | done | `pod-actions-store`: the actions by tag, a sorted lock of commits and digests, a store of tarballs and a git forge (PodUtils/podrunner#94) |
| [T-565](fleet.md) | P2 | M | M8 | feature | done | `pod-actions-results`: the ArtifactService of the results API and its blob routes, one store for each run (PodUtils/podrunner#95) |
| [T-566](fleet.md) | P3 | S | M8 | chore | done | `pod-actions-results`: a fuzz target of the input of a stranger |
| [T-567](podvm.md) | P2 | M | M6 | feature | done | `pod-rootfs`: the completion of a root of two layers, the lower layer read-only (PodUtils/podvm#326) |
| [T-568](podvm.md) | P2 | S | M6 | defect | done | `pod-enter`: a reason of `abi` carries its own verdict label, and a static object reads "names  in DT_NEEDED" (PodUtils/podvm#331) |
| [T-569](podvm.md) | P2 | S | M6 | defect | done | `pod-hostprobe`: `probe_dir_exec` names the filesystem on each `EACCES`, also on a mount that is not noexec (PodUtils/podvm#335) |
| [T-570](podvm.md) | P2 | S | M6 | defect | done | `pod-hostprobe`: the refusal of the machine tier runs its legs together (PodUtils/podvm#337) |
| [T-571](upkeep.md) | P1 | S | M5 | defect | done | `adoption.yml` at `e830136` is red for podnet: the field `preshared` of `pod_wg::Keys` |
| [T-572](fleet.md) | P2 | M | M8 | feature | done | `pod-actions-workflow`: the filters of `on:` (types, branches, tags, paths and their -ignore forms) (PodUtils/podrunner#97) |
| [T-573](podvm.md) | P1 | M | M6 | feature | done | `pod-supervise`: a foreground holder, so that a run that the caller waits for is Running (PodUtils/podvm#339) |
| [T-574](relay.md) | P2 | S | M6 | feature | done | `pod-hold`: a filter that refuses raw sockets and lets ping sockets reach the supervisor (PodUtils/podnet#550) |
| [T-575](transport.md) | P3 | S | M1 | docs | done | `pod-services`: the README names the scheme `tcp` of an endpoint (PodUtils/podnet#550) |
| [T-576](fleet.md) | P2 | L | M8 | feature | done | `pod-actions-workflow`: the calls of reusable workflows in `Run` (inputs, secrets, outputs, limits) (PodUtils/podrunner#98) |
| [T-577](fleet.md) | P3 | S | M8 | feature | done | `pod-actions-workflow`: the `workflows` filter of `workflow_run` |
| [T-578](fleet.md) | P3 | S | M8 | feature | done | `pod-actions-workflow`: a check of the keys and the filters of `on:`, as GitHub refuses a file |
| [T-579](relay.md) | P2 | S | M2 | defect | done | `pod-session`: a spare link that moved no byte backs off before the next (PodUtils/podssh#242) |
| [T-580](podvm.md) | P1 | S | M6 | defect | done | `pod-rootfs`: an image with no CA bundle gets the host bundle when `host_cas` is false (PodUtils/podvm#341) |
| [T-581](podvm.md) | P1 | S | M6 | defect | done | `pod-extract`: a file of mode 4755, 2755 or 6755 loses its setuid and setgid bits (PodUtils/podvm#342) |
| [T-582](podvm.md) | P2 | S | M6 | defect | done | `pod-supervise`: `start` keeps the end fields of the run before, and a `Dead` record keeps its pid (PodUtils/podvm#343) |
| [T-583](podvm.md) | P1 | M | M6 | defect | done | `pod-confine` and `pod-enter`: an `exec=` rule does not deny a program that the image's loader starts (PodUtils/podvm#344) |
| [T-584](podvm.md) | P2 | S | M6 | defect | done | `pod-enter`: a plan with two limits of one resource fails with `EPERM` (PodUtils/podvm#345) |
| [T-585](podvm.md) | P2 | S | M6 | defect | done | `pod-oci`: the grammar of an insecure registry host, and a platform list with no repeat and no attestation (PodUtils/podvm#346) |
| [T-586](podvm.md) | P3 | M | M6 | defect | partial | `pod-netlab`: the texts of a knob give the value as written, with its unit and range; the differences from sand-net (PodUtils/podvm#347) |
| [T-587](fleet.md) | P2 | M | M8 | feature | done | `pod-actions-store`: podrunner's additions since the import (image lines of the lock, checkouts, `HEAD`, images by tag) (PodUtils/podrunner#99) |
| [T-588](fleet.md) | P2 | M | M8 | feature | done | `pod-actions-results`: the CacheService and the replacement of an artifact on a rerun (PodUtils/podrunner#99) |
| [T-589](relay.md) | P2 | S | M2 | feature | done | `pod-netstack`: `bind_udp` with port 0 takes an ephemeral port (PodUtils/podnet#552) |
| [T-590](podvm.md) | P1 | M | M7 | feature | done | `pod-guest`: `fetch::store` resumes a download by Range, checks the size and the sha256, and has a ceiling for each entry (PodUtils/podvm#350, item 1) |
| [T-591](podvm.md) | P1 | M | M7 | feature | done | `pod-udf`: a new crate that reads UDF and gives the extent of a file in an ISO (PodUtils/podvm#350, item 2) |
| [T-592](podvm.md) | P1 | S | M7 | defect | done | `pod-guest`: the agent runs the command in a child `cmd /c`, writes a `WQREADY` marker, and has a keep-alive mode (PodUtils/podvm#350, item 3) |
| [T-593](podvm.md) | P1 | M | M7 | feature | done | `pod-guest`: the options of a plan: a NIC with `hostfwd`, virtio devices, CD-ROM and USB media, VNC, a serial channel, the Hyper-V flags (PodUtils/podvm#350, item 4) |
| [T-594](podvm.md) | P2 | M | M7 | feature | done | `pod-qga`: a new crate, a client of the QEMU Guest Agent over a Unix socket (PodUtils/podvm#350, item 5) |
| [T-595](podvm.md) | P2 | M | M7 | feature | done | `pod-guest`: the provision waits for a ready guest in place of a fixed sleep (PodUtils/podvm#350, item 6) |
| [T-596](podvm.md) | P3 | M | M7 | feature | done | `pod-guest`: a warm start from a saved state, with a fingerprint and a fallback to a cold boot (PodUtils/podvm#350, item 7) |
| [T-597](podvm.md) | P2 | S | M6 | defect | done | `pod-extract`: the umask of the process changes the modes of the tree, and the sidecar does not record it |
| [T-598](podvm.md) | P1 | S | M6 | defect | done | `pod-extract`: each directory has the mode of the image after the last layer, and a directory entry replaces a non-directory of an earlier layer (PodUtils/podvm#354) |
| [T-599](podvm.md) | P2 | S | M6 | defect | done | `pod-extract`: the `./` member of an archive is not counted as an entry that the crate could not make (PodUtils/podvm#352) |
| [T-600](fleet.md) | P3 | S | M8 | defect | done | `pod-actions-results`: an upload directory of a crashed run stays under `.pending` of the cache |
| [T-601](podvm.md) | P2 | S | M7 | chore | done | `pod-udf`: a test against images that a real mastering tool made |
| [T-602](podvm.md) | P2 | S | M7 | research | done | `pod-guest`: the next command of a guest in keep-alive mode reaches the agent through a mounted FAT mailbox |
| [T-603](podvm.md) | P2 | S | M7 | defect | done | `pod-guest`: the host removes a file of the FAT mailbox, and a poll of the outcome reads an old token as no result yet |
| [T-604](podvm.md) | P1 | S | M6 | defect | done | `pod-rootfs`: the completion of a tree whose `/etc` is closed (`0555`) fails as a user that is not root, after T-598 |
| [T-605](podvm.md) | P3 | S | M6 | defect | done | `pod-extract`: the sidecar does not record the mode that the kernel kept on a directory |
| [T-606](podvm.md) | P3 | S | M6 | defect | done | `pod-extract`: a second extraction over a closed tree fails on a directory with no owner read or under a closed parent that the layer does not name |
| [T-607](upkeep.md) | P1 | S | M5 | defect | done | the nightly run at `f6479c7` is red on `windows-11-arm`: the ARQ of a test of `pod-link` through a resolver gives up in 7 s |
| [T-608](upkeep.md) | P1 | S | M5 | defect | done | `upstream.yml` is red: the sources of `pod-warp`, `pod-netstack` and `pod-gsr` changed |
| [T-609](podvm.md) | P2 | S | M6 | defect | done | `pod-broker`: a 502 names the status of the upstream proxy or the dial error, and a 403 of `no-rule` names the half of the rule that failed (PodUtils/podvm#359) |
| [T-610](podvm.md) | P1 | S | M7 | feature | done | `pod-guest`: `Fat16::put` writes a long name of up to 32 characters with VFAT entries and an 8.3 alias (PodUtils/podvm#360) |
| [T-611](transport.md) | P2 | S | M1 | feature | done | `pod-services`: the host `software-download.microsoft.com` of the Windows evaluation media (PodUtils/podvm#361) |
| [T-612](relay.md) | P1 | S | M2 | defect | done | `pod-proxy`: the label of a unix proxy shows the user info of its URL (PodUtils/podnet#555) |
| [T-613](fleet.md) | P2 | S | M8 | feature | done | `pod-actions-store` and `pod-actions-results`: podrunner's changes after `872b43f` (T-139, T-149, T-154 of podrunner) |
| [T-614](upkeep.md) | P3 | S | M5 | chore | done | CI: a job that runs as root the tests that start a worker of uid 1000 |
| [T-615](podvm.md) | P3 | S | M6 | defect | done | `pod-extract`: `open_tree` and the removal hold one descriptor for each level of depth |
| [T-616](podvm.md) | P2 | S | M6 | defect | done | `pod-supervise`: the thread of the foreground hold blocks the shutdown signals |
| [T-617](podvm.md) | P3 | M | M7 | feature | blocked | `pod-guest`: an NTFS and hive writer for an offline setup of a Windows guest |
| [T-618](podvm.md) | P3 | S | M6 | defect | done | `pod-hostprobe`: the signal calls on mips and mips64 |
| [T-619](podvm.md) | P3 | S | M6 | defect | done | `pod-hostprobe`: the open flags and the errno numbers of mips and sparc |
| [T-620](podvm.md) | P3 | S | M7 | chore | done | `pod-udf`: a live test against a Windows ISO by Range requests |
| [T-621](upkeep.md) | P2 | S | M5 | defect | done | `pod-warp`: the test `a_close_by_the_caller_fails_the_flows_of_the_session` fails at times with a refused connection to the edge stand-in |
| [T-622](podvm.md) | P1 | S | M7 | defect | done | `pod-guest`: the provision stops the emulator on each error after the spawn (PodUtils/podvm#370) |
| [T-623](relay.md) | P2 | S | M2 | chore | done | `pod-link`: the S3 carrier against a real S3-compatible server |
| [T-624](relay.md) | P2 | S | M2 | defect | done | `pod-warp`: after a key that is not the pinned one, the client tries each door of the new record and names the mismatch |
| [T-625](podvm.md) | P3 | S | M6 | chore | done | `pod-hostprobe`: the test of the report runs the probes in a child that libtest refuses |
| [T-626](podvm.md) | P3 | M | M7 | chore | open | `pod-udf`: the metadata partition of UDF 2.50 against an image of a real tool |
| [T-627](relay.md) | P3 | S | M2 | feature | done | `pod-link`: the stand-in of the S3 API behind the feature `s3-standin` (PodUtils/podnet#564) |
| [T-628](podvm.md) | P2 | S | M7 | defect | done | `pod-guest`: the warm restore gives the next command on a new mailbox medium after the load |
| [T-629](podvm.md) | P1 | S | M7 | defect | done | `pod-guest`: a plan with a boot medium gives the root disk no `bootindex`, so OVMF deletes the boot entry of the installed system |
| [T-630](podvm.md) | P1 | S | M7 | defect | done | `pod-guest`: the swap of a USB mailbox fails while Windows locks the medium |
| [T-631](transport.md) | P3 | M | M1 | chore | done | `pod-turn`: the check of a MESSAGE-INTEGRITY-SHA256 answer against a server of another party |
| [T-632](podvm.md) | P3 | S | M7 | chore | done | `pod-guest`: the QMP commands of the mailbox name the drive by `id`, not by the deprecated `device` |
| [T-633](podvm.md) | P1 | S | M7 | defect | done | `pod-guest`: the screen sign of the readiness fails on Validation OS, so the provision never types into a Windows guest |
| [T-634](relay.md) | P1 | S | M2 | defect | done | `pod-relay`: the run of a code fails over the Nostr relays alone, though `pod_wormhole::nostr` pairs (PodUtils/podssh#247) |
| [T-635](podvm.md) | P3 | S | M7 | feature | open | `pod-guest`: a setting of the bus of the root disk, so that a warm start runs on QEMU 8.2 |
| [T-636](roads.md) | P3 | S | M4 | chore | open | `pod-derp`: the tests of the DERP link against Tailscale's derper on the loopback in CI |
| [T-637](roads.md) | P3 | S | M4 | chore | open | `pod-acme`: a whole order against Pebble on the loopback in CI |
| [T-638](roads.md) | P3 | S | M4 | feature | open | `pod-opentunnel`: the stand-in of OpenTunnel behind the feature `standin` |
| [T-639](relay.md) | P2 | S | M2 | defect | done | `pod-link`: a closed half of a split session can end the listener with "the carrier of the link ended" in place of the lost half |
| [T-640](podvm.md) | P1 | S | M7 | defect | done | `pod-guest`: a cold boot of a Windows guest under KVM reaches no shutdown on one run of two, and a run that times out keeps no trace |
| [T-641](podvm.md) | P3 | S | M7 | defect | open | `pod-guest`: the installer tries its shutdown again, and `dos::run` keeps the evidence of a run that times out |
| [T-642](roads.md) | P3 | S | M4 | chore | open | `pod-egress`: the Python packages of `forwarder-live.yml` by a hashed lock |
| [T-643](relay.md) | P2 | S | M1 | defect | done | `pod-quic`: a way whose reads keep failing spins a core, and the drop of the endpoint hangs (PodUtils/podssh#253) |
| [T-644](podvm.md) | P3 | S | M6 | feature | open | `pod-enter`: a form of `run_ladder` that gives the caller the child before its wait (PodUtils/podvm#384) |
| [T-645](upkeep.md) | P1 | S | M5 | defect | done | `secrets.yml` is red on each push: three made-up URLs of `pod-broker` read as unknown |
| [T-646](upkeep.md) | P1 | S | M5 | defect | done | `adoption.yml`: the trial of `pod-egress` in podssh fails, as podssh's record cites the files that leave podssh |
| [T-647](podvm.md) | P2 | S | M6 | feature | done | `pod-services`: the host `api.ghcr.pkgforge.dev` of the static QEMU of podvm's store (PodUtils/podvm#389) |
| [T-648](podvm.md) | P2 | S | M6 | feature | done | `pod-hostprobe`: a leg that reports a delegated cgroup2 subtree with no root (PodUtils/podvm#390) |
| [T-649](podvm.md) | P3 | M | M6 | feature | open | `pod-guest`: a FAT image writer with directories, long names and a size from the content (PodUtils/podvm#386) |
| [T-650](podvm.md) | P2 | S | M7 | feature | done | `pod-qga`: `guest-get-osinfo` (PodUtils/podvm#393) |
| [T-651](podvm.md) | P2 | S | M6 | feature | done | `pod-services`: the aarch64 netboot of Alpine 3.22.6 and the static `qemu-system-aarch64` (PodUtils/podvm#394) |
| [T-652](roads.md) | P3 | S | M4 | feature | open | `pod-egress`: the stage of a failed TLS or upgrade on the roads of VLESS, VMess and Trojan |
| [T-668](upkeep.md) | P2 | S | M6 | feature | done | `pod-oci`: the token of the manifest for the parallel blob fetches of a pull (PodUtils/podvm#395) |
| [T-669](podvm.md) | P2 | S | M6 | feature | done | `pod-enter`: a UTS namespace and a host name on the namespace rung (PodUtils/podvm#403) |
| [T-670](podvm.md) | P2 | M | M6 | feature | done | `pod-enter`: the namespace rung through a user namespace, and its leg in `pod-hostprobe` (PodUtils/podvm#406) |
| [T-671](podvm.md) | P2 | S | M6 | defect | done | `pod-enter`: the join of a run that entered the namespace rung through a user namespace |
| [T-672](podvm.md) | P3 | S | M6 | feature | done | `pod-enter`: a `/proc` for the namespace rung through a user namespace |
| [T-673](roads.md) | P1 | S | M1 | defect | done | `pod-egress`: the features `vless`, `vmess`, `trojan` and `shadowsocks` are in no static probe |
| [T-676](podvm.md) | P2 | S | M6 | defect | done | `pod-hostprobe`: the KVM leg is a yes only for a device that runs a guest of the build's architecture (PodUtils/podvm#407) |
| [T-677](upkeep.md) | P1 | S | M5 | defect | done | `plants.yml` passes its limit of 60 minutes: the plants run in shards |
| [T-678](upkeep.md) | P1 | S | M5 | defect | done | three plants of `pod-extract` no longer catch: a second guard holds their check |
| [T-679](upkeep.md) | P1 | S | M5 | defect | done | The nightly at `98017c3` is red on Windows: a test of `pod-actions-store` reads a removed variable by the wrong case |
| [T-680](upkeep.md) | P1 | S | M5 | defect | done | The nightly at `98017c3` is red on macOS: a port that a socket holds with no listen does not refuse there |
| [T-681](upkeep.md) | P1 | S | M5 | defect | done | The nightly at `98017c3` is red on windows-2025: a test of `pod-link` reads the answer of a POST before the rest of its body in the link |
| [T-682](upkeep.md) | P1 | S | M5 | defect | done | The nightly at `98017c3` is red on riscv64 and ppc64le: the upper bound of the split form of `pod-relay` over HTTP/2 is not scaled under an emulator |
| [T-683](upkeep.md) | P1 | S | M5 | defect | done | The nightly at `98017c3` is red on ppc64le: the kernel test of `pod-term` gives the numbers of `asm-generic` to a powerpc kernel |
| [T-684](upkeep.md) | P2 | S | M1 | defect | done | `services.yml` at `98017c3`: the Trojan node `node-trojan-149-28-130-108` ends the TLS handshake |
| [T-685](upkeep.md) | P2 | S | M1 | defect | done | `services.yml` at `98017c3`: both RDAP servers answer 403 for the `.xyz` names to the runner |
| [T-686](relay.md) | P1 | S | M2 | defect | done | `pod-relay`: a new link of the resumable layer rides the pooled HTTP/2 connection of the silent link it replaces (PodUtils/podssh#262) |
| [T-687](relay.md) | P2 | S | M2 | defect | done | `pod-ws`: a silent connection of the HTTP/2 pool takes the first link of each new session until its TCP fails |
| [T-688](podvm.md) | P2 | S | M6 | feature | done | `pod-enter`: the device nodes of the host on the namespace rung through a user namespace (PodUtils/podvm#414) |
| [T-689](upkeep.md) | P2 | S | M5 | defect | done | the plant `enter-userns-proc-no-remount` misses where the host's `/proc` has `nosuid`, `nodev` and `noexec` already |
| [T-691](podvm.md) | P2 | S | M6 | defect | done | `pod-enter`: the root directory and the memfd move clear of the numbers of the passed descriptors (PodUtils/podvm#422) |
| [T-692](fleet.md) | P1 | L | M8 | feature | partial | `pod-lab`: the hostile host classes as shared profiles, `podlab check` and `podlab run` (PodUtils/podrunner#180) |
| [T-693](fleet.md) | P2 | S | M8 | feature | partial | `pod-hostprobe`: the rows of the socket class (AF_INET6, connect, AF_UNIX, getsockname) |
| [T-694](fleet.md) | P2 | S | M8 | feature | open | `pod-hostprobe`: the rows of `TMPDIR` and of the inherited descriptors |
| [T-695](fleet.md) | P2 | S | M8 | feature | open | `pod-hostprobe`: the rows of the machine, the cgroup version and the `/proc` mount |
| [T-696](fleet.md) | P2 | S | M8 | feature | open | `pod-lab`: `bind-denied` refuses UDP bind and connect of AF_INET and AF_INET6 |
| [T-700](fleet.md) | P1 | L | M8 | feature | partial | `pod-manual`: one crate for the help and the manual of each command line of PodUtils (PodUtils/podnet#584) |
| [T-701](fleet.md) | P1 | M | M8 | feature | done | `pod-lab`: the host classes of podssh and podnet as profiles, with the lane of each project (PodUtils/podssh#268, PodUtils/podnet#585) |
| [T-702](fleet.md) | P1 | M | M8 | feature | partial | one POSIX installer for each project, hermetic, idempotent and fast, run in the labs (the ruling "Installer") |
| [T-703](fleet.md) | P1 | M | M8 | feature | partial | a mechanical check of the open issues and their comments against the record, for each project (the ruling "Issues") |
| [T-704](fleet.md) | P2 | S | M8 | feature | open | `podlab`: the usage errors of clap in the form of `pod_manual::error::fault` |
| [T-705](fleet.md) | P2 | S | M8 | feature | open | `check-strings.py`: the strings of Mach-O and PE binaries |
| [T-710](foundation.md) | P1 | S | M0 | chore | done | one watcher of the organization for each session: issues, pull requests and Actions runs (the ruling "Watcher") |
| [T-711](foundation.md) | P1 | S | M0 | chore | done | a message from outside the team: its own session verifies and confirms it; each other session checks the author (the ruling "Strangers") |
| [T-712](foundation.md) | P1 | S | M0 | chore | done | the watcher reports each comment that is new or edited (the owner's ask) |
| [T-713](podvm.md) | P1 | S | M6 | defect | done | `pod-enter`: the tests of `tests/userns.rs` race on `ETXTBSY` as a user with no root |
| [T-714](fleet.md) | P2 | S | M8 | feature | done | `pod-hostprobe`: the rows of the socket classes of podssh and podnet (bind alone, listen, UDP, raw and packet sockets, the loopback) |
| [T-715](fleet.md) | P2 | S | M8 | feature | done | `pod-hostprobe`: the rows of the file classes of podssh (absent paths, the terminal, the lines of the ids, `HOME`, read-only mounts) |
| [T-716](fleet.md) | P2 | M | M8 | feature | done | `pod-lab`: the labs of the network classes of podssh and podnet (podnet's hand-off) |
| [T-717](fleet.md) | P2 | S | M8 | feature | done | the installer core: a signature of the release assets, checked where `cosign` is there |
| [T-718](fleet.md) | P2 | S | M8 | feature | done | the installer core: the test on macOS and Windows |
| [T-723](fleet.md) | P1 | S | M8 | feature | done | `pod-todo`: a kind `reference` for an issue that holds no work (PodUtils/podvm#435) |
| [T-724](fleet.md) | P1 | S | M8 | defect | done | `issue-sync.yml` builds `pod-todo` under the caller's `.cargo/config.toml` (PodUtils/podvm#437) |
| [T-725](fleet.md) | P1 | M | M8 | defect | done | `pod-manual`: four gaps that podnet's take found, and false positives of `check-man.sh` and `check-strings.py` (PodUtils/podnet#589) |
| [T-726](fleet.md) | P2 | S | M8 | defect | done | `pod-manual`: the reader of clap gives no `Rule::Needs` (clap's `requires`) |
| [T-731](relay.md) | P1 | S | M2 | defect | done | `pod-transit`: `two_ends_meet_and_carry_one_mib_each_way` failed once in CI: the client paired with a node connection that the relay stub closed |
| [T-732](foundation.md) | P2 | S | M0 | defect | done | `pod-testkit`: a server of a test harness is up only when its own child listens on its port |
| [T-736](upkeep.md) | P2 | S | M5 | defect | done | the plant `enter-userns-dev-no-remount` misses where the host's `/dev` has `nosuid` and `noexec` already |
| [T-737](fleet.md) | P1 | S | M8 | feature | done | `podlab run --allow-connect PORT`: the way out to a proxy in `bind-denied` (PodUtils/podssh#278) |
| [T-738](podvm.md) | P1 | M | M6 | feature | done | `pod-enter`: bind the volumes of a plan on the namespace rung, also through a user namespace (PodUtils/podvm#440) |
| [T-739](podvm.md) | P2 | S | M6 | defect | done | `pod-enter`: a rule of the confinement on a guest path of a volume is checked in the image, not in the volume |
| [T-742](relay.md) | P1 | S | M2 | defect | done | `pod-link`: the unix carrier fails under Landlock: the move of its staged socket gives `EXDEV` (PodUtils/podnet#592) |
| [T-743](relay.md) | P1 | S | M2 | defect | done | `pod-hold`: the netns backend needs a UDP socket for its interface ioctls, which `udp-denied` refuses (PodUtils/podnet#592) |
| [T-744](podvm.md) | P2 | S | M6 | feature | done | `pod-hostprobe`: the row `chroot(/tmp) in unshare(NEWUSER+maps)`, the chroot of a driver with no root (PodUtils/podrunner#191) |
| [T-745](fleet.md) | P2 | M | M8 | feature | done | `pod-hostprobe`: the probe rows of the network classes (a CONNECT through the proxy, the CA of a handshake, a lookup, a direct connect, a ClientHello) |
| [T-746](fleet.md) | P2 | M | M8 | feature | done | `pod-lab`: a way out through the stand-in proxy of a proxy class, for the live lanes of the consumers |
| [T-747](fleet.md) | P3 | M | M8 | feature | done | `pod-lab`: the direct network classes on each address: an SNI filter by `nf_tables`, an IPv6 device with a default route |
| [T-750](podvm.md) | P2 | M | M6 | defect | done | `pod-enter`: on an overlay root with an upper directory, the working directory and the paths of the rules are checked in the lower layer alone |
| [T-751](fleet.md) | P3 | M | M8 | feature | done | `pod-hostprobe`: the rows of the network classes that send a packet out, behind a setting of the caller that is off by default |
| [T-755](podvm.md) | P3 | S | M6 | measurement | done | `pod-enter`: the opaque attribute that overlayfs reads on an overlay root in a user namespace is not measured |
| [T-756](fleet.md) | P3 | S | M8 | feature | done | `pod-manual`: the model has no rule "needs one of" for clap's `requires` of a group |
| [T-757](transport.md) | P2 | S | M1 | defect | done | `pod-term`: the tmux oracle of the calibration test fails at random with "server exited unexpectedly" |
| [T-758](podvm.md) | P1 | S | M6 | defect | done | `pod-hostprobe`: `chroot_usable` takes the row of the leg through a user namespace as the caller's chroot (PodUtils/podvm#443) |
| [T-759](fleet.md) | P1 | M | M8 | feature | done | `pod-lab`: the stand-in proxy of a network lab reaches a server of the command on the loopback of the lab, with the rules of its class (PodUtils/podssh#282) |
| [T-760](fleet.md) | P2 | S | M8 | defect | done | `pod-lab`: a network lab keeps the other proxy variables of the host (PodUtils/podssh#282) |
| [T-761](upkeep.md) | P1 | S | M5 | defect | done | The nightly at `1739b5f` is red on each cross target: the lab test of `pod-lab` expects a seccomp lab to run where its filter cannot hold |
| [T-762](upkeep.md) | P1 | S | M5 | defect | done | The nightly at `1739b5f` is red under qemu-user: the test of the rows of `pod-hostprobe` expects no value of a mount flag that the kernel did not fill |
| [T-763](upkeep.md) | P1 | S | M5 | defect | done | The nightly at `1739b5f` is red on windows-2025: a test of `pod-link` writes a POST on TLS and reads the answer with no flush |
| [T-764](upkeep.md) | P1 | S | M5 | defect | done | The nightly at `1739b5f` is red on windows-2025: the lower bound of a silence of `pod-link`'s split test does not allow the tick of the timer of Windows |
| [T-765](upkeep.md) | P1 | S | M5 | defect | done | The nightly at `1739b5f` is red on macos-15-intel: a dial of `pod-proxy` to a freed port gets a reset, not a refusal |
| [T-766](upkeep.md) | P2 | S | M5 | defect | open | The nightly at `1739b5f` is red on riscv64: two time limits of tests that the emulation of QEMU outruns |
| [T-767](relay.md) | P1 | M | M2 | defect | partial | `pod-session`: a resume that the client calls done, after which the session never moves (PodUtils/podssh#283) |
| [T-768](relay.md) | P2 | S | M2 | defect | open | `pod-session`: a spare waits 20 s, not 1 s to 3 s, when the client's writer is held by a full link |
| [T-773](fleet.md) | P2 | S | M8 | defect | done | the installer core: a tree copy with no pins stops on a host with cosign (PodUtils/podnet#597) |
| [T-774](relay.md) | P2 | S | M2 | docs | done | `pod-link`: a normative document of the wire, with the order of the checks and the rule of the timers, held to the code by a test |
| [T-775](relay.md) | P2 | S | M2 | chore | done | `pod-link`: a table of frames in bytes with the result of each, read by one test |
| [T-776](relay.md) | P2 | M | M2 | chore | done | `pod-link`: scripted cases of a link on a paused clock, and the table of the classes of defects |
| [T-777](relay.md) | P2 | S | M2 | measurement | done | `pod-link`: a committed measurement of the link: throughput, round trips, allocations for each frame and memory |
| [T-778](relay.md) | P3 | M | M2 | feature | done | `pod-link`: no allocation for each `DATA` frame, and fewer copies of each byte |
| [T-779](upkeep.md) | P3 | S | M5 | chore | open | `pod-identity`: one stack of curve25519 and of SHA-2: take `ed25519-dalek` 3 |
| [T-780](upkeep.md) | P2 | S | M5 | defect | open | A test reads the pipes of its child only after the child ends: a child that writes more than a pipe holds stops (`pod-term` calibration) |
| [T-781](upkeep.md) | P2 | S | M5 | chore | open | The findings of the study of zmux for the tests of podssh, podnet and podvm, by COORD issues |
| [T-786](podvm.md) | P1 | S | M6 | feature | done | `pod-enter`: the ids inside the user namespace of the namespace rung, so that a payload runs as the user of its image (PodUtils/podvm#447) |
| [T-787](podvm.md) | P1 | S | M6 | feature | done | `pod-enter`: the ids of the plan for a caller that makes the namespace rung with no user namespace (root) |
| [T-788](upkeep.md) | P2 | S | M5 | defect | done | The nightly at `f309b07` is red on ppc64le: a test of `pod-enter`'s `pass_clear` reads a closed number that another test thread can take |
| [T-789](upkeep.md) | P2 | S | M5 | defect | open | The nightly at `f309b07` is red on armv7: two tests of the MQTT board of `pod-wormhole` against the stand-in |
| [T-790](upkeep.md) | P2 | S | M5 | chore | done | `pod-services`: the Syncthing relay of the device DVWOU27 moved to 1.168.112.83 |
| [T-791](podvm.md) | P3 | S | M6 | feature | open | `pod-enter`: the ids of the plan for a host root (a change of ids with no user namespace) |
| [T-792](fleet.md) | P2 | S | M8 | feature | done | `pod-lab`: with a way out, the certificate file of the lab holds the roots that the host trusts for its way out (PodUtils/podssh#285) |
| [T-793](upkeep.md) | P1 | S | M5 | defect | done | `check-sources.py` reads a file that a part take names at the commit of the take of its dir: `pod-egress` shows 3 changes |
| [T-794](upkeep.md) | P1 | S | M5 | chore | done | `pod-manual`: podnet took the crate at `9217b0b`; the `taken` line is not in `SOURCES` |
| [T-795](upkeep.md) | P1 | S | M5 | chore | done | `pod-manual`: the text fixes of podssh in `data.rs` and `flags.rs` change no form that the crate took |
| [T-796](fleet.md) | P1 | S | M8 | defect | done | `pod-lab`: the stand-in and the way out ignore SIGPIPE, so that a client that resets does not end them (PodUtils/podvm#455) |
| [T-797](fleet.md) | P3 | S | M8 | defect | done | `pod-lab`: the certificate file of a way out on a host with no bundle, and an `SSL_CERT_FILE` that cannot be read (PodUtils/podvm#455) |
| [T-798](relay.md) | P3 | S | M2 | measurement | open | `pod-link`: the tcp carrier allocates about 3 times for each `DATA` frame |
| [T-799](relay.md) | P3 | M | M2 | feature | open | `pod-link`: two copies less of each byte: a read into the buffer of the decoder, and a writer queue of bytes |
| [T-800](podvm.md) | P2 | S | M6 | feature | done | `pod-services`: the host of the virtio-win guest tools, fedorapeople.org (PodUtils/podvm#456) |
| [T-801](relay.md) | P2 | M | M2 | feature | done | `pod-relay`: a link of the resumable layer over the HTTP road, where the way refuses the upgrade of a link (PodUtils/podssh#287) |
| [T-802](relay.md) | P2 | M | M2 | feature | open | `pod-turn`: a NAT probe (the mapping, the filtering and the port of RFC 5780) on the caller's datagrams (GitHub #212) |
| [T-803](relay.md) | P3 | M | M2 | feature | open | `pod-link`: an adaptive parity datagram for the ARQ, off on a clean link (GitHub #213) |
| [T-804](podvm.md) | P1 | S | M5 | defect | done | `pod-qga` has no `LICENSE` beside its manifest; a check of each crate that cargo packs (PodUtils/podvm#456) |
| [T-805](relay.md) | P2 | S | M2 | feature | done | `pod-relay`: a node that gets `409` for its own lost socket gives up on that route sooner than `rejoin`, and says so (PodUtils/podnet#604) |
| [T-806](podvm.md) | P1 | M | M6 | feature | done | `pod-enter`: a PID namespace with its own `/proc` on the namespace rung, which a join enters (PodUtils/podvm#457) |
| [T-807](fleet.md) | P2 | M | M5 | feature | done | `pod-manual`: four forms that podssh's manual needs before its text, help and JSON move to the crate (PodUtils/podssh#288) |
| [T-808](roads.md) | P2 | S | M4 | defect | done | `pod-iroh`: the fork of iroh-relay sends the name of the relay to a `socks5://` proxy (T-037) |
| [T-809](roads.md) | P3 | M | M4 | feature | open | `pod-iroh`: the relay dial of iroh takes a proxy URL, not the caller's dialer (T-037) |
| [T-810](roads.md) | P2 | M | M4 | chore | done | `pod-iroh`: the adoption trial of podssh (T-037) |
| [T-811](podvm.md) | P1 | S | M6 | defect | done | `pod-enter`: the init of a PID namespace keeps the descriptors of the caller, so `run -d` never returns (PodUtils/podvm#460) |
| [T-812](relay.md) | P2 | M | M2 | measurement | partial | `pod-relay`: a measurement by hand of the throughput of the hosted relays (the default host and its seeds) |
| [T-813](relay.md) | P3 | S | M2 | research | done | `pod-ws`: the speed of a relay path in two modes (75 and 330 MiB/s) that the dig study saw |
| [T-814](relay.md) | P3 | M | M2 | feature | partial | `pod-relay`: the bytes and the time of each session for each host, and an order by measured rate in shadow |
| [T-815](relay.md) | P3 | S | M2 | research | done | `pod-relay`: the budget of a session under a relay whose caps are larger than the default relay's |
| [T-816](upkeep.md) | P2 | S | M5 | docs | done | The dig study and the ingest study of 2026-11-16 as research documents, with their corrections |
| [T-817](upkeep.md) | P2 | S | M5 | chore | done | A call on references under GPL or AGPL: their designs, never their code |
| [T-818](relay.md) | P3 | L | M2 | feature | open | `pod-link`: forward error correction and a NACK backstop (TR-1, TR-2 of the ingest study) |
| [T-819](relay.md) | P3 | M | M2 | feature | partial | `pod-netstack`: a ladder of the path MTU, a clamp of the MSS, and a class for each flow (TR-3, TR-4 of the ingest study) |
| [T-820](relay.md) | P3 | L | M2 | feature | open | The datapath: GSO with a fate rule, rings for each worker, a pool of source ports, the offload of TUN, a table of MAC addresses in O(1) (DP-1 to DP-5 of the ingest study) |
| [T-821](relay.md) | P3 | L | M2 | feature | open | The wire: a keyed mask of the header of `pod-e2e`, cover traffic, a JA4 audit of the hello of `pod-tls`, an ML-KEM hybrid (WR-1 to WR-5 of the ingest study) |
| [T-822](transport.md) | P3 | L | M1 | feature | open | `pod-identity`: a threshold CA of the grants, an authorization of enrollment, a refresh of the shares (PK-1 to PK-7 of the ingest study) |
| [T-823](transport.md) | P3 | M | M1 | feature | partial | Routing: tokens in a proxy credential, sticky selection by HRW with tiers, a chain length, a quota that reserves before a write (RT-1 to RT-5 of the ingest study) |
| [T-824](foundation.md) | P3 | M | M0 | chore | done | Verification gates: no allocation in `pod-e2e`, bounded steps, no blocking call, exhaustive checksum equivalence, a record of results, test runs with each feature (VF-1 to VF-5 of the ingest study) |
| [T-825](transport.md) | P3 | M | M1 | feature | partial | `pod-proxy`: a plan of the dial by address family, with the families of both ends and a timeout for each attempt |
| [T-826](transport.md) | P3 | M | M1 | feature | partial | `pod-identity`: a rotation of the identity key with two slots and a retire step |
| [T-827](foundation.md) | P3 | S | M0 | feature | done | `pod-core`: a type for the text of a peer in a field of an error |
| [T-828](transport.md) | P3 | S | M1 | research | done | `pod-services`: the DIG relay, its measured limits and its framing, or a call that names it unfit |
| [T-829](relay.md) | P3 | S | M2 | chore | done | A COORD issue to podssh: the ideas of the dig study for the relay server (a ban after strikes, the PROXY header, a STUN responder, a sweep of liveness) |
| [T-830](fleet.md) | P3 | M | M8 | feature | open | Updates: a gate of health with a rollback, a check of loadability, a store of logs, scheduled updates, a floor of rollback in the feed (report 15 of the dig study, with T-104) |
| [T-831](foundation.md) | P3 | S | M0 | feature | done | `pod-core`: one table of the loopback addresses that a process takes, with a test that it can fail |
| [T-832](fleet.md) | P3 | L | M8 | research | blocked | The intake of the dig study for podswarm: NAT traversal, a DHT, gossip, sealed messages, content addressing, storage, sealed secrets |
| [T-833](fleet.md) | P3 | S | M8 | feature | done | `pod-control`: a test that the catalogue is the dispatch, tiers of surfaces, a budget of callbacks (reports 03, 11, 12 of the dig study, with T-103) |
| [T-834](relay.md) | P2 | S | M2 | defect | done | `pod-relay`: a renewal reads the default relay's `400` for an unknown reverse path as an error, not as no renewal (PodUtils/podssh#294) |
| [T-835](relay.md) | P3 | S | M2 | measurement | done | `pod-relay`: the cost of a move of a session for a copy on the default relay |
| [T-836](relay.md) | P2 | S | M2 | feature | done | `pod-relay`: a route of a node to a Unix socket (PodUtils/podssh#295) |
| [T-837](relay.md) | P2 | S | M2 | defect | done | `pod-relay`: a node ends a session whose local side still reads, at the first MiB that waits for it |
| [T-838](foundation.md) | P3 | M | M0 | chore | partial | Gates of a bounded step and of no blocking call in a hot path (VF-2, VF-3 of the ingest study) |
| [T-839](transport.md) | P2 | S | M1 | defect | done | `pod-services`: localhost.run's remote forward takes port 80, not 0 (PodUtils/podnet#607) |
| [T-840](transport.md) | P2 | S | M1 | chore | done | `pod-tls`: webpki-roots 1.0.12 and its date (PodUtils/podssh#297) |
| [T-841](transport.md) | P3 | S | M1 | chore | done | `pod-tls`: rsa 0.10, so that a consumer has one version of rsa (PodUtils/podssh#297) |
| [T-842](adapters.md) | P3 | S | M3 | defect | open | `pod-cloudflare`: a shutdown of a tunnel on the QUIC leg closes its connection before `shutdown` returns |
| [T-843](adapters.md) | P3 | S | M0 | chore | done | `static-probe`: its `main.rs` is at the limit of 500 lines |
| [T-844](relay.md) | P2 | S | M2 | feature | done | `pod-relay`: a route to a Unix socket on Windows (PodUtils/podssh#298) |
| [T-845](relay.md) | P2 | M | M2 | feature | done | `pod-relay` and `pod-session`: a relay's liveness offer, so a relay that sleeps between frames can sleep (PodUtils/podssh#300) |
| [T-846](podvm.md) | P2 | S | M6 | defect | done | `pod-rootfs`: the source fixup reads the URLs of comment lines, so it probes `help.ubuntu.com` (PodUtils/podvm#463) |
| [T-847](relay.md) | P2 | M | M2 | feature | done | `pod-ws`, `pod-relay` and `pod-egress`: headers of the caller for a host on each WebSocket road, for Cloudflare Access (PodUtils/podssh#302) |
| [T-848](relay.md) | P3 | S | M2 | feature | done | `pod-relay`: the headers of the caller on the fetches of `/relays.json` |
| [T-849](adapters.md) | P2 | S | M3 | chore | done | `pod-cloudflare`: the watches that podnet ran: the pin of cloudflared, the header oracle, the planted loop of Cap'n Proto |
| [T-850](adapters.md) | P3 | S | M0 | chore | done | `pod-core`: one clean text of a peer: the strip of escape sequences and the words of a TLS failure |
| [T-851](adapters.md) | P3 | S | M3 | chore | partial | `pod-cloudflare`: the pin of cloudflared moves to 2026.11.0 |
| [T-852](transport.md) | P3 | S | M1 | chore | open | The ignores of the unmaintained chrono and humantime (RUSTSEC-2026-0543, RUSTSEC-2026-0542) |
| [T-853](adapters.md) | P3 | S | M3 | chore | done | `pod-cloudflare`: two `Protocol` types, one of them with no user in the crate |
| [T-854](upkeep.md) | P2 | S | M1 | chore | done | `pod-services`: GitHub Actions has two more hosts (the watch of 2026-11-17) |
| [T-855](upkeep.md) | P3 | S | M1 | chore | done | `pod-services`: the node `node-trojan-167-88-61-175` fails the TLS handshake |
| [T-856](relay.md) | P3 | S | M4 | feature | open | The wire vectors of `tokens.json` through the relay role's tokens |
| [T-857](podvm.md) | P1 | S | M6 | defect | done | `pod-enter`: a copy of a tree refuses the device nodes of the fixups, so a run of root on `rundir` or `tmpfs` never stages (PodUtils/podvm#467) |
| [T-858](transport.md) | P1 | S | M1 | defect | done | `pod-tls`: `Trust::Default` does not read the CA file of the settings (PodUtils/podnet#608) |
| [T-859](fleet.md) | P2 | S | M5 | defect | done | `pod-actions-workflow`: the error of an unknown need gives the place of the job, not of the need (PodUtils/podrunner#214) |
| [T-860](relay.md) | P2 | S | M2 | defect | done | `pod-piping`: the probe of a node's upgrade sends no header of the caller (PodUtils/podssh#306) |
| [T-861](podvm.md) | P1 | S | M7 | defect | done | `pod-guest`: a mailbox run that times out before `WQREADY` tells nothing of the agent (PodUtils/podvm#468) |
| [T-862](upkeep.md) | P1 | S | M5 | defect | done | `pod-broker`: names mode still resolves each name on the host, so a host whose names resolve only through its proxy refuses each CONNECT (PodUtils/podrunner#215) |
| [T-863](fleet.md) | P2 | S | M0 | chore | open | `pod-manual`: the changes of podssh to the imported files before its take (port review) |
| [T-864](podvm.md) | P2 | S | M6 | defect | done | `pod-enter`: a new devpts instance and `/dev/ptmx` on the namespace rung, so that `openpty` works in a unit (PodUtils/podvm#469) |
| [T-865](relay.md) | P2 | S | M2 | feature | done | `pod-relay`: keep a relay member's `meet` from its `/relays.json` (PodUtils/podssh#307) |
| [T-866](fleet.md) | P2 | S | M0 | feature | done | `pod-lab`: a profile of the three limits together: no `AF_INET` bind, no network namespace, no write of another process's memory (PodUtils/podnet#649) |
| [T-867](fleet.md) | P2 | S | M0 | feature | done | `pod-manual`: a check of each example of the model against the consumer's parser (PodUtils/podnet#649) |
| [T-868](podvm.md) | P3 | S | M6 | chore | done | `pod-enter`: a test of the fallback where the kernel refuses the devpts instance of the unit |
| [T-869](relay.md) | P2 | S | M2 | defect | done | `pod-relay`: `OpenError::lines` says the cause of a refusal twice (PodUtils/podssh#309) |
| [T-870](upkeep.md) | P2 | S | M0 | chore | done | CI: each crate builds alone with its default features |
| [T-871](roads.md) | P1 | S | M2 | defect | done | `pod-ws`: a shared HTTP/2 connection dies with the runtime that opened it (PodUtils/podssh#310) |
| [T-872](podvm.md) | P1 | S | M6 | defect | done | `pod-supervise`: the temp file of the table of containers keeps no lock, so a sweep of another process deletes it before its rename (PodUtils/podvm#568) |
| [T-873](podvm.md) | P1 | S | M6 | defect | done | `pod-hostprobe`: the probe file of `probe_dir_exec` has a name that two processes of two PID namespaces share (PodUtils/podvm#568) |
| [T-874](podvm.md) | P2 | S | M6 | defect | done | `pod-oci`: a credential helper gets `ServerURL` `https://HOST/v2/`, not the key of the configuration (PodUtils/podvm#568) |
| [T-875](podvm.md) | P2 | S | M6 | feature | done | `pod-supervise`: `run -d --rm` keeps the record and the tree after the end (PodUtils/podvm#568) |
| [T-876](podvm.md) | P2 | M | M6 | feature | done | `pod-enter`: a volume whose source a link cannot redirect between the check of the caller and the bind (PodUtils/podvm#568) |
| [T-877](foundation.md) | P1 | M | M0 | feature | done | `pod-core`: one advisory file lock for each system, in place of four copies in the consumers |
| [T-878](foundation.md) | P2 | S | M0 | feature | done | `pod-core`: one injectable clock, in place of the copies of podrunner and of three crates |
| [T-879](foundation.md) | P2 | M | M0 | feature | open | `pod-detach`: the detach of a command by a re-exec, from podnet, in place of podssh's near copy |
| [T-880](foundation.md) | P2 | M | M0 | feature | open | `pod-release`: the self-update of a binary (fetch, verify, replace, trial run), from podnet and podssh |
| [T-881](foundation.md) | P2 | S | M0 | feature | done | `pod-tls`: the CA bundle that the environment names, in place of three lists in the consumers |
| [T-882](foundation.md) | P3 | M | M0 | chore | done | One reader and writer of an HTTP/1.1 head, in place of the copies in podrunner and in five crates |
| [T-883](fleet.md) | P2 | S | M0 | defect | done | the installer core: one proxy line for one proxy, and a default prefix on a noexec mount (PodUtils/podnet#650) |
| [T-884](podvm.md) | P1 | S | M6 | defect | done | `pod-enter`: SIGHUP is not blocked and forwarded to the payload (PodUtils/podvm#569) |
| [T-885](podvm.md) | P1 | S | M6 | defect | done | `pod-enter`: a fallback to chroot is not returned to the caller (PodUtils/podvm#569) |
| [T-886](podvm.md) | P2 | S | M6 | defect | done | `pod-enter`: a working directory under `/tmp` gives ENOENT (PodUtils/podvm#569) |
| [T-887](podvm.md) | P2 | S | M6 | defect | done | `pod-supervise`: `logs -f` ends at a restart (PodUtils/podvm#569) |
| [T-888](podvm.md) | P1 | S | M6 | defect | done | `pod-supervise`: the log, the lock and the memo of a container follow a symbolic link (PodUtils/podvm#569) |
| [T-889](podvm.md) | P2 | S | M6 | defect | done | `pod-rootfs`: `run_mtab` says /proc is not mounted, where the entry mounts it (PodUtils/podvm#569) |
| [T-890](podvm.md) | P2 | S | M6 | defect | done | `pod-hostprobe`: the confinement key reads the namespaces of the reading process (PodUtils/podvm#569) |
| [T-891](podvm.md) | P1 | S | M6 | defect | done | `pod-hostprobe`: the census probe of exec uses the fixed path `/tmp/execprobe` (PodUtils/podvm#569) |
| [T-892](podvm.md) | P3 | S | M6 | defect | done | `pod-hostprobe`: the never-claim text of the namespace rung is out of date (PodUtils/podvm#569) |
| [T-893](podvm.md) | P1 | S | M6 | defect | done | `pod-guest`: QEMU starts in its own process group with no `PR_SET_PDEATHSIG` (PodUtils/podvm#569) |
| [T-894](podvm.md) | P2 | S | M6 | defect | done | `pod-oci`: a wanted platform variant matches an entry with no variant (PodUtils/podvm#569) |
| [T-895](podvm.md) | P3 | S | M6 | defect | done | `pod-extract`: a hard link to a later member says "already extracted" (PodUtils/podvm#569) |
| [T-896](foundation.md) | P2 | S | M3 | feature | done | `pod-proxy`: the handshake of SOCKS5 and of CONNECT on a stream that the caller holds (a hop of a chain) (PodUtils/podnet#652) |
| [T-897](foundation.md) | P2 | M | M3 | feature | open | `pod-proxy`: the server checks a user and a password by a callback; one SOCKS server for podnet and podssh (PodUtils/podnet#652) |
| [T-898](foundation.md) | P3 | S | M3 | feature | done | `pod-dns`: an answer with an RCODE that copies the flags of the request (PodUtils/podnet#652) |
| [T-899](fleet.md) | P2 | M | M3 | feature | open | `pod-hostprobe`: rows of a network namespace with a veth, a write of another process's memory, and the binds of AF_UNIX, abstract, VSOCK and NETLINK sockets (PodUtils/podnet#652) |
| [T-900](fleet.md) | P3 | S | M3 | feature | open | The installer core: more downloaders, or a ruling, and the newest tag with an optional token (PodUtils/podnet#652) |
| [T-901](roads.md) | P3 | S | M3 | feature | done | `pod-ws`: a request of the upgrade with no token header, for a probe (PodUtils/podnet#652) |
| [T-902](fleet.md) | P2 | S | M5 | defect | done | `pod-actions-store`: `Lock::parse` keeps a digest in uppercase, and each fetch then fails (PodUtils/podrunner#288) |
| [T-903](roads.md) | P2 | S | M2 | defect | done | `pod-dns`: one line of 1000 characters, where each resolver of DNS over HTTPS failed alike (PodUtils/podssh#313) |
| [T-904](relay.md) | P3 | S | M2 | defect | done | `pod-relay`: a relay that issues no token names no way to give one (PodUtils/podssh#313) |
| [T-905](fleet.md) | P2 | S | M0 | feature | done | `pod-lab`: a row of a failure in a lane: `podlab run --expect-exit CODE --expect-stderr TEXT` (PodUtils/podnet#649) |
| [T-906](fleet.md) | P1 | S | M0 | defect | done | `pod-actions-store`: the ref of a checkout of another repository: empty, a full ref, a commit SHA (PodUtils/podrunner#290) |
| [T-907](fleet.md) | P2 | M | M0 | feature | done | A reader of the zone files (TZif) and the cron of `on.schedule` in its `timezone` (PodUtils/podrunner#291) |
| [T-908](relay.md) | P2 | S | M0 | defect | done | `pod-session`: the Close of the relay in the drain wins over the error of a write (PodUtils/podssh#314, ask 1) |
| [T-909](relay.md) | P2 | S | M0 | defect | done | `pod-relay`: the link budget of the layer under a small cap of the relay; the cap in `forward_layer::Offer` (PodUtils/podssh#314, ask 2) |
| [T-910](foundation.md) | P2 | S | M0 | defect | done | `pod-testkit`: an address that refuses each connect and that no other test can take (PodUtils/podnet#653) |
| [T-911](roads.md) | P2 | S | M0 | defect | done | `pod-proxy`: the same address twice in a race and in its error line (PodUtils/podssh#315, ask 1) |
| [T-912](relay.md) | P2 | S | M0 | defect | done | `pod-session`: a far end that lost a session by a restart is said as one that timed it out (PodUtils/podssh#315, ask 2) |
| [T-913](relay.md) | P2 | S | M0 | defect | done | `pod-relay`: the cleartext road does not say why the TLS of the relay failed (PodUtils/podssh#315, ask 3) |
| [T-914](upkeep.md) | P1 | S | M5 | defect | done | `pod-cloudflare`: the reader of Cap'n Proto multiplies with overflow on a 32-bit target (the nightly at `62ce717`) |
| [T-915](upkeep.md) | P2 | S | M5 | defect | done | `pod-cloudflare`: the test of the quick tunnel stand-in does not build on Windows and counts two registrations on macOS (the nightly at `62ce717`) |
| [T-916](upkeep.md) | P2 | M | M5 | defect | partial | The gate of no blocking call and eight time bounds fail under the emulation of QEMU (the nightly at `62ce717`) |
| [T-917](upkeep.md) | P2 | M | M5 | chore | open | The copies of product code that `codegraph.py dups` finds: one place for each |
| [T-918](upkeep.md) | P3 | M | M5 | chore | open | The helpers that the tests copy from file to file: one place for each |
| [T-919](upkeep.md) | P1 | M | M0 | chore | done | A code graph, a check of repeated work and a check of narrative history in the documents |
| [T-920](upkeep.md) | P2 | S | M5 | defect | open | `pod-guest`: a monitor socket path longer than `sun_path` waits for the whole bound, then fails with a wrong reason |
| [T-921](fleet.md) | P1 | L | M8 | feature | done | A fake GitHub as a binary for a consumer's CI: the App, the JIT configuration, the `workflow_job` webhooks and the runner broker (GitHub #223) |
| [T-922](fleet.md) | P1 | M | M8 | feature | done | The schemas of `schemas/` with one source each, a check of each against its types, and a line of each change (GitHub #223) |
| [T-923](fleet.md) | P1 | M | M8 | feature | done | `pod-job`: the fields of the job that githome and podrunner need (`trust_inputs.forge`, `limits.disk`, `cache`, the facts of a run report) (GitHub #223) |
| [T-924](foundation.md) | P1 | S | M0 | feature | done | `pod-core`: a lock wait that a signal ends, and a lock file that the caller cannot write (PodUtils/podrunner#295) |
| [T-925](fleet.md) | P1 | S | M7 | feature | done | `pod-incus`: the state of an instance, a profile with its devices, and the OOM count (PodUtils/podvm#582) |
| [T-926](relay.md) | P2 | S | M2 | defect | done | `pod-relay`: a refusal of the relay's policy no longer says the relay's cause (PodUtils/podssh#318) |
| [T-927](fleet.md) | P2 | S | M6 | feature | open | `pod-enter`: a new session with the pty as the controlling terminal of the payload (PodUtils/podvm#584) |
| [T-928](foundation.md) | P3 | M | M0 | chore | open | The other readers of an HTTP/1.1 head take `pod_core::http1` |
| [T-929](fleet.md) | P1 | S | M8 | defect | done | `static-probe`: the report of `pod-job` lacks the new fields of `RunReport`, so the workspace does not build |
| [T-930](services.md) | P2 | S | M5 | defect | done | `pod-services`: the Syncthing relay DVWOU27 moved to 61.223.4.2; cloudflared 2026.11.1 changed `quick_tunnel.go` (services 44282821174) |
| [T-931](adapters.md) | P3 | S | M3 | defect | done | `pod-cloudflare`: a quick tunnel opens two edge connections; cloudflared opens one |
| [T-932](upkeep.md) | P2 | M | M5 | chore | open | A check of the temp files that the crates of podcrates make (PodUtils/podvm#567, lesson 5) |
| [T-933](upkeep.md) | P2 | S | M5 | defect | partial | `pod-cloudflare`: three tests fail on Windows, where they ran for the first time (nightly 44275593101) |
| [T-934](transport.md) | P2 | S | M1 | defect | open | `pod-proxy`: an exec proxy runs `sh -c`, which breaks a Windows path and needs `sh` on Windows |
| [T-935](foundation.md) | P2 | S | M0 | defect | open | `pod-core`: two renames onto one name at once give "Access is denied" on Windows |
| [T-936](foundation.md) | P1 | S | M0 | defect | done | `pod-testkit`: `refused()` gives a timeout on macOS, not a refusal |
| [T-937](transport.md) | P3 | S | M1 | defect | done | `pod-dns`: a test of the resolver asks the resolver of the host (nightly 44275593101) |
| [T-938](upkeep.md) | P2 | S | M5 | defect | partial | The riscv64 job of `nightly.yml` meets its limit of 90 minutes |
| [T-939](relay.md) | P2 | M | M2 | defect | done | `pod-iroh` with `pod-session`: after a new address of the client on the relay-only path, no end resumes, and the bytes in flight are never sent again (PodUtils/podssh#283, ask 4) |
| [T-940](fleet.md) | P2 | S | M8 | feature | open | `pod-job`: a fuzz target for one line of the source wire |
| [T-941](foundation.md) | P2 | S | M0 | feature | open | `pod-core`: a lock of a file that other programs read, which on Windows refuses no reader (PodUtils/podssh#319) |
| [T-942](relay.md) | P3 | S | M0 | defect | open | `pod-relay`: a kept offer of the layer loses `forward.liveness` |
| [T-943](upkeep.md) | P2 | S | M5 | defect | done | The static job of `aarch64-apple-darwin` in `ci.yml` gets no runner, four runs in a row |
| [T-944](podvm.md) | P1 | S | M6 | defect | done | `pod-enter`: the head of a payload cannot give a `PT_INTERP` path that lies past it, so Fedora 42's programs do not run on the userland rung (PodUtils/podvm#589) |
| [T-945](relay.md) | P2 | M | M2 | feature | done | `pod-session`: the client says no wait of its own, so a hang with the node's `session.stall` cannot tell a loss on the link from bytes held for the client's application (PodUtils/podssh#283, ask 4) |
| [T-946](fleet.md) | P2 | S | M8 | feature | open | `pod-fake-github` and `pod-fake-githome`: the help and the manual from `pod-manual`, with the checks of each drift |
| [T-947](fleet.md) | P1 | S | M8 | feature | done | `pod-job`: `status.v1` has no `draining` of a source, no `max_units` and no `units` of a host, so podrunner keeps its own `Status` (PodUtils/podrunner#305) |
| [T-948](fleet.md) | P1 | M | M8 | feature | done | `pod-testkit`: a fake of githome's source, as a program that podrunner's CI can pin (PodUtils/podrunner#306, githome's G-38 and G-40) |
| [T-949](upkeep.md) | P2 | S | M2 | defect | done | `pod-link`: the gate of E2-1 at 20 % drop divides by a clean time of some 50 ms, so the noise of one run moves the factor past its bound (nightly 44388345504) |
| [T-950](upkeep.md) | P2 | M | M2 | defect | done | `pod-link`: when one half of a split session goes silent, the end that sees the far end close first names the other half as lost (windows-2025) |
| [T-951](fleet.md) | P1 | S | M8 | feature | done | `pod-testkit`: `pod-fake-githome` passes the 14 tests of podrunner's `source-conformance`, not its test 15 of a cancelled job (PodUtils/podrunner#310) |
| [T-952](fleet.md) | P1 | M | M8 | feature | done | `pod-testkit`: `pod-fake-githome` mints made-up files, so no runner of a job through podrunner's `serve` reaches the broker of `pod-fake-github` (PodUtils/podrunner#311) |
| [T-953](relay.md) | P1 | S | M2 | defect | done | `pod-relay`: under a small cap of the relay, a new link sends again more bytes than the cap, so the relay still ends links with 1009 (PodUtils/podssh#320) |
| [T-954](upkeep.md) | P1 | M | M8 | feature | done | The release binaries of podcrates are not on ghcr.io (the owner's ruling "ghcr.io", PodUtils/podrunner#312) |
| [T-955](upkeep.md) | P2 | S | M0 | chore | done | Dependabot of 2026-11-19: coturn 4.19.0, TruffleHog 3.99.9, rand 0.10.4 and rustls-pki-types; simple-dns 0.13.0 declined (GitHub #232, #233, #234, #235) |
| [T-956](podvm.md) | P1 | S | M6 | defect | done | `pod-extract`: a layer of more than one gzip member or zstd frame is cut short after the first, with no error (PodUtils/podrunner#317, ask 2) |
| [T-957](podvm.md) | P1 | M | M6 | feature | done | `pod-extract`: no extended attribute of a layer is applied, so `security.capability` of `ping` is lost (PodUtils/podrunner#317, ask 1) |
| [T-958](relay.md) | P1 | M | M2 | defect | done | `pod-relay` with `pod-session`: a move at three quarters of a small cap of the relay still passes the cap, so the relay ends most links with 1009 (PodUtils/podssh#325) |
| [T-959](relay.md) | P1 | S | M2 | feature | done | `pod-e2e`: the channel says no wait of its own, so a hang where the layer holds bytes for the channel cannot tell the channel from its reader (PodUtils/podssh#283, ask 5) |
| [T-960](relay.md) | P3 | S | M2 | feature | done | `pod-term`: a cooked session always draws its prompt, so an exec'd command on podssh's server shows `$ ` between and after its lines (PodUtils/podssh#326) |
| [T-961](podvm.md) | P2 | M | M6 | defect | done | `pod-extract`: the `tar` crate reads a PAX header or a GNU long name whole with no bound, and cuts the PAX records at each newline byte |
| [T-962](relay.md) | P1 | S | M2 | defect | done | `pod-session`: the hold of a move starts when the task beside the link wakes, not when the bytes pass the threshold, so on Windows the old link still meets a small cap (PodUtils/podssh#327) |
| [T-963](relay.md) | P2 | S | M2 | defect | partial | `live.yml`: the public Nostr relay `offchain.pub` pairs no two ends from a runner of GitHub, and pairs them from a cloud session |
| [T-964](fleet.md) | P2 | M | M8 | feature | open | `pod-release`: the bundle of Rekor v2: the inclusion proof with its signed checkpoint, and the RFC 3161 timestamp |
| [T-965](fleet.md) | P2 | M | M8 | feature | open | `pod-release`: the verifier as a static command for the CI of a product and for an installer |
| [T-966](fleet.md) | P2 | M | M8 | chore | open | `pod-release`: the adoption trials of podnet and podssh, and the hand-off of the crate |
| [T-967](fleet.md) | P3 | S | M8 | feature | open | `pod-release`: a minisign signature beside the Sigstore bundle |
