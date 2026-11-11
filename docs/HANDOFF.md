# Handoff

The state of podcrates on 2026-11-11, for a session with no memory of
the last one. Read this page first, then `AGENTS.md`,
`TODO/PROGRESS.md` (the state, the work order, the questions),
`docs/maintenance.md`, `docs/design.md` and `docs/decisions.md`. Keep this
page current; remove a line when its work is done.

## Where podcrates stands

- The release `v0.20.0` is at `4daf31c` (`docs/releases/v0.20.0.md`; the
  hand-off GitHub #64). `v0.19.0` is at `9a11bbd` (#62). On `main` for
  `v0.21.0`: `pod-incus` (T-179, PodUtils/podvm#253, GitHub #63), T-195
  and T-196 (the fix of PodUtils/podnet#444, ported).
- The consumers and the tag each takes:
  - podssh: `v0.20.0` (`0c40ed3e`; the key of `localhost-run` pinned;
    `sandhole` and `openbare-xyz` wait for PodUtils/podssh@T-743 and
    PodUtils/podssh@T-740).
    `pod-wormhole` at `baa00a06`, `pod-session` at `6f78a7c7`.
  - podnet: `v0.20.0` (`d23ce4e`, PodUtils/podnet#445: `pod-netstack`
    taken; it keeps `CongestionControl::None` in `tests/tcp_window.rs`).
    T-188 is done: Reno and CUBIC are slow only on a link that gives one
    ACK for each round trip; its result for `warp --cc reno|cubic` goes to
    podnet. `pod-hold` at `b759937`,
    its `connect` and `packet` at `e4d4049`. Next: step 2 of T-170 after
    podnet's T-174 (the netns router).
  - podvm: `v0.19.0` (`2c2b8d2`). Next: `pod-incus` of `v0.21.0` (its
    PodUtils/podvm@T-1718).
  - podrunner: `v0.19.0` (after its `6f24681`; `pod-job` at `0457ed0`).
    Its state (head `fa374d7`): no crate ask; the rest waits for a release
    of podvm or DR-7. An open defect of podvm that podrunner found:
    PodUtils/podrunner#49 (`podvm wait` gives no exit code for a payload
    that ends at once; to podvm).
- podrunner and podssh have lead sessions that this session started
  (direct messages; `list_sessions`, titles `(podrunner)` and `(podssh)`);
  podnet and podvm run in their own sessions (COORD issues).

## In flight

- At a take of a consumer (a COORD issue `(to podcrates)` or an answer on
  the hand-off): add the `taken` line to the crate's `SOURCES`
  (`check-sources.py` with clones must show 0 changes), remove the trial
  files of that consumer, set the entry to done, and close the hand-off
  issue when each consumer took its part.
- `v0.21.0`: `pod-incus` for podvm (#63). Release it when `ci` is green
  at the head, then answer #63.
- None of podrunner: the owner answered DR-4 to DR-8 and Q9 to Q14 on
  2026-11-11 (`docs/decisions.md`). DR-7 (a test organization) is "not
  now".
- M8 (cfb and githome, #35 and #36) waits (the ruling "Priority").
- The image `ghcr.io/podutils/podnet` is private; podnet asked the owner to
  make it public (podnet's record).

## Rules learned in this session

- Gate a commit on the exit code of a check (`cargo todo check >/dev/null
  2>&1 && git commit`), never on a pipe into `tail`.
- Before a release, read `ci`, `nightly`, `adoption` and `plants` at the
  head; a push to `main` cancels the runs that wait.
- After a release, read the dry run of `publish.yml` at the tag: it was red
  at `v0.14.0` and `v0.15.0` (a cycle of dev-dependencies) and no session saw it.
- A new entry id or call number: take it with `cargo todo next` just before
  the commit; sessions and agents add entries in parallel.
- A finding goes into the work at once: fixed in place, or written as an
  entry (`AGENTS.md`, section 10). No "suggested task" card.
- Write to podvm only by a COORD issue `(to podvm)` (the ruling "podvm
  online"); podssh and podnet read their COORD issues and also take direct
  messages of the parent session when one runs.
- An agent works in its own worktree under `.claude/worktrees/`, never in
  the main checkout; the disk is small: remove `target/debug` and the
  copies of the consumers after a trial.

## Channels

- Workflows: dispatch them with the GitHub MCP tool
  (`docs/maintenance.md`, "A run of a workflow").
- Messages: COORD issues (`docs/session-protocol.md`) and
  `sh scripts/agent/watch-org.sh` under the Monitor tool.
