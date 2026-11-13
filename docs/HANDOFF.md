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
2. Read the answer of podnet on #138 (T-520), then release the carriers
   of `pod-link` on `main` (the poll carriers, `quic`, `udp`).
3. Take the work order of `TODO/PROGRESS.md`, item 1.

## In flight

- The last release: `v0.39.0` at `4c2d17c` (the hand-off #137, closed:
  each consumer took it). `main` holds T-516, T-422, T-495, T-423 and
  T-424 for the next tag. #138 asks podnet for the default of KCP (T-520).
- The nightly at `d8c8ae6` (run 43417105533) passes on Windows (both
  jobs), macOS and each cross target (T-488, T-507, done).
- podswarm's tracker is PodUtils/podswarm#1 (moved from #107). githome
  builds its side of a live test (#83, 2026-11-13): it waits for T-106
  (the conformance binary of `pod-testkit`) and T-100 (`pod-broker`),
  which stay in M8 under the ruling "Open questions".
- `services.yml` at `613058c` is red; T-323 (done) has its fixes. The next
  scheduled run proves them, and T-351.
- `pod-warp`: the trial of podnet (T-412, at `9c1fc4b` with `v0.35.0`) passes each of its 23 steps; the trial of podssh
  (T-413, at `286e98d3` with `v0.36.0`) passes each of its 24 steps, with
  T-480 in the crate after `v0.36.0`. When a tag holds T-480, move the
  tags of `adoption/podssh/pod-warp.patch` to it and post the COORD issue
  of the freeze `(to podnet) (to podssh)`; its text is
  `docs/releases/pod-warp-freeze.md`.
- The disk of a cloud session is small: `target/debug/incremental` grows
  to some GB. Run the workspace tests with `CARGO_INCREMENTAL=0` when the
  disk is short.
- T-040 (partial): the trials of podvm, with each import for podvm.

## What waits

- podvm (on `v0.39.0`, `19892c50`, PodUtils/podvm#309; `v0.2.0-beta.8` out): T-330 (its pair
  commands); T-207 (`pod-netlab`, #70) for PodUtils/podvm@T-1662;
  `exec_attached` of `pod-incus` if PodUtils/podvm@T-1722 asks. Write to
  podvm by COORD issues only.
- podnet (on `v0.39.0`, `306d80b`, PodUtils/podnet#526; `pod-warp` taken): its link
  workstream (T-300 to T-305, T-420 to T-424, T-310; #88, #106), then
  T-028 (`pod-warp`) and T-029 (`pod-cloudflare`) on the chain to WARP
  (the owner's order). Write to podnet by COORD issues only.
- podssh (on `v0.39.0`, `5228b65`, PodUtils/podssh#229; `pod-warp` taken): the live runs of its PodUtils/podssh@T-737 (the
  brokers) and PodUtils/podssh@T-571 (`syncthing-relay`); `sandhole` and
  `openbare-xyz` wait for PodUtils/podssh@T-743 and PodUtils/podssh@T-740.
- podrunner (on `v0.39.0`, `69d7b14`, PodUtils/podrunner#84): its session
  runs again with podvm. Its PodUtils/podrunner@T-096 (an absolute `tar
  -C`) takes podvm `v0.2.0-beta.7` (the fix of #102, PodUtils/podvm#289).
  Write to it by an issue with `(to podrunner)` in the title.
- podswarm (#107 to #125): the owner's ruling "podswarm" (2026-11-13).
  T-439, T-444, T-447 and T-448 help a sibling and are in the work order;
  the other asks wait until the podswarm session starts.
- `pod-git` (T-217 to T-220) is blocked: `PodUtils/podforge`, deferred
  (the owner; #75). M8 (cfb and githome, #35 and #36) waits (the ruling
  "Priority").
- crates.io answers 404 for `pod-devtunnel` (2026-11-13): the owner
  reserves it before a publish (T-387; `TODO/PROGRESS.md`, "Owner
  actions").

## Channels

- Each session talks with the others by COORD issues alone, no direct
  message (the owner, 2026-11-12, the ruling "Tonight"). A session writes
  only to its own repository: link a consumer's COORD issue from the
  hand-off issue of podcrates. Do not start a session of a consumer.
- Workflows: dispatch them with the GitHub MCP tool
  (`docs/maintenance.md`, "A run of a workflow"). `services.yml` with the
  kind `forward-url` reads the banners of the SSH forward relays (no
  session has SSH).
