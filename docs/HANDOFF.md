# Handoff

The state of the unfinished work, for the next session of podcrates.
Keep it current; remove a line when its work is done. `TODO/PROGRESS.md`
has the work order; this page has only what is in flight.

## In flight: v0.4.0

The crates are on `main`: `pod-dns` (T-015), `pod-services` (T-049), the
fix of vectored writes in `pod-proxy` and `pod-tls` (T-065, `161a4d0`), and
the check of the consumers' toolchains (T-045,
`scripts/check-consumer-toolchains.py`, `794d065`). One patch for each
consumer takes `pod-dns` and `pod-services` together.

1. **The podssh trial is done**: `adoption/podssh/pod-dns.patch` at podssh
   `c880c86` (`a73aff6`); `sh scripts/adoption.sh podssh pod-dns` passed
   each step in 22 min. The `.run` skips one test of podssh-ts
   (`a_socks5_proxy_is_taken_and_the_fork_reads_it_as_one`), which fails
   at `c880c86` with no patch: podssh fixed it in its glue at `ccfef9e`.
   Before the hand-off, make the patch again on podssh's head (`ccfef9e`
   or later). In that patch, remove podssh's workaround for T-065
   (`PodUtils/podssh:crates/podssh-ws/src/dial/bare.rs` and its use in
   `ech::open`), and remove the skip of the podssh-ts test.
2. **The podnet trial is unfinished**: `adoption/podnet/wip/README.md`
   has the diff, the steps and what is left. The base is podnet
   `a0e1ffb`. Make the patch on podnet's head of that day.
3. Then: CI's adoption run (`adoption.yml`; dispatch it by hand when a
   push changes nothing under `crates/` or `adoption/`), the Done sections
   of T-015 and T-049, the progress of T-045, the version 0.4.0 in
   `Cargo.toml`, `docs/releases/v0.4.0.md`, `release.yml` with
   `version=0.4.0`, and the check of the dry run of `publish.yml`.
4. The hand-off: podssh and podnet each get `adoption/CONSUMER/pod-dns.patch`
   and `.run` at the tag, through a COORD issue of PodUtils/podcrates
   `(to podssh)` and one `(to podnet)` (or a message, when the session
   takes messages). Give both the new pin of the watcher in
   `docs/session-protocol.md` (`54427f6`, T-064) at the same time.
5. Each patch of the trials adds a line to `.github/secrets-allow.txt`
   when TruffleHog reads a git blob id as a token:
   `adoption/podnet/wip/secrets_like.py` finds those lines before the push
   (`8848ac9` did it for the podssh patch).

## Channels

- No session takes messages now. Use COORD issues
  (`docs/session-protocol.md`; `docs/maintenance.md`, "Talk between
  sessions") and `sh scripts/agent/watch-org.sh` under the Monitor tool.
- Open: PodUtils/podcrates#3 (COORD-002, to podvm): the crates of podvm,
  the synchronous face (T-063), T-040, the ask for podvm's inventory
  (T-054), and the fix of the watcher on Windows. podvm closes
  PodUtils/podvm#195 when it takes the pin. PodUtils/podvm#194 (COORD-001
  of podvm) waits for podvm's rename commit (T-055 to T-061, T-040).
- Open: PodUtils/podcrates#1 (the names on crates.io, T-042, T-043,
  T-053): the owner reserves the 14 names of podvm.
- podssh was told about T-065 on 2026-11-06: the fix comes with v0.4.0,
  and podssh keeps its workaround until then.
- The consumer sessions paused on 2026-11-06. Their next sessions take no
  messages: they read COORD issues `(to podssh)` and `(to podnet)` of
  PodUtils/podcrates, and they write COORD issues `(to podcrates)` in their
  own repository. podssh's checkpoint is its `TODO/PROGRESS.md`,
  "Checkpoint, 2026-11-06" (`ccfef9e`; PodUtils/podssh@T-684 is partial
  at v0.3.0). podnet's checkpoint is its `docs/HANDOFF.md`, after
  `ef55f3a`. Both wait for v0.4.0. Make each patch on that head, not on
  the base of the trial.

## Local state

None. The local worktrees and the scratch clones of the consumers of the
last session are gone with its container; their useful work is on
`main` or in `adoption/podnet/wip/`.
