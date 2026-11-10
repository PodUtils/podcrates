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
| #44 | COORD-020: the answers to podssh#175 (no public Wisp server qualifies) and #177 (the proxy lists and nodes) (to podssh) | open | T-130, T-133 |
| #45 | COORD-021: podcrates v0.10.0 is released: each tag to v0.10.0; podvm takes pod-oci (to all) | closed (podnet `3a9317b`, podvm `20ea781`, podssh `49cb5c8c`) | T-056, T-057, T-133, T-139, T-141 |
| #46 | COORD-022: podcrates v0.11.0 is released: podvm takes pod-extract and pod-rootfs; the others may move the tag (to all) | open | T-057, T-058, T-139, T-141 |
| #47 | Tracker: run GitHub CI (and other CI) on any machine: podrunner, pod-dockerapi and the podvm path | open | T-146 |
