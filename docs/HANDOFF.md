# Handoff

The state of unfinished work, for a session with no memory of the last
one. Read this page first, then `AGENTS.md`, `TODO/PROGRESS.md` (the
state, the work order, the questions), `docs/maintenance.md` (the
procedures, the tools and the rules of the work), `docs/design.md` and
`docs/decisions.md`. Keep this page current: remove a line when its work
is done. Git and the record keep finished work.

## The first steps of the next session

1. Start the watcher (`sh scripts/agent/watch-org.sh 90` under the Monitor
   tool) and read the open COORD issues `(to podcrates)`.
2. Read the answer of podnet on #139 (the profile of `kcp`, T-526) and the
   takes of `v0.41.0` on its hand-off issue #143.
3. Merge the local branches w-worm545 (`pod-wormhole` for podnet, #141)
   and w-enter486 (T-486 and T-107 for podvm, #142) when their agents
   finish. Then cut `v0.42.0` and answer #141, #142,
   PodUtils/podssh#234, #236, #238 and PodUtils/podrunner#91 with its
   tag. Then the work order of `TODO/PROGRESS.md`.

## In flight

- The last release: `v0.40.0` at `1335e9a` (ci 43467923338, adoption
  43467923371, release 43474412563; the hand-off #140, COORD-080): the
  poll, quic, udp, dns and icmp carriers of `pod-link`, `cpu_s` and
  `peak_rss_mib` of `pod-job`, the forms of the file hosts. Each consumer
  took it (#140, closed). `v0.41.0` at `bb39935` (ci 43538302195,
  adoption 43538302204, release 43541097888; the hand-off #143,
  COORD-083): the termios mode of `pod-term` (T-539), four tox relays
  (T-537, T-555).
- On `main` for `v0.42.0`: T-541 to T-544, T-549, T-552, T-553, T-556.
  In work on local branches: the asks of `pod-wormhole` for podnet
  (w-worm545, #141); T-486 and T-107 (w-enter486, for podvm, #142).
- Each agent of this session is merged into `main`. The open threads of
  this session:
  - T-539 is in `v0.41.0`; T-540 (open) tests it against a kernel pty.
  - T-557 and T-558 (open): findings of T-542 and T-544.
  - T-526 (partial): the fix of `kcp` upstream. podnet sets the profile of
    `kcp` with its take of the udp carrier (#139, PodUtils/podnet#527).
- The red runs of this session have fixes on `main`: T-532 and T-535
  (the bounds of the tests under an emulator; the nightly at `1335e9a`,
  run 43467939399, passes each job, riscv64 and armv7 too), T-534 and T-536 (the port of the registry
  of `pod-oci`), T-537 (a tox node that is down), T-497 (the build of the
  icmp carrier on macOS; `scripts/agent/check-macos.sh`, T-533, checks it
  here). T-351 still waits for the proof of a first "no answer".
  - `plants.yml` at `6b183f6` (run 43448941361): the plant
    `link-udp-fragment-taken` was missed; its `EXPECT` takes the stall
    now (T-521, Correction). The next run of `plants.yml` proves it.
- podswarm's tracker is PodUtils/podswarm#1 (moved from #107). githome
  builds its side of a live test (#83): it waits for T-106 (the
  conformance binary of `pod-testkit`) and T-100 (`pod-broker`), which
  stay in M8 under the ruling "Open questions".
- `pod-warp`: podnet and podssh took the crate in `v0.37.0`; their WARP
  code holds fixes only (#135). T-339 (partial) holds the rest.
- The disk of a cloud session is small: `target/debug/incremental` grows
  to some GB. Run the workspace tests with `CARGO_INCREMENTAL=0` when the
  disk is short.
- T-040 (partial): the trials of podvm, with each import for podvm.

## What waits

- podvm (on `v0.41.0`, `104bbe4b`, PodUtils/podvm#324; `v0.2.0-beta.14`
  out): T-330 (its pair commands); T-207 (`pod-netlab`, #70) for
  PodUtils/podvm@T-1662; `exec_attached` of `pod-incus` if
  PodUtils/podvm@T-1722 asks. Write to podvm by COORD issues only.
- podnet (on `v0.41.0`, `095955c`, PodUtils/podnet#546; `pod-warp`
  taken): the rest of its link workstream (#133: T-494, T-500, then group
  d), then T-029 (`pod-cloudflare`) on the chain to WARP (the owner's
  order). Write to podnet by COORD issues only.
- podssh (on `v0.41.0`, `6f77eaa`, PodUtils/podssh#240; `pod-warp`
  taken): the live runs of its PodUtils/podssh@T-737 (the brokers) and
  PodUtils/podssh@T-571 (`syncthing-relay`); `sandhole` and
  `openbare-xyz` wait for PodUtils/podssh@T-743 and PodUtils/podssh@T-740.
- podrunner (on `v0.41.0`, `9f15901`, PodUtils/podrunner#93): its session
  runs with podvm. Its T-108 fills `cpu_s` and `peak_rss_mib` with
  `v0.40.0` (PodUtils/podrunner#86). Write to it by an issue with
  `(to podrunner)` in the title.
- podswarm (#107 to #125): the owner's ruling "podswarm" (2026-11-13).
  T-439, T-444, T-447 and T-448 help a sibling and are in the work order;
  the other asks wait until the podswarm session starts.
- `pod-git` (T-217 to T-220) is blocked: `PodUtils/podforge`, deferred
  (the owner; #75, #131). M8 (cfb and githome, #35 and #36) waits (the
  ruling "Priority").

## Channels

- Each session talks with the others by COORD issues alone, no direct
  message (the owner, 2026-11-12, the ruling "Tonight"). A session writes
  only to its own repository: link a consumer's COORD issue from the
  hand-off issue of podcrates. Do not start a session of a consumer.
- Workflows: dispatch them with the GitHub MCP tool
  (`docs/maintenance.md`, "A run of a workflow"). `services.yml` with the
  kind `forward-url` reads the banners of the SSH forward relays (no
  session has SSH).
