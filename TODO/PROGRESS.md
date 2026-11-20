# Progress

`TODO/INDEX.md` holds 812 entries: 146 open, 28 partial, 7 blocked, 631 done.

## State

- `docs/HANDOFF.md` has what is in flight and what waits.
- Last release: `v0.78.1` at `edbc86b` (`docs/releases/v0.78.1.md`; the tag
  `v0.78.0` has no release, call C818). Its hand-off COORD-146 (#241) and
  COORD-145 (#240, `v0.77.0`) are closed: each consumer took `v0.78.1`.
- The next tag carries T-964 (`pod-release` verifies the bundles of Rekor
  v2) and the fixes of `main` after `v0.78.1`. T-966 and T-963 are partial.
  T-975 to T-977 (podvm's asks) are open: their code is not on `main`.
  `release.yml` has steps that run on a tag only, which the dry run does not
  test (T-974; `docs/maintenance.md`, "A release").
- The tag that the `main` of each consumer names: `v0.78.1` (podrunner,
  podssh, podnet, podvm). The details of each take are on #240 and #241.
- Takes that wait: podvm's check of T-208 against `lambda-al2023`, and
  its module set of T-206 with PodUtils/podvm@T-1662 (after `pod-netlab`,
  T-207, #70).
- Open defects: T-916 (partial), T-933,
  T-934, T-935, T-938, T-789, T-766 (the nightly); T-963 (`live.yml`: the
  second Nostr relay is `nos.lol`; a giver still waits its whole limit
  after a refusal of the relay);
  T-131 (`pod-gsr` on Windows ARM64); T-204 (a random failure of
  `pod-session`).
- The session of podcrates coordinates the five sessions: podcrates,
  podrunner (the flagship), podssh, podnet and podvm (the ruling
  "Coordinator" and "COORD only": COORD issues alone; COORD-136, #227,
  has the order of the sessions).
- The CI work of githome and cfb (the work of podrunner) comes first in
  each project (the ruling "podrunner first", COORD-134, #225). The
  channel of cfb and githome is #223: podcrates answers each in one
  comment that it edits, with the UTC time of each update. cfb gets its CI
  from githome alone (cfb's R-051 and R-053), and it is offline until the
  CI of podrunner and githome is complete. podrunner's answer to
  COORD-135 (PodUtils/podrunner#294) is in T-922 and T-923.
- The rest of M8 waits (the ruling "Priority"). The asks of podswarm wait
  until the podswarm session starts, but for T-439, T-444, T-447 and
  T-448 (the ruling "podswarm", call C245).
- Each name of `docs/design.md`, section 4, is reserved on crates.io
  (`check-crates-io-names.py`: 102 names, 0 faults). No crate goes to
  crates.io now (the ruling "Releases").
- Blocked: T-217 to T-220 (`pod-git`, the rulings "pod-git" and
  "Renames": `PodUtils/podforge`, deferred). T-538 (the attribution line
  of a comment on GitHub): Q16. T-617 and T-832: their Blocker sections.
- Keep this section short. Write the current state only; git keeps the
  history.

## Work order

Finish each entry, close it in place, change this page in the same commit,
then take the next one. Inside a milestone, take the entries in this
order. A crate is released on the day that its entry is done
(`docs/decisions.md`, Delivery and Who edits). An entry that waits for
another session stays as it is; take the next entry.

The order comes from the owner's rulings "Coordinator", "podrunner first"
(the CI work of githome and cfb, the work of podrunner, comes first),
"Priority" (podvm, podnet and podssh next; the rest of M8 waits),
"podrunner online" and "Sessions online", "The move of podssh" and
"Discipline".

First, as the coordinator: read the answers of the sessions of podrunner,
podssh, podnet and podvm to COORD-136 (#227), and route each ask between
them by COORD issues, podrunner's first. Then, in podcrates: the red nightly
(T-916, T-933, T-935, T-934, T-938) and the red live run (T-963). Then the asks of
podrunner (COORD-134, #225): each new ask of podrunner.
Then the
entries that a need of githome or cfb names for their CI: T-106 (partial;
cfb's A-X-18 waits for cfb's answer on #223, call C792),
T-966 (podssh took `pod-release`; podnet's half waits for
PodUtils/podnet@T-494), T-973
(the CA bundle of the TLS end of `pod-broker` with real clients; G-04, #223 G-35), T-105 (A-X-14), T-103 (G-08), T-102. Then cfb's asks
that podssh takes (call C782, PodUtils/podssh#316): T-112, T-110, T-111,
T-117. Then the open asks
of the consumers: T-975 (a User-Mode Linux kernel for podvm, PodUtils/podvm#602,
for the parity of PodUtils/podrunner#316; podvm answered in COORD-147, #242), T-976 (`pod-enter` and a `RUNPATH`,
PodUtils/podvm#603), T-977 (a pinned fetch of `runsc`, PodUtils/podvm#604), T-927 (PodUtils/podvm#584), T-941 (PodUtils/podssh#319), T-879 (`pod-detach`), T-897, T-899, T-900, T-852, T-863,
T-851. Then T-920.
Then the ruling "Discipline": T-692
(`pod-lab`, part 2), T-700 (`pod-manual`: the take of each consumer; its
findings T-704 and T-705), T-767 (a stalled resume of `pod-session`,
PodUtils/podssh#283; the eleventh failure names podssh's own SSH
client, which podssh takes) with T-768 (a late spare), T-703
(the issues against the record) and T-702 (the installer). Then the move
of podssh (PodUtils/podssh#248): T-030, T-039; T-036. Then the link
carriers of M2 for podnet: T-526. Then the items below, in order.

1. **The two studies** (`docs/research/dig-study.md` and
   `docs/research/ingest-study.md`): T-812 (a measurement of the
   throughput of the hosted relays), then T-814 (a rank by measured rate);
   the rest of T-818 to T-830 by priority; T-838; T-832 waits for
   podswarm.
2. **The defects and asks of the consumers:** T-386 (the live run of
   `pod-devtunnel`, GitHub #94); T-330 (podvm's pair commands; it waits
   for podvm).
3. **podrunner** (PodUtils/podrunner#1): T-692 part 2, then T-696, T-693,
   T-694, T-695. T-146 tracks the plan. Each new ask of the podrunner
   session becomes an entry here, in the order of its need. T-280 (the
   live run of `pod-actions-expr` and `pod-actions-workflow` against
   GitHub) waits.
4. **podnet** (its complete beta, PodUtils/podnet#521): T-419 (the facts
   of the file hosts from the live run of `live.yml`); T-802 (a NAT probe
   of `pod-turn`, #212, before T-499), T-496 (DNS stream), T-498 (a
   WireGuard peer), T-499 (NAT traversal with the relay), T-515 (the
   congestion window of the ARQ, with the first carrier over a shared
   path), T-803 (a parity datagram of the ARQ, #213, after T-515), T-514
   (a stream-aware ARQ, when podnet asks), T-501 (`pod-turn` with RFC
   6062), T-214 and T-215 (`pod-transit`, `pod-wormhole`). Then: T-030
   (`pod-cloudflare`), T-302 (`pod-egress`), T-039 (`pod-ts`), T-502 (a
   headscale harness). The rest of `pod-warp`: T-339 (the trial patches
   and the live run), T-482, T-481, T-470, T-410, T-409; T-407 (the fuzz
   targets of the server face of `pod-proxy`). T-121 and T-124 (the asks
   of podnet).
5. **M6, the crates of podvm:** T-207 (`pod-netlab`: then T-345, the
   probes, and T-346, the world), PodUtils/podvm#260 (PodUtils/podvm@T-1662
   waits for it and for the module set of the modloop, which `pod-guest`
   has); T-347 after both, then T-348; `exec_attached` of `pod-incus` when
   PodUtils/podvm@T-1722 asks for it. T-649 (a FAT image writer,
   PodUtils/podvm#386) is not urgent. T-448 (a wrong fact of FUSE in
   `pod-enter`); T-444 (the probe rows of the namespace and FUSE rungs;
   the rows of podswarm alone wait). T-040 (the trials of podvm) goes
   with each import. T-190 (a large blob as parallel ranges,
   PodUtils/podnet#437) after podvm asks for it.
6. **M1, the transport:** T-200 (a Bare node whose WebSocket passes, for
   PodUtils/podssh@T-740); T-151 (the watch scripts that leave podssh and
   podnet); T-041 when a crate or a consumer needs it; then T-189, T-193
   (the proposals of PodUtils/podnet#437 for `pod-proxy` and `pod-core`).
7. **M2, the relay:** the contract of `pod-link` from the study of zmux
   (`docs/research/zmux.md`): T-798, T-799; T-212; T-290 (the layer of
   `pod-relay` as a feature); then T-204 (a random failure of
   `pod-session`); then T-124 (the failover of the reverse road across the
   relays of several operators, the ruling "Open questions"), T-121 (the
   asks of podnet); then T-197 (`pod-netstack`).
8. **M3, the adapters:** T-028, T-030.
9. **M4, the roads:** T-131 (a defect of `pod-gsr` on Windows ARM64)
   first; then T-036 (with T-856, the vectors of `tokens.json`), T-039.
10. **M5, the upkeep:** T-932 (the temp files, PodUtils/podvm#567), T-917
   (the copies of product code that `codegraph.py dups` finds), T-928 (the
   other readers of an HTTP/1.1 head), T-918 (the helpers that the tests
   copy);
   T-789, T-766, T-780 (a test that reads the pipes
   of its child after the child ends), T-781 (the findings of the study of
   zmux for the consumers), T-779; T-312 (a code fence that is not closed
   in the record). A "port" entry for each change that
   `scripts/check-sources.py` finds; T-199 after a release of snow on
   `chacha20poly1305` 0.11. The live calls of CI (`docs/live-calls.md`,
   calls C150 to C153), in the order of their gain: T-351 (a host that
   gives no answer to one runner reddens `services.yml`), T-371 (the same
   class for the DNS watches, `stun-binding` and the HTTP client), T-356
   (sish and sandhole), T-357 (gs-netcat). T-359 and T-360 go with T-028
   (item 4); T-361 goes with T-036 (item 9).
11. **M7, the widened scope of podvm:** T-062 after M6, with T-447 (the
   direct boot of a Linux guest for `pod-microvm`); T-241 (the
   permissions of Landlock ABI 12); T-349 (a lab of `pod-netlab` on a
   user-space stack and with more than one guest).
12. **The rest of M8 and of the asks of #35 and #36: they wait** (the
   ruling "Priority"; the entries for the CI of githome and cfb are at the
   top, by the ruling "podrunner first"). T-490 (the server half of ECH, #130) waits too. The
   asks of podswarm that help podswarm alone (T-430 to T-437, T-440 to
   T-443, T-445, T-446, T-449, T-450) wait until the podswarm session
   starts (the ruling "podswarm"). T-102 to T-119 wait, unless a
   consumer needs one first. T-113 to T-116 wait for the first release on
   crates.io. T-191 and T-194 (the proposals of PodUtils/podnet#437 for
   `pod-broker`, for the egress of podrunner) go to item 3 when podrunner
   asks for one. T-180 (`pod-mask`) waits with M8. T-217 to T-220
   (`pod-git`) are blocked (the State).

## Calls made without a ruling

`docs/decisions.md`, "Calls made without a ruling", has each call. The
work goes on with each call until the owner rules otherwise. A new call
takes the number after the last row of that table.

## Owner actions

- Send `docs/upstream/kcp-0.6.0-wrapping.patch` to the crate `kcp`
  upstream as a pull request (T-526).
- Report to `Lazarus404/xsockets` that `recverr: true` is an option that
  no Erlang/OTP release takes (C588); `scripts/ci/xturn-recverr.patch`
  removes it for the CI of T-631 until a release of xsockets drops it.
- For the live client of `pod-devtunnel` with no account (T-489, row L45
  of `docs/live-calls.md`): set the repository secrets
  `POD_DEVTUNNEL_TUNNEL` and `POD_DEVTUNNEL_PORT` (and
  `POD_DEVTUNNEL_ACCESS_TOKEN` for a tunnel with no anonymous access),
  host the tunnel (`devtunnel host -p 22 --allow-anonymous`), and tell
  the session: it dispatches `live.yml` with `measure=devtunnel`.
- Delete the branches that #208 names, and the two `dependabot/docker`
  branches that `docs/HANDOFF.md` names.

The hand-off of each release is a COORD issue `(to all)`
(`docs/maintenance.md`, "A release").

## Questions for the owner

- **Q16.** Does the word of the owner of 2026-11-11 in the `AGENTS.md` of
  PodUtils/podrunner ("a comment on GitHub can end with the 'Generated by'
  line of the harness; a commit cannot") apply to podcrates too? The
  ruling "Git" of 2026-11-05 here forbids each attribution line in a
  comment, and each comment of a session of podcrates has the line. It
  blocks T-538. Recommendation: yes, the same rule in each repository of
  PodUtils; write it in `docs/decisions.md` and in `AGENTS.md`.
- **Q18.** Can a release of podcrates carry a static Linux kernel of
  `ARCH=um` (GPL-2.0, T-975) and gVisor's `runsc` (Apache-2.0, T-977) as
  assets, with their sources named? It blocks the release part of T-975
  and T-977. Recommendation: yes, with the source of each GPL file as an
  asset of the same release.
- **Q19.** Can the emulated jobs of `nightly.yml` (riscv64, loongarch64,
  powerpc64le) skip the network tests of `pod-iroh` when T-978 shows that
  an optimised build does not pass them? It touches T-916 and T-938.
  Recommendation: measure first (T-978); skip only with a native runner
  that runs the tests.
- **Q20.** Can `pod-release` take a newer set of Sigstore roots from TUF
  at run time, beside the roots that it holds? It touches T-964 and H-07
  of `docs/research/hard-problems.md`. Recommendation: no network in the
  verifier; a test of `upstream.yml` tells of a new root, and a release
  takes it.
Write each question as `- **Qn.** QUESTION`, with the entries that it
blocks and a recommendation. Number the questions in sequence from Q21
(Q1 to Q14: `docs/decisions.md`; Q15: the ruling "podswarm"; Q16: above;
Q17: the packages of ghcr.io are public, no action).
