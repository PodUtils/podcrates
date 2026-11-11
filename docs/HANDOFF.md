# Handoff

The state of unfinished work, for a session with no memory of the last
one. Read this page first, then `AGENTS.md`, `TODO/PROGRESS.md` (the
state, the work order, the questions), `docs/maintenance.md`,
`docs/design.md` and `docs/decisions.md`. Keep this page current: remove
a line when its work is done. Git and the record keep finished work.

## In flight

- `pod-relay` (T-025, partial; call C60): imported from podssh `75a5031`
  and adapted (a `Context` of the caller, `Settings`, no globals); 356
  tests and 28 plants pass. The next step is the trial of podssh
  (adoption/podssh/pod-relay.*): its glue gives each call that took a
  `ProxyChoice` a `Context`. podssh holds the files of the cut from
  `75a5031` to fixes (its fixes come as COORD issues). T-212 makes the
  lines events. A trial of a crate with a digit in its name needs the
  pattern of `scripts/adoption.sh` from `040f3ea`.
- The last release: `v0.25.0` at `80f5dc7` (the hand-off #76). In work
  for the next one: the podssh trial of `pod-relay` (T-025), the veth wire
  of `pod-hold` (T-201), the SSH relay facts of `pod-services`
  (PodUtils/podnet#455).

## What waits

- podvm (on `v0.25.0`, `763b7c6a`, PodUtils/podvm#266): its
  PodUtils/podvm@T-1658 sets `Plan.confine` (`--podvm-confine=SPEC`, the
  profile key `confine = true`); it checks `lambda-al2023` for T-208; its
  PodUtils/podvm@T-1662 reads the module set of T-206 after `pod-netlab`
  (T-207, #70). PodUtils/podvm@T-1722 may ask for `exec_attached` of
  `pod-incus`. Its other takes (PodUtils/podvm#194): `pod-relay` (the
  blocking face, after T-025), `pod-todo`.
- podnet (on `v0.22.0`): the move to `v0.24.0` (#73). T-170 step 2 starts
  at podnet's COORD issue of PodUtils/podnet@T-249 (the router onto
  `pod-netstack`); T-201 is done: podnet takes the veth wire with
  `adoption/podnet/pod-hold-veth.patch` (podnet `db34bae`, tag `v0.26.0`)
  and holds `netns/veth.rs` and `netns/tun.rs` to fixes from `db34bae`.
  The pod-netstack APIs of PodUtils/podnet@T-249 are
  T-222 (in `v0.25.0`, #76). `pod-git` waits: its own repository,
  deferred (the owner; #75).
- podssh (on `v0.24.0`): `sandhole` and `openbare-xyz` of `pod-services`
  wait for PodUtils/podssh@T-743 and PodUtils/podssh@T-740.
- podrunner (on `v0.25.0`, `02a1c0b`): its T-075 takes T-226 of `pod-job`
  in the next tag. Its T-042 uses `pod-confine`.
- M8 (cfb and githome, #35 and #36) waits (the ruling "Priority").
- The image `ghcr.io/podutils/podnet` is private; podnet asked the owner to
  make it public (podnet's record).

## Rules learned

- Gate a commit on the exit code of a check (`cargo todo check >/dev/null
  2>&1 && git commit`), never on a pipe into `tail`.
- Before a release, read `ci`, `nightly`, `adoption` and `plants` at the
  head; a push to `main` cancels the runs that wait. A change of a file
  that a plant patches makes the plant stale: `check.sh` runs
  `plant-each.sh --apply` (T-210).
- After a release, read the dry run of `publish.yml` at the tag: a cycle
  of dev-dependencies makes it red, and no other check sees it.
- Take a new entry id or call number with `cargo todo next` just before
  the commit; sessions and agents add entries in parallel.
- Each version bump makes a new copy of each workspace crate in
  `target/debug` (the version is in the hash of each artifact). Before the
  check of a bump, remove `target/debug` under the cargo lock
  (`flock SCRATCHPAD/cargo.lock`) when `df -h /` shows less than 25 GB
  free. Agents that build in parallel take the same lock for each cargo
  command.
- Do not share `target/` between worktrees: an artifact built in a
  worktree holds its absolute paths (`include_str!` of `pod-services`,
  `CARGO_MANIFEST_DIR` of `pod-job`), and cargo reuses it in the main
  checkout, so the doc and the tests fail there after the worktree goes.
  After a merge of an agent's work that shared `target/`, remove
  `target/debug` before the check.

## Channels

- podrunner and podssh: lead sessions that podcrates started; direct
  messages (`list_sessions`, titles `(podrunner)` and `(podssh)`).
- podnet and podvm: their own sessions; COORD issues only
  (`docs/session-protocol.md`; the ruling "podvm online"). Run
  `sh scripts/agent/watch-org.sh` under the Monitor tool.
- Workflows: dispatch them with the GitHub MCP tool
  (`docs/maintenance.md`, "A run of a workflow").
- The take of a consumer: `docs/maintenance.md`, "The take of a consumer".
