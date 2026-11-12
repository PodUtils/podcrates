# Issues

Each GitHub issue of the record, with its entries. The State starts with the
state on GitHub: `open`, `closed` or `deleted`. `TODO/github.txt` lists the
numbers. `TODO/RULES.md` gives the rules.

| Issue | Subject | State | Entries |
| --- | --- | --- | --- |
| #1 | crates.io: reserved pod-* names and trusted publishing | open (reopened for the names of podvm) | T-042, T-043, T-053 |
| #2 | COORD-001: a test of the fallback channel between the sessions | closed (`d2bab56`) | T-051 |
| #3 | COORD-002: the answer to podvm's COORD-001 (to podvm) | closed (answered; M6 goes on in T-056 to T-061) | T-040, T-054, T-063 |
| #4 | COORD-003: podvm can take pod-term now; the synchronous face is on main (to podvm) | closed (podvm `75ed306`) | T-040, T-063, T-066 |
| #5 | [gh action] requested: ci.yml and release.yml (v0.4.0) on main | closed (release 42207811181) | T-015, T-049, T-063 |
| #6 | COORD-004: the Alpine netboot pins of podvm (to podvm) | closed (podvm `5209adb`: the 3.22.6 archive) | T-066 |
| #7 | COORD-005: podcrates v0.4.0 is released: the takes of podssh, podnet and podvm (to all) | closed (each took it) | T-040, T-015, T-049 |
| #8 | [gh action] requested: the release v0.5.0 (ci.yml, nightly.yml, release.yml) | closed (release 42220040866) | T-067, T-068 |
| #9 | COORD-006: take pod-proxy (feature blocking) (to podvm) | closed (podvm `b1138c2`) | T-040, T-069, T-070, T-071 |
| #10 | COORD-007: podcrates v0.5.0 is released: each tag to v0.5.0 (to all) | closed (each took it) | T-040, T-069 |
| #11 | COORD-008: pod-hostprobe is imported from podvm-probe; the freeze; a statfs defect (to podvm) | closed (T-072 fixed it) | T-055, T-072 |
| #12 | COORD-009: pod-dns names the DoH server that answered (to podnet) | closed (podnet `872a276`) | T-073 |
| #13 | COORD-010: the registry patch of podvm on pod-proxy (to podvm) | closed (podvm `06f2427`) | T-040, T-074, T-075, T-076, T-077 |
| #14 | [gh action] requested: the release v0.6.0 (ci.yml, nightly.yml, release.yml) | closed (replaced by #15) | T-055, T-072, T-073, T-074, T-075 |
| #15 | [gh action] requested: the release v0.6.0 from 4aa2ee0 | closed (release 42261756998) | T-055, T-072 to T-076, T-078 to T-080 |
| #16 | COORD-011: podcrates v0.6.0 is released: each tag to v0.6.0 (to all) | closed (each took it) | T-055, T-073, T-075, T-076, T-078, T-079, T-080 |
| #17 | COORD-012: take pod-hostprobe (to podvm) | closed (podvm `8899c2e`) | T-055, T-040, T-081 |
| #18 | [gh action] requested: the release v0.7.0 (ci.yml, nightly.yml, release.yml) | closed (replaced by #19) | T-077, T-081 |
| #19 | [gh action] requested: nightly.yml and release.yml (v0.7.0) from a13014d | closed (the session dispatched release 42623501324) | T-077, T-081 |
| #20 | `pod-proxy`: the `Host` header of an IPv6 literal on port 443 | closed (T-082 `394d1e6`) | T-082 |
| #21 | `pod-proxy`: no SOCKS5 UDP path | closed (T-083) | T-083 |
| #22 | The record of `SOURCES` names a git blob id, not SHA-256 | closed (T-084) | T-084 |
| #23 | `check-sources.py` assumes the branch `main` | closed (T-085 `86dfd6f`) | T-085 |
| #24 | Import `pod-gsr` and its data file | closed (T-086 `79116e3`) | T-086 |
| #25 | `pod_gsr::Stream` cuts a record as a clean end | closed (T-087 `509e59d`) | T-087 |
| #26 | `pod-gsr` synchronous face | closed (T-088 `cd8d049`) | T-088 |
| #27 | `tailscale-derp` watch checks only `/Regions` | closed (T-089 `fa09d1e`) | T-089 |
| #28 | `ajam-relay` hosts have no watch | closed (T-090 `09cfb80`) | T-090 |
| #29 | `Way::lookup` skips the name check | closed (T-091 `e81f5f2`) | T-091 |
| #30 | No `live.yml` | closed (T-092) | T-092 |
| #31 | README state and crate table are stale | closed (T-093) | T-093 |
| #32 | `docs/design.md` repository table is stale | closed (T-094) | T-094 |
| #33 | The static probe lacks `pod-proxy/blocking` | closed (T-095) | T-095 |
| #34 | `pod-proxy` README row lists 4 of 6 variables | closed (T-096) | T-096 |
| #35 | cfb: meta request for the tools that cfb needs from PodUtils (84 asks) | open (routed in `docs/research/fleet/asks.md`) | T-098, T-099 to T-120 |
| #36 | githome: meta request for a forge CI runner host (12 asks) | open (routed in `docs/research/fleet/asks.md`) | T-098, T-099, T-100, T-103, T-104, T-106, T-109 |
| #37 | COORD-013: podcrates v0.7.0 is released: each tag to v0.7.0 (to all) | closed (podssh `1fd68cc4`, podnet `86b99de`, podvm `35df7f7`) | T-077, T-081 |
| #38 | COORD-014: the asks of cfb (#35) for podssh, and the rulings of 2026-11-09 (to podssh) | closed (podssh `739d851d`, PodUtils/podssh#174) | T-098 |
| #39 | COORD-015: the asks of cfb (#35) and githome (#36) for podvm, and the rulings of 2026-11-09 (to podvm) | closed (podvm `35df7f7`, `67feade`; PodUtils/podvm#218) | T-098, T-099, T-108 |
| #40 | COORD-016: the asks of cfb (#35) for podnet, and the rulings of 2026-11-09 (to podnet) | closed (podnet `86b99de`, `421eeb9`; PodUtils/podnet#414) | T-098, T-109, T-121 to T-127 |
| #41 | COORD-017: podcrates v0.8.0 is released: each tag to v0.8.0; podvm takes pod-supervise (to all) | closed (podnet `0df677d`, podssh `0e3c9eaf`, podvm `989f35b`) | T-060, T-086, T-101, T-125, T-126, T-127, T-128 |
| #42 | COORD-018: pod-proxy holds the SOCKS5 datagram path of podnet-netns upstream.rs: those lines are frozen (to podnet) | closed (podnet recorded the freeze, PodUtils/podnet#416) | T-083 |
| #43 | COORD-019: podcrates v0.9.0 is released: each tag to v0.9.0; podvm takes pod-guest and pod-enter (to all) | closed (podvm `423e817`, podnet `191f2b4`, podssh `d4b03aa7`) | T-059, T-061, T-083, T-132, T-134, T-135, T-136 |
| #44 | COORD-020: the answers to podssh#175 (no public Wisp server qualifies) and #177 (the proxy lists and nodes) (to podssh) | closed (T-130 and T-133 done, `c4c9197`; no step remains for podssh) | T-130, T-133 |
| #45 | COORD-021: podcrates v0.10.0 is released: each tag to v0.10.0; podvm takes pod-oci (to all) | closed (podnet `3a9317b`, podvm `20ea781`, podssh `49cb5c8c`) | T-056, T-057, T-133, T-139, T-141 |
| #46 | COORD-022: podcrates v0.11.0 is released: podvm takes pod-extract and pod-rootfs; the others may move the tag (to all) | closed (podnet `740eb0b`, podvm `0682c0c`, podssh `dc112ea7`) | T-057, T-058, T-139, T-141 |
| #47 | Tracker: run GitHub CI (and other CI) on any machine: podrunner, pod-dockerapi and the podvm path | open (the tracker of T-146; PodUtils/podrunner#1 holds the plan) | T-146 |
| #48 | COORD-023: podcrates v0.12.0 is released: podvm moves to the table of path maps (#224); the others may move the tag (to all) | closed (podvm on `v0.22.0`, `c9d5038`, PodUtils/podvm#262) | T-137, T-144, T-145, T-148 |
| #49 | COORD-024: podcrates v0.13.0 is released: podssh and podnet take pod-ws (to all) | closed (podnet `5eace6e`, podssh `a083593e`) | T-016 |
| #50 | COORD-025: podcrates v0.14.0 is released: podssh and podnet take pod-quic; podssh reads serveo, ssh-j and croc (to all) | closed (podssh `525f8907`, podnet `b7b0aef`) | T-019, T-153, T-156 |
| #51 | COORD-026: the asks of podvm#227 are record entries that wait for DR-1 and DR-2 (to podvm) | closed | T-160, T-161, T-162, T-163, T-164, T-165 |
| #52 | COORD-027: pod-enter: the volume of a missing -w and the reserved guest paths in Maps::new (T-155, PodUtils/podvm#226) (to podvm) | closed | T-155 |
| #53 | COORD-028: the owner pauses each session after its last runs; update the hand-off documents first (to all) | closed | T-020, T-022, T-166 |
| #54 | COORD-029: podcrates v0.15.0 is released: podssh takes pod-wormhole and pod-session; podnet takes pod-hold; podvm moves its pod-enter calls (to all) | closed (podssh `6f78a7c7`, podnet `b759937`, podvm `d9fe511`) | T-020, T-022, T-155, T-166 |
| #55 | COORD-030: the owner named podrunner (DR-1 and DR-2 are yes); podnet comes first: take pod-hold, start PodUtils/podnet@T-229 to PodUtils/podnet@T-231, two questions on the cut (to podnet) | closed (podnet `b759937`, `e4d4049`; T-170 step 2 waits, #74) | T-166, T-169, T-170 |
| #56 | COORD-031: the owner's DR-5: podrunner waits for PodUtils/podvm@T-1656, and podvm must support incus or have an adapter for it; the answer to COORD-023 (to podvm) | closed | T-173 |
| #57 | COORD-032: podcrates v0.16.0 is released: podrunner takes pod-supervise and pod-broker; podvm takes the tree kill and the private modes; podnet and podssh may move the tag (to all) | closed | T-157, T-160, T-162, T-175 |
| #58 | COORD-033: podcrates v0.17.0 is released: podrunner takes pod-job and the mutual TLS of pod-tls; podvm takes the limits of pod-enter (to all) | closed | T-163, T-164, T-165, T-176 |
| #59 | COORD-034: the take of pod_hold::connect and pod_hold::packet: adoption/podnet/pod-hold-connect.patch (to podnet) | closed | T-169, T-170 |
| #60 | COORD-035: podcrates v0.18.0 is released: podrunner takes the names mode of pod-broker and the schema rule; podssh takes three tunnels, the key of ssh-j and Rewound::new (to all) | closed | T-161, T-171, T-172, T-173, T-177, T-178, T-181 |
| #61 | COORD-036: the cut of pod-netstack (the userspace TCP and UDP stack on smoltcp 0.14): which files of podnet, and when (to podnet) | closed | T-023, T-170 |
| #62 | COORD-037: podcrates v0.19.0 is released: podvm takes the library directories of the image (podvm#247); pod-broker refuses a raw byte in a path (to all) | closed | T-142, T-149, T-152, T-182, T-183, T-184 |
| #63 | COORD-038: podcrates takes the incus client as pod-incus (T-179), for the tag v0.21.0 (to podvm) | closed | T-179 |
| #64 | COORD-039: podcrates v0.20.0 is released: podnet takes pod-netstack; podssh takes the key of localhost-run, sandhole and openbare-xyz (to all) | closed | T-023, T-185, T-186, T-187, T-188 |
| #65 | COORD-040: podcrates v0.21.0 is released: podvm takes pod-incus; pod-core and pod-identity hold one store and one pin at a time; the socket of pod-broker has its mode at once (to all) | closed | T-179, T-195, T-196 |
| #66 | COORD-041: Reno and CUBIC on smoltcp 0.14 are slow only on a link with one ACK for each round trip (T-188): podnet keeps CongestionControl::None; warp --cc (to podnet) | closed (no step remains; T-197 stays open) | T-188, T-197 |
| #67 | COORD-042: podcrates cuts pod-wg from src/warp/wg.rs at 54d7ed4 (T-024): hold wg.rs to fixes until your take (to podnet) | closed | T-024 |
| #68 | COORD-043: the answer to PodUtils/podnet#446: register.rs and route.rs need no port; keep the veth wire in veth.rs (T-201) (to podnet) | closed (no step remains; T-201 waits, #74) | T-201 |
| #69 | COORD-044: the answer to PodUtils/podvm#259: podcrates writes pod-confine and Plan.confine of pod-enter (T-108) (to podvm) | closed | T-108 |
| #70 | COORD-045: the answer to PodUtils/podvm#260: podcrates reads the modloop (T-206) and writes pod-netlab (T-207), after pod-confine (to podvm) | open (T-206 in `v0.24.0`; T-207 waits) | T-206, T-207 |
| #71 | COORD-046: v0.22.0: pod-e2e for podssh, pod-wg for podnet, the death signal of a payload for podvm and podrunner (to all) | closed | T-021, T-024, T-202, T-205 |
| #72 | COORD-047: v0.23.0: pod-confine and Plan.confine; the empty root entry of a layer (to podvm, to all) | closed | T-108, T-208 |
| #73 | COORD-048: v0.24.0: the module set of a guest from its modloop; gcr.io, mirror.gcr.io and the NIC module sums in pod-services (to podvm, to all) | closed (podssh `2f4f292b`, podrunner `6a01f4e`, podvm `96acfbea`, podnet with `v0.26.0` at `f107766`) | T-206, T-211 |
| #74 | COORD-049: the answer to PodUtils/podnet#449, #450 and #451: the freeze is in the record; WARP as a network in T-028; T-214 and T-215 for the file transfer and the mailbox server (to podnet) | open | T-026, T-028, T-029, T-170, T-201, T-214, T-215 |
| #75 | COORD-050: the answer to PodUtils/podnet#452 and #453: the hint words and the traits are in T-214 and T-028; pod-git is T-217 to T-220 (to podnet) | closed | T-028, T-214, T-217, T-218, T-219, T-220 |
| #76 | COORD-051: v0.25.0: the pod-netstack calls of podnet's router; pod-confine closes the scopes, UDP on request, MPTCP, io_uring and the root capabilities (to all) | closed (podrunner `02a1c0b`, podvm `763b7c6a`, podssh `f029594d`; podnet answered with PodUtils/podnet#457) | T-025, T-209, T-221, T-222 |
| #77 | COORD-052: v0.26.0: the veth wire of the netns hold, the facts of the SSH forward relays, the time to the first step and the job images of pod-job (to all) | closed (podrunner `4284967`, podnet `f107766`, podssh `ff2f7bac`, podvm `483a1957`) | T-201, T-224, T-226, T-227, T-233 |
| #78 | Docs: stale names and contradictions after the renames of 2026-11-12 (pods, podstat, podforge) | closed | T-242 |
| #82 | cfb asks: tracker for the 36 asks of cfb owned by podcrates (from podcrates#35) | open (the owner's tracker; each ask maps to its entry) | T-099, T-100, T-101, T-102, T-103, T-104, T-105, T-106, T-107, T-113, T-114, T-115, T-116, T-117, T-118, T-119 |
| #83 | githome asks: tracker for the 2 asks owned by podcrates (from podcrates#36) | open (the owner's tracker; each ask maps to its entry) | T-099, T-100, T-106, T-160 |
| #84 | CI plan items: tracker for the 8 items owned by podcrates (from podcrates#47) | open (the owner's tracker; phases 3 and 5 stay in T-146) | T-100, T-102, T-103, T-104, T-106, T-146, T-162, T-251 |
| #85 | COORD-053: v0.27.0: the close of a listener and no RST for a burst in pod-netstack; podnet's relay client and the podssh trial of pod-relay; the socket families of pod-confine (to all) | closed (podvm `a714e99f`, PodUtils/podvm#276; podnet `28c0e3d`, PodUtils/podnet#465; podssh `0872c2c6`, PodUtils/podssh#206; podrunner `3ef4db9`) | T-025, T-026, T-213, T-223, T-227, T-230, T-231, T-232, T-233, T-240, T-262 |
| #86 | COORD-054: v0.28.0: qemu-img convert in pod-guest; a TCP list refused with socketcall(2); the UDP of pod-netstack; Tox, Yggdrasil, HyperDHT, workerd and wasmtime in pod-services (to all) | closed (podrunner `bde47c3`; podssh `12473a10`, PodUtils/podssh#208; podnet `b08e06f`, PodUtils/podnet#477; podvm `94e7ca2a`, PodUtils/podvm#278) | T-174, T-228, T-234, T-244, T-252, T-253, T-254, T-255, T-256, T-257, T-258, T-259, T-260, T-262, T-266 |
| #87 | COORD-055: v0.29.0: the hold of a SYN in pod-hold; an MQTT meeting point in pod-wormhole; a channel handler, the Host of an IPv6 relay and one cache file for each host in pod-relay (to all) | closed (podnet `98d8a71`, PodUtils/podnet#478; podvm `c99bdd8d`, PodUtils/podvm#279; podssh `5b56c4d`, PodUtils/podssh#211; podrunner `7cdd0d5`) | T-150, T-170, T-225, T-229, T-245, T-246, T-261, T-271, T-272 |
| #88 | COORD-056: the answer to PodUtils/podnet#479: the asks of the link workstream are T-278, T-279 and T-300 to T-305; pod-link is a crate of podcrates (to podnet) | open (podnet starts each entry) | T-278, T-279, T-300, T-301, T-302, T-303, T-304, T-305 |
| #89 | COORD-057: v0.30.0: podnet's relay client over pod-relay (the trial); the MQTT brokers in the runs of a code; the Syncthing relays; pod-actions-expr and pod-actions-workflow (to all) | closed (podrunner `528a918`; podvm `c553da2e`, PodUtils/podvm#280; podnet `83e0c93`, PodUtils/podnet#481; podssh `ef3666f`, PodUtils/podssh#212) | T-026, T-251, T-267, T-276, T-277 |
| #90 | COORD-058: v0.31.0: podvm's arm64 defects in pod-extract and pod-enter; a Run that owns its workflow; the display form of a fingerprint (to all) | open (podvm `c5b61b45`, PodUtils/podvm#284; podnet `e046560`, PodUtils/podnet#502; podssh and podrunner answer with the commit of their take) | T-278, T-279, T-309, T-320, T-321 |
| #91 | COORD-059: the answer to PodUtils/podvm#283: podcrates writes the trial of pod-relay's blocking facade in podvm (T-326); the wire is reverse-v1 (to podvm) | open (T-326 in work) | T-326 |
| #92 | COORD-060: the answer to PodUtils/podnet#502: the crate's node and operator pass through the live relay from a runner with a direct egress; podcrates needs your log of relay connect -v (to podnet) | open (waits for the log of podnet) | T-327 |

#79 to #81 are pull requests.
