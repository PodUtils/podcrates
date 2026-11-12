# Handoff

The state of unfinished work, for a session with no memory of the last
one. Read this page first, then `AGENTS.md`, `TODO/PROGRESS.md` (the
state, the work order, the questions), `docs/maintenance.md` (the
procedures, the tools and the rules of the work), `docs/design.md` and
`docs/decisions.md`. Keep this page current: remove a line when its work
is done. Git and the record keep finished work.

## The first steps of the next session

1. Finish the release of `v0.32.0` if it is not tagged (`docs/maintenance.md`,
   "A release"): `ci` and `adoption` at the head, `release.yml` with no
   input, then with `version=0.32.0`; the dry run of `publish.yml`; post
   `docs/releases/v0.32.0-handoff.md` as COORD-061 `(to all)` (fill
   `COMMIT` and the run ids), map it, message podssh and podrunner.
   Read #90 (`v0.31.0`: podssh has not answered) and record each take.
2. Read the nightly at `613058c`: T-325 has its failures.
3. Start the watcher (`sh scripts/agent/watch-org.sh 90` under the Monitor
   tool) and read the open COORD issues `(to podcrates)`.
4. Take the work order of `TODO/PROGRESS.md`: T-212, T-290.

## In flight

- The last release: `v0.31.0` at `45b145a` (the hand-off #90). Each
  consumer took `v0.30.0` (#89, closed).
- T-327 is fixed on `main` (the operator flushes the local side): the next
  tag carries it to podnet (PodUtils/podnet#502, #92).
- T-326: an agent of this session writes the trial of `pod-relay` in
  podvm in a worktree under `.claude/worktrees/`; merge its branch when
  its checks pass, and answer on #91.
- T-028 (`pod-warp`): an agent of this session imports it in a worktree
  under `.claude/worktrees/` (the free ids 335 to 344, calls C135 to
  C144); merge its branch when its checks pass. The agent of T-326 has
  the free ids 330 to 334 and calls C130 to C134.
- T-207 (`pod-netlab`, the offline part: knobs, layout, scenarios, plan,
  derivation, judge): an agent of this session in a worktree under
  `.claude/worktrees/` (the free ids 345 to 349, calls C145 to C149);
  merge its branch when its checks pass.
- T-026 is done: podnet took `pod-relay` at `83e0c93` (PodUtils/podnet#481).
- The nightly at `613058c` runs: it proves T-307 (the gate of `pod-guest`,
  the TCP-list tests on the targets with `socketcall(2)`, the bound under
  riscv64). `upstream.yml` is green at `a3ee89d` (T-306, T-308).
- T-267, T-323: the next run of `services.yml` proves the URLs of the four
  live relays and the fixes of T-323 (Serveo's console, shiftr's text
  watches, localtunnel, the bound of `rdap`).
- T-040 (partial): the trials of podvm, with each import for podvm.

## What waits

- podvm (on `v0.31.0`, `c5b61b45`, PodUtils/podvm#284): T-326 (the trial of `pod-relay`, an agent of this session, PodUtils/podvm#283, #91); T-207 (`pod-netlab`, #70) for PodUtils/podvm@T-1662;
  `exec_attached` of `pod-incus` if PodUtils/podvm@T-1722 asks. Write to
  podvm by COORD issues only.
- podnet (on `v0.30.0`, `83e0c93`, PodUtils/podnet#481, with
  `pod-relay`): its link workstream (T-300, PodUtils/podnet#480), then T-028
  (`pod-warp`) and T-029 (`pod-cloudflare`) on the chain to WARP (the
  owner's order). Write
  to podnet by COORD issues only.
- podssh (on `v0.30.0`, `ef3666f`, PodUtils/podssh#212): the live runs of
  its PodUtils/podssh@T-737 (the brokers) and PodUtils/podssh@T-571 (`syncthing-relay`); `sandhole` and `openbare-xyz` wait for
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
