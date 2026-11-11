# Handoff

The state of podcrates on 2026-11-11, for a session with no memory of
the last one. Read this page first, then `AGENTS.md`,
`TODO/PROGRESS.md` (the state, the work order, the questions),
`docs/maintenance.md`, `docs/design.md` and `docs/decisions.md`. Keep this
page current; remove a line when its work is done.

## Where podcrates stands

- The release `v0.17.0` is at `4da8fb3` (`docs/releases/v0.17.0.md`; the
  hand-off GitHub #58). `v0.16.0` is at `60a675c` (#57).
- The consumers and the tag each takes:
  - podssh: `v0.15.0` (`pod-wormhole` at `baa00a06`, `pod-session` at
    `6f78a7c7`). It may move to `v0.16.0` (four nodes of `pod-services`
    are gone).
  - podnet: `v0.15.0` (`pod-hold` at `b759937`). Next: the adoption patch
    of T-169 and T-170 (not made yet).
  - podvm: `v0.17.0` (`522b437`, the limits of `pod-enter`, T-176;
    PodUtils/podvm#245 closed).
  - podrunner: `v0.16.0` for `pod-core` and `pod-todo` (`07cd854`). Next:
    `pod-supervise` of `v0.16.0`; `pod-job` and `pod_tls::mutual` of
    `v0.17.0` (T-163, T-165).
- podrunner and podssh have lead sessions that this session started
  (direct messages); podnet and podvm run in their own sessions (COORD
  issues).

## In flight

- podrunner (the rulings of 2026-11-11, C49): a lead session of
  PodUtils/podrunner runs, which this session started and briefed; the two
  talk by direct messages (`send_message`). Its index is
  PodUtils/podrunner#1. podcrates builds T-161, T-177, T-178 and T-164
  in the order of its need (T-160, T-162 and T-163 are done; podrunner
  takes `pod-job` at the next tag). A new session of podcrates finds the
  session id of podrunner with `list_sessions` (title `(podrunner)`).
- podssh: a lead session that this session started on 2026-11-11 (title
  `(podssh)`); direct messages. Its next step: the takes of `pod-wormhole`
  and `pod-session` (#54).
- podnet and podvm run in their own sessions and answer COORD issues.
  podvm resumed (PodUtils/podvm#241): it takes `v0.15.0` (T-155) and asked
  for four fuzz targets (T-173).
- Two study agents of this session: the note on pmask (the ruling
  "podrunner DR-6 and pmask") and the note on the owner's archive
  `ghactions-local` (the ruling "Q11"). Each note goes to
  `docs/research/` and to podrunner.

- T-020 `pod-session`, T-022 `pod-wormhole`: released in `v0.15.0` (the
  trials and the cross tests passed); podssh's take remains (#54). podnet
  took `pod-hold` (T-166 done); podvm took `v0.15.0` with T-155
  (PodUtils/podvm#242). podnet
  answered T-169 yes and T-170 yes in two steps (PodUtils/podnet#424).
  When a consumer writes
  its take as a COORD issue `(to podcrates)`: add the `taken` line to the
  crate's `SOURCES` (`check-sources.py` with clones must show 0 changes),
  remove the trial files of that consumer, set the entry to done, close
  #54 when each consumer took its part.
- T-171 and T-172: the last asks of podssh before the pause (PodUtils/podssh#186): three services of
  `pod-services` and `Rewound::new` of `pod-session`; read each fact again
  at its source before the data.
- `main` holds work after `v0.15.0` for the next tag: T-162 (the tree
  kill of `pod-supervise`, for podvm and podrunner), T-169 and T-170
  step 1 (`pod-hold`, for podnet), T-157 (four nodes removed from
  `pod-services`). The next release writes the adoption patches.

## Waiting for the owner

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
