# Handoff

The state of unfinished work, for a session with no memory of the last
one. Read this page first, then `AGENTS.md`, `TODO/PROGRESS.md` (the
state, the work order, the questions), `docs/maintenance.md` (the
procedures, the tools and the rules of the work), `docs/design.md` and
`docs/decisions.md`. Keep this page current: remove a line when its work
is done. Git and the record keep finished work.

## The first steps of the next session

1. Start the two watchers, `scripts/agent/watch.py` and
   `scripts/agent/runs.py`, as background processes with a log, and keep a
   Monitor on each log (the ruling "Watchers"; `docs/maintenance.md`,
   "Start of a session"). Resume from this page, and work until each task
   is done or blocked on a thing out of the control of the session (the
   ruling "Work until done"). Read the open COORD issues
   `(to podcrates)` of each repository of PodUtils. Each one maps to an
   entry (PodUtils/podvm#570, COORD-183, maps to T-1162; PodUtils/podssh#349,
   COORD-139, maps to T-1164 to T-1169; PodUtils/podssh#350, COORD-140, maps to
   T-1191 and T-1192; PodUtils/podvm#657, COORD-241, maps to T-1193 to T-1196; PodUtils/podvm#659, COORD-243, maps to T-1205; PodUtils/podnet#709, COORD-117, maps to T-1209; PodUtils/podssh#351, COORD-141, maps to T-1217 to T-1222; PodUtils/podssh#352, COORD-142, maps to T-1235 to T-1238). Map each new one
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
   - `services.yml`: the scheduled run 45394042840 failed (T-1223, done):
     the relay WMYMARB moved to a new address, and `node-trojan-45-63-30-142`
     left. `temp-sh` held (T-1034). `node-trojan-64-176-7-205` and
     `freedos-image` had a first no answer; read the next scheduled run
     (T-1096 reopens when `freedos-image` times out again).
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

The hand-off COORD-152 (#257) is closed (call C780): each consumer names
`v0.82.1`. Its Result line names the four takes:

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
- podnet names `v0.82.1` (PodUtils/podnet#708, COORD-116): `a0b727c`
  takes the tag in each manifest, each lock, `scripts/install/SOURCES` and
  `issue-sync.yml`. `refusals` of `hop.rs` reads the typed `Refused` of the
  last leg and the text of each leg (T-1116), and `pod_testkit::need_route!`
  replaces its copy in `tests/test_host/mod.rs` (T-1097): the first block of
  `pod-testkit` in `SOURCES` has the `taken` line. T-1117 is partial:
  `docs/tunnels.md` of podnet and its copy in `llms-full.txt` still give
  130 s and 280 s.

COORD-153 (#258) answers PodUtils/podssh#346: the transfer codec goes into
`pod-term` (T-1144, done on `main`; the feature `transfer`). No tag has it.

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

- T-1155 to T-1157 (`pod-incus`, PodUtils/podvm#653): an exec that ends
  with 127 or 126 gives its code (and its logs for `exec_recorded`) from
  the operation (`Client::exec_end`); `Error::operation()` names the
  operation of a failed wait; `Settings::answer_timeout` (default 30 s)
  bounds each silence of an answer (`ErrorKind::AnswerTimeout`, with
  `AnswerPhase`); after a failed write of a body the answer of incusd is
  read within a short bound. `Settings` has a new public field: a consumer
  that builds it with all its fields adds it (podvm uses `Settings::new`).

- T-1170 and T-1165 (`pod-link`, the DNS carrier; PodUtils/podssh#349): each
  answer keeps the bytes of the question (0x20) and clears RA
  (`pod_dns::txt::answer_to`, `set_no_recursion`); a new session id takes a
  handshake slot only for a part of a ClientHello, and a poll of a new id
  gets an empty answer and starts no session. `Dns::handshake_quiet`
  (default 5 s) drops a session with no query before its handshake ends:
  a caller whose dialers set `poll_max` above 5 s sets it above that on its
  listeners.

- T-1087 and T-1088 (`pod-extract`, PodUtils/podvm#643): a GNU sparse file
  of PAX (versions 0.0, 0.1 and 1.0) extracts with its holes (the module
  `sparse`, bounded maps); a `LIBARCHIVE.xattr.` record sets its attribute
  where no `SCHILY.xattr.` record of the same name is there. `pax::Member`
  has a new public field `sparse` (podvm builds no `Member`).

- T-1191, T-1192 and T-1144 (`pod-term`, PodUtils/podssh#350 and #346):
  `modes::Modes` (`feed`, `restore`; the mouse level and the encoding as
  xterm and tmux hold them, not as #350 words them; no DECSTR),
  `Ring::modes_at`, `pod_testkit::screen::Screen::modes()`, and the 7-bit
  transfer codec `pod_term::transfer` behind the feature `transfer` (podssh
  names `features = ["transfer"]`). New items only. T-1198 asks podssh about
  two gaps of its design (the `KEPT` that ends a session, the wait of the
  far sender during the offer).

- T-1205 (`pod-hostprobe`, PodUtils/podvm#659, P1 for
  PodUtils/podrunner#350): `usage::Mounts` (`of`, `of_with`, `of_host`,
  `meter`) gives the cgroup directories of the process from
  `/proc/self/mountinfo`, and `cgroup::CGROUP_SUPER_MAGIC`. Where the mount
  at `/.podvm/.meter` fails, podvm opens the meter with
  `Mounts::of_host()?.meter()` and removes its copy `incus/usage/mounts.rs`.
- T-1213 (PodUtils/podvm#661, COORD-245): `scripts/agent/runs.py`, the
  shared watcher of the Actions runs of a repository (it reads through
  `watch.py`; `watch-runs.sh` is removed). podvm and podnet take it at the
  next tag in place of their copies (podvm's `runs-watch.py`, podnet's
  `watch-runs.sh` and `runs.py`).

- T-1182 (`pod-unit`, PodUtils/podssh#347): `Unit::NAME_MAX` (200) bounds a
  unit name and label; secret names and the state directory keep 64. podssh
  removes its own length check at the tag.
- T-1209 (`pod-core`, PodUtils/podnet#709): `pod_core::files::socket_answers`
  (three connects, 50 ms apart) for `pod-control`, `pod-link` and `pod-warp`;
  `pod_warp::find::answers` is removed (podnet does not call it);
  `pod_testkit::held::hold_until_exec`. podnet removes its own
  `socket_answers` and the rule in `claim_unix_path` at the tag.

- T-1235 and T-1237 (`pod-core`, `pod-release`, PodUtils/podssh#352): the
  classes `CantCreate` (73) and `Internal` (70), the values `cantcreate` and
  `internal` of `log.v1` `class`; `pod_release::Error::CantWrite` is
  `CantCreate`. podssh's `code_of` (`devtunnel.rs`, no `_` arm) does not
  build at the tag until it adds both classes, the signal that podssh asked
  for; podnet's matches fall to their `_` arm.

- T-1171 and T-1166 (`pod-link`, the DNS carrier; PodUtils/podssh#349): a
  late query of an ended session takes no handshake slot; the listener
  holds a poll (`DnsSettings::hold`, default 1 s, below `query_timeout`; at
  most 2 held polls for each session) and the dialer waits for its next
  idle poll from the answer of the last one. New public field `hold` and
  variant `DnsSettingsError::Hold`; a caller with `query_timeout` of 1 s or
  less lowers `hold`.

### What the next hand-off carries

- podrunner: no open ask. T-1160 changes the answer of
  `pod-fake-githome` to `assigned` (the fork fact of the real job alone).
- podvm: T-1143, T-1158 (podvm removes its work-arounds at the tag of
  T-1155 to T-1157: its read of the operation after the wait, `code_of_failure`,
  and its bound of 30 s on the first `GET /1.0`; it keeps its guard of
  SIGPIPE).
- podnet: T-1117 (documents), T-1118 (`release-index.json`).
- podssh: the STUN note above, T-1182 (the bound of a unit name), T-1095
  (the order of the pseudo-fields of the HTTP/2 request of `pod-ws`,
  PodUtils/podssh#340), T-1103 (iroh 1.5).
- Asks of podvm and podssh that no one has started: T-1166 to
  T-1169 and T-1171 (`pod-link`, the DNS carrier; PodUtils/podssh#349).
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
