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

**173 entries: 62 open, 3 partial, 0 blocked, 108 done.**

| Priority | open | partial | blocked | done | total |
| --- | --- | --- | --- | --- | --- |
| P0 | 0 | 0 | 0 | 2 | 2 |
| P1 | 17 | 2 | 0 | 36 | 55 |
| P2 | 31 | 1 | 0 | 53 | 85 |
| P3 | 14 | 0 | 0 | 17 | 31 |
| **All** | 62 | 3 | 0 | 108 | 173 |

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
| [T-020](relay.md) | P1 | L | M2 | feature | partial | `pod-session`: the resumable layer and UDP in a stream |
| [T-021](relay.md) | P1 | M | M2 | feature | open | `pod-e2e`: the end-to-end channel (Noise XX) |
| [T-022](relay.md) | P1 | S | M2 | feature | partial | `pod-wormhole`: the mailbox client, SPAKE2 and the code words |
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
| [T-108](podvm.md) | P1 | M | M7 | feature | open | `pod-confine`: Landlock, a seccomp deny filter, no_new_privs and rlimits from one profile (GitHub #35) |
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
| [T-122](relay.md) | P2 | S | M2 | feature | open | `pod-relay`: a health memory of the relay hosts (PodUtils/podnet#413) |
| [T-123](relay.md) | P2 | S | M2 | feature | open | `pod-relay`: the session budget from the pool document (PodUtils/podnet#413) |
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
| [T-142](podvm.md) | P3 | S | M6 | measurement | open | `pod-oci`: a live pull from a public registry |
| [T-143](foundation.md) | P1 | S | M5 | defect | done | `plants.yml`: four plants are stale and two miss |
| [T-144](podvm.md) | P2 | M | M6 | feature | done | `pod-enter`: the volume pairs in the maps table; the workdir and the first program through a guest-to-host resolver (PodUtils/podvm#224) |
| [T-145](podvm.md) | P2 | S | M6 | feature | done | `pod-hostprobe`: `Statfs::f_flags` (PodUtils/podvm#224) |
| [T-146](fleet.md) | P2 | L | M8 | research | open | `podrunner`: run GitHub CI and other CI on any machine (the plan, GitHub #47) |
| [T-147](podvm.md) | P3 | S | M5 | chore | done | `pod-services`: podvm took `pod-oci`: the SOURCES block of podvm reads the take |
| [T-148](podvm.md) | P2 | S | M6 | defect | done | `pod-hostprobe`: `fstatat` has no fallback to `statx` (loongarch64 before Linux 6.11) |
| [T-149](podvm.md) | P3 | S | M6 | defect | open | `pod-enter`: three tests fail under qemu-user only |
| [T-150](relay.md) | P3 | S | M2 | feature | open | `pod-relay`: a handler target for the relay node (PodUtils/podnet#413) |
| [T-151](transport.md) | P2 | M | M1 | feature | open | `pod-services`: the watch scripts of podssh and podnet that the watches replace leave them |
| [T-152](transport.md) | P3 | M | M1 | feature | open | `pod-services-watch`: `proxy-banner` speaks no VLESS over XHTTP |
| [T-153](transport.md) | P2 | S | M1 | feature | done | `pod-services`: the services `serveo` and `ssh-j` (PodUtils/podssh#182) |
| [T-154](foundation.md) | P2 | S | M5 | defect | done | Timed tests under an emulator: one bound of the test kit and a sweep |
| [T-155](podvm.md) | P3 | S | M6 | feature | done | `pod-enter`: the refusal of a missing `-w` names its volume; the reserved guest paths and the bound of pairs in `Maps::new` (PodUtils/podvm#226) |
| [T-156](transport.md) | P2 | S | M1 | feature | done | `pod-services`: the service `croc` (PodUtils/podssh#183) |
| [T-157](transport.md) | P2 | S | M1 | defect | open | `pod-services`: four public Trojan nodes fail the live check |
| [T-158](transport.md) | P2 | S | M1 | defect | done | `pod-services`: the watch of `croc` upgrades with no relay, and the bridge refuses it |
| [T-159](relay.md) | P2 | S | M2 | defect | done | podnet: two QUIC connections on one WARP socket share one waker of the full send queue |
| [T-160](fleet.md) | P2 | S | M8 | feature | open | `pod-broker`: a listener bound to one job, with no token in the unit (PodUtils/podvm#227) |
| [T-161](fleet.md) | P3 | M | M8 | feature | open | `pod-broker`: send the name to an upstream that takes names only (PodUtils/podvm#227) |
| [T-162](podvm.md) | P2 | M | M8 | feature | done | `pod-supervise`: kill the tree, a wall timeout, rusage and the count of residual processes (PodUtils/podvm#227) |
| [T-163](fleet.md) | P2 | M | M8 | feature | open | Schemas and types: job offer, unit capability, run report and public report (PodUtils/podvm#227) |
| [T-164](transport.md) | P3 | S | M8 | feature | open | `pod-services`: the Actions control-plane hosts and the pin of `actions/runner`, with watches (PodUtils/podvm#227) |
| [T-165](transport.md) | P3 | M | M8 | feature | open | `pod-tls`: mutual TLS with pinned keys (PodUtils/podvm#227) |
| [T-166](podvm.md) | P2 | L | M1 | feature | done | `pod-hold`: the hold of each connect of a payload, from podnet's `podnet-netns` (PodUtils/podnet#422) |
| [T-167](relay.md) | P2 | S | M2 | defect | done | `pod-session`: a link meets its cap under qemu-user in `a_session_moves_before_each_cap` |
| [T-168](upkeep.md) | P3 | S | M5 | defect | done | Tests that bind a Unix socket under the temporary directory fail when its path is long |
| [T-169](podvm.md) | P2 | M | M1 | feature | open | `pod-hold`: the dial state of a held `connect` of the seccomp hold, from podnet's `connect.rs` |
| [T-170](podvm.md) | P2 | M | M1 | feature | open | `pod-hold`: the netns hold of a SYN (dial first, then accept or RST), from podnet's netns router |
| [T-171](transport.md) | P2 | S | M1 | feature | open | `pod-services`: the services `localhost-run`, `localtunnel` and `bore` (PodUtils/podssh#186) |
| [T-172](transport.md) | P3 | S | M2 | feature | open | `pod-session`: `Rewound::new`, a public constructor (PodUtils/podssh#186) |
| [T-173](podvm.md) | P2 | M | M6 | feature | open | Fuzz targets of four parsers of untrusted input: `pod-oci` (two), `pod-extract`, `pod-enter` (PodUtils/podvm#241) |
