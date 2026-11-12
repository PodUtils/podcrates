# Handoff

The state of unfinished work, for a session with no memory of the last
one. Read this page first, then `AGENTS.md`, `TODO/PROGRESS.md` (the
state, the work order, the questions), `docs/maintenance.md`,
`docs/design.md` and `docs/decisions.md`. Keep this page current: remove
a line when its work is done. Git and the record keep finished work.

## In flight

- `pod-relay` (T-025, partial; call C60): imported from podssh `75a5031`
  and adapted (a `Context` of the caller, `Settings`, no globals); 357
  tests and 28 plants pass. The trial of podssh (adoption/podssh/pod-relay.*,
  call C67) passed in `adoption.yml` (run 43130443273): podssh-relay keeps
  each module as glue over one `Context` of the process. Its patch names
  `v0.27.0`. T-229 (six
  plants that only podssh's tests caught) and T-228 (the live tests in
  `live.yml`) come of it. podssh holds the files of the cut from
  `75a5031` to fixes (its fixes come as COORD issues). T-212 makes the
  lines events. A trial of a crate with a digit in its name needs the
  pattern of `scripts/adoption.sh` from `040f3ea`.
- `pod-relay` and podnet (T-026, partial; calls C73 to C76): podnet's
  relay client is in the crate (`RelayBudget`, `stream::of`, the tokens of
  a run, `reverse::local`, `Settings::half_close` and `drain`,
  `OperatorLimits::eof_idle`, `pair::newest`), with the parity list in the
  crate README and a block of podnet in `SOURCES`; 374 tests and 9 new
  plants pass. T-234 is done (an IPv6 relay host in brackets;
  `open::open_on_loopback` under `plain-ws`; calls C80 to C82): the trial
  of podnet (adoption/podnet/pod-relay.*) is next. podnet's binary needs
  the feature `plain-ws` for a relay at `http://` on the loopback (C82).
- The last release: `v0.26.0` at `fdb08f6` (the hand-off #77). For
  `v0.27.0`: T-227 and T-233 (PodUtils/podvm#267 and #269), T-213
  (`pod-confine`: the families that Landlock does not check, the rights
  of ABI 9 and 10), T-240 (a busy tmpfs at its release), on `main`;
  the podssh trial of `pod-relay` (T-025; the patch names `v0.27.0`);
  T-230 to T-232 and T-223 (`pod-netstack`, PodUtils/podnet#457;
  breaking: `StackConfig::accept_pool` is gone, `StackConfig::linger` is
  new); T-026 (partial: podnet's relay client in `pod-relay`).

## What waits

- podvm (on `v0.26.0`, `483a1957`, PodUtils/podvm#270; it takes T-227 and T-233 at the next tag and closes #267 and #269 then): its
  PodUtils/podvm@T-1658 sets `Plan.confine` (`--podvm-confine=SPEC`, the
  profile key `confine = true`); it checks `lambda-al2023` for T-208; its
  PodUtils/podvm@T-1662 reads the module set of T-206 after `pod-netlab`
  (T-207, #70). PodUtils/podvm@T-1722 may ask for `exec_attached` of
  `pod-incus`. Its other takes (PodUtils/podvm#194): `pod-relay` (the
  blocking face, after T-025), `pod-todo`.
- podnet (on `v0.26.0`, `f107766`, PodUtils/podnet#458): it took the
  veth wire of `pod-hold`; its `UrlRule::default()` reads `url-skip`. Its
  PodUtils/podnet@T-249 (the router onto `pod-netstack`) waits for the
  three gaps of PodUtils/podnet#457; T-170 step 2 starts at its COORD issue. `pod-git` waits: its own repository,
  deferred (the owner; #75).
- podssh (on `v0.26.0`, `ff2f7bac`): its `pinggy::skips()` reads the URL
  rule of `pod_services::forward`; the trial patch of `pod-relay` changes
  `pinggy::dashboard()` at `75a5031`, so its take keeps `skips()`. `sandhole` and `openbare-xyz` of `pod-services`
  wait for PodUtils/podssh@T-743 and PodUtils/podssh@T-740.
- podrunner (on `v0.26.0`, `4284967`): its T-075 took T-226 of
  `pod-job`. Its work order waits for podnet (PodUtils/podrunner#64), the
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
