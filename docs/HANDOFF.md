# Handoff

The state of unfinished work, for a session with no memory of the last
one. Read this page first, then `AGENTS.md`, `TODO/PROGRESS.md` (the
state, the work order, the questions), `docs/maintenance.md` (the
procedures, the tools and the rules of the work), `docs/design.md` and
`docs/decisions.md`. Keep this page current: remove a line when its work
is done. Git and the record keep finished work.

## The first steps of the next session

1. Finish the release of `v0.29.0` (`docs/maintenance.md`, "A release"):
   the version and the notes (`docs/releases/v0.29.0.md`) are on `main`
   at `52e461a`, and its full local check passed. Read `ci` and
   `adoption` at the head of `main` (`live.yml` run 43174276510 passed at
   `52e461a`); then run `release.yml` with no input, then with
   `version=0.29.0`; read the dry run of `publish.yml` at the tag; post
   `docs/releases/v0.29.0-handoff.md` as the hand-off issue (title
   "COORD-055: v0.29.0: ... (to all)"; fill `COMMIT` and the run ids);
   map it in `TODO/issues.md` and `TODO/github.txt`; message podrunner and
   podssh.
2. Read the hand-off issue #86 (`v0.28.0`) and record each new take.
3. Start the watcher (`sh scripts/agent/watch-org.sh 90` under the Monitor
   tool) and read the open COORD issues `(to podcrates)` of each
   repository of PodUtils.
4. Take the work order of `TODO/PROGRESS.md`. Its head now: T-026 (the
   trial of `pod-relay` in podnet), T-251 (podrunner's ask), T-276
   (PodUtils/podssh#209), T-212, T-267.

## In flight

- The last release: `v0.28.0` at `4ce3625` (the hand-off #86, closed):
  podrunner `bde47c3`, podssh `12473a10`, podnet `b08e06f`
  (PodUtils/podnet#477) and podvm `94e7ca2a` (PodUtils/podvm#278) took it. `v0.29.0` is prepared, not tagged (step 1 above).
- T-026 (partial; calls C73 to C76, C80 to C82, C101 to C103): podnet's
  relay client is in `pod-relay`. The trial of podnet
  (`adoption/podnet/pod-relay.patch`, `.commit`, `.run`) on podnet
  `1abfa91` is not written: no fix of podnet's three frozen files needs a
  port; the Done section of T-026 lists what the patch changes (the
  manifests, the glue of `ws_relay.rs` and `relay_reverse.rs`, the removal
  of `relay_pool.rs`, the callers, podnet's tests and checks) and the
  findings for it (a test token under 8 characters, no public constructor
  of `pair::Pair`, the mode of a pair directory, podnet's layer and size
  allowlists, podnet's citations of `src/relay_pool.rs`). After the trial,
  the words "a binary does not turn it on" of `plain-ws` change (C101).
  podnet holds the three files to fixes from `7c5f87e`.
- T-170 (partial): step 2 is `pod_hold::syn_hold` (feature `syn-hold`;
  calls C90, C91, C100); the trial `adoption/podnet/pod-hold-syn.*` at
  podnet `a7350b2` passed in `adoption.yml` (run 43164667092). podnet's
  take remains; four defects of podnet's router are in the Done section
  and in the hand-off of `v0.29.0`.
- T-040 (partial): the trials of podvm, with each import for podvm.

## What waits

- podvm (on `v0.28.0`, `94e7ca2a`, PodUtils/podvm#278): T-207 (`pod-netlab`, #70) for PodUtils/podvm@T-1662;
  `exec_attached` of `pod-incus` if PodUtils/podvm@T-1722 asks. Write to
  podvm by COORD issues only.
- podnet (on `v0.28.0`, `b08e06f`, PodUtils/podnet#477; #462 to #464 are
  closed): its take of `syn_hold` (T-170); then T-026's trial, then T-028 (`pod-warp`) and
  T-029 (`pod-cloudflare`) on the chain to WARP (the owner's order). Write
  to podnet by COORD issues only.
- podssh (on `v0.28.0`, `12473a10`, PodUtils/podssh#208): its
  PodUtils/podssh@T-737 wires the MQTT meeting point (T-261);
  PodUtils/podssh#209 is T-276; `sandhole` and `openbare-xyz` wait for
  PodUtils/podssh@T-743 and PodUtils/podssh@T-740.
- podrunner (on `v0.28.0`, `bde47c3`): T-251 (`pod-actions-workflow`,
  `pod-actions-expr` and the expansion of jobs, for its local mode
  PodUtils/podrunner@T-046) is its one crate ask.
- `pod-git` (T-217 to T-220) is blocked: its own repository, deferred (the
  owner; #75). M8 (cfb and githome, #35 and #36) waits (the ruling
  "Priority").

## Channels

- podrunner and podssh: lead sessions that podcrates started; direct
  messages (`list_sessions`, titles `(podrunner)` and `(podssh)`). A
  session writes only to its own repository: link a consumer's COORD
  issue from the hand-off issue of podcrates.
- podnet and podvm: their own sessions; COORD issues only
  (`docs/session-protocol.md`).
- Workflows: dispatch them with the GitHub MCP tool
  (`docs/maintenance.md`, "A run of a workflow"). `services.yml` with the
  kind `forward-url` reads the banners of the SSH forward relays (no
  session has SSH).
