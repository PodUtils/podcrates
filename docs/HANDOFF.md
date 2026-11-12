# Handoff

The state of unfinished work, for a session with no memory of the last
one. Read this page first, then `AGENTS.md`, `TODO/PROGRESS.md` (the
state, the work order, the questions), `docs/maintenance.md`,
`docs/design.md` and `docs/decisions.md`. Keep this page current: remove
a line when its work is done. Git and the record keep finished work.

## In flight

- `pod-relay` (T-025, done; call C60): podssh took it at `0872c2c6`
  (PodUtils/podssh#206); podssh-relay keeps each module as glue over one
  `Context` of the process. T-212 (the lines as events) remains; T-229
  put the six plants of podssh on the crate.
- `pod-relay` and podnet (T-026, partial; calls C73 to C76): podnet's
  relay client is in the crate (`RelayBudget`, `stream::of`, the tokens of
  a run, `reverse::local`, `Settings::half_close` and `drain`,
  `OperatorLimits::eof_idle`, `pair::newest`), with the parity list in the
  crate README and a block of podnet in `SOURCES`; 374 tests and 9 new
  plants pass. T-234 is done (an IPv6 relay host in brackets;
  `open::open_on_loopback` under `plain-ws`; calls C80 to C82): the trial
  of podnet (adoption/podnet/pod-relay.*) on podnet `1abfa91` is next. It
  started: no fix to port; the glue is not written, and no file of the
  trial is committed. The Done section of T-026 lists what the patch
  changes, and calls C101 to C103 give its design (podnet turns on
  `plain-ws`; podnet keeps the relay of its process; the wire of a stored
  pair by the scheme of its file).
- The last release: `v0.28.0` at `4ce3625` (the hand-off #86; #85 is
  closed: each consumer took `v0.27.0`). Each consumer answers on #86 with the
  commit of its take. For the next tag: T-025's take (the `taken` line),
  T-225 (the watch `forward-url`), T-229 (the six plants of podssh), T-245
  and T-246 (the `Host` of an IPv6 relay, the names of the cache files),
  T-271 (PodUtils/podssh#207); in work: T-170 step 2 and T-261.

## What waits

- podvm (on `v0.27.0`, `a714e99f`, PodUtils/podvm#276; #267 and #269
  are closed; it waits for T-254 for its last reported host and for T-262,
  the refusal of a TCP list on i686 and powerpc64le): its
  PodUtils/podvm@T-1658 sets `Plan.confine` (`--podvm-confine=SPEC`, the
  profile key `confine = true`); it checks `lambda-al2023` for T-208; its
  PodUtils/podvm@T-1662 reads the module set of T-206 after `pod-netlab`
  (T-207, #70). PodUtils/podvm@T-1722 may ask for `exec_attached` of
  `pod-incus`. Its other takes (PodUtils/podvm#194): `pod-relay` (the
  blocking face), `pod-todo`.
- podnet (on `v0.27.0`, `28c0e3d`, PodUtils/podnet#465): its
  `UrlRule::default()` reads `url-skip`. Its PodUtils/podnet@T-249 (the
  router onto `pod-netstack`) drops its workarounds; its commit comes in
  PodUtils/podnet#451 (`a7350b2`); T-170 step 2 (partial) imported its
  hold of a SYN as `pod_hold::syn_hold`. Its PodUtils/podnet@T-248
  (the guest-wire router) takes T-257 to T-259 with the next tag. `pod-git` waits: its own repository,
  deferred (the owner; #75).
- podssh (on `v0.27.0`, `0872c2c6`, PodUtils/podssh#206): it took
  `pod-relay` and kept `pinggy::skips()`. Its asks PodUtils/podssh#196,
  #203 and #204 are in `v0.28.0`; #205 is T-261. `sandhole` and `openbare-xyz` of `pod-services`
  wait for PodUtils/podssh@T-743 and PodUtils/podssh@T-740.
- podrunner (on `v0.28.0`, `bde47c3`): its T-042 confines a unit
  through podvm's `--podvm-confine`. Its work order waits for podnet (PodUtils/podrunner#64), the
  next podvm beta, and later crates (`pod-unit`, `pod-release`, the
  registry of `pod-todo`; `pod-actions-workflow` and `pod-actions-expr`
  for its local mode). Its T-042 uses `pod-confine`.
- The banners of the SSH forward relays (T-225, the watch `forward-url`):
  no session of podcrates has SSH. A run of `services.yml` with the kind
  `forward-url` (the daily run, or by hand with the kind) gives the host
  of the URL of each relay; a `MOVED` on `url-match` is a fact that
  podnet's `ssh-relay` reads. T-267 proves the URL through the tunnel.
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
