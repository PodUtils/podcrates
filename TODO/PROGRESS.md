# Progress

`TODO/INDEX.md` holds 215 entries: 60 open, 4 partial, 0 blocked, 151 done.

## State

- `docs/HANDOFF.md` has what is in flight and what waits.
- Last release: `v0.24.0` at `228e2e3` (`docs/releases/v0.24.0.md`; ci
  43045626419, release 43053076977): the module set of a guest from its
  modloop in `pod-guest` (T-206), gcr.io, mirror.gcr.io and the NIC module
  sums in `pod-services` (T-211). The hand-off is GitHub #73 (COORD-048).
  `v0.23.0` is at `de770a2` (#72); `v0.22.0` at `93de04a` (#71: each
  consumer took it).
- The next release: `pod-relay` (T-025, partial), T-209 (`pod-confine`).
- The consumers: podssh is on `v0.22.0` (`75a50318`, PodUtils/podssh#191;
  it took `pod-e2e`; earlier `6de121b2`, PodUtils/podssh#189;
  the key of `localhost-run` pinned; it took `pod-wormhole` at `baa00a06` and
  `pod-session` at `6f78a7c7`); podnet is on `v0.22.0`
  (`089e1c9`, PodUtils/podnet#447: it took `pod-wg`; earlier `54bd958`, PodUtils/podnet#448; `pod-services-watch` at `0b945a3`;
  `pod-netstack` at `d23ce4e`; `pod-hold` at `b759937`,
  its `connect` and `packet` at `e4d4049`); podvm is on `v0.22.0` (`c9d5038`,
  PodUtils/podvm#262: its launcher sets `die_with_parent`, experiment 437;
  its incus tier calls `pod-incus`); podrunner is on `v0.22.0`
  (`82b7af0`; its tests 25 and 26 pass on the fake unit and on podvm; `pod-job` at `0457ed0`; its T-048 takes `check_upstream`,
  `names_upstream` and the flagged rules).
- Takes that wait: podvm's take of `pod-confine` for PodUtils/podvm@T-1658 (T-108, #69,
  #72).
- Partial: T-170 (step 2 waits for podnet), T-040 (the trial of podvm),
  T-025 (`pod-relay` imported and adapted, 356 tests; the trial of podssh
  remains). Open defects: T-131 (`pod-gsr` on
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
3. **podnet:** the chain to WARP as a network of podnet
   (PodUtils/podnet#449; about twenty podnet entries wait): T-025 (the
   trial of podssh), T-026 (podnet's relay client), then T-028 (`pod-warp`
   with its `Dial` and `Datagrams`), then T-029 (`pod-cloudflare`). podnet
   holds the files of the three crates to fixes from `7c5f87e`. T-170 step 2
   after PodUtils/podnet@T-249 (the router onto `pod-netstack`); T-201 after
   the root CI step of PodUtils/podnet@T-250; T-214 and T-215 (the file
   transfer and the mailbox server of magic-wormhole) after T-027; T-150,
   T-121 to T-124 (the asks of podnet).
4. **M6, the crates of podvm:** T-207 (`pod-netlab`), PodUtils/podvm#260
   (PodUtils/podvm@T-1662 waits for it and for the module set of the
   modloop, which `pod-guest` has); T-213 (the sockets that Landlock and
   the filter of `pod-confine` do not see);
   `exec_attached` of `pod-incus` when PodUtils/podvm@T-1722 asks for it. T-040 (the trials of podvm) goes with each import. podvm's open takes (PodUtils/podvm#194):
   `pod-relay` (after T-025, the blocking face), `pod-services` and
   `pod-todo`. T-190 (a large blob as parallel ranges, PodUtils/podnet#437)
   after podvm asks for it.
5. **M1, the transport:** T-174 (a Trojan node); T-200 (a Bare node whose WebSocket passes, for PodUtils/podssh@T-740); T-151 (the watch scripts that leave podssh and podnet); T-041 when a crate or a consumer needs
   it; then T-189, T-192, T-193 (the proposals of PodUtils/podnet#437 for
   `pod-proxy` and `pod-core`).
6. **M2, the relay:** T-025 (partial: the trial of podssh, step 1 of its
   Done section), T-212, T-026; then T-204 (a random failure of
   `pod-session`); then T-122,
   T-123, T-124, T-121, T-150 (the asks of podnet); then T-197.
7. **M3, the adapters:** T-027, T-028, T-029, T-030.
8. **M4, the roads:** T-031, T-034, T-035, T-033, T-032, T-036, T-037,
   T-038, T-039.
9. **M5, the upkeep:** a "port" entry for each change that
   scripts/check-sources.py finds; T-199 after a release of snow on
   `chacha20poly1305` 0.11.
10. **M7, the widened scope of podvm:** T-062 after M6.
11. **M8, the fleet, and the other asks of #35 and #36: they wait** (the
   owner, 2026-11-09). T-100, T-102 to T-120 wait, unless a
   consumer needs one first (T-107 with M6). T-113 to T-116 wait for the first release on crates.io.
   The asks of PodUtils/podvm#227 for podrunner are in item 2. T-191 and
   T-194 (the proposals of PodUtils/podnet#437 for `pod-broker`, for the
   egress of podrunner) wait too; each goes to item 2 when podrunner asks
   for it.

## Calls made without a ruling

`docs/decisions.md`, "Calls made without a ruling", has each call (C1 to C64), and the work goes on with it until the owner rules otherwise.

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
