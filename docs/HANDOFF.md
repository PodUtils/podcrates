# Handoff

The state of podcrates at the pause of 2026-11-10, for a session with no
memory of the last one. Read this page first, then `AGENTS.md`,
`TODO/PROGRESS.md` (the state, the work order, the questions),
`docs/maintenance.md`, `docs/design.md` and `docs/decisions.md`. Keep this
page current; remove a line when its work is done.

## Where podcrates stands

- The release `v0.15.0` is at `27a4ccd` (`docs/releases/v0.15.0.md`). Its
  hand-off to the consumers is GitHub #54 (COORD-029); its runs are in
  `TODO/PROGRESS.md`, State.
- The consumers and the tag each takes:
  - podssh: `v0.14.0` (it took `pod-ws` and `pod-quic`). Next: `pod-wormhole`
    and `pod-session` of `v0.15.0` (`adoption/podssh/`, made on podssh
    `9975fe92`; `pod-session.needs` names `pod-wormhole`).
  - podnet: `v0.14.0` (it took `pod-ws` and `pod-quic`; no quinn is left).
    Next: `pod-hold` of `v0.15.0` (`adoption/podnet/pod-hold.*`, made on
    podnet `9e8d177`).
  - podvm: `v0.12.0` (it took its seven crates). Next: the `pod-enter` change
    of T-155 in `v0.15.0` (`Maps::new` takes `&Names`; `Names::reserved`);
    the README of `pod-enter`, "Adoption map", lists podvm's change. No
    patch: a move of calls.
- Each session of PodUtils paused on 2026-11-10 by the owner's word (GitHub
  #53). Each consumer wrote its own `docs/HANDOFF.md`.

## In flight

- T-020 `pod-session`, T-022 `pod-wormhole`, T-166 `pod-hold`, T-155:
  released in `v0.15.0` (the trials and the cross tests passed); the takes
  of the consumers remain (#54). T-169 and T-170 ask podnet whether two
  more parts belong to the cut of `pod-hold`. When a consumer writes
  its take as a COORD issue `(to podcrates)`: add the `taken` line to the
  crate's `SOURCES` (`check-sources.py` with clones must show 0 changes),
  remove the trial files of that consumer, set the entry to done, close
  #54 when each consumer took its part.
- T-171 and T-172: the last asks of podssh before the pause (PodUtils/podssh#186): three services of
  `pod-services` and `Rewound::new` of `pod-session`; read each fact again
  at its source before the data.
- T-157: four public Trojan nodes of podssh fail the live check. On
  2026-11-11 or later, run `services.yml` by hand (input `kind`
  `proxy-banner`); remove or replace each node that fails both days.

## Waiting for the owner

- GitHub #47 (the CI tracker, `podrunner`): "leave it alone for now". T-146,
  the questions Q8 to Q14 and the podrunner entries T-160 to T-165 (from
  PodUtils/podvm#227) wait for the owner's DR-1 (name the plan) and DR-2
  (code in PodUtils/podrunner). Do not build them before.
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
