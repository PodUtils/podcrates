# Progress

`TODO/INDEX.md` holds 45 entries: 29 open, 0 partial, 2 blocked, 14 done.

## State

- The owner gave the go-ahead on 2026-11-05. Done: `pod-todo`, the
  record, the checks, `pod-core`, `pod-testkit`, the trials (M0), and
  `pod-term`, `pod-identity` (M1). Each workflow is green.
- Released: `v0.1.0` (T-009) and `v0.2.0` (`pod-tls`, Rust 1.99; the run
  42057101369 of `release.yml` and the dry run 42057132368 of
  `publish.yml` on the tag). podssh took each crate of v0.1.0 (`04f4551`),
  podnet took `pod-todo` (`6c210ed`). The patches of `pod-tls` went to
  both sessions (PodUtils/podssh@T-684, PodUtils/podnet@T-137).
- `pod-tls` is done (T-012), with its trials on podssh `85d4e9d` and
  podnet `c7a31f3`. Next: `pod-proxy` (T-013).
- `publish.yml` is in place (T-042); the switch `crates-io` is false
  while Q1 stands. The names on crates.io wait for the owner (T-043,
  GitHub #1).
- The sessions of podssh and podnet run the rename and the migration
  only, with the briefing of this session.
- Keep this section short. Write the current state only; git keeps the
  history.

## Work order

Finish each entry, close it in place, change this page in the same commit,
then take the next one. Inside a milestone, take the entries in this
order. A crate is released on the day that its entry is done
(`docs/decisions.md`, Delivery and Who edits).

1. **M1, the transport:** T-013, T-014, T-015, T-016; T-017 after
   T-013; T-045; T-041 when a crate or a consumer needs it.
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
| T-043 | Reserve the 12 names that wait in the queue of GitHub #1, and set the trusted publisher of each (`python3 -I scripts/check-crates-io-names.py` lists them) |

## Questions for the owner

- **Q4.** When does `publish.yml` publish real versions of the `pod-*`
  crates to crates.io? It blocks no entry. Recommendation: not yet, as
  the ruling of podnet of 2026-11-05; the dry run of each release keeps
  the packaging ready (`docs/decisions.md`).

Write each question as `- **Qn.** QUESTION`, with the entries that it
blocks and a recommendation. Number the questions in sequence from Q5
(the owner ruled Q1 to Q3: `docs/decisions.md`).
