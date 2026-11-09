# Progress

`TODO/INDEX.md` holds 120 entries: 68 open, 1 partial, 0 blocked, 51 done.

## State

- Released: `v0.7.0` at `a13014d` (CI 42266116185, nightly 42295275787,
  release 42623501324, the dry run of `publish.yml` 42623531702):
  keep-alive in the client of `pod-proxy` (T-077), `Failure::io_cause` and
  `Class::NotFound` (T-081). Each consumer is on `v0.6.0` (podssh, podnet
  `872a276`, podvm `685112c`); COORD-013 hands `v0.7.0` to each.
- M6: podvm took `pod-hostprobe` (podvm `8899c2e`); T-055 is done. Next
  in M6: T-061 and T-059.
- Untriaged: GitHub #20 to #34. GitHub #35 (cfb, 84 asks) and #36 (githome,
  12 asks) are routed (`docs/research/fleet/asks.md`): T-099 to T-120 and
  milestone M8. The sessions of podssh, podnet and podvm triage their own
  issues; the reconciliation follows.
- M8: `pod-broker` exists (T-099, calls C28 to C32); its TLS end is T-100.
- The names on crates.io wait for the owner (T-043, GitHub #1); the switch
  `crates-io` is false while Q1 stands.
- Keep this section short. Write the current state only; git keeps the
  history.

## Work order

Finish each entry, close it in place, change this page in the same commit,
then take the next one. Inside a milestone, take the entries in this
order. A crate is released on the day that its entry is done
(`docs/decisions.md`, Delivery and Who edits).

0. **Before M2** (call C26): T-100
   (`pod-broker`: the TLS end), T-112 (`pod-services` party filter) and T-108
   (`pod-confine`); then T-102 to T-105 (`pod-unit`, `pod-control`,
   `pod-release`, the acceptance registry); then the rest in milestone order.
1. **podvm first** (the sibling that is most behind; `docs/HANDOFF.md`):
   T-040 (the trials of
   `pod-tls` and `pod-proxy` in podvm); then M6 (item 7).
2. **M1, the transport:** T-085 first (it blocks the import of gsocket),
   then T-091, T-082 (defects), T-089, T-090 (data of `pod-services`),
   T-092, T-095 (CI), T-083; then T-084, T-093, T-094, T-096 (docs); T-016;
   T-050 (`pod-services`); T-041 when a crate or a consumer needs it.
3. **M2, the relay:** T-018, T-019, T-020, T-021, T-022, T-023, T-024,
   T-025, T-026.
4. **M3, the adapters:** T-027, T-028, T-029, T-030.
5. **M4, the roads:** T-031, T-034, T-035, T-033, T-032, T-036, T-037,
   T-038, T-039; then T-086, T-087, T-088 (deferred by the owner, 2026-11-09).
6. **M5, the upkeep:** T-040 (the trials of podvm) when podvm gave the
   commit of its rename; a "port" entry for each change that
   scripts/check-sources.py finds.
7. **M6, the crates of podvm** (`docs/research/podvm/m6-imports.md`, at
   podvm `main` on the day of each import): T-061 and T-059;
   T-060; T-056; T-057; T-058.
8. **M7, the widened scope of podvm:** T-062 after M6.
9. **M8, the fleet:** T-106 after T-025, T-028 and T-029; T-107 with M6;
   T-110, T-111 and T-120 with M2; T-113 to T-116 after the first release on
   crates.io (T-116 also needs test accounts); T-117 with M4; T-118 with M5;
   T-119 with M7.

## Calls made without a ruling

`docs/decisions.md`, "Calls made without a ruling", has each call (C1 to
C32), and the work goes on with it until the owner rules otherwise.

## Owner actions

| Entry | Action |
| --- | --- |
| T-053 | Reserve the 14 names of the crates of podvm (GitHub #1): `pod-hostprobe`, `pod-oci`, `pod-extract`, `pod-rootfs`, `pod-enter`, `pod-supervise`, `pod-guest`, `pod-microvm`, `pod-sud`, `pod-confine`, `pod-netlab`, `pod-pcap`, `pod-sandprobe`, `pod-probe` |
| New repositories (2026-11-09 ruling) | Create `podrunner`, `podnode`, `podinit`, `podlog`, `podwatch`, `podkey`, `podsnap`, `podrepl`, `podrelay-worker` and `podutils` |
| T-099, T-102, T-103, T-104 | Reserve `pod-broker`, `pod-unit`, `pod-control`, `pod-release`, `pod-gsr` (GitHub #1) |
| Each release | Tell the consumer sessions to take the crates |

## Questions for the owner

None open. The owner ruled Q4, Q7 and the questions of GitHub #35 and #36 on 2026-11-09 (`docs/decisions.md`). Q6 closes: the session dispatches each run itself (`docs/maintenance.md`, "A run of a workflow").

Write each question as `- **Qn.** QUESTION`, with the entries that it
blocks and a recommendation. Number the questions in sequence from Q8
(the owner ruled Q1 to Q7: `docs/decisions.md`).
