# Handoff

The state of unfinished work, for a session with no memory of the last
one. Read this page first, then `AGENTS.md`, `TODO/PROGRESS.md` (the
state, the work order, the questions), `docs/maintenance.md` (the
procedures, the tools and the rules of the work), `docs/design.md` and
`docs/decisions.md`. Keep this page current: remove a line when its work
is done. Git and the record keep finished work.

## The first steps of the next session

1. Read the hand-off issue #89 (`v0.30.0`) and record each new take;
   podnet's take of `pod-relay` ends T-026 (the `taken` line, the trial
   files go).
2. Start the watcher (`sh scripts/agent/watch-org.sh 90` under the Monitor
   tool) and read the open COORD issues `(to podcrates)`.
3. Take the work order of `TODO/PROGRESS.md`: T-300 (`pod-link`, podnet
   started its link workstream, PodUtils/podnet#480), T-278, T-279, T-212,
   T-290.

## In flight

- The last release: `v0.30.0` at `613058c` (the hand-off #89): podrunner
  took it at `528a918`. Each consumer took `v0.29.0` (#87, closed).
- T-026 (partial; calls C73 to C76, C80 to C82, C101 to C103, C110 to
  C112): the trial `adoption/podnet/pod-relay.*` on podnet `98d8a71`
  passed in `adoption.yml` 43191076065; podnet's take remains.
- The nightly at `613058c` runs: it proves T-307 (the gate of `pod-guest`,
  the TCP-list tests on the targets with `socketcall(2)`, the bound under
  riscv64). `upstream.yml` is green at `a3ee89d` (T-306, T-308).
- T-267: the next run of `services.yml` proves the URLs of the four live
  relays.
- T-040 (partial): the trials of podvm, with each import for podvm.

## What waits

- podvm (on `v0.29.0`, `c99bdd8d`, PodUtils/podvm#279): T-207 (`pod-netlab`, #70) for PodUtils/podvm@T-1662;
  `exec_attached` of `pod-incus` if PodUtils/podvm@T-1722 asks. Write to
  podvm by COORD issues only.
- podnet (on `v0.29.0`, `98d8a71`, PodUtils/podnet#478, with
  `syn_hold`): T-026's trial, then T-028 (`pod-warp`) and
  T-029 (`pod-cloudflare`) on the chain to WARP (the owner's order). Write
  to podnet by COORD issues only.
- podssh (on `v0.29.0`, `5b56c4d`, PodUtils/podssh#211): its
  PodUtils/podssh@T-737 wires the MQTT meeting point (T-261, and T-277
  with the next tag); PodUtils/podssh#209 is T-276 (with the next tag); `sandhole` and `openbare-xyz` wait for
  PodUtils/podssh@T-743 and PodUtils/podssh@T-740.
- podrunner (on `v0.30.0`, `528a918`): its local mode
  (PodUtils/podrunner@T-046) takes T-251; a defect comes as an ask.
- podnet's link workstream (PodUtils/podnet#479, P3): T-278, T-279,
  T-300 to T-305 wait for podnet's start of each (the answer #88).
- `pod-git` (T-217 to T-220) is blocked: its own repository, deferred (the
  owner; #75). M8 (cfb and githome, #35 and #36) waits (the ruling
  "Priority").

## Channels

- podrunner and podssh: lead sessions that podcrates started; direct
  messages (`list_sessions`, titles `(podrunner) lead session` and
  `(podssh) lead session`). A lead session that goes idle wakes on a
  message. A
  session writes only to its own repository: link a consumer's COORD
  issue from the hand-off issue of podcrates.
- podnet and podvm: their own sessions; COORD issues only
  (`docs/session-protocol.md`).
- Workflows: dispatch them with the GitHub MCP tool
  (`docs/maintenance.md`, "A run of a workflow"). `services.yml` with the
  kind `forward-url` reads the banners of the SSH forward relays (no
  session has SSH).
