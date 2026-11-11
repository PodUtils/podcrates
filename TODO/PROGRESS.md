# Progress

`TODO/INDEX.md` holds 197 entries: 56 open, 2 partial, 0 blocked, 139 done.

## State

- `docs/HANDOFF.md` has what is in flight and what waits.
- Last release: `v0.21.0` at `ada3699` (`docs/releases/v0.21.0.md`; ci
  42983162246, plants, adoption, release 42989260695, the dry run of
  `publish.yml` 42989462396): `pod-incus` for podvm (T-179), one store and
  one pin at a time in `pod-core` and `pod-identity` (T-195), the socket of
  `pod-broker` with its mode at once (T-196). The hand-off is GitHub #65
  (COORD-040); podvm answers on #63.
- The consumers: podssh is on `v0.20.0` (`0c40ed3e`: the key of
  `localhost-run` pinned; it took `pod-wormhole` at `baa00a06` and
  `pod-session` at `6f78a7c7`); podnet is on `v0.20.0`
  (`d23ce4e`: `pod-netstack`, PodUtils/podnet#445; `pod-hold` at `b759937`,
  its `connect` and `packet` at `e4d4049`); podvm is on `v0.19.0` (`2c2b8d2`, its
  `--inhibit-cache` from `LoaderPlan::inhibit_cache`; PodUtils/podvm#249,
  read by podcrates); podrunner is on `v0.19.0`
  (after its `6f24681`; `pod-job` at `0457ed0`; its T-048 at `6f24681`
  takes `check_upstream`, `names_upstream` and the flagged rules).
- Partial: T-170 (step 2 waits for podnet), T-040 (the trial of podvm). Open defects: T-131 (`pod-gsr` on
  Windows ARM64), T-149 (qemu-user only).
- podrunner and podssh have lead sessions that podcrates started (the
  rulings "podrunner online" and "Sessions online", 2026-11-11; C49); they
  talk with podcrates by direct messages. podnet and podvm run in their
  own sessions and answer COORD issues. M8 waits (the ruling "Priority").
- Each name of `docs/design.md`, section 4, is reserved on crates.io
  (`check-crates-io-names.py`) but the four that GitHub #1 lists under "To
  register" for the owner's agent (`pod-job`, `pod-actions-workflow`,
  `pod-incus`, `pod-mask`); no crate goes to crates.io now (the owner,
  2026-11-09).
- Keep this section short. Write the current state only; git keeps the
  history.

## Work order

Finish each entry, close it in place, change this page in the same commit,
then take the next one. Inside a milestone, take the entries in this
order. A crate is released on the day that its entry is done
(`docs/decisions.md`, Delivery and Who edits).

The owner's ruling "Priority" (2026-11-09): podvm, podnet and podssh come
first; the asks of cfb and githome (M8) wait.
The owner's ruling "podvm online" (2026-11-10): write to podvm only by a
COORD issue of podcrates titled `(to podvm)`; no direct message.
The owner's rulings of 2026-11-11 ("podrunner online", "Sessions
online"): the work for podrunner and podnet comes first, then podssh and
podvm. An entry that waits for another session stays as it is; take the
next entry.

1. **The defects of the consumers:** none open.
2. **podrunner** (PodUtils/podrunner#1): no crate ask is open. T-146
   tracks the plan. Each new ask of the podrunner session becomes an
   entry here, in the order of its need.
3. **podnet:** T-170 step 2 after podnet's T-174 (the netns router); T-150, T-121 to T-124 (the asks of
   podnet).
4. **M6, the crates of podvm:** podvm's take of `pod-incus` (`v0.21.0`,
   GitHub #63 and #65; its PodUtils/podvm@T-1722). T-040 (the trials of podvm) goes with each import. podvm's open takes (PodUtils/podvm#194):
   `pod-relay` (after T-025, the blocking face), `pod-services` and
   `pod-todo`. T-190 (a large blob as parallel ranges, PodUtils/podnet#437)
   after podvm asks for it.
5. **M1, the transport:** T-174 (a Trojan node); T-151 (the watch scripts that leave podssh and podnet); T-041 when a crate or a consumer needs
   it; then T-189, T-192, T-193 (the proposals of PodUtils/podnet#437 for
   `pod-proxy` and `pod-core`).
6. **M2, the relay:** T-021, T-024, T-025, T-026; then T-122,
   T-123, T-124, T-121, T-150 (the asks of podnet); then T-197.
7. **M3, the adapters:** T-027, T-028, T-029, T-030.
8. **M4, the roads:** T-031, T-034, T-035, T-033, T-032, T-036, T-037,
   T-038, T-039.
9. **M5, the upkeep:** a "port" entry for each change that
   scripts/check-sources.py finds.
10. **M7, the widened scope of podvm:** T-062 after M6.
11. **M8, the fleet, and the other asks of #35 and #36: they wait** (the
   owner, 2026-11-09). T-100, T-102 to T-120 wait, unless a
   consumer needs one first (T-108 `pod-confine` for PodUtils/podvm@T-1658, T-107
   with M6). T-113 to T-116 wait for the first release on crates.io.
   The asks of PodUtils/podvm#227 for podrunner are in item 2. T-191 and
   T-194 (the proposals of PodUtils/podnet#437 for `pod-broker`, for the
   egress of podrunner) wait too; each goes to item 2 when podrunner asks
   for it.

## Calls made without a ruling

`docs/decisions.md`, "Calls made without a ruling", has each call (C1 to C50), and the work goes on with it until the owner rules otherwise.

## Owner actions

| Entry | Action |
| --- | --- |
| Each release | Tell the consumer sessions to take the crates |

## Questions for the owner

None open. The owner answered Q8 to Q14 and the decisions of the
podrunner design review on 2026-11-11 (`docs/decisions.md`: C49, the rows
"podrunner DR-4" to "Q11").

Write each question as `- **Qn.** QUESTION`, with the entries that it
blocks and a recommendation. Number the questions in sequence from Q15
(Q1 to Q14: `docs/decisions.md`).
