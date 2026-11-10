# Progress

`TODO/INDEX.md` holds 172 entries: 62 open, 4 partial, 0 blocked, 106 done.

## State

- The sessions of PodUtils paused on 2026-11-10 (the owner; GitHub #53).
  `docs/HANDOFF.md` has what is in flight and what waits.
- Last release: `v0.15.0` at `27a4ccd` (`docs/releases/v0.15.0.md`; CI
  42872309453, nightly 42872313856, adoption 42872309501, release
  42880337688): `pod-session`, `pod-wormhole`, `pod-hold`, the volumes of
  `pod-enter`. The hand-off is GitHub #54 (COORD-029); the takes of the
  consumers follow it. The dry run of `publish.yml` fails at the tags
  `v0.14.0` and `v0.15.0`; `e24f159` fixes it on `main`.
- The consumers: podssh and podnet are on `v0.14.0` (both took `pod-ws`
  and `pod-quic`); podvm is on `v0.12.0` (it took its seven crates of M6).
- Partial: T-020 `pod-session` and T-022 `pod-wormhole` (podssh's take),
  T-166 `pod-hold` (podnet's take), T-155 (podvm's move of calls). Open defects: T-131 (`pod-gsr` on Windows ARM64), T-149
  (qemu-user only), T-157 (public Trojan nodes; a second day of the watch).
- M8 and the podrunner entries (T-146, T-160 to T-165) wait for the
  owner (the rulings "Priority" and "CI tracker"; DR-1, DR-2).
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

1. **The defects of the consumers:** T-157 (four public Trojan nodes of
   podssh fail the live check).
2. **M6, the crates of podvm:** T-149; T-142. T-040 (the trials of
   podvm) goes with each import. podvm's open takes (PodUtils/podvm#194):
   `pod-relay` (after T-025, the blocking face), `pod-services` and
   `pod-todo`.
3. **M1, the transport:** T-171 (podssh's services, PodUtils/podssh#186); T-166 (partial: podnet's take, #54); T-169 and T-170 (after podnet's view); T-151 (the watch scripts that leave podssh and podnet), T-152 (VLESS over XHTTP); T-041 when a crate or a consumer needs
   it.
4. **M2, the relay:** T-020 (partial: podssh's take, #54), T-021,
   T-022 (partial: podssh's take, #54), T-023, T-024,
   T-025, T-026; then T-122, T-123, T-124, T-121, T-150 (the asks of podnet).
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
   The asks of PodUtils/podvm#227 wait for the owner's decisions DR-1 (name
   the plan of GitHub #47) and DR-2 (the place of the code): T-160 and
   T-161 (`pod-broker`), T-162 (`pod-supervise`), T-163 (the schemas),
   T-164 (`pod-services`) and T-165 (`pod-tls`, also DR-4). The owner can
   lift the wait for one of them.

## Calls made without a ruling

`docs/decisions.md`, "Calls made without a ruling", has each call (C1 to C48), and the work goes on with it until the owner rules otherwise.

## Owner actions

| Entry | Action |
| --- | --- |
| Each release | Tell the consumer sessions to take the crates |

## Questions for the owner

The questions of the CI plan (GitHub #47, T-146,
`docs/research/ci/podrunner-plan.md`, section 9). They wait with #47 (the
owner, 2026-11-10, "CI tracker"): do not ask them before the owner names
#47 again.

- **Q8.** Lift the wait (the ruling "Priority") for `podrunner` phase 1
  and the crate `pod-dockerapi`, after the podvm items of phase 1?
  Blocks T-146. Recommendation: yes.
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
blocks and a recommendation. Number the questions in sequence from Q8
(the owner ruled Q1 to Q7: `docs/decisions.md`).
