# Progress

`TODO/INDEX.md` holds 410 entries: 118 open, 7 partial, 4 blocked, 281 done.

## State

- `docs/HANDOFF.md` has what is in flight and what waits.
- Last release: `v0.39.0` at `4c2d17c` (`docs/releases/v0.39.0.md`; ci
  43399257316, adoption 43399257385, release 43405496549; the hand-off
  #137, COORD-077, closed: each consumer took it): T-508
  (PodUtils/podnet#524), T-506, T-510, T-310.
- The consumers and their tags:
  - podssh: `v0.39.0` (`5228b65`, PodUtils/podssh#229; its crates/podssh-warp is glue over `pod-warp` and holds fixes only (#135); its server of the HTTP road takes `http_road::window`; its
    PodUtils/podssh@T-755 is on its `main` over `pod-devtunnel`): the MQTT
    brokers of `code::run` (PodUtils/podssh@T-737) and `syncthing-relay`
    (PodUtils/podssh@T-571). The trial of `pod-warp` passes at `286e98d3`
    (T-413; its glue needs T-480, in the tag after `v0.36.0`).
  - podnet: `v0.39.0` (`306d80b`, PodUtils/podnet#526; its own fix of #524 dropped; its `src/warp/` is glue over `pod-warp` and holds fixes only (#135); its
    PodUtils/podnet@T-268 runs on the `tcp` carrier of `pod-link`; its
    relay client is glue over `pod-relay`; its netns router holds each SYN
    with `pod_hold::syn_hold`). The trial of `pod-warp` passes at
    `9c1fc4b` (T-412); the take waits for the freeze.
  - podvm: `v0.39.0` (`19892c50`, PodUtils/podvm#309; its channel of a guest runs on `pod-link`; its `v0.2.0-beta.8` is out; its PodUtils/podvm@T-1746 and PodUtils/podvm@T-1749 are low priority now; `pod-netlab` in its
    `podvm lab`; its relay over `pod_relay::blocking`, T-326; its fix of
    #102 is `655cf941`, PodUtils/podvm#289, in its `v0.2.0-beta.7`).
  - podrunner: `v0.39.0` (`69d7b14`, PodUtils/podrunner#84; its live gates start on a test organization, its DR-7): its local
    mode takes `pod-actions-expr` and `pod-actions-workflow`
    (PodUtils/podrunner@T-046); its T-042 takes `pod-confine` through
    podvm's `--podvm-confine`; its T-048 takes `check_upstream`,
    `names_upstream` and the flagged rules of `pod-broker`; its
    PodUtils/podrunner@T-096 waits for podvm `v0.2.0-beta.7`.
- Takes that wait: podvm's check of T-208 against `lambda-al2023`, and
  its module set of T-206 with PodUtils/podvm@T-1662 (after `pod-netlab`,
  T-207, #70).
- Partial: T-028 and T-339 (`pod-warp`), T-040 (the trials of podvm),
  T-107 (the report of `pod-hostprobe`), T-207 (`pod-netlab`), T-351 (`services.yml`), T-378 (`pod-devtunnel`).
  Open defects: T-307
  (the nightly at `0edd703`; the next green nightly proves it), T-131
  (`pod-gsr` on Windows ARM64), T-204 (a random failure of `pod-session`).
- Each session talks with podcrates by COORD issues alone, no direct
  message (the ruling "Tonight"). podrunner's session waits for podvm. M8
  waits (the ruling "Priority"). The asks of podswarm wait until the
  podswarm session starts, but for T-439, T-444, T-447 and T-448 (the
  ruling "podswarm", call C245).
- Each name of `docs/design.md`, section 4, is reserved on crates.io
  (`check-crates-io-names.py`) but `pod-devtunnel`: crates.io answers 404
  (2026-11-13; T-387). No crate goes to crates.io now (the owner,
  2026-11-09).
- Blocked: T-217 to T-220 (`pod-git`): the owner's rulings "pod-git" and
  "Renames" (`PodUtils/podforge`, deferred; an open issue of podcrates
  names the four entries for the owner's agent of podforge).
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

1. **The defects and asks of the consumers:** T-489 (a client of
   `pod-devtunnel` with no user token); T-486 (an overlay root in
   the namespace rung of `pod-enter`, PodUtils/podvm#294); T-381 (the options of
   RFC 8489 and RFC 8656 that `pod-turn` leaves out; coturn on the
   loopback proves the client on each push, so a test against coturn goes
   with each option), T-403 (the SHA-256 credentials against a server of
   another party: coturn keys with SHA-1 alone); T-478 (the client of
   the HTTP road never sends `close`); T-378 (`pod-devtunnel`, GitHub #94 and
   PodUtils/podssh#216: partial: the client side is in `v0.33.0` for
   PodUtils/podssh@T-755; the second part is the creation of tunnels, the
   logins and the token store; then T-386, T-387, T-385); T-330 (podvm's pair
   commands, after its take of the blocking facade, PodUtils/podvm#285;
   it waits for podvm).
2. **podrunner** (PodUtils/podrunner#1): no crate ask is open. T-146
   tracks the plan. Each new ask of the podrunner session becomes an
   entry here, in the order of its need. T-280 (the live run of
   `pod-actions-expr` and `pod-actions-workflow` against GitHub) waits.
3. **podnet** (its complete beta, PodUtils/podnet#521, COORD-064; the
   answer names a tag for each group, an estimate). podnet holds the files
   of `pod-relay`, `pod-warp` and `pod-cloudflare` to fixes from `7c5f87e`.
   a. `v0.37.0` and the freeze of `pod-warp` (#135) are done: podnet
      (`f6abdc3`) and podssh (`812c61c`) took the crate; their WARP code
      holds fixes only.
   b. `v0.38.0` holds the port rules of `pod-warp`, the TXT and CNAME of
      `pod-dns`, the exporter and the record layer of `pod-e2e`, the
      accept loop `pod_ws::serve`, and the carriers `ws` and `h2` of
      `pod-link`; `v0.39.0` holds the rules of podnet's WebSocket
      carrier, the fuzz target of the records, the ARQ of `pod-link` and
      the fix of PodUtils/podnet#524.
   c. The tag after: the poll carriers, the quic carrier and the udp
      carrier with KCP of `pod-link` (done); T-520 (the default of the
      congestion window of KCP, when podnet answers), T-521 (the fuzz
      target of the input of KCP), T-493 (DNS simplex), T-494 (S3
      simplex), T-497 (ICMP echo), T-500 (an up and a down carrier).
   d. Then: T-496 (DNS stream), T-498 (a WireGuard peer), T-499 (NAT
      traversal with the relay), T-517 and T-518 (a cap of the bytes and
      a fuzz target of the poll carriers), T-515 (the congestion window of the ARQ,
      with the first carrier over a shared path), T-514 (a stream-aware
      ARQ, when podnet asks), T-501 (`pod-turn` with RFC 6062), T-027,
      T-214 and T-215 (`pod-transit`, `pod-wormhole`).
   e. Then: T-029 and T-030 (`pod-cloudflare`), T-035 and T-302
      (`pod-egress`), T-031 (`pod-derp`), T-039 (`pod-ts`), T-034
      (`pod-acme`), T-502 (a headscale harness).
   The rest of `pod-warp`: T-482, T-481, T-470, T-410. T-121 and T-124
   (the asks of podnet).
4. **M6, the crates of podvm:** T-207 (`pod-netlab`, partial: the knobs,
   the layout, the scenarios, the plan, the derivation and the judge are
   in; then T-345, the probes, and T-346, the world), PodUtils/podvm#260
   (PodUtils/podvm@T-1662 waits for it and for the module set of the
   modloop, which `pod-guest` has); T-347 after both;
   `exec_attached` of `pod-incus` when PodUtils/podvm@T-1722 asks for it.
   T-107 (partial: the report of `pod-hostprobe` is in; the JSON Schema in
   `schemas/` for PodUtils/podvm@T-1714 remains); T-448 (a wrong fact of
   FUSE in `pod-enter`); T-444 (the probe rows of
   the namespace and FUSE rungs; the rows of podswarm alone wait). T-040
   (the trials of podvm) goes with each import. podvm's open takes
   (PodUtils/podvm#194): `pod-services` and `pod-todo`. T-190 (a large blob as parallel ranges, PodUtils/podnet#437)
   after podvm asks for it.
5. **M1, the transport:** T-200 (a Bare node whose WebSocket passes, for PodUtils/podssh@T-740); T-151 (the watch scripts that leave podssh and podnet); T-041 when a crate or a consumer needs
   it; then T-189, T-192, T-193 (the proposals of PodUtils/podnet#437 for
   `pod-proxy` and `pod-core`).
6. **M2, the relay:** T-212; T-290 (the layer of
   `pod-relay` as a feature); then T-204 (a random failure of
   `pod-session`); then
   T-124 (the failover of the reverse road across the relays of several
   operators, the owner's ruling "Open questions"), T-121 (the asks of
   podnet); then T-197
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
   DNS watches, `stun-binding` and the HTTP client), T-356 (sish and
   sandhole), T-357 (gs-netcat); coturn on the loopback (`scripts/ci/turn-tests.sh`)
   proves the STUN codec of `pod-proxy` and `pod-turn`. T-359 and T-360 go with
   T-028 and T-029 (item 3); T-361 goes with T-036 (item 8).
10. **M7, the widened scope of podvm:** T-062 after M6, with T-447 (the
   direct boot of a Linux guest for `pod-microvm`); T-241 (the
   permissions of Landlock ABI 12).
11. **M8, the fleet, and the other asks of #35 and #36: they wait** (the
   owner, 2026-11-09; again 2026-11-13). T-490 (the server half of ECH,
   #130) waits too. The asks of podswarm that help podswarm alone
   (T-430 to T-437, T-440 to T-443, T-445, T-446, T-449, T-450) wait
   until the podswarm session starts (the ruling "podswarm"). T-100, T-102 to T-120 wait, unless a
   consumer needs one first (T-107 is in item 4). T-113 to T-116 wait for the first release on crates.io.
   The asks of PodUtils/podvm#227 for podrunner are in item 2. T-191 and
   T-194 (the proposals of PodUtils/podnet#437 for `pod-broker`, for the
   egress of podrunner) wait too; each goes to item 2 when podrunner asks
   for it.

## Calls made without a ruling

`docs/decisions.md`, "Calls made without a ruling", has each call (C1 to C95, C100 to C112, C115 to C125, C130 to C142, C145 to C153, C155, C156, C160 to C181, C185 to C221, C225 to C229, C230 to C245; C78, C79, C96 to C99, C143, C144, C154 and C157 to C159 are not used), and the work goes on with it until the owner rules otherwise.

## Owner actions

- The owner reserves `pod-devtunnel` and `podswarm` on crates.io (the
  ruling "Open questions": in work, and no work waits for it).

The hand-off of each release is a COORD issue `(to all)`
(`docs/maintenance.md`, "A release").

## Questions for the owner

None open. The owner's ruling "podswarm" (2026-11-13,
`docs/decisions.md`) settled Q15 and the rulings R-01 to R-16.

Write each question as `- **Qn.** QUESTION`, with the entries that it
blocks and a recommendation. Number the questions in sequence from Q16
(Q1 to Q14: `docs/decisions.md`; Q15: the ruling "podswarm").
