# Progress

`TODO/INDEX.md` holds 52 entries: 28 open, 1 partial, 1 blocked, 22 done.

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

1. **M1, the transport:** T-015 when podssh and podnet took `pod-proxy`
   (an import before would see the adoption as drift), T-016; T-045; T-049, then T-050 (`pod-services`); T-041 when a crate or a consumer needs it.
2. **M2, the relay:** T-018, T-019, T-020, T-021, T-022, T-023, T-024,
   T-025, T-026.
3. **M3, the adapters:** T-027, T-028, T-029, T-030.
4. **M4, the roads:** T-031, T-034, T-035, T-033, T-032, T-036, T-037,
   T-038, T-039.
5. **M5, the upkeep:** T-040 when the owner gives the repository of podvm;
   a "port" entry for each change that scripts/check-sources.py finds.

## Calls made without a ruling

`docs/decisions.md`, "Calls made without a ruling", has each call (C1 to
C19), and the work goes on with it until the owner rules otherwise.

## Owner actions

| Entry | Action |
| --- | --- |
| T-040 | The repository of podvm, and access for this session |
| Each release | Tell the consumer sessions to take the crates |

## Questions for the owner

- **Q4.** When does `publish.yml` publish real versions of the `pod-*`
  crates to crates.io? It blocks no entry. Recommendation: not yet, as
  the ruling of podnet of 2026-11-05; the dry run of each release keeps
  the packaging ready (`docs/decisions.md`).

- **Q5.** May the sessions of podssh and podnet open and amend a
  `COORD-NNN` issue in their own repository when the podcrates session
  asks for it, with no word of the owner each time? podnet's session did
  (PodUtils/podnet#317); podssh's session waits for the owner, as its
  rules make a public post only on its own user's word (the owner said yes
  for the test, PodUtils/podssh#151). It blocks no entry. Recommendation:
  yes, for `COORD-NNN` issues only, and tell podssh's session so once.

Write each question as `- **Qn.** QUESTION`, with the entries that it
blocks and a recommendation. Number the questions in sequence from Q6
(the owner ruled Q1 to Q3: `docs/decisions.md`).
