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
| #20 | `pod-proxy`: the `Host` header of an IPv6 literal on port 443 | open | T-082 |
| #21 | `pod-proxy`: no SOCKS5 UDP path | open | T-083 |
| #22 | The record of `SOURCES` names a git blob id, not SHA-256 | open | T-084 |
| #23 | `check-sources.py` assumes the branch `main` | open | T-085 |
| #24 | Import `pod-gsr` and its data file | open | T-086 |
| #25 | `pod_gsr::Stream` cuts a record as a clean end | open | T-087 |
| #26 | `pod-gsr` synchronous face | open | T-088 |
| #27 | `tailscale-derp` watch checks only `/Regions` | open | T-089 |
| #28 | `ajam-relay` hosts have no watch | open | T-090 |
| #29 | `Way::lookup` skips the name check | open | T-091 |
| #30 | No `live.yml` | open | T-092 |
| #31 | README state and crate table are stale | open | T-093 |
| #32 | `docs/design.md` repository table is stale | open | T-094 |
| #33 | The static probe lacks `pod-proxy/blocking` | open | T-095 |
| #34 | `pod-proxy` README row lists 4 of 6 variables | open | T-096 |
