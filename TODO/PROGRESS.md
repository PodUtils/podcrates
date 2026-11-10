# Progress

`TODO/INDEX.md` holds 157 entries: 58 open, 2 partial, 0 blocked, 97 done.

## State

- Released: `v0.13.0` at `947603a` (CI 42797326140, nightly 42797327113,
  release 42802815017): `pod-ws`. COORD-024 (GitHub #49) hands it to
  podssh and podnet. podvm took `v0.12.0` (`5e5629a`,
  PodUtils/podvm#226); podssh and podnet are on `v0.11.0`. Open: T-131
  (`pod-gsr` on Windows ARM64), T-149 (qemu-user only), T-154 (timed tests
  under an emulator).
- M6: podvm took each of its seven generic crates: `pod-hostprobe`,
  `pod-supervise`, `pod-guest`, `pod-enter` (podvm `423e817`), `pod-oci`
  (`20ea781`), `pod-extract` and `pod-rootfs` (`0682c0c`,
  PodUtils/podvm#225). No trial of podvm is open.
- M1: `pod-services-watch` exists (T-050, call C42): `services.yml` runs
  each watch of `pod-services` each day, and `scripts/services/` holds the
  measurements; the trial of podnet names `v0.14.0`.
- M1: `pod-ws` exists (T-016 partial, call C41): the trials of podssh
  (`2f8e0bb`) and podnet (`22d99a1`) pass and name `v0.13.0`, the next
  tag after `v0.12.0`. The release and the two takes remain; from the
  import, podssh's `podssh-ws` WebSocket files and podnet's `src/ws.rs`
  take fixes only (`crates/pod-ws/SOURCES`).
- GitHub #20 to #36 are in the record; #35 and #36 are routed
  (`docs/research/fleet/asks.md`). podnet's asks for `pod-relay`, `pod-dns`
  and `pod-services` are T-121 to T-127 and T-150 (PodUtils/podnet#413).
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

1. **The defects of the consumers:** T-157 (four public Trojan nodes of
   podssh fail the live check).
2. **M6, the crates of podvm** (`docs/research/podvm/m6-imports.md`, at
   podvm `main` on the day of each import): T-142; T-149; T-155. T-040 (the
   trials of podvm) goes with each import.
3. **M1, the transport:** T-016 (partial: the
   takes of podssh and podnet, `adoption/{podssh,podnet}/pod-ws.*`); T-151 (the watch scripts that leave podssh and podnet), T-152 (VLESS over XHTTP); T-041 when a crate or a consumer needs
   it.
4. **M2, the relay:** T-019, T-020, T-021, T-022, T-023, T-024,
   T-025, T-026; then T-122, T-123, T-124, T-121, T-150 (the asks of podnet).
5. **M3, the adapters:** T-027, T-028, T-029, T-030.
6. **M4, the roads:** T-031, T-034, T-035, T-033, T-032, T-036, T-037,
   T-038, T-039.
7. **M5, the upkeep:** T-154 (the timed tests under an emulator); a "port" entry for each change that
   scripts/check-sources.py finds.
8. **M7, the widened scope of podvm:** T-062 after M6.
9. **M8, the fleet, and the other asks of #35 and #36: they wait** (the
   owner, 2026-11-09). T-100, T-102 to T-120 wait, unless a
   consumer needs one first (T-108 `pod-confine` for PodUtils/podvm@T-1658, T-107
   with M6). T-113 to T-116 wait for the first release on crates.io.

## Calls made without a ruling

`docs/decisions.md`, "Calls made without a ruling", has each call (C1 to
C43), and the work goes on with it until the owner rules otherwise.

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
