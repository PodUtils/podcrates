# Progress

`TODO/INDEX.md` holds 68 entries: 34 open, 1 partial, 0 blocked, 33 done.

## State

- The owner gave the go-ahead on 2026-11-05. Done: M0 (`pod-todo`, the
  record, the checks, `pod-core`, `pod-testkit`, the trials, the
  workflows, `publish.yml`, the shared zig fetch) and, of M1, `pod-term`,
  `pod-identity`, `pod-tls`, `pod-proxy`.
- Released: `v0.1.0`, `v0.2.0` (`pod-tls`, Rust 1.99) and `v0.3.0`
  (`pod-proxy`; the run 42113384858 of `release.yml`, the dry run
  42113413975 of `publish.yml`). podssh took each crate of v0.2.0
  (`6ece2ff`), podnet too (`a5894fe`). The patches of `pod-proxy` went to
  both sessions (PodUtils/podssh@T-684, PodUtils/podnet@T-137).
- Released v0.4.0 at `ad0f265` (the run 42207811181 of `release.yml`,
  after the CI run 42207327373; the owner's account dispatched both,
  PodUtils/podcrates#5): `pod-dns`, `pod-services` with the data of podvm,
  the fix of vectored writes, the synchronous face (T-063). podvm took
  `pod-term` (podvm `75ed306`); the trial of `pod-tls` in podvm passes
  (T-040).
- `pod-services` (T-049, T-050) has its design; the owner reserves its
  name. The names on crates.io wait for the owner (T-043, GitHub #1);
  the switch `crates-io` is false while Q1 stands.
- The sessions of podssh and podnet run the rename and the migration
  only, with the briefing of this session; between sessions, messages,
  else one issue mailbox for each writer (`docs/maintenance.md`).
- Keep this section short. Write the current state only; git keeps the
  history.

## Work order

Finish each entry, close it in place, change this page in the same commit,
then take the next one. Inside a milestone, take the entries in this
order. A crate is released on the day that its entry is done
(`docs/decisions.md`, Delivery and Who edits).

1. **podvm first** (the sibling that is most behind; `docs/HANDOFF.md`):
   the hand-off of v0.4.0 to podssh, podnet and podvm (the patch of
   `pod-tls` to podvm); T-040 (the trials of
   `pod-tls` and `pod-proxy` in podvm); then M6 (item 7).
2. **M1, the transport:** T-016; T-050 (`pod-services`); T-041 when a
   crate or a consumer needs it.
3. **M2, the relay:** T-018, T-019, T-020, T-021, T-022, T-023, T-024,
   T-025, T-026.
4. **M3, the adapters:** T-027, T-028, T-029, T-030.
5. **M4, the roads:** T-031, T-034, T-035, T-033, T-032, T-036, T-037,
   T-038, T-039.
6. **M5, the upkeep:** T-040 (the trials of podvm) when podvm gave the
   commit of its rename; a "port" entry for each change that
   scripts/check-sources.py finds.
7. **M6, the crates of podvm** (`docs/research/podvm/m6-imports.md`, at
   podvm `0249527`): T-055 (`pod-hostprobe`) first; then T-061 and T-059;
   T-060; T-056; T-057; T-058.
8. **M7, the widened scope of podvm:** T-062 after M6.

## Calls made without a ruling

`docs/decisions.md`, "Calls made without a ruling", has each call (C1 to
C19), and the work goes on with it until the owner rules otherwise.

## Owner actions

| Entry | Action |
| --- | --- |
| T-053 | Reserve the 14 names of the crates of podvm (GitHub #1): `pod-hostprobe`, `pod-oci`, `pod-extract`, `pod-rootfs`, `pod-enter`, `pod-supervise`, `pod-guest`, `pod-microvm`, `pod-sud`, `pod-confine`, `pod-netlab`, `pod-pcap`, `pod-sandprobe`, `pod-probe` |
| Each release | Tell the consumer sessions to take the crates |

## Questions for the owner

- **Q4.** When does `publish.yml` publish real versions of the `pod-*`
  crates to crates.io? It blocks no entry. Recommendation: not yet, as
  the ruling of podnet of 2026-11-05; the dry run of each release keeps
  the packaging ready (`docs/decisions.md`).

- **Q6.** GitHub Actions is off for the account that pushes ("Actions has
  been disabled for this user"): no push starts a workflow. The way now: an
  issue `[gh action] requested` with the gh commands, which the owner's
  account runs (PodUtils/podcrates#5 released v0.4.0 so). It blocks no
  entry, and each push waits for its CI. Recommendation: turn Actions on
  for the account of the sessions.


Write each question as `- **Qn.** QUESTION`, with the entries that it
blocks and a recommendation. Number the questions in sequence from Q6
(the owner ruled Q1 to Q5: `docs/decisions.md`).
