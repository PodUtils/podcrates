# Progress

`TODO/INDEX.md` holds 987 entries: 153 open, 28 partial, 7 blocked, 799 done.

## State

- `docs/HANDOFF.md` has what is in flight and what waits.
- Last GitHub release with `release.yml` assets and `ghcr.io` images:
  `v0.83.0` at `5fd6fb7` (`docs/releases/v0.83.0.md`, call C818). It has
  the GitHub release, the signed `SHA256SUMS`, the assets and the images
  of `ghcr.io/podutils`; the run on the tag and the dry run of
  `publish.yml` passed. Each consumer names it (podssh, podvm, podnet and
  podrunner), so its hand-off COORD-154 (#259) is closed (call C780), with
  a Result line. The hand-off COORD-152 (#257) of `v0.82.1` is closed.
- On `main` after `v0.83.0`, for the next tag: T-1243 and T-1355 (ports
  with no change for a consumer).
- T-963 is partial. T-975 and T-977 (podvm's asks) are open: their code is
  not on `main`. `release.yml` has steps that run on a tag only, which the
  dry run does not test (T-974; `docs/maintenance.md`, "A release").
- podssh's ask PodUtils/podssh#341 (COORD-134 of podssh): the fork
  `vendor/iroh-relay` at 1.4.2 for iroh 1.4.2 (T-1102, call C905) is in
  `v0.82.1`; T-1103 (iroh 1.5) follows.
- podssh's ask PodUtils/podssh#346 (COORD-137 of podssh): the sans-IO
  transfer codec is in `pod-term` (T-1144, `v0.83.0`); the answer is #258
  (COORD-153), open until podssh names the commit that wires the codec
  (its T-831).
- The tag that the `main` of each consumer names: `v0.83.0` (podssh,
  PodUtils/podssh#354; podvm, PodUtils/podvm#663; podnet,
  PodUtils/podnet#710; podrunner, PodUtils/podrunner#351). The details of
  the earlier takes are on #250, #254 and #259.
- PodUtils/static-pkgs builds and publishes the static programs that no
  one else publishes in the form that PodUtils needs (the owner,
  2026-11-20); its first release is `qemu-11.1.3`.
- Takes that wait: podvm's check of T-208 against `lambda-al2023`, and
  podvm's lab guest (PodUtils/podvm@T-1662 to PodUtils/podvm@T-1664), which
  takes T-345 to T-347 of `pod-netlab` (T-207, partial) when they are done.
  podvm closed PodUtils/podvm#260 and took the reader of the modloop (T-206)
  and the offline part of T-207.
- Red runs: the nightly 45504345202 at `6f5f4951` (a dispatch): `gate`, the
  installer jobs, `test (windows-2025)` and the jobs `cross` of aarch64,
  arm, armv7, powerpc64le and i686 pass. T-1296, T-1297, T-1298 and T-1303
  are done (the emulated jobs pass `pod-enter`, `dns_hold` and `mail`; the
  macOS jobs pass `pod-incus` and end in 21 to 25 minutes; each job `test`
  prints the seconds of each binary, the longest is 70 s, and the limits
  are 300 s for a binary and 600 s for the doc tests). Open failures:
  `riscv64` and `loongarch64` fail in `pod-iroh` (`road.rs`, `share.rs`:
  T-916; Q19); the riscv64 job took 127 of its 150 minutes and
  `session_resume_twice` ended in 91 s (T-938, partial); both macOS jobs
  fail in three tests of `files::socket` of `pod-core` (T-1359, done on
  `main`); the job of Windows ARM64 fails in the UDP bind of
  `quick_tunnel_standin` (T-1360, done on `main`).
  T-789 and T-766 passed again (partial in the record). `ci.yml` run
  45487233017 (`25747d3b`, the fix of T-1295): each job passes but `static
  (aarch64-apple-darwin, macos-15-intel)`, which met its limit of 40
  minutes in "Build the probe" (T-1325); the runs after it were cancelled by
  newer pushes. None open in `services.yml`:
  T-1223 is done; read the next scheduled run.
- Open defects: T-963 (`live.yml`: the second Nostr relay is `nos.lol`;
  a giver still waits its whole limit after a refusal of the relay);
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
  until the podswarm session starts, but for T-444, T-447 and T-448 (the
  ruling "podswarm", call C245).
- 108 of the 110 names of `docs/design.md`, section 4, are reserved on
  crates.io; `pod-natprobe` and `pod-tpm` are missing ("Owner actions"). No crate goes to
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
them by COORD issues, podrunner's first. Then, in
podcrates: the red nightly (T-916, T-938; read the macOS and Windows ARM64 jobs of the next `nightly.yml` run), the macOS job of `ci` (T-1325),
T-1053 (a lossy run of E2-1 that takes 20 s), the next scheduled
`services.yml` run (read it) and the red live run (T-963). Then the asks of podrunner
(COORD-134, #225): each new ask of podrunner. Then the entries that a need
of githome or cfb names for their CI: T-106 (partial; cfb's A-X-18 waits
for cfb's answer on #223, call C792). Then cfb's asks that podssh takes
(call C782, PodUtils/podssh#316): T-112, T-110, T-111, T-117. Then the open
asks of the consumers, podvm's first, then podnet's, then podssh's:

- podvm: T-975 (a User-Mode Linux kernel for podvm, PodUtils/podvm#602,
  for the parity of PodUtils/podrunner#316; podvm answered in COORD-147,
  #242); T-977 (a pinned fetch of `runsc`, PodUtils/podvm#604); T-1032
  (the launcher of `pod-supervise` blocks before the start of the
  payload); T-927 (PodUtils/podvm#584); T-1321 (a C backend of
  `pod-gzip`, measured).
- podnet: T-1343 (a temp directory and a temp file with a random name,
  PodUtils/podnet#711, COORD-119); T-1135 (`pod-cloudflare` says a cause two
  times); T-879 (`pod-detach`); T-897.
- podssh: T-1169 (`pod-link`, the DNS carrier; PodUtils/podssh#349,
  COORD-139: a test through `unbound`);
  T-1103 (iroh 1.5 and its fork
  of iroh-relay, after PodUtils/podssh#341); T-1095 (the order of the
  pseudo-fields of the HTTP/2 request of `pod-ws`; PodUtils/podssh#340,
  closed by podssh, which keeps its patch of h2 until a tag carries the order);
  T-1238 (`EX_DATAERR`; PodUtils/podssh#352, COORD-142);
  T-941 (PodUtils/podssh#319); T-1339 and T-1340 (the fuzz targets of
  `pod-tpm`, and a key file of the crate in tpm2-openssl and in
  go-tpm-keyfiles against swtpm; PodUtils/podssh#351).

Then T-899, T-900, T-852, T-851, and T-920.
Then the ruling "Discipline": T-692
(`pod-lab`, part 2), T-700 (`pod-manual`: the take of each consumer; its
findings T-704 and T-705), T-768 (a late spare of `pod-session`), T-703
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
   of the file hosts from the live run of `live.yml`); T-496 (DNS stream), T-498 (a
   WireGuard peer), T-499 (NAT traversal with the relay), T-515 (the
   congestion window of the ARQ, with the first carrier over a shared
   path), T-803 (a parity datagram of the ARQ, #213, after T-515), T-514
   (a stream-aware ARQ, when podnet asks), T-214 and T-215 (`pod-transit`,
   `pod-wormhole`). Then: T-030
   (`pod-cloudflare`), T-302 (`pod-egress`), T-039 (`pod-ts`), T-502 (a
   headscale harness). The rest of `pod-warp`: T-339 (the trial patches
   and the live run), T-482, T-481, T-470, T-410, T-409; T-407 (the fuzz
   targets of the server face of `pod-proxy`). T-121 and T-124 (the asks
   of podnet).
5. **M6, the crates of podvm:** T-207 (`pod-netlab`, partial: the offline
   part is taken by podvm; PodUtils/podvm#260 is closed): T-345 (the
   probes) and T-346 (the world), which podvm's lab guest
   (PodUtils/podvm@T-1662 to PodUtils/podvm@T-1664) takes; T-347 after both,
   then T-348; `exec_attached` of `pod-incus` when
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
10. **M5, the upkeep:** T-1343 (the shared temp names of `pod-core`), then
   T-932 (the temp files, PodUtils/podvm#567), T-917
   (the copies of product code that `codegraph.py dups` finds), T-1115
   (the two copies of the bind of a Unix socket at a long path), T-928 (the
   other readers of an HTTP/1.1 head), T-918 (the helpers that the tests
   copy), T-1044 (the doc tests that the checks for Windows and macOS do
   not compile), T-1146 (the comments of `pod-natprobe` in ASD-STE100);
   T-789, T-766, T-780 (a test that reads the pipes
   of its child after the child ends), T-781 (the findings of the study of
   zmux for the consumers), T-779; T-312 (a code fence that is not closed
   in the record). A "port" entry for each change that
   `scripts/check-sources.py` finds; T-199 after a release of snow on
   `chacha20poly1305` 0.11. The live calls of CI (`docs/live-calls.md`,
   calls C150 to C153), in the order of their gain: T-1150 (a watch of the facts of
   RFC 5780 of a STUN server), T-351 (a host that
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
   starts (the ruling "podswarm"). T-110 to T-119 wait, unless a
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
- Reserve the name `pod-natprobe` on crates.io (T-802), as for each name
  of `docs/design.md`, section 4 (the other 108 are reserved;
  `scripts/check-crates-io-names.py` reports `pod-natprobe` alone).
- Reserve the name `pod-tpm` on crates.io (T-1217), as for each name of
  `docs/design.md`, section 4 (call C927).
- Reserve the name `pod-gzip` on crates.io (T-1196 added the crate and its
  row of `docs/design.md`, section 4).
- Delete the remote branch `wip/t1073` (merged into `main`): the proxy of
  the sessions refuses `git push origin --delete`.

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
- **Q21.** Must a close of an issue by a team member on GitHub win over
  the record, with no limit of time? Today the sync opens it again after
  24 hours (T-1159, call C924), because the ruling "Issues" (2026-11-15)
  says that the record gives the state. It touches T-1159 and the open
  issues #35, #36, #47, #82, #83, #84 and #109 to #123, whose entries
  wait on start conditions or on the podswarm session (call C245).
  Recommendation: yes for a close by the owner. The sync then does not
  open the issue again. It fails with the line "closed by hand: close it
  in the record or answer", and a session changes the record at once; an
  entry that the owner calls stale is closed by a ruling of the owner,
  which removes it (`TODO/RULES.md`, section 2).
Write each question as `- **Qn.** QUESTION`, with the entries that it
blocks and a recommendation. Number the questions in sequence from Q22
(Q1 to Q14: `docs/decisions.md`; Q15: the ruling "podswarm"; Q16: above;
Q17: the packages of ghcr.io are public, no action).
