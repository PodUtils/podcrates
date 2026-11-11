# Progress

`TODO/INDEX.md` holds 223 entries: 61 open, 4 partial, 4 blocked, 154 done.

## State

- `docs/HANDOFF.md` has what is in flight and what waits.
- Last release: `v0.24.0` at `228e2e3` (`docs/releases/v0.24.0.md`; the
  hand-off GitHub #73, COORD-048). The next release: `pod-relay` (T-025),
  T-209 and T-221 (`pod-confine`), T-222 (`pod-netstack`), T-216.
- The consumers and their tags:
  - podssh: `v0.24.0` (`2f4f292b`, PodUtils/podssh#192).
  - podnet: `v0.22.0` (`089e1c9`, PodUtils/podnet#447).
  - podvm: `v0.22.0` (`c9d5038`, PodUtils/podvm#262).
  - podrunner: `v0.24.0` (`6a01f4e`); its T-042 takes `pod-confine`, its
    T-048 takes `check_upstream`, `names_upstream` and the flagged rules
    of `pod-broker`.
- Takes that wait: podvm's take of `v0.23.0` (`pod-confine` for
  PodUtils/podvm@T-1658, T-108, #69 and #72; T-208 for
  PodUtils/podvm@T-1659) and of `v0.24.0` (T-206 for
  PodUtils/podvm@T-1662, #73); podnet's move to `v0.24.0` (#73).
- Partial: T-025 (`pod-relay`: the trial of podssh remains), T-040 (the
  trial of podvm), T-170 (step 2 waits for PodUtils/podnet@T-249), T-174
  (a Trojan node that passes the live check). Open defects: T-131
  (`pod-gsr` on Windows ARM64), T-204 (a random failure of
  `pod-session`), T-213 (`pod-confine`).
- podrunner and podssh have lead sessions that podcrates started (the
  rulings "podrunner online" and "Sessions online"; C49); they talk with
  podcrates by direct messages. podnet and podvm run in their own sessions
  and answer COORD issues. M8 waits (the ruling "Priority").
- Each name of `docs/design.md`, section 4, is reserved on crates.io
  (`check-crates-io-names.py`); no crate goes to crates.io now (the owner,
  2026-11-09).
- Blocked: T-217 to T-220 (`pod-git`): the owner's ruling "pod-git" (its
  own repository, deferred).
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
   transfer and the mailbox server of magic-wormhole) after T-027; T-150, T-121 to T-124 (the asks of podnet).
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
   T-123, T-124, T-121, T-150 (the asks of podnet); then T-197 and T-223
   (`pod-netstack`).
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

`docs/decisions.md`, "Calls made without a ruling", has each call (C1 to C65), and the work goes on with it until the owner rules otherwise.

## Owner actions

None. The hand-off of each release is a COORD issue `(to all)` and a
direct message to each lead session (`docs/maintenance.md`, "A release").

## Questions for the owner

None open.

Write each question as `- **Qn.** QUESTION`, with the entries that it
blocks and a recommendation. Number the questions in sequence from Q15
(Q1 to Q14: `docs/decisions.md`).
