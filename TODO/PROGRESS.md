# Progress

`TODO/INDEX.md` holds 41 entries: 32 open, 0 partial, 1 blocked, 8 done.

## State

- The owner gave the go-ahead on 2026-11-05. M0 runs: `pod-todo` and the
  record are done (T-001, T-002).
- No crate is released. The consumers wait: podssh
  (PodUtils/podssh@T-684), podnet (PodUtils/podnet@T-137).
- Keep this section short. Write the current state only; git keeps the
  history.

## Work order

Finish each entry, close it in place, change this page in the same commit,
then take the next one. Inside a milestone, take the entries in this
order. A crate is released on the day that its entry is done
(`docs/decisions.md`, Delivery and Who edits).

1. **M0, the foundation:** T-007, T-008, then
   the first release T-009.
2. **M1, the transport:** T-012, T-013, T-014, T-015, T-016;
   T-017 after T-013; T-041 when a crate or a consumer needs it.
3. **M2, the relay:** T-018, T-019, T-020, T-021, T-022, T-023, T-024,
   T-025, T-026.
4. **M3, the adapters:** T-027, T-028, T-029, T-030.
5. **M4, the roads:** T-031, T-034, T-035, T-033, T-032, T-036, T-037,
   T-038, T-039.
6. **M5, the upkeep:** T-040 when the owner gives the repository of podvm;
   a "port" entry for each change that scripts/check-sources.py finds.

## Calls made without a ruling

`docs/decisions.md`, "Calls made without a ruling", has each call (C1 to
C17), and the work goes on with it until the owner rules otherwise.

## Owner actions

| Entry | Action |
| --- | --- |
| T-040 | The repository of podvm, and access for this session |
| Each release | Tell the consumer sessions to take the crates |

## Questions for the owner

None now. Write each question as `- **Qn.** QUESTION`, with the entries
that it blocks and a recommendation. Number the questions in sequence
from Q4 (the owner ruled Q1 to Q3: `docs/decisions.md`).
