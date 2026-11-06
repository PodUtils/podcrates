# Handoff

The state of the unfinished work, for the next session of podcrates.
Keep it current; remove a line when its work is done. `TODO/PROGRESS.md`
has the work order; this page has only what is in flight.

## In flight

- **v0.4.0** (`pod-dns`, T-015; `pod-services`, T-049; the static check of
  the trials, T-045). The crates are on `main`. Their trials (one patch
  for each consumer, `adoption/CONSUMER/pod-dns.patch`, which takes both
  crates) are made on podssh `c880c86` and podnet `a0e1ffb`. Then: CI's
  adoption run, the Done sections, the version 0.4.0 and its notes,
  `release.yml`, the hand-off of each patch.
- **A regression in podssh after it took `pod-proxy`**: the interop step
  `resume.cut.up.sftp` of podssh's release gate (a `cp` up through a
  stand-in relay that cuts each 1.5 MB fails at write 0, exit 70; it
  passed at podssh `d3745ab`, before the adoption). podssh is finding the
  cause. If it is in `pod-proxy`: a fix with a test that fails before it,
  and a patch release. `write_limit.rs` is the same as podssh's;
  `lowat::set` takes the byte count from its caller now.
- **Rulings of the owner that podssh reported (2026-11-06)**, to record in
  `docs/decisions.md` when they are on podssh's `main` (its
  `docs/decisions.md`): Q5 (a standing yes for COORD issues of podssh);
  Q103 (podvm takes the blocking facade from `pod-relay` by a tag; the test
  of two clients at once goes with the facade into `pod-relay`); no podssh
  release until the migration is done.

## Channels

- podssh: the migration session takes messages
  (`session_01DXe55uLRzFzFT57MQXxejx`) until the migration is done; after
  it, COORD issues of PodUtils/podssh `(to podcrates)`.
- podnet: no messages from 2026-11-06 on: SOURCES blocks, the patches at
  the tags, COORD issues (`docs/maintenance.md`, "The channel of a consumer
  session with no messages").
- `scripts/agent/watch-org.sh` reports each message to podcrates.

## Local state of this session (not in git)

- Worktrees under `.claude/worktrees/`: `pod-proxy` (the branches of the
  crates and trials; each merged branch is on `main`) and `pod-tls` (the
  branch `t045`, whose useful part is on `main`). Nothing in them needs to
  be kept.
