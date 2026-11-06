# Handoff

The state of the unfinished work, for the next session of podcrates.
Keep it current; remove a line when its work is done. `TODO/PROGRESS.md`
has the work order; this page has only what is in flight.

## In flight: v0.4.0

The crates are on `main`: `pod-dns` (T-015), `pod-services` (T-049), the
fix of vectored writes in `pod-proxy` and `pod-tls` (T-065, `161a4d0`), and
the check of the consumers' toolchains (T-045). One patch for each consumer
takes `pod-dns` and `pod-services` together.

1. **Done: the podssh trial** at podssh `fd261a1` (`8847ba3`): each step
   passed, 2,356 tests. The patch removes podssh's workaround of T-065
   (`dial/bare.rs`) and runs the test of podssh-ts that was skipped.
2. **Done: the podnet trial** at podnet `57cfc0b`: each step passed (773
   tests, cargo-deny, clippy, the fuzz targets, the static binary). The
   moved constants are reads in `src/services.rs`, so that no file of
   podnet's size allowlist grows; `report::log_filter` gives `-v` the lines
   of the pod crates; the test `the_dns_step_of_a_url` reads `literal` for
   an IP literal, as pod-dns names that step.
3. **Next: the podvm trial of `pod-term`** (T-040). The patch and the
   `.run` are drafted in the session's scratchpad, not in git: podvm's
   `crates/podvm-ssh/src/session.rs` takes `pod_term::echo` (Discipline,
   Event, Sig, PROMPT, LINE_CAP, HISTORY_CAP), keeps its supervisor, and its
   own tests of the discipline run against pod-term (`ToShell` is
   `ToRemote`, `ToClient` is `ToLocal`). Base: podvm `0249527` (C21). Make
   it again from that description if the scratchpad is gone, then
   `sh scripts/adoption.sh podvm pod-term`, and add `podvm` to the matrix of
   `adoption.yml`.
4. Then: CI's adoption run, the Done sections of T-015 and T-049, the
   progress of T-045, the version 0.4.0 in `Cargo.toml`,
   `docs/releases/v0.4.0.md`, `release.yml` with `version=0.4.0`, and the
   check of the dry run of `publish.yml`.
5. The hand-off: COORD issues of PodUtils/podcrates `(to podssh)`,
   `(to podnet)` and `(to podvm)`, with the patch at the tag and the new pin
   of the watcher in `docs/session-protocol.md` (`54427f6`, T-064).
6. Before each push of a patch: `python3 -I scripts/agent/secrets_like.py
   PATCH` finds the lines that `.github/secrets-allow.txt` must list.

## In flight: T-063, the synchronous face (for podvm)

The code is in the local worktree `.claude/worktrees/t063` (branch `t063`,
not pushed, not compiled yet): `pod-core` and `pod-tls` get a default
feature `async` (tokio); with no default feature they build with no tokio;
`pod-tls` gets `tls_over_blocking` and `server::server_config`;
`pod-proxy` gets the feature `blocking` (`src/blocking.rs`,
`tests/blocking.rs`); a CI step runs clippy of `pod-core` and `pod-tls` with
no default feature. After v0.4.0: compile, test, measure the size, merge,
release as v0.5.0.

## podvm

- T-054 is done (`docs/research/services/podvm.md`); T-066 takes its data
  files. podvm sent its own inventory later (PodUtils/podvm#196): compare it
  with the page.
- The import plan of M6 is `docs/research/podvm/m6-imports.md`
  (`957d47e`). Its Q2 and Q3 are for the podvm session.

## Channels

- No session takes messages now. Use COORD issues
  (`docs/session-protocol.md`; `docs/maintenance.md`, "Talk between
  sessions") and `sh scripts/agent/watch-org.sh` under the Monitor tool.
- Open: PodUtils/podcrates#3 (COORD-002, to podvm), PodUtils/podcrates#1
  (the names on crates.io). PodUtils/podvm#194 (COORD-001): podvm's rename
  is on its `main` at `0249527`.
