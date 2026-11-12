# Progress

`TODO/INDEX.md` holds 323 entries: 85 open, 5 partial, 5 blocked, 228 done.

## State

- `docs/HANDOFF.md` has what is in flight and what waits.
- Last release: `v0.32.0` at `032e86f` (`docs/releases/v0.32.0.md`; ci
  43231610374, adoption 43231610238 with podvm's trial of `pod-relay`,
  release 43236171454; the hand-off GitHub #93, COORD-061): T-327 (the
  flush of the operator, PodUtils/podnet#502), `pod-link` (T-300, T-311),
  T-370 and T-329 (PodUtils/podssh#214, #213), T-322, T-323 to T-325, and
  the trial of podvm (T-326). Each consumer took `v0.31.0` (#90, closed).
- On `main` after the tag: `pod-netlab` (T-207, partial), `pod-warp`
  (T-028, partial).
- The consumers and their tags:
  - podssh: `v0.31.0` (`02650f1`; earlier `v0.30.0` at `ef3666f`, PodUtils/podssh#212): the MQTT brokers
    of `code::run` (PodUtils/podssh@T-737) and `syncthing-relay` (PodUtils/podssh@T-571).
  - podnet: `v0.32.0` (`fa6b5ba`, PodUtils/podnet#508; `pod-link` is its
    next take); its relay client
    is glue over `pod-relay` (T-026); its netns router holds each SYN with
    `pod_hold::syn_hold` (T-170).
  - podvm: `v0.32.0` (`15485d1b`, PodUtils/podvm#285; its relay over
    `pod_relay::blocking`, T-326; its 362 unit tests pass).
  - podrunner: `v0.32.0` (`997b001`; its token of `format()` reads `Template::parts`, PodUtils/podrunner@T-097); its local mode
    PodUtils/podrunner@T-046 takes `pod-actions-expr` and
    `pod-actions-workflow`); its T-042
    takes `pod-confine` through podvm's `--podvm-confine`, its T-048 takes
    `check_upstream`, `names_upstream` and the flagged rules of `pod-broker`.
- Takes that wait: podvm's check of T-208 against `lambda-al2023`, and
  its module set of T-206 with PodUtils/podvm@T-1662 (after `pod-netlab`,
  T-207, #70).
- Partial: T-026
  (`pod-relay`: podnet's capabilities and the plain mode are in the
  crate, with T-234; the trial of podnet on its `1abfa91` remains: no fix
  to port, the glue is not written, calls C101 to C103), T-040 (the
  trial of podvm). Open defects: T-131
  (`pod-gsr` on Windows ARM64), T-204 (a random failure of
  `pod-session`).
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

1. **The defects and asks of the consumers:** T-390 (podnet's A11,
   PodUtils/podnet#512); T-377 (`pod-turn`, GitHub #100, for
   PodUtils/podnet#485 and PodUtils/podssh@T-621, an agent of this
   session); T-378 (`pod-devtunnel`, GitHub #94 and PodUtils/podssh#216:
   the client side first, an agent of this session); T-330 (podvm's pair
   commands, after its take of the blocking facade, PodUtils/podvm#285;
   it waits for podvm).
2. **podrunner** (PodUtils/podrunner#1): no crate ask is open. T-146
   tracks the plan. Each new ask of the podrunner session becomes an
   entry here, in the order of its need. The next tag releases
   `Template::parts` (PodUtils/podrunner@T-089); T-280 (the live run of
   `pod-actions-expr` and `pod-actions-workflow` against GitHub) waits.
3. **podnet:** the chain to WARP as a network of podnet
   (PodUtils/podnet#449; about twenty podnet entries wait): T-028 (`pod-warp`
   with its `Dial` and `Datagrams`; partial: the registration, the identity,
   the doors, the session over HTTP/2, the network and the rules are in),
   then T-335, T-336, T-337, T-338 and T-339 (the rest of `pod-warp`), then
   T-029 (`pod-cloudflare`). podnet
   holds the files of the three crates to fixes from `7c5f87e`. T-214 and T-215 (the file
   transfer and the mailbox server of magic-wormhole) after T-027; T-121 and T-124 (the asks of podnet).
   podnet started its link workstream (PodUtils/podnet#480): T-300
   (partial: `pod-link` with the streams, the windows, the keepalive, the
   unix and the memory carriers is in; the release and the reserve of its
   name on crates.io remain; a link across a resume of `pod-session` is
   proven); T-310 (the ARQ) when podnet names a lossy carrier.
   The other asks of PodUtils/podnet#479 wait for podnet's start of each:
   T-301, T-302 (after T-035), T-304, T-305; the answer of ask A5 is in #88.
4. **M6, the crates of podvm:** T-207 (`pod-netlab`, partial: the knobs,
   the layout, the scenarios, the plan, the derivation and the judge are
   in; then T-345, the probes, and T-346, the world), PodUtils/podvm#260
   (PodUtils/podvm@T-1662 waits for it and for the module set of the
   modloop, which `pod-guest` has); T-347 after both;
   `exec_attached` of `pod-incus` when PodUtils/podvm@T-1722 asks for it. T-040 (the trials of podvm) goes with each import. podvm's open takes (PodUtils/podvm#194):
   `pod-relay` (the blocking face), `pod-services` and
   `pod-todo`. T-190 (a large blob as parallel ranges, PodUtils/podnet#437)
   after podvm asks for it.
5. **M1, the transport:** T-200 (a Bare node whose WebSocket passes, for PodUtils/podssh@T-740); T-151 (the watch scripts that leave podssh and podnet); T-041 when a crate or a consumer needs
   it; then T-189, T-192, T-193 (the proposals of PodUtils/podnet#437 for
   `pod-proxy` and `pod-core`).
6. **M2, the relay:** T-212; T-290 (the layer of
   `pod-relay` as a feature); then T-204 (a random failure of
   `pod-session`); then
   T-124, T-121 (the asks of podnet); then T-197
   (`pod-netstack`).
7. **M3, the adapters:** T-027, T-028, T-029, T-030.
8. **M4, the roads:** T-031, T-034, T-035, T-033, T-032, T-036, T-037,
   T-038, T-039.
9. **M5, the upkeep:** a "port" entry for each change that
   scripts/check-sources.py finds; T-199 after a release of snow on
   `chacha20poly1305` 0.11. The live calls of CI (`docs/live-calls.md`,
   calls C150 to C153), in the order of their gain: T-351 (a host that
   gives no answer to one runner reddens `services.yml`; partial: the
   scheduled runs after the merge prove it), T-371 (the same class for the
   DNS watches, `stun-binding` and the HTTP client), T-352 (`live.yml` each
   week; no host of another party in `ci.yml`), T-353 (`pod-oci` against
   distribution; ECR Public gives 429), T-354 (magic-wormhole's servers on
   each push), T-355 (the cache of the downloads), T-356 (sish and
   sandhole), T-357 (gs-netcat), T-358 (coturn). T-359 and T-360 go with
   T-028 and T-029 (item 3); T-361 goes with T-036 (item 8).
10. **M7, the widened scope of podvm:** T-062 after M6; T-241 (the permissions of Landlock ABI 12).
11. **M8, the fleet, and the other asks of #35 and #36: they wait** (the
   owner, 2026-11-09). T-100, T-102 to T-120 wait, unless a
   consumer needs one first (T-107 with M6). T-113 to T-116 wait for the first release on crates.io.
   The asks of PodUtils/podvm#227 for podrunner are in item 2. T-191 and
   T-194 (the proposals of PodUtils/podnet#437 for `pod-broker`, for the
   egress of podrunner) wait too; each goes to item 2 when podrunner asks
   for it.

## Calls made without a ruling

`docs/decisions.md`, "Calls made without a ruling", has each call (C1 to C95, and C100 to C112, C115 to C125, C150 to C153; C78, C79 and C96 to C99 are not used), and the work goes on with it until the owner rules otherwise.

## Owner actions

None. The hand-off of each release is a COORD issue `(to all)` and a
direct message to each lead session (`docs/maintenance.md`, "A release").

## Questions for the owner

None open.

Write each question as `- **Qn.** QUESTION`, with the entries that it
blocks and a recommendation. Number the questions in sequence from Q15
(Q1 to Q14: `docs/decisions.md`).
