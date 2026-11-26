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

**1043 entries: 170 open, 31 partial, 6 blocked, 836 done.**

| Priority | open | partial | blocked | done | total |
| --- | --- | --- | --- | --- | --- |
| P0 | 0 | 0 | 0 | 2 | 2 |
| P1 | 4 | 9 | 0 | 245 | 258 |
| P2 | 76 | 12 | 4 | 435 | 527 |
| P3 | 90 | 10 | 2 | 154 | 256 |
| **All** | 170 | 31 | 6 | 836 | 1043 |

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
| [T-100](fleet.md) | P1 | L | M8 | feature | done | `pod-broker`: TLS end with a CA for each job, placeholder credentials and the scrub of answers (GitHub #35, #36) |
| [T-101](foundation.md) | P1 | S | M1 | feature | done | `pod-core`: the classes Revoked, Policy and Unsupported, one exit code for each class, one JSON log schema (GitHub #35) |
| [T-102](fleet.md) | P1 | M | M8 | feature | done | `pod-unit`: hardened service units for systemd, launchd and Windows, with no secret in a unit (GitHub #35) |
| [T-103](fleet.md) | P1 | M | M8 | feature | done | `pod-control`: one JSON control API on a Unix socket or a named pipe (GitHub #35, #36) |
| [T-104](fleet.md) | P1 | M | M8 | feature | done | `pod-release`: the release index, its writer for CI and its verifier (GitHub #35, #36) |
| [T-105](foundation.md) | P1 | M | M8 | feature | done | `pod-todo`: an acceptance registry of ask ids and their tests (GitHub #35) |
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
| [T-131](roads.md) | P1 | M | M4 | defect | partial | `pod-gsr`: the handshake tests crash on Windows ARM64 |
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
| [T-501](transport.md) | P2 | M | M1 | feature | done | `pod-turn`: the TCP relay of RFC 6062 (PodUtils/podnet#521) |
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
| [T-526](relay.md) | P3 | S | M2 | chore | partial | `pod-link`: the arithmetic of the crate `kcp` wraps by design: no overflow checks for it, and the fix kept as a patch |
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
| [T-538](upkeep.md) | P3 | S | M5 | docs | done | The rule of attribution of a comment on GitHub: the documents of podcrates and the practice of its sessions differ |
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
| [T-766](upkeep.md) | P2 | S | M5 | defect | partial | The nightly at `1739b5f` is red on riscv64: two time limits of tests that the emulation of QEMU outruns |
| [T-767](relay.md) | P1 | M | M2 | defect | done | `pod-session`: a resume that the client calls done, after which the session never moves (PodUtils/podssh#283) |
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
| [T-789](upkeep.md) | P2 | S | M5 | defect | partial | The nightly at `f309b07` is red on armv7: two tests of the MQTT board of `pod-wormhole` against the stand-in |
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
| [T-802](relay.md) | P2 | M | M2 | feature | done | `pod-natprobe`: a NAT probe (the mapping, the filtering and the port of RFC 5780) on the caller's datagrams (GitHub #212) |
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
| [T-863](fleet.md) | P2 | S | M0 | chore | done | `pod-manual`: the changes of podssh to the imported files before its take (port review) |
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
| [T-880](foundation.md) | P2 | M | M0 | feature | done | `pod-release`: the self-update of a binary (fetch, verify, replace, trial run), from podnet and podssh |
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
| [T-933](upkeep.md) | P2 | S | M5 | defect | done | `pod-cloudflare`: three tests fail on Windows, where they ran for the first time (nightly 44275593101) |
| [T-934](transport.md) | P2 | S | M1 | defect | done | `pod-proxy`: an exec proxy runs `sh -c`, which breaks a Windows path and needs `sh` on Windows |
| [T-935](foundation.md) | P2 | S | M0 | defect | done | `pod-core`: two renames onto one name at once give "Access is denied" on Windows |
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
| [T-963](relay.md) | P2 | S | M2 | defect | partial | `live.yml`: public services of other parties fail from a runner of GitHub and pass from a cloud session (the Nostr relay `offchain.pub`, the WebTorrent tracker `tracker.openwebtorrent.com`) |
| [T-964](fleet.md) | P1 | M | M8 | feature | done | `pod-release`: the bundle of Rekor v2: the inclusion proof with its signed checkpoint, and the RFC 3161 timestamp |
| [T-965](fleet.md) | P1 | M | M8 | feature | done | `pod-release`: the verifier as a static command for the CI of a product and for an installer |
| [T-966](fleet.md) | P2 | M | M8 | chore | done | `pod-release`: the adoption trials of podnet and podssh, and the hand-off of the crate |
| [T-967](fleet.md) | P3 | S | M8 | feature | open | `pod-release`: a minisign signature beside the Sigstore bundle |
| [T-968](upkeep.md) | P2 | S | M5 | defect | done | The fuzz run of 2026-11-19 cannot build `russh`: the pinned nightly of 2026-11-01 meets a cycle in its query of async drop on `ssh-key` 0.7.0-rc.11 |
| [T-969](upkeep.md) | P1 | M | M5 | chore | done | `upstream.yml` is red: the sources of `pod-job`, `pod-manual` and `pod-testkit` changed in podrunner and podssh |
| [T-970](upkeep.md) | P2 | S | M5 | defect | done | `pod-wormhole`: the stand-in tracker test reads the announces before the `stopped` of an end arrived (macos-15) |
| [T-971](fleet.md) | P3 | S | M8 | defect | done | `pod-release`: `update::plan` answers `UpToDate` both for the version of this build and for an older release |
| [T-972](fleet.md) | P3 | S | M8 | chore | open | `pod-lab`: the CAs of a lab take the P-256 key and the PEM writer of `pod-acme` |
| [T-973](fleet.md) | P2 | M | M8 | chore | done | `pod-broker`: the CA bundle of a job with curl, git, Python and Node.js in a lab |
| [T-974](upkeep.md) | P1 | S | M5 | defect | done | The run on the tag `v0.78.0` failed after the signature: two steps of `release.yml` share the scratch path `$RUNNER_TEMP/changed` |
| [T-975](podvm.md) | P2 | L | M6 | feature | open | `pod-microvm`: a User-Mode Linux kernel in seccomp mode and its launcher, for a guest with no KVM, no ptrace and no namespace |
| [T-976](podvm.md) | P2 | M | M6 | defect | done | `pod-enter`: the library path of the loader family has no `DT_RUNPATH` or `DT_RPATH` directory of a program or its libraries |
| [T-977](podvm.md) | P2 | M | M6 | feature | open | `pod-release`: the pinned fetch of a static tool that a project runs (gVisor's `runsc` for podvm) |
| [T-978](upkeep.md) | P1 | M | M5 | defect | partial | The findings of the report of the hard problems that the record does not have |
| [T-979](foundation.md) | P1 | S | M0 | feature | done | `pod-todo`: an opt-in rule that a done defect names its test (PodUtils/podnet#657) |
| [T-980](fleet.md) | P2 | M | M8 | chore | done | `pod-broker`: the bundle of a job for Go, Java, Deno, pip, npm and Hex |
| [T-985](adapters.md) | P2 | S | M3 | defect | done | `pod-cloudflare`: an error line names the request two times with a proxy that does not answer (PodUtils/podnet#658) |
| [T-986](adapters.md) | P2 | S | M3 | defect | done | `pod-warp`: an error of the WARP API names a proxy that does not answer once for each host (PodUtils/podnet#658) |
| [T-987](roads.md) | P2 | S | M2 | defect | done | `pod-ws`: the dial error of a dialer of the caller names its target two times (PodUtils/podnet#658) |
| [T-988](podvm.md) | P2 | S | M6 | feature | done | `pod-enter`: the check of `exec=` for the first program of a caller with guest paths: `PATH`, a relative name and each `#!` line (PodUtils/podvm#613) |
| [T-990](relay.md) | P2 | S | M2 | defect | done | `pod-quic`: an endpoint on `[::]` is IPv6 only on Windows, so the QUIC leg of `pod-cloudflare` sends nothing to an edge of IPv4 |
| [T-995](podvm.md) | P1 | S | M6 | defect | done | `pod-enter`: `Child::wait_forwarding` forwards SIGHUP, SIGINT and SIGTERM only, so a SIGUSR1 or a SIGQUIT kills the waiter and the payload stays |
| [T-996](podvm.md) | P3 | S | M6 | defect | done | `pod-supervise`: a SIGQUIT, SIGUSR1, SIGUSR2, SIGALRM or SIGPWR ends the detached launcher, and the launcher reads SIGHUP and gives it to nobody |
| [T-997](podvm.md) | P2 | S | M6 | feature | done | `pod-beneath`: one open of a path under a base with no link and no allocation, for the volume base of podvm and pod-enter |
| [T-998](podvm.md) | P2 | S | M6 | feature | done | `pod-confine` and `pod-enter`: the program that a dynamic loader runs takes the rule of `exec=` (PodUtils/podvm#613) |
| [T-999](podvm.md) | P2 | S | M6 | feature | done | `pod-confine`: a table of any access in the shape of the exec table, and `pod-enter` gives the payload a write table (PodUtils/podvm#613) |
| [T-1000](fleet.md) | P2 | S | M8 | feature | done | `pod-broker`: the control of a broker on the wire and the tiers of `pod-control` |
| [T-1005](fleet.md) | P3 | S | M8 | chore | open | `pod-broker`: the clients of a job in the versions that no test runs |
| [T-1009](podvm.md) | P1 | S | M6 | defect | done | `pod-supervise` and `pod-enter`: the thread of a foreground hold blocks only SIGHUP, SIGINT, SIGQUIT and SIGTERM, so a SIGUSR1 to a waiter with a hold ends the process and leaves the payload |
| [T-1010](podvm.md) | P2 | S | M6 | defect | done | `pod-enter`: the child of the payload keeps the signal mask of the caller, so a payload of a caller that blocked a signal never gets it |
| [T-1012](podvm.md) | P2 | M | M7 | feature | done | `pod-guest`: a saved state of the caller's machine line with no UEFI `Plan`, for podvm's Linux guest (PodUtils/podvm#618) |
| [T-1013](podvm.md) | P2 | S | M6 | feature | done | `pod-services`: the pinned static QEMU at 11.0.0, with the fix of the use-after-free of `address_space_translate_for_iotlb` (PodUtils/podvm#618) |
| [T-1014](podvm.md) | P2 | M | M6 | feature | done | `pod-services`: a static QEMU of the newest stable point release; the pin of T-1013 is 11.0.0, and 11.0.5 and 11.1.3 are out |
| [T-1016](fleet.md) | P2 | S | M8 | defect | done | `pod-release`: a line of the chain names a certificate by its subject, and a leaf of Fulcio has none |
| [T-1018](podvm.md) | P2 | S | M6 | defect | done | `pod-enter`: `the_tokens_agree_with_the_loader_of_the_host` fails where the test binary is of another machine than the `/bin/true` of the host |
| [T-1019](fleet.md) | P2 | S | M8 | defect | done | `pod-broker`: the cargo leg of `tls_clients` runs the proxy of rustup in a temporary directory, where it finds no toolchain |
| [T-1020](fleet.md) | P1 | S | M8 | defect | done | `pod-broker`: cargo on macOS does not read `SSL_CERT_FILE`, so the cargo of a job refuses the leaf of the job |
| [T-1021](foundation.md) | P2 | S | M0 | defect | done | `pod-testkit`: `host::tool` finds no program on Windows, so a test that needs git skips on the Windows runners |
| [T-1022](fleet.md) | P2 | S | M8 | defect | done | `pod-broker`: Go on macOS reads no variable of a CA file, so `go_takes_the_bundle_by_ssl_cert_file` can fail on the macOS jobs |
| [T-1023](fleet.md) | P2 | S | M8 | defect | done | `pod-control`: the bytes of a line that holds a token or a secret are not wiped |
| [T-1026](fleet.md) | P2 | M | M8 | feature | done | `pod-unit`: a dispatcher of the service control manager, so that a static program runs as a Windows service |
| [T-1027](fleet.md) | P2 | M | M8 | feature | done | `pod-unit`: the install and the removal of a unit of the system |
| [T-1028](fleet.md) | P3 | S | M8 | chore | open | `pod-release`: the trial run of a new binary on `pod_core::process::run_bounded` |
| [T-1030](fleet.md) | P3 | S | M8 | measurement | done | `pod-actions-store`: the git tests on the Windows jobs, with the forge URL of a Windows path |
| [T-1031](fleet.md) | P3 | S | M8 | measurement | open | `pod-broker`: a Go job on macOS refuses the leaf of the job, and no variable gives Go the CA |
| [T-1032](podvm.md) | P3 | S | M6 | defect | open | `pod-supervise`: a signal of the list to the detached launcher between the start of the payload and its block ends the launcher |
| [T-1033](podvm.md) | P2 | S | M6 | defect | done | `pod-enter`: podvm's two rules for the take of `check_exec_argv` and `check_exec_guest`: a rule over a volume, and the `#!` chain (PodUtils/podvm#613) |
| [T-1034](transport.md) | P2 | S | M1 | defect | done | `pod-services`: the watch `http-status` of the upload of `temp-sh` gets 429 from a runner of GitHub, not 405 |
| [T-1035](podvm.md) | P1 | S | M7 | defect | done | `pod-guest`: the live test of the warm start expects QEMU 11.0 to save the NVMe root disk of the plan, and only QEMU 11.1.0 and later migrate an NVMe device |
| [T-1036](podvm.md) | P3 | S | M6 | defect | done | `pod-supervise`: `end_tree` waits on one pidfd at a time, so a tree with the init of a PID namespace and a second process of it waits the whole `TREE_KILL_MS` (PodUtils/podvm#624) |
| [T-1037](podvm.md) | P2 | S | M7 | defect | done | `pod-guest`: `qmp::Monitor` waits 5 s for one answer with no bound of the caller and no read of a late answer, so a slow `query-migrate` fails a save (PodUtils/podvm#618) |
| [T-1040](fleet.md) | P2 | S | M8 | defect | done | `pod-control`: the doc test of `conformance` calls `Client::connect_unix`, which Windows does not have |
| [T-1041](fleet.md) | P1 | S | M8 | defect | done | `pod-control`: on macOS the timeout of a Unix socket whose peer closed fails with EINVAL, so a read of the end is an error |
| [T-1042](fleet.md) | P2 | S | M8 | defect | done | `pod-broker`: the Node.js of Homebrew reads `SSL_CERT_FILE`, so the node and npm legs of `tls_clients` fail on macOS |
| [T-1043](upkeep.md) | P2 | S | M2 | defect | done | `pod-link`: the EDNS test of `dns_resolver` compares two counts that the resolver still moves (armv7) |
| [T-1044](upkeep.md) | P3 | S | M5 | defect | open | `check-windows.sh` and `check-macos.sh` compile no doc test, so a doc test with an item of Unix fails only in the nightly |
| [T-1045](podvm.md) | P2 | S | M6 | feature | done | `pod-supervise`: a streamed read of a log: a copy of a range in pieces, the tail found from the end, a byte bound of a follow (PodUtils/podvm#631) |
| [T-1046](podvm.md) | P2 | S | M6 | feature | done | `pod-incus`: a read of a file in pieces to a writer, with a `Range` and a byte bound of the caller (PodUtils/podvm#631) |
| [T-1048](fleet.md) | P3 | M | M8 | defect | done | `pod-control`: the scratch buffer of `serde_json` keeps a string of a line with an escape, with no wipe |
| [T-1052](upkeep.md) | P2 | S | M2 | defect | done | `pod-link`: the gate of E2-1 at 20 % drop divides by a clean time that the CPU of the host sets, so a fast runner gives a factor above its bound (nightly 44792996234) |
| [T-1053](upkeep.md) | P3 | S | M2 | measurement | open | `pod-link`: a transfer of KCP with `nodelay` at 20 % drop can take ten times its median, as the RTO of a segment grows by half at each timeout |
| [T-1056](podvm.md) | P1 | S | M7 | defect | done | `pod-guest`: `qmp::Monitor::connect` puts no bound on the `connect()` itself, so a connect to the full queue of the monitor of QEMU waits in the kernel with no end (PodUtils/podvm#633) |
| [T-1057](podvm.md) | P2 | S | M7 | defect | done | `pod-guest`: the tests of `warm::line` have no stand-in of the monitor of QEMU with one client and a queue of one connection (PodUtils/podvm#633) |
| [T-1059](relay.md) | P2 | S | M2 | defect | done | `pod-relay`: the test of a code in no line takes a part of a word for a word of the code (`ci` 44806766580) |
| [T-1060](fleet.md) | P3 | S | M8 | defect | done | `pod-control`: an answer that the server drops on a side path, after a timeout or a spent budget, is not wiped |
| [T-1066](foundation.md) | P2 | S | M0 | feature | done | `pod-core`: one atomic replace of a file at a path that the caller names, with `Mode::Keep`, a followed link, a made directory and errors that name the step (PodUtils/podnet#677) |
| [T-1070](fleet.md) | P3 | S | M8 | defect | done | `pod-control`: podrunner's test of a ninth client fails 6 of 30 runs, as the place of the call before it is not back yet |
| [T-1073](fleet.md) | P3 | S | M8 | defect | done | `pod-control`: the wait of `Context::call` can end before the server reads the answer of the client |
| [T-1080](podvm.md) | P2 | S | M6 | defect | done | `pod-enter`: `E2BIG` at `execve` gives 127 ("not found"), and a search of the path keeps the errno of its last candidate, so a program that was found and refused does not give 126 (PodUtils/podvm#639) |
| [T-1082](upkeep.md) | P2 | S | M5 | defect | done | The gate of no blocking call counts the waits that the other test threads of its binary cause (i686, nightly 44848003241) |
| [T-1086](podvm.md) | P1 | S | M6 | defect | done | `pod-extract`: a layer of GNU tar `--acls` is refused whole, as the PAX record `SCHILY.acl.access` holds a newline byte (PodUtils/podvm#643) |
| [T-1087](podvm.md) | P3 | M | M6 | defect | done | `pod-extract`: a GNU sparse file of the PAX form (`GNU.sparse.`) refuses its layer, as the `tar` reader does not read it |
| [T-1088](podvm.md) | P3 | S | M6 | defect | done | `pod-extract`: the attributes of an archive of libarchive with `xattrheader=LIBARCHIVE` are dropped with no word |
| [T-1089](fleet.md) | P1 | S | M8 | defect | done | `pod-unit`: a second call of `windows::serve` in a program that the manager did not start gets `Class::Unavailable` (error 1056), not `Class::Usage` (nightly 44871383085) |
| [T-1093](services.md) | P2 | S | M5 | defect | done | `services.yml` 45009160652: three `.xyz` RDAP records refused, three Trojan nodes gone, the Syncthing relay DVWOU27 at a new address, and two hosts dark to the runner |
| [T-1094](podvm.md) | P1 | S | M7 | chore | done | `pod-confine`: the changes of podvm to the files of the loader (port review, `upstream.yml` 45024358679) |
| [T-1095](relay.md) | P2 | M | M2 | feature | done | `pod-ws`: the order of the request's pseudo-fields of the browser shape is a patch of h2 in podssh, not a part of the crate (PodUtils/podssh#340) |
| [T-1096](services.md) | P2 | S | M5 | measurement | done | `pod-services`: `localtunnel.me` and `download.freedos.org` give no answer to the runners of GitHub on each run, but answer other networks |
| [T-1097](foundation.md) | P2 | S | M0 | chore | done | `pod-testkit`: the change of podnet to `tests/test_host/mod.rs` (port review) |
| [T-1098](fleet.md) | P1 | S | M8 | feature | done | `pod-job`: `run-report.v1` refuses a reason of a time that the report does not know (`start_s`) (PodUtils/podrunner#342) |
| [T-1102](roads.md) | P1 | S | M4 | chore | done | `pod-iroh`: the fork of iroh-relay at 1.4.2, so that iroh 1.4.2 builds it (PodUtils/podssh#341) |
| [T-1103](roads.md) | P2 | M | M4 | chore | open | `pod-iroh`: iroh 1.5 and iroh-relay 1.5, past the range of the crate (T-1102) |
| [T-1104](fleet.md) | P1 | S | M8 | defect | done | `pod-unit`: a path after a `.` reads as a long random word, so a config under a directory of `mktemp -d` is refused (PodUtils/podrunner#343) |
| [T-1105](fleet.md) | P2 | S | M8 | defect | done | `pod-unit`: the check of a secret refuses a public key of OpenSSH and a fingerprint as a long random word (PodUtils/podssh#342 ask (a)) |
| [T-1106](fleet.md) | P1 | S | M8 | feature | done | `pod-unit`: a removal and an install refuse what an older version of the program wrote, as its mark differs (PodUtils/podssh#342) |
| [T-1107](fleet.md) | P1 | S | M8 | defect | done | `pod-unit`: a unit of the system that runs as a named account removes the IPC of that account at its stop (`RemoveIPC=yes`) (PodUtils/podssh#342) |
| [T-1108](fleet.md) | P1 | S | M8 | defect | done | `pod-unit`: a unit of systemd has no limit of starts for systemd older than 230 (`StartLimitInterval=0`) (PodUtils/podssh#342) |
| [T-1109](foundation.md) | P1 | S | M0 | feature | done | `pod-core`: an atomic replace has no check of the temporary file before the rename (PodUtils/podssh#342) |
| [T-1110](fleet.md) | P2 | S | M8 | chore | done | `pod-unit`: podrunner's unit of the system loses `RemoveIPC=yes` at its take of the tag of T-1107, and its test of the unit fails |
| [T-1111](foundation.md) | P2 | S | M0 | chore | done | `pod-guest`, `pod-release`, `pod-supervise`: the changes of podvm and podnet at their heads (port review) |
| [T-1112](podvm.md) | P1 | S | M6 | defect | done | `pod-enter`: the init of a PID namespace and the relay of a join show the command line and the environment of the caller to the payload, and a payload of the same user can trace them (PodUtils/podvm#646) |
| [T-1113](podvm.md) | P2 | S | M6 | defect | done | `pod-enter`: the init of a PID namespace in a user namespace of the rung stays dumpable, so a payload of the same caller can trace it and read the memory of the caller |
| [T-1114](upkeep.md) | P1 | S | M5 | defect | done | `pod-core`: `bind_staged` binds at the path when the stage does not fit in `sun_path`, and macOS refuses a path in its temporary directory (PodUtils/podssh#343) |
| [T-1115](upkeep.md) | P2 | S | M5 | chore | open | Two more copies of the bind of a Unix socket at a long path: `pod-link` (`bind_private`) and `pod-supervise` (`control_via_fd`) |
| [T-1116](adapters.md) | P1 | S | M3 | defect | done | `pod-cloudflare`: the edge refuses a new quick tunnel with "Tunnel not found" for more than three tries, and the start exits 69 (PodUtils/podnet#685) |
| [T-1117](adapters.md) | P3 | S | M3 | docs | done | podnet's documents give the serial limits of the legs of `pod-cloudflare` as 130 s and 280 s; T-1116 makes them 110 s and 210 s |
| [T-1118](fleet.md) | P3 | S | M8 | chore | done | `pod-release`: podnet writes `release-index.json` in its release workflow (PodUtils/podnet#686) |
| [T-1120](podvm.md) | P1 | S | M6 | feature | done | `pod-hostprobe`: the CPU time, the peak of memory and the OOM kills of a cgroup at the end of a run (PodUtils/podvm#650) |
| [T-1122](foundation.md) | P1 | S | M0 | chore | done | `pod-testkit`, `pod-unit`: the changes of podrunner and podssh at their heads (port review) |
| [T-1124](podvm.md) | P2 | S | M6 | chore | done | podvm opens `/proc/PID` and the pidfd of the init of a run, which is not dumpable after T-1113, so its `exec -t`, `cp` and the join of the UTS namespace fail for a caller with no capabilities |
| [T-1125](relay.md) | P1 | S | M2 | defect | done | `pod-hold`: a failed `execve` of `spawn::start` exits 127 with no errno, and `spawn::resolve` does not check a path with a `/` (PodUtils/podnet#707) |
| [T-1126](relay.md) | P2 | S | M2 | defect | done | `pod-wormhole`: `Failed::Expired` rounds the life of the code up to whole minutes ("within its 1 minutes" for 5 s) (PodUtils/podnet#707) |
| [T-1127](relay.md) | P2 | S | M2 | defect | done | `pod-wormhole`: `Failed::Unreachable` keeps only the words of each meeting point, so a caller loses the stage Proxy or Dns (PodUtils/podnet#707) |
| [T-1128](fleet.md) | P3 | S | M8 | defect | done | `pod-unit`: podnet's `autostart remove` of an unknown name exits 69, and the import of `autostart.rs` is behind (PodUtils/podnet#707) |
| [T-1129](roads.md) | P2 | S | M2 | defect | done | `pod-ws`: a refusal of the proxy from a dialer of the caller loses the stage Proxy, and its line names the target two times (PodUtils/podnet#707, ask 5) |
| [T-1130](roads.md) | P2 | S | M2 | defect | done | `pod-proxy`: the line of a proxy that does not answer names the proxy two times (PodUtils/podnet#707, ask 6) |
| [T-1131](roads.md) | P2 | S | M2 | defect | done | `pod-dns`: the line of a name that does not resolve holds the list of each resolver of DNS over HTTPS, 567 bytes and more (PodUtils/podnet#707, ask 7) |
| [T-1132](transport.md) | P2 | S | M1 | defect | done | `pod-tls`: the text of a certificate for another name holds the Debug form `DnsName("x")` of rustls-webpki (PodUtils/podnet#707, ask 8) |
| [T-1133](adapters.md) | P2 | S | M3 | defect | done | `pod-warp`: an enrolment through a dead proxy loses the stage Proxy, and an answer that is not JSON shows no word of its body (PodUtils/podnet#707, ask 9) |
| [T-1134](adapters.md) | P2 | S | M3 | defect | done | `pod-cloudflare`: the chain of a refused origin says the error of I/O two times (PodUtils/podnet#707, ask 10) |
| [T-1135](adapters.md) | P3 | M | M3 | defect | done | `pod-cloudflare`: each failure of `fail::caused`, `or_fail` and `wrap` holds the text of its cause and the cause, so a reader that joins each text of the chain says the cause two times |
| [T-1137](transport.md) | P1 | S | M1 | feature | done | `pod-proxy`: a body over the bound is refused with no status, no size, no bound and no byte of the body (PodUtils/podvm#651) |
| [T-1138](podvm.md) | P1 | S | M6 | feature | done | `pod-incus`: an answer over `max_body` is an I/O error with the words of pod-proxy alone, and the caller cannot tell it from another fault (PodUtils/podvm#651) |
| [T-1139](podvm.md) | P1 | S | M6 | feature | done | `pod-incus`: an `error` envelope over `max_body` is refused by size, and the words of incusd go (PodUtils/podvm#651) |
| [T-1140](podvm.md) | P1 | S | M6 | defect | done | `pod-incus`: a text of incus cut at 512 characters loses the cause after a long command line and a path (PodUtils/podvm#651) |
| [T-1141](podvm.md) | P2 | S | M6 | feature | done | `pod-incus`: no read of a storage pool, of the room of a pool, or of an image that incus has (PodUtils/podvm#651) |
| [T-1142](podvm.md) | P2 | S | M6 | feature | done | `pod-hostprobe`: `sys::Statfs` has no `f_bfree`, so a check for a daemon of root cannot name the blocks kept for root (PodUtils/podvm#651) |
| [T-1143](podvm.md) | P2 | S | M6 | chore | done | podvm's class of a refusal by size reads the words of an `Io` error, and finds none after T-1138 |
| [T-1144](transport.md) | P2 | M | M3 | feature | done | `pod-term`: a sans-IO 7-bit transfer codec for a pty that damages ZMODEM (PodUtils/podssh#346) |
| [T-1146](relay.md) | P3 | S | M2 | docs | open | `pod-natprobe`: the comments of the code are not in ASD-STE100 |
| [T-1150](services.md) | P2 | S | M5 | feature | open | `pod-services-watch`: a watch of the facts of RFC 5780 of a STUN server (OTHER-ADDRESS, CHANGE-REQUEST, RESPONSE-PORT) |
| [T-1151](relay.md) | P3 | S | M2 | chore | done | `pod-natprobe`: the ignored live test runs in `live.yml` |
| [T-1152](relay.md) | P1 | S | M2 | defect | done | `pod-natprobe`: the SSRF guards of UPnP and of the STUN replies take a host from an untrusted reply |
| [T-1153](relay.md) | P2 | S | M2 | defect | done | `pod-natprobe`: each stray datagram adds one observation, with no bound |
| [T-1154](relay.md) | P3 | S | M2 | defect | done | `pod-natprobe`: the resolver of the servers is the system's, a setting is never read, and a doc names two servers |
| [T-1155](podvm.md) | P1 | S | M6 | defect | done | `pod-incus`: an exec whose command ends with 126 or 127 ends the wait as an error of incusd, and `exec_recorded` and `Attached::wait` give no exit code, no output and no operation id (PodUtils/podvm#653) |
| [T-1156](podvm.md) | P1 | S | M6 | feature | done | `pod-incus`: no bound of the answer of a call: a socket that takes the connection and never answers holds the caller (PodUtils/podvm#653) |
| [T-1157](podvm.md) | P2 | S | M6 | defect | done | `pod-incus`: after a failed write of a body, the answer that incusd already sent is read with no bound and no test, and the documentation does not tell the caller about SIGPIPE (PodUtils/podvm#653) |
| [T-1158](podvm.md) | P2 | S | M6 | chore | done | podvm removes its work-arounds for `pod-incus` at its take of the tag of T-1155 to T-1157 |
| [T-1159](fleet.md) | P2 | S | M8 | defect | open | `issue-sync`: a close of an issue by a person is reverted at the next push, before a session can change the record |
| [T-1160](foundation.md) | P1 | S | M0 | chore | done | `pod-testkit`: the fork fact of a job in the fake githome follows podrunner's rule of `e629fa8` (the second port after T-1122) |
| [T-1161](upkeep.md) | P2 | S | M5 | defect | done | `pod-control`: `a_call_back_meets_one_deadline_for_its_write` fails on riscv64 under the emulation of QEMU (the nightly at `0f5dad6`) |
| [T-1162](fleet.md) | P3 | S | M8 | chore | done | Answer PodUtils/podvm#570 (COORD-183): name the commit of the sections "Shared code first" and "Quality first" of `docs/session-protocol.md` |
| [T-1163](upkeep.md) | P3 | S | M5 | chore | done | Two more readers of a `Retry-After` value: `pod-cloudflare` and `pod-oci` keep their own copy of `pod_core::clock::retry_after_secs` |
| [T-1164](relay.md) | P2 | S | M2 | defect | done | `pod-link`: the DNS listener sends no answer to a query that is not TXT or whose name does not decode, so a resolver retries and serves SERVFAIL (PodUtils/podssh#349) |
| [T-1165](relay.md) | P2 | M | M2 | defect | done | `pod-link`: a query with a new session id takes a handshake slot of the DNS listener for 20 s before any key is checked (PodUtils/podssh#349) |
| [T-1166](relay.md) | P3 | M | M2 | feature | done | `pod-link`: the DNS listener answers each poll at once; a held poll (dnstt's 1 s) halves the idle queries and cuts the latency of data down (PodUtils/podssh#349) |
| [T-1167](relay.md) | P3 | M | M2 | feature | done | `pod-link`: an option to carry the ARQ of the DNS carrier with no TLS for a caller that seals above (PodUtils/podssh#349) |
| [T-1168](relay.md) | P3 | M | M2 | feature | done | `pod-link`: the datagrams of the DNS carrier over DNS over HTTPS (RFC 8484), for a dialer behind an HTTP proxy with no UDP (PodUtils/podssh#349) |
| [T-1169](relay.md) | P3 | M | M2 | chore | open | `pod-link`: a test of the DNS carrier through `unbound` with its default settings, in the box of CI (PodUtils/podssh#349) |
| [T-1170](relay.md) | P2 | S | M2 | defect | done | `pod-link`: the DNS listener answers with the question in lower case, so a resolver that uses 0x20 rejects the answer |
| [T-1171](relay.md) | P3 | S | M2 | defect | done | `pod-link`: a late query of an ended DNS session starts a new handshake and holds a permit |
| [T-1180](foundation.md) | P1 | S | M0 | chore | done | `pod-testkit`: the contract of the source and the driver at podrunner `aab63e5` (the third port of `source-driver.md`) |
| [T-1181](fleet.md) | P1 | S | M0 | chore | done | `pod-unit`: the words of the forms at podssh `ea4c290` (port review of `text.rs`) |
| [T-1182](fleet.md) | P2 | S | M0 | feature | done | `pod-unit`: the name of a unit has at most 64 characters, so podssh's longer node names are refused at `--install` |
| [T-1191](transport.md) | P2 | M | M1 | feature | done | `pod-term`: `modes::Modes`, a sans-IO tracker of the modes of a terminal's output with `restore` (PodUtils/podssh#350) |
| [T-1192](transport.md) | P2 | M | M1 | feature | done | `pod-term`: `Ring::modes_at`, the modes that the stream set before an offset, with an oracle test on the captures of vim, less and top (PodUtils/podssh#350) |
| [T-1193](podvm.md) | P2 | L | M6 | feature | done | `pod-extract`: `flatten`, the final tree of the layers of an image as one tar stream with no tree on the disk (PodUtils/podvm#657) |
| [T-1194](podvm.md) | P2 | M | M6 | feature | done | `pod-extract`: `flatten` keeps the hard links across layers, with the target before the link (PodUtils/podvm#657) |
| [T-1195](podvm.md) | P2 | M | M6 | feature | done | `pod-extract`: `flatten` keeps the PAX records of the image, and takes a prefix and the entries of the caller (PodUtils/podvm#657) |
| [T-1196](podvm.md) | P2 | M | M6 | feature | done | `pod-gzip`: a writer of gzip members, one member for each block, by up to eight threads, with the same bytes for any count of threads (PodUtils/podvm#657) |
| [T-1197](transport.md) | P3 | S | M5 | chore | done | CRC-32 has three hand copies in the workspace; each crate takes `crc32fast` |
| [T-1198](transport.md) | P2 | S | M3 | defect | done | `pod-term`: the transfer codec loses the `KEPT` that ends a session, and the far sender's 20 s wait runs while a person answers the offer |
| [T-1205](podvm.md) | P1 | S | M6 | feature | done | `pod-hostprobe`: `usage::Mounts`, the three cgroup directories of a process from the text of `/proc/self/mountinfo`, where a mount of cgroup is refused (PodUtils/podvm#659) |
| [T-1209](upkeep.md) | P2 | S | M5 | defect | done | `pod-core`: one probe of a Unix socket that a forked child can hold, for `pod-control`, `pod-link` and `pod-warp` (PodUtils/podnet#709) |
| [T-1213](upkeep.md) | P2 | S | M5 | chore | done | One watcher of the Actions runs of a repository: `scripts/agent/runs.py` with podvm's surface, in place of `watch-runs.sh` and the copies of podvm and podnet |
| [T-1217](fleet.md) | P2 | M | M8 | feature | done | `pod-tpm`: the crate, the TPM 2.0 command frame, the response codes and the commands of podssh's module, and the trait `Transport` (PodUtils/podssh#351) |
| [T-1218](fleet.md) | P2 | S | M8 | feature | done | `pod-tpm`: the PCR selection and the policy digest of `TPM2_PolicyPCR` (PodUtils/podssh#351) |
| [T-1219](fleet.md) | P2 | M | M8 | feature | done | `pod-tpm`: the key file `TSS2 PRIVATE KEY`, read and write in DER and PEM, with bounds on an untrusted file (PodUtils/podssh#351) |
| [T-1220](fleet.md) | P2 | M | M8 | feature | done | `pod-tpm`: the sequences over a `Transport` (create a primary, create a key under a PCR policy, load, sign) that flush each handle on each path (PodUtils/podssh#351) |
| [T-1221](fleet.md) | P2 | M | M8 | feature | done | `pod-tpm`: the transports of Linux (`/dev/tpmrm0`, an swtpm socket) and of Windows (TBS), each command bounded in size and time (PodUtils/podssh#351) |
| [T-1222](fleet.md) | P2 | M | M8 | feature | done | `pod-tpm`: a TPM in software (feature `soft`) and a CI lane against swtpm and the reference simulator, with the key of PCR 7 as the proof (PodUtils/podssh#351) |
| [T-1223](services.md) | P2 | S | M5 | defect | done | `services.yml` 45394042840: the Syncthing relay WMYMARB at a new address, the Trojan node `node-trojan-45-63-30-142` gone, and two hosts with a first no answer |
| [T-1235](foundation.md) | P2 | S | M0 | feature | done | `pod-core`: the classes `CantCreate` (73) and `Internal` (70) (PodUtils/podssh#352) |
| [T-1236](foundation.md) | P2 | S | M0 | docs | done | `pod-core`: the codes that a consumer keeps where its class gives another, in the README (PodUtils/podssh#352) |
| [T-1237](foundation.md) | P2 | S | M0 | feature | done | `pod-release`: `Error::CantWrite` has the class `CantCreate` (73) (PodUtils/podssh#352) |
| [T-1238](foundation.md) | P3 | S | M0 | research | done | `pod-core`: no class for `EX_DATAERR` (65), which podssh uses (PodUtils/podssh#352) |
| [T-1243](foundation.md) | P1 | S | M0 | chore | done | `pod-testkit`: the auth of the fake broker at podrunner `2a55cd9` (port review of `auth.rs`) |
| [T-1253](podvm.md) | P2 | S | M6 | defect | done | `pod-extract`: the sidecar row of a removed path holds for a directory that an entry below it makes again |
| [T-1254](podvm.md) | P2 | S | M6 | chore | done | `pod-extract`: a fuzz target of `flatten`, the reader of untrusted layers into one tar stream |
| [T-1255](podvm.md) | P3 | S | M6 | defect | done | `pod-extract`: the umask of the process cuts the mode of a directory that an entry below it makes |
| [T-1269](podvm.md) | P3 | S | M6 | defect | done | `pod-extract`: `flatten` fails with `Unsupported` for a hard link to an inode that no name of the stream holds after a second entry of its path in one layer |
| [T-1270](podvm.md) | P3 | S | M6 | defect | done | `pod-extract`: `flatten` drops the extended attributes of a hard-link entry, which `extract` sets on the inode |
| [T-1279](relay.md) | P3 | S | M2 | defect | done | `pod-dns`: a `Pipe` asks Cloudflare and Google over HTTP/1.1, so the queries of the DNS carrier over DoH go one at a time behind a held poll |
| [T-1295](upkeep.md) | P1 | S | M6 | defect | done | `pod-extract`: `flatten` tests fail for a user that is not root: GNU tar cannot open `usr/bin/sh`, and `walk` cannot read a directory with no search mode |
| [T-1296](upkeep.md) | P2 | S | M6 | defect | done | `pod-enter`: the two title tests of the init and the relay fail under qemu-user on each emulated job (the nightly at `f39b8e2`) |
| [T-1297](upkeep.md) | P2 | S | M2 | defect | done | `pod-link`: `dns_hold` gives a hold that is longer than its query timeout under an emulator, and `a_server_that_never_answers_a_command_is_the_named_timeout` of `mail` ends with no error on arm |
| [T-1298](upkeep.md) | P1 | M | M6 | defect | done | `pod-incus`: two tests of `images` hang on macOS, and the job `test (macos-15)` and `test (macos-15-intel)` meet their limit of 60 minutes |
| [T-1299](upkeep.md) | P3 | S | M6 | feature | done | `pod-extract`: a stream of `flatten` that GNU tar as a user that is not root can unpack |
| [T-1303](upkeep.md) | P2 | S | M5 | defect | done | The step "Test" of the jobs `test` of `nightly.yml` has no limit for each test binary, so one hung binary hides the later binaries of macOS and Windows |
| [T-1307](podvm.md) | P3 | S | M6 | feature | done | `pod-incus`: a cut `error` envelope has no flag, so a caller reads the words of its text to know that the answer was over `max_body` (PodUtils/podvm#663) |
| [T-1321](podvm.md) | P3 | S | M6 | measurement | open | `pod-gzip`: measure a C backend of deflate against `miniz_oxide` at four threads |
| [T-1325](upkeep.md) | P2 | S | M5 | defect | partial | `ci.yml`: the job `static (aarch64-apple-darwin, macos-15-intel)` met its limit of 40 minutes in "Build the probe" |
| [T-1326](upkeep.md) | P3 | S | M5 | chore | done | Three readers of the fields after the name in `/proc/PID/stat`: `pod-hostprobe`, `pod-supervise` and `pod-enter` |
| [T-1339](fleet.md) | P2 | S | M8 | chore | done | `pod-tpm`: the fuzz targets of the response reader and of the key file reader, and a plant for each bound of T-1217 and T-1219 |
| [T-1340](fleet.md) | P2 | M | M8 | measurement | open | `pod-tpm`: a key file that the crate writes loads and signs in tpm2-openssl and in go-tpm-keyfiles against swtpm |
| [T-1343](upkeep.md) | P2 | M | M5 | feature | partial | `pod-core`: one temp directory and one temp file with a random name (PodUtils/podnet#711, lesson 5 of PodUtils/podvm#567) |
| [T-1347](relay.md) | P3 | S | M2 | chore | open | `pod-h2`: a command that reproduces the need of each patch of `vendor/patches/h2/`, and the check of each h2 release |
| [T-1351](adapters.md) | P3 | M | M3 | defect | done | `pod-cloudflare`, `pod-core`, `pod-proxy`: a failure whose text holds the words of its typed cause says them two times to a reader that joins the chain |
| [T-1355](foundation.md) | P1 | S | M0 | chore | done | `pod-testkit`: the contract of the source and the driver at podrunner `4fc871c` (the fourth port of `source-driver.md`) |
| [T-1359](upkeep.md) | P2 | S | M5 | defect | done | `pod-core`: three tests of `files::socket` fail on macOS because the temporary directory is longer than their directory of 40 bytes |
| [T-1360](upkeep.md) | P3 | S | M5 | defect | done | `pod-cloudflare`: `quick_tunnel_standin_falls_back_to_http2_when_udp_is_dropped` cannot bind its UDP socket on Windows ARM64 (WSAEACCES) |
| [T-1361](upkeep.md) | P1 | S | M8 | defect | done | adoption/podssh/pod-tpm.run: the check of the moved names reads all of `podssh-ssh/src`, and it finds two other structs named `Out` |
| [T-1362](fleet.md) | P1 | S | M8 | feature | done | `pod-job`: `RealJob` takes the optional fields `ref` and `repo_id` (PodUtils/podrunner#352) |
| [T-1363](upkeep.md) | P1 | S | M8 | defect | done | `adoption/podssh`: the trial `pod-tpm` does not build, because podssh's relay role gives a stream of the h2 of crates.io to `h2_road::duplex`, which takes the types of `pod-h2` |
| [T-1365](adapters.md) | P2 | S | M3 | defect | done | `pod-warp`: a client of the proxy with no WARP session gets a reset, not a SOCKS5 or HTTP failure reply (PodUtils/podnet#712) |
| [T-1366](adapters.md) | P2 | S | M3 | defect | done | `pod-warp`: a pin mismatch names the cause by the issuer of the door certificate, and says each cause once (PodUtils/podnet#726) |
| [T-1367](relay.md) | P1 | S | M2 | feature | done | `pod-e2e`: the binding of the channel to the check of a grant, at the node and at the operator (PodUtils/podssh#355) |
| [T-1368](relay.md) | P1 | S | M2 | feature | done | `pod-relay`: a bearer grant bound to the channel, and its verifier (PodUtils/podssh#355) |
| [T-1369](relay.md) | P2 | S | M2 | chore | done | `pod-relay`: a fuzz target of `grant::bearer::Bearer::read`, the reader of a bearer from the peer |
| [T-1370](fleet.md) | P1 | S | M8 | feature | done | `pod-job`: warm and cold units in `run-report.v1` and `status.v1` (PodUtils/podrunner#353) |
| [T-1371](relay.md) | P2 | S | M2 | feature | done | `pod-relay`: the `Mint` of a grant writes no extension, so a stock sshd gives its certificate no pty and no forward (PodUtils/podssh#356) |
| [T-1372](fleet.md) | P3 | S | M8 | release | open | crates.io: the names `pod-natprobe`, `pod-tpm`, `pod-gzip` and `pod-h2` (GitHub #261) |
| [T-1373](fleet.md) | P3 | M | M8 | feature | open | `pod-release`: an opt-in fetch of newer Sigstore roots from TUF at run time |
| [T-1374](upkeep.md) | P2 | M | M5 | chore | done | One session start for each harness and host: `scripts/session-start.sh` sets the identity, the hooks path and `main`, and starts both watchers |
| [T-1375](foundation.md) | P2 | S | M0 | chore | done | `pod-testkit`: the contract of the source and the driver after podrunner `4fc871c` (the fifth port of `source-driver.md`) |
| [T-1377](adapters.md) | P3 | M | M3 | defect | open | `pod-proxy`: the failure of a dial and of a request holds the words of its `DialError` or its cause, so a reader that joins the chain says them two times |
| [T-1378](adapters.md) | P3 | S | M3 | defect | open | `pod-warp`: the failure of an enrolment with no answer says the text of the path three times to a reader that joins the chain |
| [T-1379](podvm.md) | P1 | S | M6 | feature | done | `pod-incus`: a typed config PATCH of an instance with an ETag condition, for live limits (PodUtils/podvm#664) |
| [T-1380](fleet.md) | P2 | M | M8 | feature | open | `pod-tpm`: the transport `Tbs` loads `tbs.dll` when the process starts and bounds no command, so podssh keeps its own copy (PodUtils/podssh#357, ask 1) |
| [T-1381](fleet.md) | P2 | S | M8 | feature | open | `pod-tpm`: `SoftTpm` has no owner's hierarchy with a password, and `flow` has no fallback to the kept storage key, so podssh wraps the model and keeps the fallback itself (PodUtils/podssh#357, ask 2) |
| [T-1382](upkeep.md) | P2 | S | M8 | chore | done | `adoption/podssh`: the trials `pod-tpm` and `pod-h2` stay after podssh took `v0.84.0`, and the list of the files of `tpm/` in `pod-tpm.run` is not podssh's (PodUtils/podssh#357, ask 3) |
| [T-1383](relay.md) | P3 | M | M2 | feature | open | `pod-relay`: `Mint` signs a certificate with an Ed25519 issuer alone, so a gateway whose CA is ECDSA or RSA keeps its own signer (PodUtils/podssh#357, ask 4) |
| [T-1384](upkeep.md) | P1 | S | M5 | defect | done | `scripts/agent/check.sh`: a full check in the main checkout runs test binaries that a worktree built in the shared target |
| [T-1385](upkeep.md) | P1 | M | M8 | chore | done | agent tooling: read the latest commits of the sibling repositories and take each improvement of their agent tooling (Q23) |
| [T-1386](upkeep.md) | P3 | S | M5 | chore | open | `scripts/agent/check.sh`: the tests run in a temp directory of their own, and an entry that a test leaves there is a fault |
| [T-1387](upkeep.md) | P2 | S | M8 | chore | partial | COORD `(to all)`: one `docs/hosted-sessions.md` with one role table for each repository of PodUtils |
| [T-1388](upkeep.md) | P3 | S | M5 | chore | open | The debug info of the dev and test profiles, measured for the size of the target |
| [T-1389](podvm.md) | P1 | M | M6 | feature | done | `pod-enter`: a required tmpfs with a byte limit at a guest path of the namespace rung (PodUtils/podvm#667) |
| [T-1390](transport.md) | P2 | M | M1 | feature | open | `pod-term`: the transfer codec keeps each frame within the rows of the pane, with a floor of the chunk under 1 KiB and a resend at the floor (PodUtils/podssh#358) |
| [T-1391](podvm.md) | P2 | M | M6 | feature | open | A pinned static `mkfs.erofs` for a rootless disk image from a streamed OCI tar (PodUtils/podvm#668) |
| [T-1392](transport.md) | P2 | S | M1 | defect | done | `pod-dns`: the line of a name that does not resolve is cut inside the text of a cause, and it names the cause of one group of resolvers only (PodUtils/podnet#753) |
| [T-1393](transport.md) | P2 | M | M1 | defect | done | `pod-proxy`: a proxy at a command that does not exist floods the stderr of the process, gives a wrong cause and is started again for each dial (PodUtils/podnet#754) |
| [T-1394](adapters.md) | P2 | S | M3 | defect | done | `pod-cloudflare`: the DNS over HTTPS of the edge discovery waits about 35 s behind a proxy with no line, and with no cause at `-v` (PodUtils/podnet#755) |
| [T-1395](relay.md) | P2 | S | M2 | feature | done | `pod-session`: `resume::move_under`, the largest move under the cap of a relay, and `Settings::under_cap` (PodUtils/podnet#756) |
| [T-1396](transport.md) | P2 | S | M1 | feature | done | `pod-ws`: `ConnectError::Ended`, a final end of a stream with no relay and no HTTP, shown in its own words (PodUtils/podssh#359) |
| [T-1397](foundation.md) | P2 | S | M0 | chore | done | `pod-testkit`: the contract of the source and the driver after podrunner `7bf541c` (the sixth port of `source-driver.md`) |
| [T-1398](fleet.md) | P2 | S | M0 | chore | done | `pod-unit`: podnet's `autostart.rs` after `34f0d30` (port review) |
| [T-1401](upkeep.md) | P2 | S | M5 | defect | done | `pod-core`: the drop of a `FileLock` only closes its descriptor, so a copy of the descriptor in a child that a spawn makes keeps the lock after the drop |
| [T-1403](relay.md) | P2 | S | M2 | defect | done | `pod-e2e`: a failure of the node's target reads as "the stream under the channel failed", and the node's line does not name the target |
| [T-1405](relay.md) | P2 | S | M2 | defect | open | `pod-relay`: `reverse::operator::exchange` leaves its two tasks alive when it is dropped, so a dropped leg keeps its socket open (PodUtils/podssh#361) |
| [T-1407](relay.md) | P2 | S | M2 | defect | open | `pod-relay`: the refusals of a pair: the body of a 403, a refusal of a new pair, a spent token of `--once`, and a doubled subject in `file_name` (PodUtils/podssh#362, ask 2) |
| [T-1408](relay.md) | P2 | S | M2 | defect | open | `pod-e2e`: `Redeem` does not say whether a bearer or a grant was refused or let in, and the node says "a grant" for a bearer (PodUtils/podssh#362, ask 3) |
| [T-1413](foundation.md) | P3 | S | M0 | feature | open | `pod-core`: a class for an output that could not be written (exit 74, `EX_IOERR`) (PodUtils/podssh#363, ask 3) |
| [T-1415](services.md) | P2 | S | M5 | defect | done | `services.yml` 45603028501: the Trojan node `node-trojan-64-176-7-205` gone, and the mailbox of magic-wormhole with an HTTP 500 |
| [T-1420](fleet.md) | P3 | S | M8 | chore | open | `pod-job`: the rule of a plain name of a file of a credential, shared by the source and podrunner |
| [T-1430](podvm.md) | P2 | S | M6 | defect | open | `pod-enter`: a rule of the confinement on a tmpfs of the plan that the image does not hold is refused before the fork |
| [T-1444](upkeep.md) | P1 | S | M5 | defect | done | `pod-core`: `a_path_that_is_there_is_never_opened_truncated_or_removed` fails on Windows: a file made at the name of a directory gives "Access is denied", not `AlreadyExists` |
| [T-1445](upkeep.md) | P1 | S | M5 | defect | done | `pod-tpm`: two tests of `tests/socket.rs` fail on macOS: `set_read_timeout` on a stream whose peer closed gives an error, so the call says `Unavailable`, not `Protocol` |
| [T-1446](upkeep.md) | P1 | S | M5 | defect | done | `pod-warp`: `warp_exec_finds_warp_up_of_another_state_directory` fails on macOS: its socket path is longer than 104 bytes |
| [T-1447](upkeep.md) | P1 | S | M5 | defect | done | `nightly.yml`: the step of the doc tests runs the doc tests of `pod-h2`, which name `h2` and not `pod_h2`, so 41 fail on each system |
| [T-1450](relay.md) | P2 | M | M2 | defect | open | `pod-session`: the window of a link bounds the payload in flight, not the bytes of its records, so small records pass the relay's hold for a slow receiver and the cap of a leg |
| [T-1451](relay.md) | P2 | S | M2 | defect | open | `pod-relay`: `Offer::fit` moves at three quarters of the cap with a window of an eighth, which `pod_session::resume::move_under` does not allow |
| [T-1452](relay.md) | P3 | S | M2 | defect | open | `pod-session`: a window under 64 KiB waits 200 ms for each window, as the far end acknowledges each 64 KiB or after 200 ms |
| [T-1453](fleet.md) | P3 | S | M8 | chore | open | `pod-control`: take `pod_core::sockopt::shut_needs_no_limit` in place of `frame::shut_needs_no_timeout`, a twin of it |
| [T-1460](fleet.md) | P3 | S | M8 | defect | open | `pod-unit`: an install accepts a program in a directory that the host clears at a reboot or a logout, so the unit fails after the next reboot |
