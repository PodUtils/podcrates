# Handoff

The state of unfinished work, for a session with no memory of the last
one. Read this page first, then `AGENTS.md`, `TODO/PROGRESS.md` (the
state, the work order, the questions), `docs/maintenance.md` (the
procedures, the tools and the rules of the work), `docs/design.md` and
`docs/decisions.md`. Keep this page current: remove a line when its work
is done. Git and the record keep finished work.

## The first steps of the next session

1. Start the watcher (`sh scripts/agent/watch-org.sh 90` under the Monitor
   tool) and read the open COORD issues `(to podcrates)` of each
   repository of PodUtils.
2. Merge or continue the work of the two agents of this session (see "In
   flight"), then take the work order of `TODO/PROGRESS.md`. Its head
   now: T-026, T-251, T-267, T-212. The next release (`v0.30.0`) carries
   T-276, T-277, T-251 and the trial of T-026.

## In flight

- The last release: `v0.29.0` at `a4d3630` (the hand-off #87): podnet
  took it at `98d8a71` (PodUtils/podnet#478, with `syn_hold`), podvm at
  `c99bdd8d` (PodUtils/podvm#279), podssh at `5b56c4d`
  (PodUtils/podssh#211), podrunner at `7cdd0d5`; #87 is closed.
- On `main` after the tag: T-276 (`syncthing-relay`, call C104), T-277
  (the MQTT brokers in the runs of a code, call C115), T-306 (the podvm
  block of `pod-services`, which made `upstream.yml` red), and the gate of
  `crates/pod-oci/tests/live.rs` (the nightly at `66d7e96` did not build
  it on macOS and Windows; `nightly.yml` was dispatched at `0edd703`).
- Two agents of this session work in worktrees under `.claude/worktrees/`
  on local branches: T-251 (`pod-actions-expr`, `pod-actions-workflow`;
  the ids 280 to 289 and the calls C105 to C109 are its own) and the trial
  of T-026 on podnet `98d8a71` (the ids 290 to 299 and the calls C110 to
  C114). A new
  session finds their branches with `git worktree list`; it merges a
  branch whose checks pass, or takes the work up from its commits.
- T-267 (`forward-url` proves the URL): the source and the tests are in
  work in the tree of the lead (not committed at the time of this line).
- T-026 (partial; calls C73 to C76, C80 to C82, C101 to C103): podnet's
  relay client is in `pod-relay`. The trial of podnet
  (adoption/podnet/pod-relay.patch, `.commit`, `.run`) on podnet
  `98d8a71` is in work (an agent, above): no fix of podnet's three frozen
  files needs a port; the Done section of T-026 lists what the patch changes (the
  manifests, the glue of `ws_relay.rs` and `relay_reverse.rs`, the removal
  of `relay_pool.rs`, the callers, podnet's tests and checks) and the
  findings for it (a test token under 8 characters, no public constructor
  of `pair::Pair`, the mode of a pair directory, podnet's layer and size
  allowlists, podnet's citations of `src/relay_pool.rs`). After the trial,
  the words "a binary does not turn it on" of `plain-ws` change (C101).
  podnet holds the three files to fixes from `7c5f87e`.
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
- podrunner (on `v0.29.0`, `7cdd0d5`): T-251
  (`pod-actions-workflow`, `pod-actions-expr` and the expansion of jobs,
  for its local mode PodUtils/podrunner@T-046) is its one crate ask.
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
