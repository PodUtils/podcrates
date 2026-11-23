# Issues

Each GitHub issue of the record, with its kind and its entries. The State
starts with the state of the issue in the record: `open`, `closed` or
`deleted`; the sync (`scripts/issue-sync.py`) makes GitHub agree.
`TODO/github.txt` lists the numbers. `TODO/RULES.md`, section 4, gives the
kinds and the rules.

| Issue | Kind | Subject | State | Entries |
| --- | --- | --- | --- | --- |
| #1 | tracker | crates.io: reserved pod-* names and trusted publishing | closed | T-042, T-043, T-053, T-438 |
| #2 | coord | COORD-001: a test of the fallback channel between the sessions | closed | T-051 |
| #3 | coord | COORD-002: the answer to podvm's COORD-001 (to podvm) | closed | T-040, T-054, T-063 |
| #4 | coord | COORD-003: podvm can take pod-term now; the synchronous face is on main (to podvm) | closed | T-040, T-063, T-066 |
| #5 | request | [gh action] requested: ci.yml and release.yml (v0.4.0) on main | closed | T-015, T-049, T-063 |
| #6 | coord | COORD-004: the Alpine netboot pins of podvm (to podvm) | closed | T-066 |
| #7 | coord | COORD-005: podcrates v0.4.0 is released: the takes of podssh, podnet and podvm (to all) | closed | T-040, T-015, T-049 |
| #8 | request | [gh action] requested: the release v0.5.0 (ci.yml, nightly.yml, release.yml) | closed | T-067, T-068 |
| #9 | coord | COORD-006: take pod-proxy (feature blocking) (to podvm) | closed | T-040, T-069, T-070, T-071 |
| #10 | coord | COORD-007: podcrates v0.5.0 is released: each tag to v0.5.0 (to all) | closed | T-040, T-069 |
| #11 | coord | COORD-008: pod-hostprobe is imported from podvm-probe; the freeze; a statfs defect (to podvm) | closed | T-055, T-072 |
| #12 | coord | COORD-009: pod-dns names the DoH server that answered (to podnet) | closed | T-073 |
| #13 | coord | COORD-010: the registry patch of podvm on pod-proxy (to podvm) | closed | T-040, T-074, T-075, T-076, T-077 |
| #14 | request | [gh action] requested: the release v0.6.0 (ci.yml, nightly.yml, release.yml) | closed | T-055, T-072, T-073, T-074, T-075 |
| #15 | request | [gh action] requested: the release v0.6.0 from 4aa2ee0 | closed | T-055, T-072 to T-076, T-078 to T-080 |
| #16 | coord | COORD-011: podcrates v0.6.0 is released: each tag to v0.6.0 (to all) | closed | T-055, T-073, T-075, T-076, T-078, T-079, T-080 |
| #17 | coord | COORD-012: take pod-hostprobe (to podvm) | closed | T-055, T-040, T-081 |
| #18 | request | [gh action] requested: the release v0.7.0 (ci.yml, nightly.yml, release.yml) | closed | T-077, T-081 |
| #19 | request | [gh action] requested: nightly.yml and release.yml (v0.7.0) from a13014d | closed | T-077, T-081 |
| #20 | report | `pod-proxy`: the `Host` header of an IPv6 literal on port 443 | closed | T-082 |
| #21 | report | `pod-proxy`: no SOCKS5 UDP path | closed | T-083 |
| #22 | report | The record of `SOURCES` names a git blob id, not SHA-256 | closed | T-084 |
| #23 | report | `check-sources.py` assumes the branch `main` | closed | T-085 |
| #24 | report | Import `pod-gsr` and its data file | closed | T-086 |
| #25 | report | `pod_gsr::Stream` cuts a record as a clean end | closed | T-087 |
| #26 | report | `pod-gsr` synchronous face | closed | T-088 |
| #27 | report | `tailscale-derp` watch checks only `/Regions` | closed | T-089 |
| #28 | report | `ajam-relay` hosts have no watch | closed | T-090 |
| #29 | report | `Way::lookup` skips the name check | closed | T-091 |
| #30 | report | No `live.yml` | closed | T-092 |
| #31 | report | README state and crate table are stale | closed | T-093 |
| #32 | report | `docs/design.md` repository table is stale | closed | T-094 |
| #33 | report | The static probe lacks `pod-proxy/blocking` | closed | T-095 |
| #34 | report | `pod-proxy` README row lists 4 of 6 variables | closed | T-096 |
| #35 | tracker | cfb: meta request for the tools that cfb needs from PodUtils (84 asks) | open (the owner's tracker; routed in `docs/research/fleet/asks.md`; M8 waits) | T-098, T-099 to T-120 |
| #36 | tracker | githome: meta-meta tracker of the 14 asks of githome (rulings, trackers, index) | open (the owner's tracker; routed in `docs/research/fleet/asks.md`; M8 waits) | T-098, T-099, T-100, T-103, T-104, T-106, T-109 |
| #37 | coord | COORD-013: podcrates v0.7.0 is released: each tag to v0.7.0 (to all) | closed | T-077, T-081 |
| #38 | coord | COORD-014: the asks of cfb (#35) for podssh, and the rulings of 2026-11-09 (to podssh) | closed | T-098 |
| #39 | coord | COORD-015: the asks of cfb (#35) and githome (#36) for podvm, and the rulings of 2026-11-09 (to podvm) | closed | T-098, T-099, T-108 |
| #40 | coord | COORD-016: the asks of cfb (#35) for podnet, and the rulings of 2026-11-09 (to podnet) | closed | T-098, T-109, T-121 to T-127 |
| #41 | coord | COORD-017: podcrates v0.8.0 is released: each tag to v0.8.0; podvm takes pod-supervise (to all) | closed | T-060, T-086, T-101, T-125, T-126, T-127, T-128 |
| #42 | coord | COORD-018: pod-proxy holds the SOCKS5 datagram path of podnet-netns upstream.rs: those lines are frozen (to podnet) | closed | T-083 |
| #43 | coord | COORD-019: podcrates v0.9.0 is released: each tag to v0.9.0; podvm takes pod-guest and pod-enter (to all) | closed | T-059, T-061, T-083, T-132, T-134, T-135, T-136 |
| #44 | coord | COORD-020: the answers to podssh#175 (no public Wisp server qualifies) and #177 (the proxy lists and nodes) (to podssh) | closed | T-130, T-133 |
| #45 | coord | COORD-021: podcrates v0.10.0 is released: each tag to v0.10.0; podvm takes pod-oci (to all) | closed | T-056, T-057, T-133, T-139, T-141 |
| #46 | coord | COORD-022: podcrates v0.11.0 is released: podvm takes pod-extract and pod-rootfs; the others may move the tag (to all) | closed | T-057, T-058, T-139, T-141 |
| #47 | tracker | Tracker: run GitHub CI (and other CI) on any machine: podrunner, pod-dockerapi and the podvm path | open (the tracker of T-146; PodUtils/podrunner#1 holds the plan) | T-146 |
| #48 | coord | COORD-023: podcrates v0.12.0 is released: podvm moves to the table of path maps (#224); the others may move the tag (to all) | closed | T-137, T-144, T-145, T-148 |
| #49 | coord | COORD-024: podcrates v0.13.0 is released: podssh and podnet take pod-ws (to all) | closed | T-016 |
| #50 | coord | COORD-025: podcrates v0.14.0 is released: podssh and podnet take pod-quic; podssh reads serveo, ssh-j and croc (to all) | closed | T-019, T-153, T-156 |
| #51 | coord | COORD-026: the asks of podvm#227 are record entries that wait for DR-1 and DR-2 (to podvm) | closed | T-160, T-161, T-162, T-163, T-164, T-165 |
| #52 | coord | COORD-027: pod-enter: the volume of a missing -w and the reserved guest paths in Maps::new (T-155, PodUtils/podvm#226) (to podvm) | closed | T-155 |
| #53 | coord | COORD-028: the owner pauses each session after its last runs; update the hand-off documents first (to all) | closed | T-020, T-022, T-166 |
| #54 | coord | COORD-029: podcrates v0.15.0 is released: podssh takes pod-wormhole and pod-session; podnet takes pod-hold; podvm moves its pod-enter calls (to all) | closed | T-020, T-022, T-155, T-166 |
| #55 | coord | COORD-030: the owner named podrunner (DR-1 and DR-2 are yes); podnet comes first: take pod-hold, start PodUtils/podnet@T-229 to PodUtils/podnet@T-231, two questions on the cut (to podnet) | closed | T-166, T-169, T-170 |
| #56 | coord | COORD-031: the owner's DR-5: podrunner waits for PodUtils/podvm@T-1656, and podvm must support incus or have an adapter for it; the answer to COORD-023 (to podvm) | closed | T-173 |
| #57 | coord | COORD-032: podcrates v0.16.0 is released: podrunner takes pod-supervise and pod-broker; podvm takes the tree kill and the private modes; podnet and podssh may move the tag (to all) | closed | T-157, T-160, T-162, T-175 |
| #58 | coord | COORD-033: podcrates v0.17.0 is released: podrunner takes pod-job and the mutual TLS of pod-tls; podvm takes the limits of pod-enter (to all) | closed | T-163, T-164, T-165, T-176 |
| #59 | coord | COORD-034: the take of pod_hold::connect and pod_hold::packet: adoption/podnet/pod-hold-connect.patch (to podnet) | closed | T-169, T-170 |
| #60 | coord | COORD-035: podcrates v0.18.0 is released: podrunner takes the names mode of pod-broker and the schema rule; podssh takes three tunnels, the key of ssh-j and Rewound::new (to all) | closed | T-161, T-171, T-172, T-173, T-177, T-178, T-181 |
| #61 | coord | COORD-036: the cut of pod-netstack (the userspace TCP and UDP stack on smoltcp 0.14): which files of podnet, and when (to podnet) | closed | T-023, T-170 |
| #62 | coord | COORD-037: podcrates v0.19.0 is released: podvm takes the library directories of the image (podvm#247); pod-broker refuses a raw byte in a path (to all) | closed | T-142, T-149, T-152, T-182, T-183, T-184 |
| #63 | coord | COORD-038: podcrates takes the incus client as pod-incus (T-179), for the tag v0.21.0 (to podvm) | closed | T-179 |
| #64 | coord | COORD-039: podcrates v0.20.0 is released: podnet takes pod-netstack; podssh takes the key of localhost-run, sandhole and openbare-xyz (to all) | closed | T-023, T-185, T-186, T-187, T-188 |
| #65 | coord | COORD-040: podcrates v0.21.0 is released: podvm takes pod-incus; pod-core and pod-identity hold one store and one pin at a time; the socket of pod-broker has its mode at once (to all) | closed | T-179, T-195, T-196 |
| #66 | coord | COORD-041: Reno and CUBIC on smoltcp 0.14 are slow only on a link with one ACK for each round trip (T-188): podnet keeps CongestionControl::None; warp --cc (to podnet) | closed | T-188, T-197 |
| #67 | coord | COORD-042: podcrates cuts pod-wg from src/warp/wg.rs at 54d7ed4 (T-024): hold wg.rs to fixes until your take (to podnet) | closed | T-024 |
| #68 | coord | COORD-043: the answer to PodUtils/podnet#446: register.rs and route.rs need no port; keep the veth wire in veth.rs (T-201) (to podnet) | closed | T-201 |
| #69 | coord | COORD-044: the answer to PodUtils/podvm#259: podcrates writes pod-confine and Plan.confine of pod-enter (T-108) (to podvm) | closed | T-108 |
| #70 | coord | COORD-045: the answer to PodUtils/podvm#260: podcrates reads the modloop (T-206) and writes pod-netlab (T-207), after pod-confine (to podvm) | closed | T-206, T-207 |
| #71 | coord | COORD-046: v0.22.0: pod-e2e for podssh, pod-wg for podnet, the death signal of a payload for podvm and podrunner (to all) | closed | T-021, T-024, T-202, T-205 |
| #72 | coord | COORD-047: v0.23.0: pod-confine and Plan.confine; the empty root entry of a layer (to podvm, to all) | closed | T-108, T-208 |
| #73 | coord | COORD-048: v0.24.0: the module set of a guest from its modloop; gcr.io, mirror.gcr.io and the NIC module sums in pod-services (to podvm, to all) | closed | T-206, T-211 |
| #74 | coord | COORD-049: the answer to PodUtils/podnet#449, #450 and #451: the freeze is in the record; WARP as a network in T-028; T-214 and T-215 for the file transfer and the mailbox server (to podnet) | closed | T-026, T-028, T-029, T-170, T-201, T-214, T-215 |
| #75 | coord | COORD-050: the answer to PodUtils/podnet#452 and #453: the hint words and the traits are in T-214 and T-028; pod-git is T-217 to T-220 (to podnet) | closed | T-028, T-214, T-217, T-218, T-219, T-220 |
| #76 | coord | COORD-051: v0.25.0: the pod-netstack calls of podnet's router; pod-confine closes the scopes, UDP on request, MPTCP, io_uring and the root capabilities (to all) | closed | T-025, T-209, T-221, T-222 |
| #77 | coord | COORD-052: v0.26.0: the veth wire of the netns hold, the facts of the SSH forward relays, the time to the first step and the job images of pod-job (to all) | closed | T-201, T-224, T-226, T-227, T-233 |
| #78 | report | Docs: stale names and contradictions after the renames of 2026-11-12 (pods, podstat, podforge) | closed | T-242 |
| #82 | tracker | cfb asks: tracker for the 36 asks of cfb owned by podcrates (from podcrates#35) | open (the owner's tracker; each ask maps to its entry) | T-099, T-100, T-101, T-102, T-103, T-104, T-105, T-106, T-107, T-113, T-114, T-115, T-116, T-117, T-118, T-119, T-443, T-444 |
| #83 | tracker | githome asks: tracker for the 2 asks owned by podcrates (from podcrates#36) | open (the owner's tracker; each ask maps to its entry) | T-099, T-100, T-106, T-160 |
| #84 | tracker | CI plan items: tracker for the 8 items owned by podcrates (from podcrates#47) | open (the owner's tracker; phases 3 and 5 stay in T-146) | T-100, T-102, T-103, T-104, T-106, T-146, T-162, T-251 |
| #85 | coord | COORD-053: v0.27.0: the close of a listener and no RST for a burst in pod-netstack; podnet's relay client and the podssh trial of pod-relay; the socket families of pod-confine (to all) | closed | T-025, T-026, T-213, T-223, T-227, T-230, T-231, T-232, T-233, T-240, T-262 |
| #86 | coord | COORD-054: v0.28.0: qemu-img convert in pod-guest; a TCP list refused with socketcall(2); the UDP of pod-netstack; Tox, Yggdrasil, HyperDHT, workerd and wasmtime in pod-services (to all) | closed | T-174, T-228, T-234, T-244, T-252, T-253, T-254, T-255, T-256, T-257, T-258, T-259, T-260, T-262, T-266 |
| #87 | coord | COORD-055: v0.29.0: the hold of a SYN in pod-hold; an MQTT meeting point in pod-wormhole; a channel handler, the Host of an IPv6 relay and one cache file for each host in pod-relay (to all) | closed | T-150, T-170, T-225, T-229, T-245, T-246, T-261, T-271, T-272 |
| #88 | coord | COORD-056: the answer to PodUtils/podnet#479: the asks of the link workstream are T-278, T-279 and T-300 to T-305; pod-link is a crate of podcrates (to podnet) | closed | T-278, T-279, T-300, T-301, T-302, T-303, T-304, T-305, T-442 |
| #89 | coord | COORD-057: v0.30.0: podnet's relay client over pod-relay (the trial); the MQTT brokers in the runs of a code; the Syncthing relays; pod-actions-expr and pod-actions-workflow (to all) | closed | T-026, T-251, T-267, T-276, T-277 |
| #90 | coord | COORD-058: v0.31.0: podvm's arm64 defects in pod-extract and pod-enter; a Run that owns its workflow; the display form of a fingerprint (to all) | closed | T-278, T-279, T-309, T-320, T-321 |
| #91 | coord | COORD-059: the answer to PodUtils/podvm#283: podcrates writes the trial of pod-relay's blocking facade in podvm (T-326); the wire is reverse-v1 (to podvm) | closed | T-326 |
| #92 | coord | COORD-060: the answer to PodUtils/podnet#502: the crate's node and operator pass through the live relay from a runner with a direct egress; podcrates needs your log of relay connect -v (to podnet) | closed | T-327 |
| #93 | coord | COORD-061: v0.32.0: the flush of pod-relay's operator; pod-link; TURN's integrity in pod-proxy; smee-io and hostc; podvm's trial of the blocking facade (to all) | closed | T-300, T-311, T-322, T-323, T-324, T-325, T-326, T-327, T-329, T-370 |
| #94 | report | pod-devtunnel: add a Dev Tunnels management and relay client | open (T-386, the live run, waits for the owner's token) | T-378, T-386, T-385 |
| #95 | report | pod-services: add SSH over 443 to the github-ssh service | closed | T-372 |
| #96 | report | pod-services: add QSocket and Portal candidate relay entries with their measured limits | closed | T-376 |
| #97 | report | pod-services: record Sandhole's required remote port | closed | T-373 |
| #98 | report | pod-services: verify Serveo's URL rule and record anonymous authentication | closed | T-374 |
| #99 | report | pod-services: add tuns.sh as an SSH tunnel entry | closed | T-375 |
| #100 | report | pod-turn: add a TURN client on the existing STUN codec | closed | T-377 |
| #101 | coord | COORD-062: the answer to PodUtils/podnet#510 and #512: pod-devtunnel and pod-turn in work; remote-port and Serveo's rule done; A11 and A12 in work, A13 stands as C119 (to podnet) | closed | T-373, T-374, T-377, T-378, T-390, T-391, T-392 |
| #102 | coord | COORD-063: GNU tar with an absolute -C fails in a unit of the supervise rung; a relative -C passes (PodUtils/podrunner#70, podcrates T-328) (to podvm) (to podrunner) | closed | T-328 |
| #103 | coord | COORD-064: v0.33.0: pod-turn, pod-devtunnel, pod-warp (first part), pod-netlab (offline part), A11 and A12 (to all) | closed | T-028, T-207, T-372, T-373, T-374, T-375, T-376, T-377, T-378, T-390, T-391 |
| #104 | coord | COORD-065: v0.34.0: the TCP carrier of pod-link with the pin of an Ed25519 key, podnet's ip.rs settings, closed WARP sessions fail their flows (to all) | closed | T-304, T-335, T-355, T-394, T-396 |
| #105 | coord | COORD-066: the answer to PodUtils/podssh#220: the kind file-host (T-416) and a board over pod_wormhole::mqtt (T-417) (to podssh) | closed | T-416, T-417, T-418 |
| #106 | coord | COORD-067: the answer to PodUtils/podnet#515: the next carriers of pod-link in your order (T-420 to T-424, T-310) (to podnet) | closed | T-310, T-420, T-421, T-422, T-423, T-424 |
| #107 | tracker | podswarm: meta tracker of the spec and of the asks of podswarm to PodUtils (crates, repository, consumers) | deleted (moved to PodUtils/podswarm#1) | T-425, T-430, T-431, T-432, T-433, T-434, T-435, T-436, T-437, T-438, T-439, T-440, T-441, T-442, T-443, T-444, T-445, T-446, T-447, T-448, T-449, T-450 |
| #108 | report | podswarm: the text of the spec (one comment for each file) | closed | T-425 |
| #109 | report | pod-cas: add a BLAKE3 content store with resume, placement, sweep and tree manifests (podswarm P-12) | open (an ask of podswarm; it waits until the podswarm session starts: the ruling "podswarm") | T-434 |
| #110 | report | pod-fuse: add a FUSE server with no libfuse, with the write opcodes and invalidation (podswarm P-16) | open (an ask of podswarm; it waits until the podswarm session starts: the ruling "podswarm") | T-433 |
| #111 | report | pod-tty: add a guard of the local terminal with raw mode, restore and the escape line (podswarm P-11) | open (an ask of podswarm; it waits until the podswarm session starts: the ruling "podswarm") | T-432 |
| #112 | report | pod-ssh: add an SSH client library with a host key policy, an auth chain and an exec channel (podswarm P-13) | open (an ask of podswarm; it waits until the podswarm session starts: the ruling "podswarm") | T-437 |
| #113 | report | pod-proc: add portable process control with a spawner, a keeper, containment rungs and a tree kill (podswarm P-1) | open (an ask of podswarm; it waits until the podswarm session starts: the ruling "podswarm") | T-430, T-431 |
| #114 | report | pod-view: add a live view of a directory with records, a frame codec and an ordered write-back log (podswarm P-17) | open (an ask of podswarm; it waits until the podswarm session starts: the ruling "podswarm") | T-435 |
| #115 | report | pod-attest: add the check of an SEV-SNP attestation report and the expected measurement of a guest set (podswarm P-19) | open (an ask of podswarm; it waits until the podswarm session starts: the ruling "podswarm") | T-436 |
| #116 | report | pod-ws: add a serve module with an accept loop, stage deadlines and caps per address (podswarm P-2) | closed | T-439 |
| #117 | report | pod-tls: add an enrolment mode to the client verifier of peer that admits any Ed25519 key and reports it (podswarm P-3) | open (an ask of podswarm; it waits until the podswarm session starts: the ruling "podswarm") | T-440 |
| #118 | report | pod-job: add LimitMode::Polled for a limit that a keeper measures and enforces by a kill (podswarm P-8) | open (an ask of podswarm; it waits until the podswarm session starts: the ruling "podswarm") | T-449 |
| #119 | report | pod-core: add Stage::Task for a failure of a task or its process tree (podswarm P-9) | open (an ask of podswarm; it waits until the podswarm session starts: the ruling "podswarm") | T-450 |
| #120 | report | pod-proxy: add ChildIo::spawn_args with no shell, a stderr pipe and a wait future (podswarm P-10) | open (an ask of podswarm; it waits until the podswarm session starts: the ruling "podswarm") | T-441 |
| #121 | report | pod-hold: grow the seccomp hold into a shared trap: a marker, ENOSYS calls, argv reads and a plan before exec (podswarm P-14, P-15) | open (an ask of podswarm; it waits until the podswarm session starts: the ruling "podswarm") | T-445, T-446 |
| #122 | report | pod-guest: add a Linux flavour of plan::argv with one virtio-serial channel, one raw disk, mem-lock and an SNP variant (podswarm P-18) | open (it helps podvm's `pod-microvm`: M7; the SNP variant waits until the podswarm session starts) | T-447 |
| #123 | report | pod-enter: correct the text of fuse.rs: mount(2) of FUSE works in a user namespace since Linux 4.18 (podswarm P-20) | open (it helps podvm: M6, item 5 of the work order) | T-448 |
| #124 | coord | podswarm asks podvm: a library call for run, the probe cache, cgroup limits, payload death, a copy-free root, warm exec, the guest plan and versioned schemas (to podvm) | closed | T-425 |
| #125 | coord | podswarm asks podnet: live doors, port rules, an exit on stdio, a decision on one-flow aggregation and a report of podnet pod (to podnet) | closed (podnet answered the five asks N-1 to N-5 in PodUtils/podnet#517; T-425 is done in `v0.33.0`; no work is left in podcrates) | T-425 |
| #126 | coord | COORD-068: the answer to PodUtils/podnet#518 (T-475, port rules) and PodUtils/podssh#221 (T-476, invariants 6 to 10 of the HTTP road) (to podnet) (to podssh) | closed | T-475, T-476 |
| #127 | coord | COORD-069: v0.35.0: pod-warp (relay race, HTTP/3, WireGuard, proxy, L4 door, Live, TUN), the kind file-host, an MQTT board, coturn and distribution on the loopback (to all) | closed | T-336, T-337, T-338, T-339, T-353, T-358, T-380, T-393, T-395, T-406, T-416, T-417 |
| #128 | coord | COORD-070: the answer to PodUtils/podvm#291: the document of pod-hostprobe's own types, with its reader (T-485) (to podvm) | closed | T-485 |
| #129 | coord | COORD-071: v0.36.0: the HTTP road's invariants 6 to 10 and its serial deadlock fixed, the report of pod-hostprobe with its reader, the trial of pod-warp in podnet (to all) | closed | T-412, T-476, T-477, T-485 |
| #130 | deferred | [DEFERRED] Serve ECH, shape the ClientHello, and measure the handshake | open (deferred by its author) | T-490 |
| #131 | coord | pod-git: T-217 to T-220 move to PodUtils/podforge (deferred) (to podforge) | closed | T-217, T-218, T-219, T-220 |
| #132 | coord | COORD-072: podrunner's Q10 (a Docker Engine API socket for each job): which decision did podrunner record? (to podrunner) | closed | T-146 |
| #133 | coord | COORD-073: the answer to PodUtils/podnet#521: the plan, the order and the tags for podnet's complete beta (to podnet) | closed | T-027, T-029, T-301, T-305, T-310, T-420, T-421, T-475, T-493, T-494, T-495, T-496, T-497, T-498, T-499, T-500, T-501, T-502 |
| #134 | coord | COORD-074: v0.37.0: pod-warp's team token, pod-enter's /proc and its joined entry for exec, the Windows build of pod-relay's tests (to all) | closed | T-480, T-488, T-491, T-492 |
| #135 | coord | COORD-075: pod-warp: the freeze of src/warp and crates/podssh-warp (to podnet) (to podssh) | closed | T-339, T-412, T-413 |
| #136 | coord | COORD-076: v0.38.0: pod-warp's port rules, pod-dns TXT, pod-e2e's record layer, pod-ws serve and the ws and h2 carriers; /dev/pts and the knob flags; ActorClass agent and bot (to all) | closed | T-301, T-305, T-420, T-421, T-439, T-475, T-488, T-503, T-504, T-505, T-507 |
| #137 | coord | COORD-077: v0.39.0: QUIC and nc over WARP's datagrams to IPv4 (podnet#524), the rules of podnet's WebSocket carrier, the ARQ of pod-link (to all) | closed | T-310, T-506, T-508, T-510 |
| #138 | coord | COORD-078: a question on the udp carrier: the congestion window of KCP by default, or nodelay, against the gate of 20 % loss (T-520) (to podnet) | closed | T-520 |
| #139 | coord | COORD-079: the crate kcp needs no overflow checks in a debug build: set the same profile in podnet (T-526) (to podnet) | closed | T-526 |
| #140 | coord | COORD-080: v0.40.0: the poll, quic, udp, dns and icmp carriers of pod-link; cpu_s and peak_rss_mib in pod-job; the forms of the file hosts (to all) | closed | T-422, T-423, T-424, T-493, T-495, T-497, T-517, T-518, T-520, T-521, T-526, T-527, T-528, T-529, T-531 |
| #141 | coord | COORD-081: the answer to PodUtils/podnet#533, #535, #538 and #540: the entries of the asks (to podnet) | closed | T-214 |
| #142 | coord | COORD-082: the answer to PodUtils/podvm#322: the order of T-486, T-107 and T-345 to T-347 (to podvm) | closed | T-486, T-107, T-345, T-346, T-347 |
| #143 | coord | COORD-083: v0.41.0: the termios mode of pod-term; four tox relays (to all) | closed | T-537, T-539, T-555 |
| #144 | coord | COORD-084: the answer to PodUtils/podnet#547: the preshared key and a responder with no I/O of pod-wg (to podnet) | closed | T-561, T-562 |
| #145 | coord | COORD-085: v0.42.0: the asks of each consumer (to all) | closed | T-486, T-107, T-541, T-542, T-543, T-544, T-549, T-552, T-553, T-556, T-559, T-563 |
| #146 | coord | COORD-086: the answer to PodUtils/podvm#326: a completion of two layers in pod-rootfs (to podvm) | closed | T-567 |
| #147 | coord | COORD-087: the answer to PodUtils/podvm#331, #335 and #337: three defects of text in pod-enter and pod-hostprobe (to podvm) | closed | T-568, T-569, T-570 |
| #148 | coord | COORD-088: the answer to PodUtils/podvm#339: a foreground holder in pod-supervise (T-573) (to podvm) | closed | T-573 |
| #149 | coord | COORD-089: v0.43.0: pod-wg for podnet; pod-actions-store and pod-actions-results for podrunner; pod-rootfs, pod-enter and pod-hostprobe for podvm (to all) | closed | T-561, T-562, T-564, T-565, T-567, T-568, T-569, T-570, T-571 |
| #150 | coord | COORD-090: the answer to PodUtils/podvm#341, #342 and #343: three defects of the QA report (to podvm) | closed | T-580, T-581, T-582 |
| #151 | coord | COORD-091: the answer to PodUtils/podvm#344 to #347: four more defects of the QA report (to podvm) | closed | T-583, T-584, T-585, T-586 |
| #152 | coord | COORD-092: the answer to PodUtils/podvm#350: the Windows guest (to podvm) | closed | T-590, T-591, T-592, T-593, T-594, T-595, T-596 |
| #153 | coord | COORD-093: the answer to PodUtils/podvm#352 and #354: pod-extract (to podvm) | closed | T-597, T-598, T-599 |
| #154 | coord | COORD-094: v0.44.0: a fix of security in pod-extract; podvm's QA defects; the foreground hold; pod-actions-workflow; the socket rules of pod-hold (to all) | closed | T-573, T-580, T-581, T-582, T-597, T-574, T-558, T-575, T-572, T-577, T-576 |
| #155 | coord | COORD-095: the answer to PodUtils/podvm#359, #360 and #361: pod-broker texts, a long name in fat16, a host of pod-services (to podvm) | closed | T-609, T-610, T-611 |
| #156 | coord | COORD-096: the answer to PodUtils/podnet#555: the label of a unix proxy and the capnp reader of pod-cloudflare (to podnet) | closed | T-612, T-029 |
| #157 | coord | COORD-097: v0.45.0: pod-udf and pod-qga, pod-guest for the Windows guest; a credential leak in pod-proxy; podvm's QA defects; pod-session and pod-relay; pod-netstack; podrunner's changes (to all) | closed | T-583, T-584, T-585, T-586, T-590, T-591, T-592, T-593, T-594, T-595, T-598, T-599, T-603, T-604, T-605, T-606, T-609, T-610, T-611, T-612, T-616, T-589, T-557, T-579, T-560, T-587, T-588, T-600, T-566, T-613, T-578 |
| #158 | coord | COORD-098: the answer to PodUtils/podvm#370: the provision stops the emulator on each error (to podvm) | closed | T-622 |
| #159 | coord | COORD-099: v0.46.0: the warm start and the emulator guard of pod-guest; the S3 carrier and the split session of pod-link (to all) | closed | T-596, T-622, T-615, T-618, T-619, T-494, T-500, T-525 |
| #160 | coord | COORD-100: v0.47.0: Nostr, trackers and ntfy as meeting points; pod-devtunnel with no user token; the swap of a Windows mailbox (to all) | closed | T-550, T-551, T-554, T-489, T-478, T-381, T-403, T-627, T-623, T-602, T-629, T-630 |
| #161 | coord | COORD-101: the answer to PodUtils/podvm#372: Sign::After and a screen sign that ignores a cursor (to podvm) | closed | T-633 |
| #162 | coord | COORD-102: v0.48.0: Sign::After and a screen sign that ignores a cursor in pod-guest (to all) | closed | T-633 |
| #163 | coord | COORD-103: v0.49.0: the Nostr fix of the run of a code; the warm restore on a new mailbox medium; the SHA-256 answers of pod-turn (to all) | closed | T-634, T-631, T-632, T-628 |
| #164 | coord | COORD-104: v0.50.0: step 1 of the move of podssh: pod-transit, pod-acme, pod-piping, pod-opentunnel, pod-derp (to all) | closed | T-027, T-034, T-032, T-033, T-031 |
| #165 | request | [gh action] requested: guest-windows.yml on main at 1368e49 (v0.51.0) | closed | T-640 |
| #166 | request | [gh action] requested: ci, adoption, plants, secrets and deny on main at 0c6c4c0 | closed | T-643, T-645, T-646, T-647, T-648, T-650 |
| #167 | request | [gh action] requested: the checks of a push at a634d3a | closed | T-645 |
| #168 | request | [gh action] requested: ci, adoption, plants, secrets and deny on main at a634d3a | closed | T-643, T-645, T-646, T-647, T-648, T-650 |
| #169 | request | [gh action] requested: the gates of main at 482ba53 and the release v0.51.0 | closed | T-035, T-639, T-640, T-643, T-647, T-648, T-650, T-651 |
| #170 | request | [gh action] requested: the gates of main at 98017c3 and the release v0.51.0 | closed | T-677, T-035, T-639, T-640, T-643, T-647, T-648, T-650, T-651 |
| #171 | request | [gh action] requested: the gates of main at 7119770 and the release v0.51.0 | closed | T-678, T-035, T-639, T-640, T-643, T-647, T-648, T-650, T-651 |
| #172 | coord | COORD-105: v0.51.0: pod-egress part 1; the fixes of podnet#572, podvm#380, podssh#253; the asks of podvm#389, #390, #393, #394 (to all) | closed | T-035, T-639, T-640, T-643, T-647, T-648, T-650, T-651 |
| #173 | request | [gh action] requested: ci, adoption, plants, secrets and deny on main at 2f23632 (v0.52.0) | closed | T-035, T-668, T-669, T-670, T-671, T-672, T-676, T-678 |
| #174 | request | [gh action] requested: the gates of main at a866f99 and the release v0.52.0; the reruns of services.yml | closed | T-035, T-668, T-669, T-670, T-676, T-679, T-684, T-685 |
| #175 | request | [gh action] requested: the gates of main at 7998b72 and the release v0.52.0; the reruns of services.yml | closed | T-035, T-668, T-669, T-670, T-676, T-679, T-684, T-685 |
| #176 | coord | COORD-106: v0.52.0: pod-egress parts 2 and 3; the asks of podvm#395, #403, #406, #407 (to all) | closed | T-035, T-668, T-669, T-670, T-671, T-672, T-676 |
| #177 | request | [gh action] requested: ci, adoption, plants, secrets and deny on main at 3ee1988 | closed | T-686, T-687, T-684 |
| #178 | request | [gh action] requested: the gates of main at 44e9538 and the release v0.53.0 | closed | T-686, T-687, T-688, T-684 |
| #179 | request | [gh action] requested: the gates of main at 7ef7aab and the release v0.53.0 | closed | T-686, T-687, T-688, T-691, T-684, T-713 |
| #180 | coord | COORD-107: the answer to PodUtils/podrunner#180: podcrates takes pod-lab (T-692); each project runs its releases in the labs (to all) | closed | T-692 |
| #181 | coord | COORD-108: the answer to PodUtils/podnet#584: podcrates takes pod-manual (T-700); each command line takes its help and its manual from it (to all) | closed | T-700 |
| #182 | coord | COORD-109: the owner's ruling "Installer": one POSIX installer for each project, hermetic, idempotent, fast, run in the labs (T-702); podrunner, please give your lessons (to all) | closed | T-702 |
| #183 | coord | COORD-110: the owner's ruling "Issues": the state of each issue comes from the record by a tool, not by hand (T-703) (to all) | closed | T-703 |
| #184 | request | [gh action] requested: the gates of main at d0f38a6 and the release v0.53.0 | closed | T-686, T-687, T-688, T-691, T-684, T-713 |
| #185 | coord | COORD-111: v0.53.0: the fix of PodUtils/podssh#262, the device nodes and the passed descriptors of pod-enter (PodUtils/podvm#414, #422) (to all) | closed | T-686, T-687, T-688, T-691, T-684 |
| #186 | coord | COORD-112: the owner's rulings "Watcher" and "Strangers": one watcher of the organization for each session, scripts/agent/watch.py at a tag of podcrates; replace your own (to all) | open (podrunner runs its own watcher) | T-710, T-711, T-712 |
| #187 | request | [gh action] requested: the gates of main at 32f7a72 and the release v0.54.0 | closed | T-692, T-700, T-710, T-711, T-712 |
| #188 | coord | COORD-113: v0.54.0: pod-lab and pod-manual part 1; run your release in the labs, and take your help and manual from pod-manual (to all) | closed | T-692, T-700 |
| #189 | request | [gh action] requested: the checks of main at aec056d and a dry run of issue-sync.yml | closed | T-703 |
| #190 | request | [gh action] requested: the checks of main at bf13591 and a dry run of issue-sync.yml | closed | T-703 |
| #191 | request | [gh action] requested: the checks of main at 320ebdf (the rules of the issues opt in) and a dry run of issue-sync.yml | closed | T-703 |
| #192 | coord | COORD-114: the rules of the issues found drift in the records of podssh and podnet; repair the map, then turn the rules on (to podssh) (to podnet) | closed | T-703 |
| #193 | request | [gh action] requested: the gates of main at e3c3e1d and the release v0.55.0 | closed | T-703, T-701 |
| #194 | coord | COORD-115: v0.55.0: the sync of the issues (call issue-sync.yml at this tag) and 18 new lab profiles from podssh and podnet (to all) | closed | T-703, T-701 |
| #195 | request | [gh action] requested: the gates of main at 93bf133 and the release v0.56.0 | closed | T-723 |
| #196 | request | [gh action] requested: the gates of main at 0d6421c, a dry run of issue-sync.yml, and the release v0.57.0 | closed | T-724, T-731 |
| #197 | request | [gh action] requested: ci and a dry run of issue-sync.yml at 0d6421c again, and the release v0.57.0 | closed | T-724 |
| #198 | request | [gh action] requested: the gates of main at f84fb23 and the release v0.58.0 | closed | T-725, T-737, T-731, T-702 |
| #199 | request | [gh action] requested: the gates of main at ca1107f and the release v0.58.0 | closed | T-725, T-737, T-731, T-702 |
| #200 | coord | COORD-116: v0.58.0: the installer core (ruling "Installer"), podlab as a release asset, --allow-connect, and podnet's gaps of pod-manual (to all) | closed | T-702, T-725, T-737 |
| #201 | coord | COORD-117: the answer to PodUtils/podrunner#191: the mount row exists since v0.52.0; the chroot row in a user namespace is new on main (T-744) (to podrunner) | closed | T-744, T-670 |
| #202 | request | [gh action] requested: the gates of main at 5792f06 and the release v0.59.0 | closed | T-738, T-742, T-743, T-744, T-736, T-732 |
| #203 | request | [gh action] requested: the gates of main at 0e78211 and the release v0.59.0 | closed | T-738, T-739, T-742, T-743, T-744, T-736, T-732 |
| #204 | coord | COORD-118: v0.59.0: the volumes of pod-enter (podvm#440), podnet's two defects (podnet#592), the chroot row (podrunner#191) (to all) | closed | T-738, T-739, T-742, T-743, T-744 |
| #205 | request | [gh action] requested: the gates of main at 725b729 and the release v0.60.0 | closed | T-716, T-714, T-715, T-745, T-726, T-750 |
| #206 | coord | COORD-119: v0.60.0: the network labs of pod-lab, 20 host-class rows of pod-hostprobe, clap's requires in pod-manual, the overlay upper layer of pod-enter (to all) | closed | T-716, T-714, T-715, T-745, T-726, T-750 |
| #207 | coord | COORD-120: the owner's ruling "Web access": use the services of web.pkgforge.dev first for each web or git search and read (to all) | closed | none |
| #208 | request | [gh action] requested: the delete of the four `wip/` branches whose work is on main | closed (the four branches are deleted: `main` is the one branch of the repository) | T-717, T-718, T-746, T-751 |
| #209 | coord | COORD-121: v0.61.0: podlab run --way-out, the rows that send a packet out, the fix of chroot_usable (podvm#443), the signed installer (to all) | closed | T-746, T-751, T-758, T-717, T-718 |
| #210 | coord | COORD-122: v0.62.0: the servers of the command in a network lab (podssh#282), the ids inside the user namespace (podvm#447), the line of each resume (podssh#283) (to all) | closed | T-759, T-760, T-786, T-767 |
| #211 | coord | COORD-123: v0.63.0: the ids of the plan for a foreground podvm run (podvm#452), the installer for a copy of a tree (podnet#597), the rule "needs one of" of pod-manual (to all) | closed | T-787, T-773, T-756, T-788 |
| #212 | report | pod-turn has no STUN client, so the NAT probe lives in podssh | open | T-802 |
| #213 | report | the pod-link ARQ has no forward erasure code | open | T-803 |
| #214 | coord | COORD-124: v0.64.0: pod-tor and pod-iroh (the move of podssh), the SIGPIPE of the lab (podvm#455), the wire of pod-link, the line of each stall (podssh#283) (to all) | closed | T-767, T-792, T-747, T-755, T-796, T-774, T-775, T-776, T-777, T-778, T-800, T-804, T-038, T-037 |
| #215 | coord | COORD-125: v0.65.0: the PID namespace of pod-enter (podvm#457), a node on a held lost socket (podnet#604), the layer on the HTTP road (podssh#287), four forms of pod-manual (podssh#288), the roots of a way out (podvm#455) (to all) | closed | T-806, T-805, T-801, T-797, T-807, T-808, T-810 |
| #216 | coord | COORD-126: v0.66.0: the init of a PID namespace keeps no descriptor of the caller (podvm#460), the writes of pod-devtunnel (to all) | closed | T-811, T-378 |
| #217 | coord | COORD-127: the dig study and the relay role: two speeds of `podssh relay serve`, and five ideas for the server (to podssh) | closed | T-813, T-829 |
| #218 | coord | COORD-128: v0.67.0: a route to a Unix socket (podssh#295), the renewal against the default relay (podssh#294), the remote port of localhost.run (podnet#607), a durable replace of a file (to all) | closed | T-836, T-834, T-839, T-192, T-837 |
| #219 | coord | COORD-129: v0.68.0: a relay's liveness offer (podssh#300), the headers of Cloudflare Access (podssh#302), a Unix route on Windows (podssh#298), new roots and one rsa (podssh#297), the source fixup reads no comment (podvm#463) (to all) | closed | T-845, T-847, T-848, T-844, T-840, T-841, T-846 |
| #220 | coord | COORD-130: v0.69.0: pod-cloudflare for podnet's take, one clean text of a peer, the hosts of GitHub Actions, two advisories ignored (to all) | closed | T-029, T-850, T-849, T-854, T-855, T-852 |
| #221 | coord | COORD-131: the rulings Shared code and Quality first, and the copies of shared code in each project (to all) | open (the copies of each consumer; podnet answered in PodUtils/podnet#651 (COORD-104): ask 1 is done, ask 2 names T-879, T-880 and T-877, and ask 3 is podnet's survey, PodUtils/podnet@T-473, done: PodUtils/podnet#686 names its copies taken at `08232dd`, `94e3cab`, `08b9c2e` and `c5615d7`, and the take of `pod-release` at `af42f0d`; T-879 open) | T-877, T-878, T-879, T-880, T-881, T-882 |
| #222 | coord | COORD-132: v0.70.0: the outside QA fixes, the shared code of COORD-131, and the asks of each consumer of 2026-11-17 (to all) | closed (each consumer took `v0.70.0`: podssh `99e9fd5`, podnet `7573016`, podvm `6b5a6cb`, podrunner `9c67bb3`; the next hand-off is COORD-137, #228) | T-857, T-858, T-859, T-860, T-861, T-862, T-864, T-865, T-866, T-867, T-869, T-871, T-872, T-877, T-878, T-881, T-883, T-896, T-898, T-901, T-902, T-903, T-904, T-905, T-906, T-908, T-910, T-911, T-912, T-913 |
| #223 | coord | from cfb: the channel of cfb and githome for the state that they read and their asks of podcrates (cfb's C-1 to C-5; githome's G-30 to G-35) | open (the channel of cfb and githome; podcrates answers each in one comment that it edits) | T-104, T-106, T-107, T-880, T-909, T-921, T-922, T-923, T-099, T-100, T-965 |
| #224 | coord | COORD-133: cfb asks whether a beta of podssh before its three gates can hold A-S-01 and A-S-02 (to podssh) | closed (cfb withdrew the question by its operator's ruling R-053, #223: the gates of podssh come first) | T-909, T-030, T-039, T-036 |
| #225 | coord | COORD-134: the owner's rulings "podrunner first" and "Coordinator" (2026-11-18): from the next session, the CI work of githome and cfb comes first, and podcrates coordinates the five sessions; the schema ids of pod-job (to all) | closed (podcrates, podrunner (PodUtils/podrunner#293), podvm (PodUtils/podvm#571), podnet (PodUtils/podnet#654) and podssh (PodUtils/podssh#316) recorded the rulings; T-907 and T-906 are in `v0.71.0`, T-882 in `v0.72.0`, T-104 and T-880 in `v0.77.0`; #227 holds the order of the sessions) | T-907, T-906, T-882, T-104, T-880 |
| #226 | coord | COORD-135: pod-job takes the schemas of podrunner's status and source wire, for githome's G-31 (T-922) (to podrunner) | closed (podrunner answered in PodUtils/podrunner#294; T-922 and T-923 hold the answer) | T-922, T-923 |
| #227 | coord | COORD-136: the coordinator's order of the sessions of 2026-11-18: podrunner first; COORD issues are the one channel (to all) | open (the order of the sessions; it waits for the answer of each session) | T-914, T-915, T-916, T-907, T-882, T-923, T-909 |
| #228 | coord | COORD-137: v0.71.0: pod-time, the fields of pod-job for githome and podrunner, two forms of FileLock, the state of an incus instance, and the asks of podssh and podnet (to all) | closed (each consumer took `v0.71.0`: podrunner `9e6bd57`, podvm `c213375`, podnet `ab3b826`, podssh `a6a3a3e`) | T-905, T-906, T-907, T-908, T-910, T-911, T-912, T-913, T-914, T-915, T-923, T-924, T-925, T-926 |
| #229 | coord | COORD-138: v0.72.0: podrunner's status and source wire in pod-job, one reader of an HTTP/1.1 head, the layer under a small cap, PT_INTERP past the head (to all) | closed (each consumer took `v0.72.0`: podrunner `e422c23`, podvm `b3ff9c3`, podnet `8fa14ca`, podssh `bcd2b2f`) | T-882, T-909, T-922, T-930, T-936, T-939, T-944 |
| #230 | coord | COORD-139: the answer to COORD-095 and COORD-096: the three fields of status.v1 (T-947), and a fake of githome's source in pod-testkit (T-948) (to podrunner) | closed (T-947 and T-948 are in `v0.73.0`; podrunner took it at `0fd2218`, COORD-140, #231) | T-921, T-947, T-948 |
| #231 | coord | COORD-140: v0.73.0: the three fields of status.v1, pod-fake-github and pod-fake-githome for the CI of podrunner, the client's line of each wait of pod-session (to all) | closed (each consumer took `v0.73.0`: podrunner `0fd2218`, podvm `20313a0`, podssh `bcd2b2f`, podnet `c123c18` with `v0.74.0`) | T-921, T-945, T-947, T-948, T-951, T-952, T-953 |
| #236 | coord | COORD-142: the owner's ruling "ghcr.io": each project publishes its release binaries on ghcr.io in one form (call C789) (to all) | open (podcrates (T-954, `v0.75.0`), podvm and podnet publish in the form; podnet attests the index from its next release; podssh pushes at its next tag (its answer is PodUtils/podssh#322, COORD-123: podssh T-847, a job image of its release workflow in the form of call C789, a dry run passed, no ask of podcrates); podrunner's T-553 is open) | T-954 |
| #237 | coord | COORD-141: v0.74.0: a runner job through serve from pod-fake-githome to pod-fake-github, the window of the layer under a small cap, the lost half of a split session (to all) | closed (each consumer took `v0.74.0`: podrunner `93df6d3`, podvm `ef56d16`, podssh `9989ae6`, podnet `c123c18`) | T-949, T-950, T-951, T-952, T-953 |
| #238 | coord | COORD-143: v0.75.0: each member of a compressed layer, the old link held during a move under a small cap, the first images on ghcr.io (to all) | closed (each consumer took `v0.75.0`: podrunner `5fc513d`, podvm `f3beec1`, podssh `ba19367`, podnet `44986bc`) | T-954, T-955, T-956, T-958 |
| #239 | coord | COORD-144: v0.76.0: the extended attributes of a layer, the waits of the channel of pod-e2e, the hold of a move from the threshold (to all) | closed (each consumer took `v0.76.0`: podrunner `393f413`, podvm `16f2395`, podnet `a3386ee`, podssh `5975bb0`) | T-957, T-959, T-962 |
| #240 | coord | COORD-145: v0.77.0: the crate pod-release (a signed release index, its verifier, the self-update of a binary), the PAX headers of a layer by length, a session with no prompt (to all) | closed (each consumer took `v0.77.0`: podrunner `32a9d40`, podnet `cbd52ca`, podssh `b3005e5`, podvm `3272e3d`) | T-104, T-880, T-960, T-961, T-963 |
| #241 | coord | COORD-146: v0.78.1: the TLS end of pod-broker (placeholder credentials), the static command pod-release, Plan::Newer (to all) | closed (each consumer took `v0.78.1`: podrunner `f437793`, podssh `e5b7414`, podnet `2f1ea82`, podvm `3272e3d`) | T-100, T-965, T-971, T-974 |
| #242 | coord | COORD-147: podcrates takes the User-Mode Linux kernel in seccomp mode and its launcher, in pod-microvm (to podvm) | closed (podvm waits for the crate; its answers on PodUtils/podvm#602: x86_64 alone, no patch, the exit code with stdout and stderr) | T-975 |
| #243 | coord | COORD-148: podcrates takes the RUNPATH of pod-enter and the pinned fetch of runsc (to podvm) | closed (podvm waits for both; it pins the SHA-256 that it measures, PodUtils/podvm#604) | T-976, T-977 |
| #244 | request | [gh action] requested: the gates of main at 68fbc96 and the release v0.79.0 | closed (`ci` 44696784279, `adoption` 44696788701, the dry run 44700582224, the release 44701339585 and the run on the tag 44702020444 passed; `v0.79.0` is out) | T-964, T-973, T-979, T-105, T-934, T-935, T-990, T-985, T-986, T-987, T-995, T-976 |
| #245 | coord | COORD-149: v0.79.0: Rekor v2 in pod-release, the clients of a job through pod-broker, defect_tests and acceptance of pod-todo, an error line names each cause once, the signals and the RUNPATH of pod-enter (to all) | closed (each consumer took `v0.79.0`: podvm `e864b31`, podnet `4559204`, podssh `4552b86`, podrunner `80c928a`) | T-964, T-973, T-979, T-105, T-985, T-986, T-987, T-995, T-976 |
| #246 | request | [gh action] requested: the gates of main at 8a2cdd9, the proof of the static QEMU 11.0.0 and a nightly | closed (`ci` 44739311567 red in `lint` alone: `check-private-replace` on `crates/pod-guest/src/warm/line.rs`, fixed in `15c05d0e`; `adoption`, `deny`, `secrets`, `issue-sync` passed; `services.yml` 44739344293 (`hermeticbuild-qemu`) passed, 44739420519 (`temp-sh`) red: T-1034; `guest.yml` 44740879451 red on the save of a `Plan`: T-1035) | T-1013, T-1018, T-1019, T-1020, T-131 |
| #247 | request | [gh action] requested: the gates of main at 0a795b9, guest.yml, and the first runs of PodUtils/static-pkgs (its lint, the dry run of the static QEMU 11.1.3) | closed (podcrates `ci` 44750392194, `adoption`, `plants`, `deny`, `secrets`, `issue-sync` and `guest.yml` 44750433447 passed; static-pkgs `release.yml` 44750449898 dry run passed on amd64 and arm64; static-pkgs `lint` 44750441124 red on one shellcheck warning) | T-1013, T-1014, T-1035 |
| #248 | request | [gh action] requested: the gates of main at 62b8039 and the release v0.80.0 | closed (`ci` 44792959550, `adoption`, `deny`, `secrets`, `issue-sync` passed; the dry run 44798371884, the release 44799760259 and the run on the tag 44800648913 passed; `v0.80.0` is out) | T-103, T-1000, T-102, T-980, T-1020, T-997, T-998, T-999, T-988, T-1033, T-1009, T-996, T-1010, T-1036, T-1012, T-1037, T-1013, T-1035, T-1016, T-1021 |
| #249 | request | [gh action] requested: the first release of PodUtils/static-pkgs, the static QEMU 11.1.3 | closed (`lint.yml` 44794462908 and the dry run 44794467438 passed on amd64 and arm64; the release 44800211410 published `qemu-11.1.3`) | T-1014 |
| #250 | coord | COORD-150: v0.80.0: pod-control, pod-unit, pod-beneath, the bundle of pod-broker for 14 clients, the loader and write table of pod-enter, the signals, the warm line form (to all) | closed (each consumer took `v0.80.0`: podnet `cdd1d55`, podssh `77c63da`, podrunner `6442f43`, podvm `4dc5744`) | T-103, T-1000, T-102, T-980, T-997, T-998, T-999, T-988, T-1033, T-1009, T-996, T-1010, T-1036, T-1012, T-1037, T-1016 |
| #251 | request | [gh action] requested: the gates of main at 0d4af7e and the proof of the static QEMU 11.1.3 (services.yml, guest.yml) | closed (`adoption`, `plants`, `deny`, `secrets`, `issue-sync` passed; `services.yml` 44806797116 (`static-pkgs-qemu`) and `guest.yml` 44809029300 passed: T-1014 done; `ci` 44806766580 red in `test` alone on a false finding of a test of `pod-relay`: T-1059) | T-1014, T-1045, T-1046, T-1059 |
| #252 | request | [gh action] requested: the gates and a nightly of main at a7fb465 | closed (the gates passed; the nightly 44848003241 was red in two jobs: i686, the gate of `arq_machine` of `pod-link`, T-1082, fixed; riscv64, two tests of `pod-iroh`, T-916) | T-1045, T-1046, T-1056, T-1057, T-1023, T-1052, T-1059, T-1082, T-916, T-938 |
| #253 | request | [gh action] requested: the gates of main at 0707cfa and the release v0.81.0 | closed (`ci` 44858297283, `adoption` 44858300591, `deny`, `secrets`, `issue-sync` passed; the dry run 44860939426, the release 44861348592 and the run on the tag 44861709582 passed; `plants` 44858303817 passed; `v0.81.0` is out) | T-1014, T-1045, T-1046, T-1056, T-1057, T-1023, T-1048, T-1027, T-1066, T-1052, T-1059, T-1041, T-1042, T-1030, T-1070 |
| #254 | coord | COORD-151: v0.81.0: the wipe of the control lines, the system units of pod-unit, the QEMU 11.1.3 of static-pkgs, the warm line and the bounded QMP connect, the streamed log reads, one atomic replace of a file (to all) | closed (each consumer took `v0.81.0` or a later tag: podrunner `v0.81.0` at `f0dad16`, `ef8df01` (PodUtils/podrunner#340); podnet `v0.82.0` at `4df28952`, after `9a9a0a9` (PodUtils/podnet#654); podssh `v0.82.0` at `e728db9f`, after `467274b` (PodUtils/podssh#337, PodUtils/podssh#340); podvm `v0.82.0` at `4fa56ec` to `a43e80cd` (PodUtils/podvm#645). `v0.82.0` has no hand-off issue: the hand-off of `v0.82.1` carries the parts of `v0.82.0` that podrunner has not taken) | T-1014, T-1045, T-1046, T-1056, T-1057, T-1023, T-1048, T-1027, T-1066, T-1070 |
| #255 | request | [gh action] requested: the gates and a nightly of main at ab73e75 (the first live run of T-1026) | closed (`ci` 44871374413, `adoption` 44871377124 and `plants` 44871380137 passed; `nightly` 44871383085 failed in the Windows jobs of `pod-unit`, T-1089, and in the riscv64 job of `pod-iroh`: T-916, T-938) | T-1026, T-1060, T-916, T-938, T-1089 |

#79 to #81 and #232 to #235 are pull requests of Dependabot (T-243, T-955).
