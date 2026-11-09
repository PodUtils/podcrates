# Progress

`TODO/INDEX.md` holds 131 entries: 56 open, 4 partial, 0 blocked, 71 done.

## State

- Released: `v0.8.0` at `f680113` (CI 42689735618, nightly 42689624747,
  release 42694145849): `pod-supervise`, `pod-gsr`, `pod-broker`, the exit
  codes of `pod-core`, the fixes of `pod-dns`, `pod-proxy`, `pod-services`
  and of `pod-hostprobe` on 32-bit; COORD-017 (GitHub #41) hands it to each
  consumer. Each consumer is on `v0.7.0` until it takes it. Open: T-131
  (`pod-gsr` on Windows ARM64).
- M6: `pod-hostprobe` is taken (T-055). `pod-supervise` (T-060) is partial:
  its trial of podvm passed; the release and the take of podvm remain.
  `pod-guest` (T-061) and `pod-enter` (T-059) are partial: their trials
  of podvm passed; the release `v0.9.0` and the take of podvm remain.
  Next in M6: T-056.
- GitHub #20 to #36 are in the record; #35 and #36 are routed
  (`docs/research/fleet/asks.md`). podnet's asks for `pod-relay`, `pod-dns`
  and `pod-services` are T-121 to T-127 (PodUtils/podnet#413).
- M8: `pod-broker` exists (T-099, calls C28 to C32). The rest of M8 waits
  (the owner, 2026-11-09, "Priority"). The owner created the repositories
  of the new projects; they stay empty.
- M4: `pod-gsr` exists with its blocking face (T-086 to T-088; calls C34,
  C35: a static OpenSSL for TLS with SRP, and `End`).
- Each name of `docs/design.md`, section 4, is reserved on crates.io
  (`check-crates-io-names.py`: 56 names, 0 faults). No crate goes to
  crates.io now (the owner, 2026-11-09).
- Keep this section short. Write the current state only; git keeps the
  history.

## Work order

Finish each entry, close it in place, change this page in the same commit,
then take the next one. Inside a milestone, take the entries in this
order. A crate is released on the day that its entry is done
(`docs/decisions.md`, Delivery and Who edits).

The owner's ruling "Priority" (2026-11-09): podvm, podnet and podssh come
first; the asks of cfb and githome (M8) wait.

1. **The defects of the consumers:** T-130 (the
   public Wisp servers of podssh).
2. **M6, the crates of podvm** (`docs/research/podvm/m6-imports.md`, at
   podvm `main` on the day of each import): T-060 (partial: the release
   and the take of podvm), T-061 and T-059 (partial: the release and the
   take of podvm), T-056, T-057, T-058. T-040 (the trials of podvm)
   goes with each import.
3. **M1, the transport:** T-016 (`pod-ws`, the next take of podssh and
   podnet); T-050 (`pod-services`); T-041 when a crate or a consumer needs
   it.
4. **M2, the relay:** T-018, T-019, T-020, T-021, T-022, T-023, T-024,
   T-025, T-026; then T-122, T-123, T-124, T-121 (the asks of podnet).
5. **M3, the adapters:** T-027, T-028, T-029, T-030.
6. **M4, the roads:** T-031, T-034, T-035, T-033, T-032, T-036, T-037,
   T-038, T-039.
7. **M5, the upkeep:** a "port" entry for each change that
   scripts/check-sources.py finds.
8. **M7, the widened scope of podvm:** T-062 after M6.
9. **M8, the fleet, and the other asks of #35 and #36: they wait** (the
   owner, 2026-11-09). T-100, T-102 to T-120 wait, unless a
   consumer needs one first (T-108 `pod-confine` for PodUtils/podvm@T-1658, T-107
   with M6). T-113 to T-116 wait for the first release on crates.io.

## Calls made without a ruling

`docs/decisions.md`, "Calls made without a ruling", has each call (C1 to
C35), and the work goes on with it until the owner rules otherwise.

## Owner actions

| Entry | Action |
| --- | --- |
| Each release | Tell the consumer sessions to take the crates |

## Questions for the owner

None open. The owner ruled Q4, Q7 and the questions of GitHub #35 and #36 on 2026-11-09 (`docs/decisions.md`). Q6 closes: a push to `main` starts `ci.yml` again, and the session dispatches the other runs (`docs/maintenance.md`, "A run of a workflow").

Write each question as `- **Qn.** QUESTION`, with the entries that it
blocks and a recommendation. Number the questions in sequence from Q8
(the owner ruled Q1 to Q7: `docs/decisions.md`).
