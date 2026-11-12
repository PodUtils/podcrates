# Handoff

The state of unfinished work, for a session with no memory of the last
one. Read this page first, then `AGENTS.md`, `TODO/PROGRESS.md` (the
state, the work order, the questions), `docs/maintenance.md` (the
procedures, the tools and the rules of the work), `docs/design.md` and
`docs/decisions.md`. Keep this page current: remove a line when its work
is done. Git and the record keep finished work.

## The first steps of the next session

1. Finish the release of `v0.30.0` (`docs/maintenance.md`, "A release")
   if it is not tagged: read `ci` and `adoption` at the head (the trial
   `adoption/podnet/pod-relay.*` runs there), run `release.yml` with no
   input, then with `version=0.30.0`; read the dry run of `publish.yml`;
   post `docs/releases/v0.30.0-handoff.md` as COORD-057 `(to all)` (fill
   `COMMIT` and the run ids), map it, message podrunner and podssh.
2. Start the watcher (`sh scripts/agent/watch-org.sh 90` under the Monitor
   tool) and read the open COORD issues `(to podcrates)`.
3. Take the work order of `TODO/PROGRESS.md`. Its head now: T-212, T-290,
   then the chain to WARP for podnet (T-028, T-029).

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
- `v0.30.0` is prepared on `main`: T-251 (`pod-actions-expr`,
  `pod-actions-workflow`; calls C105 to C109; T-280 waits), T-276, T-277,
  T-267, and the trial of T-026.
- T-267 (`forward-url` proves the URL) is on `main`; the next run of
  `services.yml` proves the URLs of the four live relays.
- T-026 (partial; calls C73 to C76, C80 to C82, C101 to C103, C110 to
  C112): the trial of podnet (`adoption/podnet/pod-relay.patch`,
  `.commit`, `.run`) on podnet `98d8a71` passed each of its 22 steps on
  this host; `adoption.yml` runs it at the push. `a176a93` makes the notes
  of the opener `Send` (podssh and podvm pass `Send` closures at their
  next take). The findings for the hand-off are in the Done section (the
  fake relay's `ready`, the behaviours of the base, the new pair file, 9
  packages more: T-290). Remains: the words "a binary does not turn it on"
  of `plain-ws` (C101), then the release and podnet's take. podnet holds
  the three files to fixes from `7c5f87e`.
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
- podrunner (on `v0.29.0`, `7cdd0d5`): its crate ask T-251 is on `main`
  and goes with `v0.30.0` (PodUtils/podrunner@T-046).
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
