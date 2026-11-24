# Handoff

The state of unfinished work, for a session with no memory of the last
one. Read this page first, then `AGENTS.md`, `TODO/PROGRESS.md` (the
state, the work order, the questions), `docs/maintenance.md` (the
procedures, the tools and the rules of the work), `docs/design.md` and
`docs/decisions.md`. Keep this page current: remove a line when its work
is done. Git and the record keep finished work.

## The first steps of the next session

1. Start the watcher under the Monitor tool and keep it running
   (`docs/maintenance.md`, "The issue loop"). Read the open COORD issues
   `(to podcrates)` of each repository of PodUtils. Each one maps to an
   entry (PodUtils/podvm#570, COORD-183, maps to T-1162; PodUtils/podssh#349,
   COORD-139, maps to T-1164 to T-1169; PodUtils/podssh#350, COORD-140, maps to
   T-1191 and T-1192; PodUtils/podvm#657, COORD-241, maps to T-1193 to T-1196). Map each new one
   in the same commit.
2. Coordinate the five sessions by COORD issues alone (the rulings
   "Coordinator" and "COORD only"): podcrates, podrunner (the flagship),
   podssh, podnet and podvm. The owner starts the sessions; send no direct
   message. COORD-136 (#227) gives the order of each session. Read each
   answer `(to podcrates)`, route each ask of podrunner to its owner at
   once, and edit #227 when the order changes.
3. Read the runs of the head of `main` (`ci`, `plants`, `adoption`,
   `secrets`, `deny`, `issue-sync`). A new red run needs an entry before
   other work. A plant that a run misses once with no change of its crate
   runs its test five times in its `.plant` (as
   `link-quic-drop-closes-at-once`). Then read the scheduled runs:
   - `nightly.yml` 45212967290 at `0f5dad6`: the jobs of Windows and macOS
     and `powerpc64le` pass (T-1089 is proven). `riscv64` and `loongarch64`
     fail in `pod-iroh` (`road.rs`, `share.rs`: T-916, T-938, Q19); `riscv64`
     also fails in `pod-control` (`tests/server.rs`: T-1161, partial: its
     hang guards are scaled; read the next riscv64 job). Open entries
     of the nightly: T-916 and T-938 (partial), T-789, T-766, T-1161, and
     T-1044 (`check-windows.sh` and `check-macos.sh` compile no doc test).
   - `upstream.yml` (job `sources`): the port of podrunner's changes of
     `pod-testkit` is done (T-1160); `scripts/check-sources.py` with a
     clone of podrunner `27a2e5d` shows 0 changes. Read the next daily run.
   - `services.yml`: the scheduled run 45159148662 failed; the dispatches
     45174182632 and 45174187730 passed. T-1034 (`temp-sh` gets 429 from a
     runner) is done: a limit of the host gets one more try. Read the next
     scheduled run.
   - `live.yml`: T-963 (partial; the second Nostr relay is `nos.lol`).
   - `publish.yml` 45321873395, the dry run on `v0.82.1`, passed.
   - `docs/research/hard-problems.md` has the hard open problems (H-01 to
     H-18). T-978 (partial) holds the findings that no other entry has.
4. The work of podcrates (`TODO/PROGRESS.md`, "Work order"): the red
   gates above, then the entries for the CI of githome and cfb (T-106,
   T-1118), then the asks of the consumers.
5. The fakes of the CI of podrunner and githome are release assets of
   each tag: `pod-fake-github` (T-921) and `pod-fake-githome` (T-948), for
   x86_64 and aarch64 musl in the signed `SHA256SUMS`, and an image of
   each on ghcr.io (`crates/pod-testkit/README.md`). When podrunner writes
   a step of githome's answers that differs from the fake, it is an entry
   here.
6. The channel of cfb and githome is #223. podcrates keeps one reply
   comment to each there (cfb: 6970315549; githome: 6970683028) and edits
   it, with the UTC time of each update and of each acknowledgement. cfb
   is offline until the CI of podrunner and githome is complete; it reads
   `docs/research/fleet/asks.md` and the block of #223 at its next start.
   cfb's A-X-18 (T-106, partial) waits for cfb's answer on #223.
7. T-767 (partial, PodUtils/podssh#283) and T-768: the client's line
   `resume::Note::Stall` (T-945) and the channel's line `e2e.wait` of
   `pod-e2e` (T-959) name the reader that stopped; podssh takes it
   (PodUtils/podssh@T-780) with no new ask of podcrates.
8. Then the work order of `TODO/PROGRESS.md`.
9. Run `scripts/agent/check.sh` with a short `TMPDIR` (for example
   `TMPDIR=/tmp`): a long one makes the socket paths of `pod-guest` too
   long for `sun_path` (T-920).
10. Rules of the session that the documents do not repeat:
    - A push of this account starts the runs of Actions. Dispatch a
      workflow with the GitHub MCP tool (`actions_run_trigger`); no request
      `[gh action] requested` is needed.
    - Delegate by the ruling "Delegation": Opus agents (medium or high
      effort) implement entries; Sonnet agents (high effort) do housekeeping,
      recon and basic fixes; the session coordinates and reviews.
    - The usage limit can stop an agent in a build. Tell each agent to
      commit early on its local branch (`docs/maintenance.md`, "Rules of
      the work").
    - Agents share one cargo lock (`flock`) and each builds in its own
      `CARGO_TARGET_DIR`. A full check needs 22 GB free: read `df -h /`
      first.
    - A pull request from outside the team gets CI runs with the state
      `action_required`. Review its code before you approve or dispatch
      its CI (the ruling "Strangers").

## Release: v0.82.1

`v0.82.1` (`0d7832b`, `docs/releases/v0.82.1.md`) is released: the GitHub
release, the signed `SHA256SUMS`, the assets and the images of
`ghcr.io/podutils` exist. It carries `v0.82.0` (`51c09390`,
`docs/releases/v0.82.0.md`, tagged without `release.yml`, so with no
release and no images) and each change of `main` before the tag.

The hand-off COORD-152 (#257) is open (call C780). It closes when each
consumer names `v0.82.1` or a later tag:

- podrunner names `v0.82.1` (PodUtils/podrunner#348, closed): its unit of
  the system calls `service_account()` (T-1110), and its report gives the
  reason of each absent time (T-1098).
- podvm names `v0.82.1` (PodUtils/podvm#656, COORD-240): `18160173` takes
  the tag (`pod_enter::joinable` in each join, `Plan::relay_title`,
  `pod_enter::title`), and `39a85246` takes `pod_hostprobe::usage`. The
  second blocks of `pod-enter` and `pod-hostprobe` in `SOURCES` have the
  `taken` lines, and T-1124 is done.
- podssh names `v0.82.1` (PodUtils/podssh#347, COORD-138, commit
  `17999753`). Its ask of a longer bound for the name of a unit is T-1182.
- #257 names the takes of podrunner, podvm and podssh. Amend it at the take
  of podnet.
- podnet names `v0.82.0`. Its asks of #257: T-1117 (the limits 110 s and
  210 s in its documents). #257 stays open until podnet names `v0.82.1` or
  a later tag; it can close then.

COORD-153 (#258) answers PodUtils/podssh#346: the transfer codec goes into
`pod-term` (T-1144, open). No tag has it.

### On `main` after `v0.82.1` (for the next tag)

- podnet's QA pass (PodUtils/podnet#707): T-1125 to T-1134. T-1127 changes
  the field type of `pod-wormhole` `Failed::Unreachable` to `Missed`: a
  caller that binds it as a `String` adds `.to_string()`.
- podvm's asks (PodUtils/podvm#651): T-1137 to T-1142. podvm must match
  `ErrorKind::TooLarge` of `pod-incus` (T-1143).
- `pod-natprobe` (T-802, from PodUtils/podcrates#256, which is closed as
  superseded by `f9abd7bb`), with T-1151 to T-1154. The kind `stun` of
  `pod-services` has three default servers (call C923): the doctor of
  podssh takes each server by rank, and its test
  `the_variable_names_the_servers_or_turns_the_line_off` pins two. The
  hand-off of the next tag names this change to podssh by a COORD message.
- T-1160: `pod-fake-githome` answers `assigned` with the fork fact of the
  real job alone (else `unknown`); the text of `head_is_fork` in
  `job-offer.v1`, `source-reply.v1` and `source-request.v1` changed.
- T-1164: the DNS listener of `pod-link` answers each query of its zone
  that it cannot take (NOERROR with no record and AA; REFUSED out of the
  zone; NOTIMP for another opcode). `pod_dns::txt` has `empty_answer`,
  `set_authoritative`, `RCODE_NOERROR`, `RCODE_NOTIMP` and
  `Answer.authoritative`, a new field of a public struct: a consumer that
  builds `Answer` with all its fields must add it. A grep of `Answer {` in
  podnet and podssh finds no such build.

### What the next hand-off carries

- podrunner: no open ask. T-1160 changes the answer of
  `pod-fake-githome` to `assigned` (the fork fact of the real job alone).
- podvm: T-1143, T-1158 (podvm removes its work-arounds at the tag of
  T-1155 to T-1157).
- podnet: T-1117 (documents), T-1118 (`release-index.json`).
- podssh: the STUN note above, T-1182 (the bound of a unit name), T-1095
  (the order of the pseudo-fields of the HTTP/2 request of `pod-ws`,
  PodUtils/podssh#340), T-1103 (iroh 1.5).
- Asks of podvm and podssh that no one has started: T-1155, T-1156 and
  T-1157 (`pod-incus`, PodUtils/podvm#653), T-1144 (`pod-term`), T-1165 to
  T-1171 (`pod-link`, the DNS carrier; PodUtils/podssh#349).
- Entries of podcrates that no one has started: T-1162
  (the answer to PodUtils/podvm#570), T-1163 (two more readers of
  `Retry-After`), T-1115 (the copies of the bind at a
  long path), T-1135 (`pod-cloudflare` says a cause two times), T-1146 (the
  comments of `pod-natprobe` are not in ASD-STE100), T-1150 (a watch of the
  facts of RFC 5780 of `easybell-stun`).
- T-963 is partial. T-975 and T-977 (podvm's asks #602 and #604; Q18 blocks
  their release part) are open.

## Unfinished work

- `origin/wip/t1073` is merged into `main` (T-1073, T-1070). The proxy
  refuses `git push origin --delete`, so the owner deletes the branch on
  GitHub (`TODO/PROGRESS.md`, "Owner actions"). No other `wip/` branch
  exists.

## What waits

- podvm: T-330 (its pair commands); T-207 (`pod-netlab`, #70) for
  PodUtils/podvm@T-1662.
- podswarm: its asks wait until its session starts, but for T-444, T-447
  and T-448 (the ruling "podswarm").
- `pod-git` (T-217 to T-220) is blocked (`PodUtils/podforge`, deferred).
  The rest of M8 waits (the ruling "Priority"); the entries for the CI of
  githome and cfb are in the work order.
- T-386 (the live run of `pod-devtunnel`) waits for the owner's token and
  tunnel (`TODO/PROGRESS.md`, "Owner actions").
