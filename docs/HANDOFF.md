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
2. Finish the release `v0.36.0` (below), then fix T-488 (the nightly
   fails to compile on Windows).
3. Take the work order of `TODO/PROGRESS.md`, item 1.

## In flight

- The last release: `v0.35.0` at `311c6bb` (the hand-off #127, open:
  podssh has not taken it). `v0.36.0` is prepared on `main` (`2c0548f`:
  T-476, T-477, T-485, T-412): finish its release (`docs/maintenance.md`,
  "A release") and post `docs/releases/v0.36.0-handoff.md` as COORD-071
  `(to all)` (fill `COMMIT`, the run ids and the time).
- The nightly at `613058c` is red (run 43195242737): macOS and riscv64
  are T-325 (done); the two Windows jobs do not compile
  `crates/pod-relay/tests/reverse_plain.rs` (T-488). The next nightly
  after T-488 proves T-325 and T-307.
- `services.yml` at `613058c` is red; T-323 (done) has its fixes. The next
  scheduled run proves them, and T-351.
- Agents of this session work in worktrees under `.claude/worktrees/` on
  T-413. Merge its branch after `scripts/agent/check.sh` passes on the
  merge.
- `pod-warp`: the trial of podnet (T-412) passes; the trial of podssh is
  T-413. When both pass and a tag holds them, post the COORD issue of the
  freeze `(to podnet) (to podssh)`; its text is
  `docs/releases/pod-warp-freeze.md`.
- The disk of a cloud session is small: `target/debug/incremental` grows
  to some GB. Run the workspace tests with `CARGO_INCREMENTAL=0` when the
  disk is short.
- T-040 (partial): the trials of podvm, with each import for podvm.

## What waits

- podvm (on `v0.35.0`, `7e379a13`, PodUtils/podvm#293): T-330 (its pair
  commands); T-207 (`pod-netlab`, #70) for PodUtils/podvm@T-1662;
  `exec_attached` of `pod-incus` if PodUtils/podvm@T-1722 asks. Write to
  podvm by COORD issues only.
- podnet (on `v0.35.0`, `5562f21`, PodUtils/podnet#519): its link
  workstream (T-300 to T-305, T-420 to T-424, T-310; #88, #106), then
  T-028 (`pod-warp`) and T-029 (`pod-cloudflare`) on the chain to WARP
  (the owner's order). Write to podnet by COORD issues only.
- podssh (on `v0.34.0`, `287e23b`, PodUtils/podssh#219): its take of
  `v0.35.0` (#127); the live runs of its PodUtils/podssh@T-737 (the
  brokers) and PodUtils/podssh@T-571 (`syncthing-relay`); `sandhole` and
  `openbare-xyz` wait for PodUtils/podssh@T-743 and PodUtils/podssh@T-740.
- podrunner (on `v0.35.0`, `0a400b5`, PodUtils/podrunner#73): its session
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
