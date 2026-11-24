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
   entry (PodUtils/podvm#570, COORD-183, maps to T-1162, answered on #259; PodUtils/podssh#349,
   COORD-139, maps to T-1164 to T-1169; PodUtils/podssh#350, COORD-140, maps to
   T-1191 and T-1192; PodUtils/podvm#657, COORD-241, maps to T-1193 to T-1196; PodUtils/podvm#659, COORD-243, maps to T-1205; PodUtils/podnet#709, COORD-117, maps to T-1209; PodUtils/podssh#351, COORD-141, maps to T-1217 to T-1222; PodUtils/podssh#353, COORD-143, maps to T-1198 and T-1220; PodUtils/podssh#352, COORD-142, maps to T-1235 to T-1238; PodUtils/podvm#661, COORD-245, maps to T-1213). Map each new one
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
   - `upstream.yml` (job `sources`): the run 45423205689 at `5fd6fb7` found
     one change, podrunner `2a55cd9` in `auth.rs` (`pod-testkit`). T-1243
     (done) ports it. Read the next daily run.
   - `services.yml`: the scheduled run 45394042840 failed (T-1223, done):
     the relay WMYMARB moved to a new address, and `node-trojan-45-63-30-142`
     left. `temp-sh` held (T-1034). `node-trojan-64-176-7-205` and
     `freedos-image` had a first no answer; read the next scheduled run
     (T-1096 reopens when `freedos-image` times out again).
   - `live.yml`: T-963 (partial; the second Nostr relay is `nos.lol`).
   - `publish.yml` 45433657871, the dry run on `v0.83.0`, passed.
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

## Release: v0.83.0

`v0.83.0` (`5fd6fb7`, `docs/releases/v0.83.0.md`) is released: the GitHub
release, the signed `SHA256SUMS`, the assets and the images of
`ghcr.io/podutils` exist. The run on the tag (45432576718, with the jobs
`image` and `notes`) and the dry run of `publish.yml` (45433657871) passed.
It carries each change of `main` after `v0.82.1`.

The hand-off COORD-154 (#259) is open (call C780). It closes when each
consumer names `v0.83.0` or a later tag. Amend its body at each take. It has
the digest of each image beside the SHA-256 of its asset, and the section
of each consumer. The hand-off COORD-152 (#257) of `v0.82.1` is closed.
COORD-153 (#258) answers PodUtils/podssh#346: the codec is in `v0.83.0`
(`pod-term`, the feature `transfer`); it closes when podssh names the
commit that wires the codec into its session (its T-831).

Takes of `v0.83.0`: podssh (`3001570`, PodUtils/podssh#354, COORD-144).
podrunner, podvm and podnet have not taken it.

The asks of #259 to the consumers:

- podvm: the answer to PodUtils/podvm#570 (T-1162, done) is on #259; podvm
  names the commit of its take of the four copies (PodUtils/podvm@T-1955)
  there, and closes its #570. podvm removes its work-arounds for
  `pod-incus` (T-1158) and matches `ErrorKind::TooLarge` (T-1143), takes
  `usage::Mounts` (T-1205) and `socket_answers` (T-1209) where it has a copy.
- podssh: took `v0.83.0` at `3001570` (PodUtils/podssh#354, COORD-144), with
  no frozen file replaced and no `taken` line due. Open: the wiring of the
  codec (podssh's T-831; podssh names the commit on #258); T-1167 to T-1169
  and PodUtils/podssh#349 (the datagrams of the DNS carrier over DoH;
  podssh's T-877 takes `Dns::handshake_quiet` and `DnsSettings::hold` at its
  merge); PodUtils/podssh#352 (ask (b): the codes
  that podssh keeps; T-1236 gives the README table, T-1238 the code 65).
- podnet: T-1117 is partial (`docs/tunnels.md` and its copy in
  `llms-full.txt` give 130 s and 280 s; the limits are 110 s and 210 s);
  `socket_answers` replaces its own copy (T-1209, PodUtils/podnet#709).
- podrunner: no open ask. The fork fact of `pod-fake-githome` (T-1160),
  `usage::Mounts` for podvm's report (T-1205, PodUtils/podrunner#350), and
  the shared watcher `scripts/agent/runs.py` with `watch.py` at the tag
  (T-1213; the rulings "Watchers" and "Work until done").

### On `main` after `v0.83.0` (for the next tag)

- T-1243: a port of the auth of the fake broker of `pod-testkit` at podrunner
  `2a55cd9`. No change for a consumer.
- T-1193 (`pod-extract`, PodUtils/podvm#657): `flatten` (`flatten`, `Options`,
  `Flattened`, `LAYER_EVENT`, `PATHS_MAX`) streams the final tree of the
  layers as one tar in one pass from the newest layer, in the order that
  incusd's GNU tar takes; podvm can stream with `ImageData::Split` today.
  Hard links across layers (T-1194), xattrs and the unified form (T-1195)
  and gzip (T-1196) follow. `extract` fixes: a directory that a later layer
  replaces, the mode of a remade directory, `Refusal::TooLong` for a
  component over 255 bytes (`COMPONENT_MAX`), `Refusal::Stream`. New entries
  T-1253 to T-1255.

- T-1163 (`pod-core`, `pod-oci`, `pod-cloudflare`): `pod_core::clock::retry_after`;
  in podvm's pull a `Retry-After` date gives its wait (capped by
  `BACKOFF_CAP`); `pod_cloudflare::quicktunnel::parse_retry_after` refuses a
  date out of range. No break.
- T-1197 (`pod-harness`, `pod-egress`, `pod-turn`): CRC-32 from `crc32fast`;
  `pod_harness::zip::crc32` and `pod_egress::vmess::kdf::crc32` are removed (a
  caller takes `crc32fast::hash`).

- T-1236 (`pod-core`, PodUtils/podssh#352): the section "Codes that a consumer
  keeps" of the README (podssh's `Revoked` 69 and `Capability` 78) with a test
  that holds it to `Class::exit_code`. The next hand-off answers #352 with it:
  podssh may move to the table at any time.

- T-1198 (`pod-term`, PodUtils/podssh#353): the transfer codec is wire version
  2 (trigger `#PODSSH-TX2:`; a v1 end and a v2 end refuse to start): the
  frame `HOLD` while a person answers an offer or the caller renames a copy,
  the verdict kept for `LINGER` (25 s) after `KEPT`, `Transfer::lingers`,
  `Ended::Unconfirmed`, `stop::UNSUPPORTED` at a `HELO` of another version.
  podssh changes `docs/terminal.md` ("The 7-bit envelope") to match.

- T-1194 and T-1195 (`pod-extract`, PodUtils/podvm#657): `flatten` keeps hard
  links across layers (each link after its target; `Flattened::{links,
  rereads}`), writes the attributes as `SCHILY.xattr.` records (ACLs in their
  binary form), takes `Options::{prefix, prefix_mode, end}` and the caller's
  entries by `flatten_with` and `Entries`. podvm streams the unified tarball
  of incus with no tree on disk (`prefix: "rootfs/"`, the metadata file of incus before,
  `rootfs/.podvm/podvm` after) into `pod_incus::ImageData::Unified`. `extract`
  refuses a link to a missing target or a directory with `Refusal::Link`
  (`safety::LINK_MISSING`), and a link waits for a missing directory above
  its target. New entries T-1269 and T-1270.

- T-1217 and T-1218: the new crate `pod-tpm` (PodUtils/podssh#351), from
  podssh's module (`SOURCES`): the command frame (bounded by `TPM_BUFSIZE`,
  4096), the response codes, the fourteen commands, `Transport` and
  `transact`, the templates of a storage primary and of an ECDSA P-256
  signing key, `PcrSelection` and the PolicyPCR and PolicyAuthorize digests.
  The key file (T-1219), the sequences (T-1220), the transports (T-1221) and
  the soft TPM with its CI lane (T-1222) follow; podssh takes the crate at
  the tag of T-1222. The owner reserves the name on crates.io.

### What the next hand-off carries

- podrunner: no open ask.
- podvm: T-1143, T-1158; the commit of its take of the four copies
  (PodUtils/podvm@T-1955); PodUtils/podvm#657 (T-1193 to T-1196: `flatten`
  of `pod-extract`, a writer of gzip members) is open in podcrates.
- podnet: T-1117 (documents), T-1118 (`release-index.json`).
- podssh: T-1198 (the codec's `HOLD` and linger, by podssh's answer); T-1095 (the order of the
  pseudo-fields of the HTTP/2 request of `pod-ws`, PodUtils/podssh#340),
  T-1103 (iroh 1.5).
- Asks of podssh that no one has started: T-1167 to T-1169 (`pod-link`,
  the DNS carrier; PodUtils/podssh#349), and `pod-tpm` (T-1217 to T-1222,
  PodUtils/podssh#351).
- Entries of podcrates that no one has started: T-1163 (two more readers of
  `Retry-After`), T-1115 (the copies of the bind at a long path), T-1135
  (`pod-cloudflare` says a cause two times), T-1146 (the comments of
  `pod-natprobe` are not in ASD-STE100), T-1150 (a watch of the facts of
  RFC 5780 of `easybell-stun`).
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
