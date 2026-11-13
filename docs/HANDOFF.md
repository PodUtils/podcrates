# Handoff

The state of unfinished work, for a session with no memory of the last
one. Read this page first, then `AGENTS.md`, `TODO/PROGRESS.md` (the
state, the work order, the questions), `docs/maintenance.md` (the
procedures, the tools and the rules of the work), `docs/design.md` and
`docs/decisions.md`. Keep this page current: remove a line when its work
is done. Git and the record keep finished work.

## The first steps of the next session

1. Read the nightly at `613058c`: T-325 has its failures.
2. Start the watcher (`sh scripts/agent/watch-org.sh 90` under the Monitor
   tool) and read the open COORD issues `(to podcrates)`.
3. Take the work order of `TODO/PROGRESS.md`: T-212, T-290.

## In flight

- T-327: `v0.32.0` carries the flush of the operator to podnet
  (PodUtils/podnet#502, #92); podnet's `live.yml` proves it.
- The last release: `v0.35.0` at `311c6bb` (the hand-off #127, open).
  `main` holds T-476 for the next tag.
- podswarm (#107 to #125): its asks to podcrates are T-430 to T-450; they
  wait for the owner's Q15 (`TODO/PROGRESS.md`), with its 16 rulings.
- Agents of this session work in worktrees under `.claude/worktrees/` on
  T-477 and T-413. Merge each branch after `scripts/agent/check.sh` passes
  on the merge.
- `pod-warp`: the trial of podnet (T-412, at `9c1fc4b` with `v0.35.0`) passes each of its 23 steps; the trial of podssh is
  T-413. When both pass and a tag holds them, post the COORD issue of the
  freeze `(to podnet) (to podssh)`; its text is
  `docs/releases/pod-warp-freeze.md`.
- The disk of a cloud session is small: `target/debug/incremental` grows
  to some GB. Run the workspace tests with `CARGO_INCREMENTAL=0` when the
  disk is short.
- T-026 is done: podnet took `pod-relay` at `83e0c93` (PodUtils/podnet#481).
- The nightly at `613058c` runs: it proves T-307 (the gate of `pod-guest`,
  the TCP-list tests on the targets with `socketcall(2)`, the bound under
  riscv64). `upstream.yml` is green at `a3ee89d` (T-306, T-308).
- T-267, T-323: the next run of `services.yml` proves the URLs of the four
  live relays and the fixes of T-323 (Serveo's console, shiftr's text
  watches, localtunnel, the bound of `rdap`).
- T-040 (partial): the trials of podvm, with each import for podvm.

## What waits

- podvm (on `v0.34.0`, `fc1c5d60`, PodUtils/podvm#288, with `pod-relay`): T-330 (its pair commands); T-207 (`pod-netlab`, #70) for PodUtils/podvm@T-1662;
  `exec_attached` of `pod-incus` if PodUtils/podvm@T-1722 asks. Write to
  podvm by COORD issues only.
- podnet (on `v0.34.0`, `ef8c901`, PodUtils/podnet#515, with
  `pod-relay`): its link workstream (T-300, PodUtils/podnet#480), then T-028
  (`pod-warp`) and T-029 (`pod-cloudflare`) on the chain to WARP (the
  owner's order). Write
  to podnet by COORD issues only.
- podssh (on `v0.34.0`, `287e23b`, PodUtils/podssh#219): the live runs of
  its PodUtils/podssh@T-737 (the brokers) and PodUtils/podssh@T-571 (`syncthing-relay`); `sandhole` and `openbare-xyz` wait for
  PodUtils/podssh@T-743 and PodUtils/podssh@T-740.
- podrunner (on `v0.34.0`, `2a4f35c`, PodUtils/podrunner#72): its session
  runs again with podvm. Its PodUtils/podrunner@T-096 (an absolute `tar
  -C`) takes podvm `v0.2.0-beta.7` (the fix of #102, PodUtils/podvm#289).
  Write to it by an issue with `(to podrunner)` in the title.
- podnet's link workstream (PodUtils/podnet#479, P3): T-278, T-279,
  T-300 to T-305 wait for podnet's start of each (the answer #88).
- `pod-git` (T-217 to T-220) is blocked: its own repository, deferred (the
  owner; #75). M8 (cfb and githome, #35 and #36) waits (the ruling
  "Priority").

## Channels

- Each session, podssh and podrunner too: COORD issues alone, no direct
  message (the owner, 2026-11-12, the ruling "Tonight"; podssh states the
  same on PodUtils/podssh#217). A session writes only to its own
  repository: link a consumer's COORD issue from the hand-off issue of
  podcrates.
- podnet: its own session; COORD issues only, no direct message (the
  ruling "Tonight"). Do not start a podnet session.
- podvm: its own sessions; COORD issues only (`docs/session-protocol.md`).
  Not available on the evening of 2026-11-12: podrunner's session ends
  after its housekeeping, and a new one starts with podvm.
- Workflows: dispatch them with the GitHub MCP tool
  (`docs/maintenance.md`, "A run of a workflow"). `services.yml` with the
  kind `forward-url` reads the banners of the SSH forward relays (no
  session has SSH).
