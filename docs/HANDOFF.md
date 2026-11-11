# Handoff

The state of podcrates on 2026-11-11, for a session with no memory of
the last one. Read this page first, then `AGENTS.md`,
`TODO/PROGRESS.md` (the state, the work order, the questions),
`docs/maintenance.md`, `docs/design.md` and `docs/decisions.md`. Keep this
page current; remove a line when its work is done.

## Where podcrates stands

- The release `v0.22.0` is at `93de04a` (`docs/releases/v0.22.0.md`; ci
  43021342956, release 43028589210; the hand-off GitHub #71, COORD-046):
  `pod-e2e` (T-021), `pod-wg` (T-024), `Plan.die_with_parent` (T-205), a
  gone launcher in `pod-supervise` (T-202), `zerotier-tcp-relay` (T-203).
  `v0.21.0` is at `ada3699` (the hand-off #65).
- The next release, `v0.23.0`: `pod-confine` and `Plan.confine` of
  `pod-enter` (T-108, call C57; #69), the empty root entry of `pod-extract`
  (T-208, PodUtils/podvm#261), `plant-each.sh --apply` (T-210). Breaking:
  `Plan` has the field `confine`.
- The consumers and the tag each takes:
  - podssh: `v0.22.0` (`75a50318`, PodUtils/podssh#191; it took `pod-e2e`;
    the key of `localhost-run` pinned;
    `sandhole` and `openbare-xyz` wait for PodUtils/podssh@T-743 and
    PodUtils/podssh@T-740).
    `pod-wormhole` at `baa00a06`, `pod-session` at `6f78a7c7`.
  - podnet: `v0.21.0` (`54bd958`, PodUtils/podnet#448; `pod-services-watch`
    at `0b945a3`). `pod-netstack` at `d23ce4e` (PodUtils/podnet#445:
    taken; it keeps `CongestionControl::None` in `tests/tcp_window.rs`).
    T-188 is done: Reno and CUBIC are slow only on a link that gives one
    ACK for each round trip; its result for `warp --cc reno|cubic` goes to
    podnet. `pod-hold` at `b759937`,
    its `connect` and `packet` at `e4d4049`. Next: step 2 of T-170 after
    podnet's T-174 (the netns router).
  - podvm: `v0.22.0` (`c9d5038`, PodUtils/podvm#262; the launcher of
    `podvm-supervise` sets `die_with_parent`, proved by its experiment 437;
    its incus tier calls `pod-incus`). It waits for `v0.23.0` (`pod-confine`
    for PodUtils/podvm@T-1658, the fix of #261 for PodUtils/podvm@T-1659). Its
    PodUtils/podvm@T-1722 may ask for `exec_attached`.
  - podrunner: `v0.22.0` (`82b7af0`; `pod-job` at `0457ed0`). Its tests
    25 and 26 pass on the fake unit and on podvm `v0.2.0-beta.3` (run
    43018641300); no code of podrunner uses `pod-enter` or
    `pod-supervise`. No crate ask.
    Its state (gate green at `995de09`): its T-053 covers each design row
    but a real guest (its T-036); its T-063 kills a `setsid env -i`
    process with a subreaper shim of its own; no crate ask. Earlier
    (head `4ca19f5`): its T-035 is done (a job with a
    `container:` key runs through the hooks of a podvm unit; gate
    42991333922). Its milestone M-B holds (PodUtils/podrunner#26
    closed; runner-image run 42987612426): on a hosted runner with no root,
    the podvm driver (podvm `v0.2.0-beta.3`, the supervise rung) ran the
    official runner 2.338.2 on a canned job to Succeeded, and destroy
    counted 0 processes and 0 files. No crate ask. Its finding
    PodUtils/podrunner#61 (an absolute link of a rootfs resolves on the
    host on the supervise rung) is podvm's `podvm-interpose`, not a crate
    of podcrates (`pod-enter` names the limit); podvm takes it as
    PodUtils/podvm@T-1725.
- podrunner and podssh have lead sessions that this session started
  (direct messages; `list_sessions`, titles `(podrunner)` and `(podssh)`);
  podnet and podvm run in their own sessions (COORD issues).

## In flight

- `pod-relay` (T-025): the import takes podssh `75a50318` (podssh's
  answer, PodUtils/podssh#191). podssh holds the files of the row
  `pod-relay` (`docs/design.md`, section 4) to fixes and sends each fix as a
  COORD issue with its commit. podssh's `tests/e2e_layered.rs` comes with
  it. A trial of a crate with a digit in its name needs `040f3ea` (the
  pattern of `scripts/adoption.sh`).
- `pod-wg` (T-024, done; call C56): released in `v0.22.0`; podnet's take
  remains. `adoption/podnet/pod-wg.patch` (podnet `54d7ed4`, the commit that
  PodUtils/podnet#447 holds) is the hand-off; its checks passed here (the
  patch applies, podnet's record, clippy, rustfmt, cargo-deny,
  `cargo test -p podnet --lib`: 457 passed), and `adoption.yml` runs the
  whole `.run`. Tell podnet on GitHub #67: the defects that the crate fixed
  (the T-024 Done section), the cookie, and that its take moves from
  `v0.21.0` (`54bd958`). podnet answers on #67 with the commit of its take.
- At a take of a consumer (a COORD issue `(to podcrates)` or an answer on
  the hand-off): add the `taken` line to the crate's `SOURCES`
  (`check-sources.py` with clones must show 0 changes), remove the trial
  files of that consumer, set the entry to done, and close the hand-off
  issue when each consumer took its part.
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
  head; a push to `main` cancels the runs that wait. A change of a file
  that a plant patches makes the plant stale: `check.sh` runs
  `plant-each.sh --apply` (T-210).
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
