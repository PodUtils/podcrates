# Progress

`TODO/INDEX.md` holds 172 entries: 62 open, 3 partial, 0 blocked, 107 done.

## State

- The sessions of PodUtils paused on 2026-11-10 (the owner; GitHub #53).
  `docs/HANDOFF.md` has what is in flight and what waits.
- Last release: `v0.15.0` at `27a4ccd` (`docs/releases/v0.15.0.md`; CI
  42872309453, nightly 42872313856, adoption 42872309501, release
  42880337688): `pod-session`, `pod-wormhole`, `pod-hold`, the volumes of
  `pod-enter`. The hand-off is GitHub #54 (COORD-029); the takes of the
  consumers follow it. The dry run of `publish.yml` fails at the tags
  `v0.14.0` and `v0.15.0`; `e24f159` fixes it on `main` (dry run
  42880918874 green).
- The consumers: podnet is on `v0.15.0` (it took `pod-hold` at
  `b759937`); podssh is on `v0.14.0` (it took `pod-ws` and `pod-quic`);
  podvm is on `v0.12.0` (it took its seven crates of M6).
- Partial: T-020 `pod-session` and T-022 `pod-wormhole` (podssh's take),
  T-155 (podvm's move of calls). Open defects: T-131 (`pod-gsr` on Windows ARM64), T-149
  (qemu-user only), T-157 (public Trojan nodes; a second day of the watch).
- podrunner has a lead session (the ruling "podrunner online",
  2026-11-11; C49). podcrates and podrunner talk by direct messages.
  podssh and podvm are on break; only podnet answers a COORD issue (the
  ruling "The sessions of 2026-11-11"). M8 waits (the ruling "Priority").
- Each name of `docs/design.md`, section 4, is reserved on crates.io
  (`check-crates-io-names.py`); no crate goes to crates.io now (the owner,
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
The owner's rulings of 2026-11-11 ("podrunner online", "The sessions of
2026-11-11"): the work for podrunner and podnet comes first. An entry that
waits for podssh or podvm stays as it is; take the next entry.

1. **The defects of the consumers:** T-157 (four public Trojan nodes of
   podssh fail the live check).
2. **podrunner** (PodUtils/podrunner#1): T-163 (the schemas, for
   PodUtils/podrunner#17), T-162 (the tree kill, for PodUtils/podrunner#23),
   T-160 and T-161 (`pod-broker`, for PodUtils/podrunner#24), T-164 (the
   Actions hosts, for PodUtils/podrunner#30); T-165 after DR-4
   (PodUtils/podrunner#4); T-146 tracks the plan. Each new ask of the
   podrunner session becomes an entry here, in the order of its need.
3. **podnet:** T-169 (the dial state of `connect.rs`), T-170 (step 1:
   `packet.rs`; step 2 after podnet's COORD issue), from podnet's answer
   PodUtils/podnet#424; T-150, T-121 to T-124 (the asks of podnet).
4. **M6, the crates of podvm:** T-149; T-142. T-040 (the trials of
   podvm) goes with each import. podvm's open takes (PodUtils/podvm#194):
   `pod-relay` (after T-025, the blocking face), `pod-services` and
   `pod-todo`.
5. **M1, the transport:** T-171 (podssh's services, PodUtils/podssh#186); T-151 (the watch scripts that leave podssh and podnet), T-152 (VLESS over XHTTP); T-041 when a crate or a consumer needs
   it.
6. **M2, the relay:** T-020 (partial: podssh's take, #54), T-021,
   T-022 (partial: podssh's take, #54), T-023, T-024,
   T-025, T-026; then T-122, T-123, T-124, T-121, T-150 (the asks of podnet).
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
   The asks of PodUtils/podvm#227 for podrunner are in item 2.

## Calls made without a ruling

`docs/decisions.md`, "Calls made without a ruling", has each call (C1 to C48), and the work goes on with it until the owner rules otherwise.

## Owner actions

| Entry | Action |
| --- | --- |
| Each release | Tell the consumer sessions to take the crates |

## Questions for the owner

The questions of the CI plan (GitHub #47, T-146,
`docs/research/ci/podrunner-plan.md`, section 9). The owner named
podrunner on 2026-11-11 (C49 answers Q8 for phase 1). PodUtils/podrunner#4
to #8 hold the open decisions of the design review; the podrunner session
records their answers.

- **Q9.** Is the official .NET runner, as a payload in a podvm unit,
  accepted under "everything must be statically compiled"? Blocks T-146.
  Recommendation: yes: the delivered binaries are static; the runner is
  user content, as Node is.
- **Q10.** May `podrunner` serve a Docker API socket for each job, beside
  podvm's rule "no daemon"? Blocks phase 3 of T-146. Recommendation: yes:
  a library hosted by the product, alive for one job, each missing verb
  refused by name.
- **Q11.** A local mode (`podrunner run workflow.yml`)? Recommendation:
  not now.
- **Q12.** The GitHub terms for a third-party runner. Blocks phase 5 only.
  Recommendation: the official runner first; decide before phase 5.
- **Q13.** `docker build` and Dockerfile actions: refuse by name, or a
  rootless build? Recommendation: refuse by name first, then measure.
- **Q14.** The names `pod-dockerapi`, `pod-actions-proto`,
  `pod-actions-expr`, `pod-actions-exec`. Recommendation: accept, and
  reserve them with `check-crates-io-names.py`.

Write each question as `- **Qn.** QUESTION`, with the entries that it
blocks and a recommendation. Number the questions in sequence from Q15
(Q1 to Q7: `docs/decisions.md`; Q8: C49).
