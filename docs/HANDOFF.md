# Handoff

The state of podcrates on 2026-11-11, for a session with no memory of
the last one. Read this page first, then `AGENTS.md`,
`TODO/PROGRESS.md` (the state, the work order, the questions),
`docs/maintenance.md`, `docs/design.md` and `docs/decisions.md`. Keep this
page current; remove a line when its work is done.

## Where podcrates stands

- The release `v0.19.0` is at `9a11bbd` (`docs/releases/v0.19.0.md`; the
  hand-off GitHub #62). `v0.18.0` is at `c284d3a` (#60). The import of
  `pod-netstack` (T-023, the cut of PodUtils/podnet#427) is under way.
- The consumers and the tag each takes:
  - podssh: `v0.18.0` (`50ef8922`; T-171, T-172 and T-181 taken, the key
    of `ssh-j` pinned; PodUtils/podssh#186 closed). `pod-wormhole` at
    `baa00a06`, `pod-session` at `6f78a7c7`.
  - podnet: `v0.17.0` (`dd04b36`; `pod-hold` at `b759937`, its
    `connect` and `packet` at `e4d4049`, PodUtils/podnet#426; it released
    `v0.4.0-beta.3`). Next: step 2 of T-170 after podnet's COORD issue.
  - podvm: `v0.19.0` (`2c2b8d2`; PodUtils/podvm#249, read by podcrates;
    T-182 proved by its experiment 356, clause 32).
  - podrunner: `v0.19.0` (after its `6f24681`; `pod-job` at `0457ed0`, its
    `taken` line is in `crates/pod-job/SOURCES`; its T-048 at `6f24681`
    takes `check_upstream`, `names_upstream` and the flagged rules). Its
    state (head `fa374d7`): the donor proposals of PodUtils/podnet#439 and
    #440 done as its #54 to #60; the official runner runs the job corpus
    of `pod-job` on its fake path; no crate ask for podcrates; the rest
    waits for a release of podvm or DR-7. It uses `pod_tls::mutual` for its
    remote mint (its T-031) and the pin `github-actions-runner` of
    `pod-services` for its runner image (its T-030). Next: `v0.18.0`
    (T-161, T-177, T-178). An open defect of podvm that podrunner found:
    PodUtils/podrunner#49 (`podvm wait` gives no exit code for a payload
    that ends at once; to podvm, on break).
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
- T-023 `pod-netstack` (the cut of PodUtils/podnet#427) is on `main` for
  `v0.20.0`. Its hand-off to podnet is `adoption/podnet/pod-netstack.patch`
  (tag `v0.20.0`). After the release, tell podnet in a COORD issue
  `(to podnet)`: the stack of podnet-vnet drops the bytes of a receive when
  the channel to the app is full (a slow reader gets a stream with a hole;
  the crate fixes it), and `warp --cc reno|cubic` is slow on smoltcp 0.14
  (T-188). At podnet's take, add the `taken` line to
  `crates/pod-netstack/SOURCES` and remove the trial files.
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
