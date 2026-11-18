# Progress

`TODO/INDEX.md` holds 746 entries: 135 open, 21 partial, 7 blocked, 583 done.

## State

- `docs/HANDOFF.md` has what is in flight and what waits.
- Last release: `v0.70.0` at `ed6d84c` (`docs/releases/v0.70.0.md`; ci
  44154512588, adoption 44154512580 and plants 44154512569 at `ed6d84c`,
  the release dry run 44206901563, the release 44207524079, publish
  44208190090): the asks and the outside QA of 2026-11-17 (T-857 to T-904,
  the rulings "Shared code" and "Quality first"). The hand-off is
  COORD-132. `v0.69.0` (`d85f2d2`) has COORD-130 (#220).
- The consumers and their tags (the details of each take are on its
  hand-off issue):
  - podssh: `v0.69.0` (`d32954d`, PodUtils/podssh#305: 3657 tests; its
    own chrono moved to jiff; `v0.68.0` in `94c01b0`, PodUtils/podssh#303: 3619 tests; the
    liveness offer and the headers of Access taken, its own opener of a
    Unix route gone; `Headers::in_cleartext` and `--access-host` on more
    commands are its T-718; `v0.67.0` in `a4b1436`; `v0.66.0` in
    `ae9cf1b`, PodUtils/podssh#296: a bump of the tag;
    `v0.65.0` in `843e3c5`, PodUtils/podssh#289: `pod-tor` and
    `pod-iroh` taken, their trials removed; next the layer on the HTTP road,
    its T-798, and its text, help and JSON on `pod-manual`, its T-790): it runs the one
    watcher and the sync of the issues; its lane passes in the ten labs
    that hold its classes (`podssh doctor`, `release-short.sh`, `ssh`, `cp`
    of 200 MiB), with the release asset of `podlab`; `pod-manual` is
    PodUtils/podssh@T-790; its installer on the core is
    PodUtils/podssh@T-793.
  - podnet: `v0.68.0` (`0d9de01`, `3a09c01`, PodUtils/podnet#597:
    webpki-roots 1.0.12, russh 0.65.2 with rsa 0.10.0; its `deny.toml`
    ignores no advisory; `replace_private` taken in `7fe60e0`; its
    `v0.4.0-beta.15` is on `v0.67.0`; `v0.67.0` in `71924ec`;
    `v0.66.0` in `060147b`; `v0.65.0` in
    `17fa8e6`: on `NameInUse` it takes the next stored pair (T-805); its help and
    manual come from `pod-manual` with no workaround of T-725; it runs the one
    watcher and the sync of the issues; its installer on
    `scripts/install/core.sh` is PodUtils/podnet@T-360).
  - podvm: `v0.68.0` (PodUtils/podvm#465: PodUtils/podvm#463 answered;
    its tests pass on the tag; `v0.67.0`, PodUtils/podvm#462; `v0.66.0` in `b1fa48f`, PodUtils/podvm#461:
    `pid_namespace: true` for each run of the namespace rung of
    `podvm-supervise`; its installer core is still of `v0.58.0`,
    PodUtils/podvm@T-1925).
  - podrunner: `v0.52.0` (`45bd964`, PodUtils/podrunner#169).
- Takes that wait: podvm's check of T-208 against `lambda-al2023`, and
  its module set of T-206 with PodUtils/podvm@T-1662 (after `pod-netlab`,
  T-207, #70).
- Partial: T-028 and T-339 (`pod-warp`), T-040 (the trials of podvm),
  T-207 (`pod-netlab`), T-351
  (`services.yml`), T-378 (`pod-devtunnel`), T-526 (the fix of `kcp`
  upstream).
  Open defects: T-131
  (`pod-gsr` on Windows ARM64), T-204 (a random failure of `pod-session`).
- Each session talks with podcrates by COORD issues alone, no direct
  message (the ruling "Tonight"). podrunner's session runs with podvm. M8
  waits (the ruling "Priority"). The asks of podswarm wait until the
  podswarm session starts, but for T-439, T-444, T-447 and T-448 (the
  ruling "podswarm", call C245).
- Each name of `docs/design.md`, section 4, is reserved on crates.io
  (`check-crates-io-names.py`: 94 names, 0 faults, 2026-11-13; T-387).
  No crate goes to crates.io now (the owner, 2026-11-09).
- Blocked: T-217 to T-220 (`pod-git`): the owner's rulings "pod-git" and
  "Renames" (`PodUtils/podforge`, deferred; #131, closed, gave the four
  entries to the owner's agent of podforge). T-538 (the attribution line
  of a comment on GitHub): Q16.
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

First: the next tag (the fixes of
PodUtils/podssh#262, on `main`). Then the ruling "Discipline": T-692
(`pod-lab`, part 1), T-700 (`pod-manual`, PodUtils/podnet#584; part 1
done, then the take of each consumer; its findings T-704 and T-705),
T-767 (a stalled resume of `pod-session`, PodUtils/podssh#283; partial: the far end's lines of each resume and of each wait that holds are in, the cause is open) with its finding T-768 (a late spare), T-703 (the issues against
the record) and T-702 (the installer). Then the move of podssh's waves 3 and 4 (the
ruling "The move of podssh", PodUtils/podssh#248): T-030, T-039; T-036. Then the link carriers of M2
for podnet (item 3c): T-526. Then the items below, in order.

1. **The two studies of 2026-11-16** (an archive of the owner each:
   the dig study and the ingest study, `docs/research/dig-study.md` and
   `docs/research/ingest-study.md`): T-812 (a measurement of the throughput
   of the hosted relays: podcrates has none), then T-814 (partial: a rank by measured rate); the rest of T-818 to
   T-830 by priority; T-832 waits for podswarm.
1. **The defects and asks of the consumers:** `pod-devtunnel` (GitHub #94
   and PodUtils/podssh#216: the client, the writes, the logins and the token
   store are in): T-386 (its live run); T-330 (podvm's pair
   commands, after its take of the blocking facade, PodUtils/podvm#285;
   it waits for podvm).
2. **podrunner** (PodUtils/podrunner#1): T-692 (`pod-lab`, PodUtils/podrunner#180; part 1 done, for the tag after `v0.53.0`; part 2 next), then T-696, T-693, T-694, T-695. T-146
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
   c. `v0.40.0` holds the poll, quic, udp, dns and icmp carriers of
      `pod-link` and the gate of E2-1. The tag after: T-419 (the facts of
      the file hosts from the live run of the file hosts, `live.yml`); T-526 (partial: the fix of `kcp`
      upstream).
   d. Then: T-802 (a NAT probe of `pod-turn`, #212, before T-499), T-496 (DNS stream), T-498 (a WireGuard peer), T-499 (NAT
      traversal with the relay), T-515 (the congestion window of the ARQ,
      with the first carrier over a shared path), T-803 (a parity datagram of the ARQ, #213, after T-515), T-514 (a stream-aware
      ARQ, when podnet asks), T-501 (`pod-turn` with RFC 6062),
      T-214 and T-215 (`pod-transit`, `pod-wormhole`).
   e. Then: T-030 (`pod-cloudflare`), T-302
      (`pod-egress`), T-039 (`pod-ts`), T-502 (a headscale harness).
   The rest of `pod-warp`: T-339 (partial: the trial patches and the
   live run), T-482, T-481, T-470, T-410, T-409; T-407 (the fuzz targets
   of the server face of `pod-proxy`). T-121 and T-124
   (the asks of podnet).
4. **M6, the crates of podvm:** T-207 (`pod-netlab`, partial: the knobs,
   the layout, the scenarios, the plan, the derivation and the judge are
   in; then T-345, the probes, and T-346, the world), PodUtils/podvm#260
   (PodUtils/podvm@T-1662 waits for it and for the module set of the
   modloop, which `pod-guest` has); T-347 after both, then T-348;
   `exec_attached` of `pod-incus` when PodUtils/podvm@T-1722 asks for it.
   T-649 (a FAT image writer, PodUtils/podvm#386) is not urgent.
   T-448 (a wrong fact of
   FUSE in `pod-enter`); T-444 (the probe rows of
   the namespace and FUSE rungs; the rows of podswarm alone wait). T-040
   (the trials of podvm) goes with each import. podvm's open takes
   (PodUtils/podvm#194): `pod-services` and `pod-todo`. T-190 (a large blob as parallel ranges, PodUtils/podnet#437)
   after podvm asks for it.
5. **M1, the transport:** T-200 (a Bare node whose WebSocket passes, for PodUtils/podssh@T-740); T-151 (the watch scripts that leave podssh and podnet); T-041 when a crate or a consumer needs
   it; then T-189, T-193 (the proposals of PodUtils/podnet#437 for
   `pod-proxy` and `pod-core`).
6. **M2, the relay:** the contract of `pod-link` from the study of zmux
   (`docs/research/zmux.md`): T-798, T-799; T-212; T-290 (the layer of
   `pod-relay` as a feature); then T-204 (a random failure of
   `pod-session`); then
   T-124 (the failover of the reverse road across the relays of several
   operators, the owner's ruling "Open questions"), T-121 (the asks of
   podnet); then T-197
   (`pod-netstack`).
7. **M3, the adapters:** T-028, T-030.
8. **M4, the roads:** T-131 (a defect of `pod-gsr` on Windows ARM64)
   first; then T-036 (with T-856, the vectors of `tokens.json`),
   T-039.
9. **M5, the upkeep:** T-789 (the nightly at `f309b07`), T-766, T-780 (a test that reads the pipes of its child
   after the child ends), T-781 (the findings of the study of zmux for the
   consumers), T-779; T-312 (a
   code fence that is not closed in the record). A "port" entry for each
   change that scripts/check-sources.py finds; T-199 after a release of snow on
   `chacha20poly1305` 0.11. The live calls of CI (`docs/live-calls.md`,
   calls C150 to C153), in the order of their gain: T-351 (a host that
   gives no answer to one runner reddens `services.yml`; partial: the
   scheduled runs after the merge prove it), T-371 (the same class for the
   DNS watches, `stun-binding` and the HTTP client), T-356 (sish and
   sandhole), T-357 (gs-netcat); coturn on the loopback (`scripts/ci/turn-tests.sh`)
   proves the STUN codec of `pod-proxy` and `pod-turn`. T-359 and T-360 go with
   T-028 (item 3); T-361 goes with T-036 (item 8).
10. **M7, the widened scope of podvm:** T-062 after M6, with T-447 (the
   direct boot of a Linux guest for `pod-microvm`); T-241 (the
   permissions of Landlock ABI 12); T-349 (a lab of `pod-netlab` on a
   user-space stack and with more than one guest).
11. **M8, the fleet, and the other asks of #35 and #36: they wait** (the
   owner, 2026-11-09; again 2026-11-13). T-490 (the server half of ECH,
   #130) waits too. The asks of podswarm that help podswarm alone
   (T-430 to T-437, T-440 to T-443, T-445, T-446, T-449, T-450) wait
   until the podswarm session starts (the ruling "podswarm"). T-100, T-102 to T-119 wait, unless a
   consumer needs one first. T-113 to T-116 wait for the first release on crates.io.
   The asks of PodUtils/podvm#227 for podrunner are in item 2. T-191 and
   T-194 (the proposals of PodUtils/podnet#437 for `pod-broker`, for the
   egress of podrunner) wait too; each goes to item 2 when podrunner asks
   for it. T-180 (`pod-mask`) waits with M8. T-217 to T-220 (`pod-git`)
   are blocked (the State).

## Calls made without a ruling

`docs/decisions.md`, "Calls made without a ruling", has each call (C1 to C77, C80 to C95, C100 to C112, C115 to C125, C130 to C142, C145 to C153, C155, C156, C160 to C181, C185 to C221, C225 to C248, C250 to C260, C262 to C285; the numbers between are not used), and the work goes on with it until the owner rules otherwise.

## Owner actions

- Reserve `pod-actions-store` (T-564), `pod-actions-results` (T-565),
  `pod-udf` (T-591), `pod-qga` (T-594), `pod-transit`, `pod-acme`,
  `pod-piping`, `pod-opentunnel` and `pod-derp` (the move of podssh) on crates.io at `0.0.0` with the repository of
  podcrates (the ruling "crates.io names"). A release by a git tag does
  not wait for them.
- The owner reserved `pod-devtunnel` and `podswarm` on crates.io (T-387).
- Send `docs/upstream/kcp-0.6.0-wrapping.patch` to the crate `kcp`
  upstream as a pull request (T-526).
- Report to `Lazarus404/xsockets` that `recverr: true` is an option that
  no Erlang/OTP release takes (C588); `scripts/ci/xturn-recverr.patch`
  removes it for the CI of T-631 until a release of xsockets drops it.
- For the live client of `pod-devtunnel` with no account (T-489, row L45
  of `docs/live-calls.md`): set the repository secrets
  `POD_DEVTUNNEL_TUNNEL` and `POD_DEVTUNNEL_PORT` (and
  `POD_DEVTUNNEL_ACCESS_TOKEN` for a tunnel with no anonymous access),
  host the tunnel (`devtunnel host -p 22 --allow-anonymous`), and tell
  the session: it dispatches `live.yml` with `measure=devtunnel`.

The hand-off of each release is a COORD issue `(to all)`
(`docs/maintenance.md`, "A release").

## Questions for the owner

- **Q16.** Does the word of the owner of 2026-11-11 in the `AGENTS.md` of
  PodUtils/podrunner ("a comment on GitHub can end with the 'Generated by'
  line of the harness; a commit cannot") apply to podcrates too? The
  ruling "Git" of 2026-11-05 here forbids each attribution line in a
  comment, and each comment of a session of podcrates has the line. It
  blocks T-538. Recommendation: yes, the same rule in each repository of
  PodUtils; write it in `docs/decisions.md` and in `AGENTS.md`.

The owner's ruling "podswarm" (2026-11-13, `docs/decisions.md`) settled
Q15 and the rulings R-01 to R-16.

Write each question as `- **Qn.** QUESTION`, with the entries that it
blocks and a recommendation. Number the questions in sequence from Q17
(Q1 to Q14: `docs/decisions.md`; Q15: the ruling "podswarm"; Q16: above).
