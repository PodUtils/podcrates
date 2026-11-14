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
   takes of `v0.43.0` on its hand-off issue.
3. Merge the agents' work for PodUtils/podrunner#97 and #98 and for
   PodUtils/podvm#339 when it is done, and cut `v0.44.0` with it. Then the work order of
   `TODO/PROGRESS.md`.

## In flight

- The last release: `v0.43.0` at `ccbea49` (ci 43562056951, adoption
  43562056931, release 43564313833; the hand-off #149, COORD-089).
  Takes: podrunner `51f9e6b`, podvm `b0bb87f1`, podnet `c8ccc40`; podssh
  waits. `v0.42.0` at `b6983fc`: each consumer took it (#145, closed).
- On `main` for `v0.44.0`: T-573 (#148), T-572, T-576, T-577
  (PodUtils/podrunner#97, #98), T-574 and T-558 (`pod-hold`: the raw part
  and the ping part; podnet's `Calls::sockets().allow_raw()` now refuses
  ping sockets: its form is `.allow_ping()`, PodUtils/podnet#550). The
  owner reserves the names of `pod-actions-store` and
  `pod-actions-results` on crates.io ("Owner actions" of
  `TODO/PROGRESS.md`).
- Agents in worktrees of `.claude/worktrees/` (their branches merge into
  `main` when they are done): `qa580` (T-580 to T-582, #150), `qa584`
  (T-584, T-585, T-583, T-586, #151), `sess579` (T-579, then T-560),
  `actions587` (T-587, T-588, PodUtils/podrunner#99), `guest590` (T-592,
  T-590, T-593: the Windows guest, #152), `udf591` (T-589, then the new
  crate `pod-udf`, T-591).
- The open threads of this session:
  - T-539 is in `v0.41.0`; T-540 (open) tests it against a kernel pty.
  - T-557 (open): a finding of T-542.
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

- podvm (on `v0.42.0`, `296a1fc1`, PodUtils/podvm#325; `v0.2.0-beta.14`
  out): T-330 (its pair commands); T-207 (`pod-netlab`, #70) for
  PodUtils/podvm@T-1662; `exec_attached` of `pod-incus` if
  PodUtils/podvm@T-1722 asks. Write to podvm by COORD issues only.
- podnet (on `v0.42.0`, `654c0f1`, PodUtils/podnet#550; T-574 for its
  filter of raw sockets): the rest of its link workstream (#133: T-494, T-500, then group
  d), then T-029 (`pod-cloudflare`) on the chain to WARP (the owner's
  order). Write to podnet by COORD issues only.
- podssh (on `v0.42.0`, `7e457b4`, PodUtils/podssh#241; T-560 is open,
  as its role serves the one form over HTTP/2): the live runs of its PodUtils/podssh@T-737 (the brokers) and
  PodUtils/podssh@T-571 (`syncthing-relay`); `sandhole` and
  `openbare-xyz` wait for PodUtils/podssh@T-743 and PodUtils/podssh@T-740.
- podrunner (on `v0.42.0`, `05ceaec`, PodUtils/podrunner#96): its session
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
