# Handoff

The state of unfinished work, for a session with no memory of the last
one. Read this page first, then `AGENTS.md`, `TODO/PROGRESS.md` (the
state, the work order, the questions), `docs/maintenance.md` (the
procedures, the tools and the rules of the work), `docs/design.md` and
`docs/decisions.md`. Keep this page current: remove a line when its work
is done. Git and the record keep finished work.

## The first steps of the next session

1. Make sure that the two watchers, `scripts/agent/watch.py` and
   `scripts/agent/runs.py`, run as background processes with a log, and
   keep a Monitor on each log (the ruling "Watchers"; `docs/maintenance.md`,
   "Start of a session"). `scripts/session-start.sh` starts them; the
   Claude Code hook runs it, and another harness runs it by hand. Resume
   from this page, and work until each task is done or blocked on a thing
   out of the control of the session (the ruling "Work until done"). Read
   the open COORD issues `(to podcrates)` of each repository of PodUtils.
   Each one maps to an entry:
   - Answered by the tag `v0.84.0`; the hand-off #260 (COORD-155) is open
     and waits for the takes: PodUtils/podnet#711 (COORD-119, T-1343,
     partial: the functions are in the tag, the moves of the other crates
     remain), PodUtils/podnet#712 (COORD-120, T-1365),
     PodUtils/podnet#726 (COORD-121, T-1366), PodUtils/podssh#340 (T-1095),
     PodUtils/podssh#349 (COORD-139, T-1164 to T-1169; T-1169 is open),
     PodUtils/podssh#351 (COORD-141, T-1217 to T-1222),
     PodUtils/podssh#352 (COORD-142, T-1235 to T-1238),
     PodUtils/podssh#353 (COORD-143, T-1198 and T-1220),
     PodUtils/podssh#355 (COORD-145, T-1367 and T-1368),
     PodUtils/podssh#356 (COORD-146, T-1371), PodUtils/podrunner#352
     (COORD-130, T-1362) and PodUtils/podrunner#353 (COORD-131, T-1370).
     PodUtils/podvm#657 (COORD-241, T-1193 to T-1196) is in the tag too.
   - New asks, open in the record: PodUtils/podvm#664 (COORD-248 of podvm,
     for PodUtils/podrunner#354) maps to T-1379, the config PATCH of
     `pod-incus` with an ETag condition; it is the first entry of the work
     order, as podrunner's warm units with lower limits wait for it.
     PodUtils/podssh#357 (COORD-147) maps to T-1380 to T-1383. T-1384 (the
     shared target of the worktrees) comes before the next full check of
     merged agent work: until it is done, `touch` each source of the
     workspace before a full check in the main checkout.
   - Answered on #259 (closed): PodUtils/podvm#570 (COORD-183, T-1162),
     PodUtils/podnet#710 (COORD-118, T-1117, T-1118 and T-1209),
     PodUtils/podrunner#351 (COORD-129, T-1355) and PodUtils/podvm#663
     (COORD-247, T-1143, T-1158 and T-1307).
   - A take of `v0.84.0`, verified: PodUtils/podssh#357 (COORD-147). The
     commit `806e0a4` is on `main` of podssh, and each `Cargo.toml` there
     that names a crate of podcrates names `tag = "v0.84.0"` and no other
     tag. It takes PodUtils/podssh#340 (T-1095), #351 (T-1217 to T-1222),
     #352 (T-1238) and #356 (T-1371); the bearer grant (#355, T-1367 and
     T-1368) and the DNS carrier (#349, T-1167 and T-1168) are podssh's
     open T-834 and T-900. It asks four things: T-1380 and T-1381
     (`pod-tpm`: the time limit of `Tbs`, the owner's password in
     `SoftTpm`), T-1382 (the trials of podssh and the `taken` line) and
     T-1383 (`Mint` with an issuer that is not Ed25519).
   - A take of `v0.84.0`, verified: PodUtils/podvm#666 (COORD-249). The
     commit `04c8b7f4` is on `main` of podvm, and each `Cargo.toml` there
     that names a crate of podcrates names `tag = "v0.84.0"` and no other
     tag. It takes `pod_gzip::Members` (PodUtils/podvm#657, T-1196). It asks
     nothing: the use of `pod_extract::flatten_with` (T-1193 to T-1195) is
     a follow-up of podvm with its live proof pending.
   - A take of `v0.84.0`, to verify: PodUtils/podrunner#356 (COORD-134; it
     answers #352 and #353, and it asks nothing). The change of
     `docs/contracts/source-driver.md` that `check-sources.py` finds in
     podrunner after `4fc871c` is T-1375.
   - Other mapped asks: PodUtils/podssh#350 (COORD-140, T-1191 and T-1192);
     PodUtils/podvm#659 (COORD-243, T-1205); PodUtils/podnet#709
     (COORD-117, T-1209); PodUtils/podvm#661 (COORD-245, T-1213).
   Map each new one in the same commit.
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
   - `nightly.yml` 45504345202 at `6f5f4951` (a dispatch): `gate`, the
     installer jobs, `test (windows-2025)` and the jobs `cross` of aarch64,
     arm, armv7, powerpc64le and i686 pass. The emulated jobs pass
     `pod-enter`, `dns_hold` and `mail` (T-1296 and T-1297 are done), the
     macOS jobs pass `pod-incus` (T-1298 is done), and each job `test`
     prints the seconds of each binary (T-1303 is done; the longest binary
     takes 70 s, so the limits are 300 s and 600 s). Open failures:
     `riscv64` fails in `pod-iroh` (`road.rs`, `share.rs`) and
     `loongarch64` in `share.rs` (T-916, T-938; the ruling "Emulated nightly jobs"); the riscv64 job took
     127 of its 150 minutes, and `session_resume_twice` ended in 91 s. Both
     jobs of macOS fail in three tests of `files::socket` of `pod-core`: the
     temporary directory is longer than 40 bytes (T-1359, done on `main`).
     The job of Windows ARM64 fails in the UDP bind of
     `quick_tunnel_standin`, with 10013 (T-1360, done on `main`). T-789 and T-766 passed again (partial). Open entries of
     the nightly: T-916 and T-938 (partial), T-789,
     T-766, and T-1044 (`check-windows.sh` and `check-macos.sh` compile no
     doc test).
   - `ci.yml`: the job `static (aarch64-apple-darwin, macos-15-intel)` met
     its limit of 40 minutes in runs 45487233017 and 45561305578 (T-1325,
     partial). The two macOS rows of `static` have a limit of 60 minutes
     since `v0.84.0`. Runs 45568238286 at `ca1d3e3` and 45578000852 at
     `d9e5da03` pass; the two macOS rows took 24.9 and 20.4 minutes in the
     second. T-1325 closes with the times of five runs: read the next `ci`
     runs that end.
   - `upstream.yml` (job `sources`): the run 45423205689 at `5fd6fb7` found
     one change, podrunner `2a55cd9` in `auth.rs` (`pod-testkit`). T-1243
     (done) ports it; T-1355 (done) ports podrunner `4fc871c` in
     `source-driver.md`. Read the next daily run.
   - `services.yml`: the scheduled run 45394042840 failed (T-1223, done):
     the relay WMYMARB moved to a new address, and `node-trojan-45-63-30-142`
     left. `temp-sh` held (T-1034). `node-trojan-64-176-7-205` and
     `freedos-image` had a first no answer; read the next scheduled run
     (T-1096 reopens when `freedos-image` times out again).
   - `live.yml`: T-963 (partial; the second Nostr relay is `nos.lol`).
   - `publish.yml` 45575024311, the dry run on `v0.84.0`, passed.
   - `docs/research/hard-problems.md` has the hard open problems (H-01 to
     H-18). T-978 (partial) holds the findings that no other entry has.
4. The work of podcrates (`TODO/PROGRESS.md`, "Work order"): the red
   gates above, then the entries for the CI of githome and cfb (T-106),
   then the asks of the consumers.
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
7. T-767 is done: podssh closed PodUtils/podssh#283, because the hold of its
   later failures is in its own pump (the client's line `resume::Note::Stall`
   of T-945 and the channel's line `e2e.wait` of T-959 named the reader), and
   it asks nothing more of podcrates. T-768 (a spare of `pod-session` that
   waits 20 s) is open.
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

## Release: v0.84.0

`v0.84.0` (`ca1d3e3`, `docs/releases/v0.84.0.md`) is released: the GitHub
release, the signed `SHA256SUMS` and `SHA256SUMS.sigstore.json`, 14 assets
and the images of `ghcr.io/podutils` (`podlab`, `pod-fake-github`,
`pod-fake-githome` and `pod-release`) exist. `ci` run 45568238286 at
`ca1d3e3`, the dry run of `release.yml` 45573005407, the dispatch run
45573664209, the run on the tag 45574255033 and the dry run of
`publish.yml` 45575024311 passed. It carries each change of `main` after
`v0.83.0`: the new crates `pod-tpm`, `pod-gzip` and `pod-h2` (the fork of
h2), `flatten` of `pod-extract`, the bearer grant, the bare and DoH
forms of the DNS carrier, `RealJob` and the warm units, and the class
`Data`.

The hand-off COORD-155 (#260) is open. Its body has the digest of each
image beside the SHA-256 of its asset, and the section of each consumer.
It closes when each consumer names `v0.84.0` or a later tag (call C780).
The hand-off COORD-154 (#259) of `v0.83.0` is closed. COORD-153 (#258)
answers PodUtils/podssh#346: the codec is in `v0.83.0` (`pod-term`, the
feature `transfer`) and its version 2 is in `v0.84.0`; it closes when
podssh names the commit that wires the codec into its session (its
T-831).

Takes of `v0.84.0`: podssh and podvm are verified. The body of #260 has one
"- CONSUMER" line for each of the four consumers; the line changes at each
verified take. podssh: PodUtils/podssh#357 (COORD-147) names the take at
`806e0a4`. `git ls-remote` gives `806e0a4` as the head of `main` of
podssh, and the 55 `Cargo.toml` files of that tree name `tag = "v0.84.0"`
for each crate of podcrates and no other tag (`Cargo.lock` names no other).
The line of podssh in #260 is the take line. The take replaces
`vendor/h2` with `pod-h2`, makes `crates/podssh-ssh/src/tpm` glue over
`pod-tpm`, adds `Class::Data` to the Dev Tunnels map, and signs the
gateway certificate with `Mint::sign_with`. `check-sources.py` with a clone
of podssh and the `taken` line of `pod-tpm` are in T-1382. podvm:
PodUtils/podvm#666 (COORD-249; `PodUtils/podvm@T-2085`) names the take at
`04c8b7f4`. `git ls-remote` gives `04c8b7f4` as the head of `main` of
podvm, and each `Cargo.toml` of that tree that names a crate of podcrates
names `tag = "v0.84.0"` and no other tag (`Cargo.lock` of the workspace
names no other). The line of podvm in #260 is the take line. podvm uses
`pod_gzip::Members` behind its selection of the upload space; the use of
`flatten_with` is in a follow-up. podrunner: PodUtils/podrunner#356 (COORD-134) names the take at
`72cd0ab` (T-1362 at `3391edc`, T-1370 at `f2716a1`), but `origin/main` of
podrunner holds none of these three commits (its head is `f6a9a940`, and
each `Cargo.toml` there names `v0.83.0`). The line of podrunner in #260
stays "not yet" until `git cat-file -t 72cd0ab` finds the commit and each
`Cargo.toml` of podrunner at it names `tag = "v0.84.0"` and no other tag.
Then write the line as the take lines of #259 (the commit, #356, COORD-134)
and run `check-sources.py` for `pod-testkit`, `pod-job`, `pod-unit`,
`pod-time`, `pod-actions-results` and `pod-actions-store`.

The asks that #260 answers:

- podvm: took `pod-gzip` (`pod_gzip::Members`) at `04c8b7f4`
  (PodUtils/podvm#666). Not named in its take: `flatten` of `pod-extract`
  (PodUtils/podvm#657, T-1193 to T-1195; a follow-up of podvm),
  `Refusal::TooLong`, `Refusal::Stream` and `Refusal::Link` (an exhaustive
  `match` adds three arms), `Settings::write_timeout` and the field `cut`
  of `ErrorKind::Incus` (T-1298, T-1307).
- podssh: took `pod-tpm` (PodUtils/podssh#351), the fork `pod-h2` and the
  removal of `vendor/h2` (PodUtils/podssh#340), `Mint::sign_with`
  (PodUtils/podssh#356) and the class `Data` (PodUtils/podssh#352) at
  `806e0a4` (PodUtils/podssh#357). Not taken yet: the bare form and the
  DoH form of the DNS carrier (PodUtils/podssh#349; its T-900), the bearer
  grant and `Binding` (PodUtils/podssh#355; its T-834), and the codec
  version 2 (PodUtils/podssh#353; the take does not name it).
- podnet: no take. The refusal replies of `pod-warp` (PodUtils/podnet#712,
  T-1365), the cause by issuer of the door (PodUtils/podnet#726, T-1366),
  the one cause of each failure of `pod-cloudflare` (T-1135), `new_temp_dir`
  and `new_temp_file` (PodUtils/podnet#711, T-1343) and the class `Data`
  (`DATA_ERROR`).
- podrunner: no take. The fields `ref` and `repo_id` of `RealJob`
  (PodUtils/podrunner#352, T-1362) and the warm units of `run-report.v1`
  and `status.v1` (PodUtils/podrunner#353, T-1370).

### On `main` after `v0.84.0` (for the next tag)

- T-1369 (`pod-relay`, fuzz): the fuzz target `relay_bearer` reads a
  bearer with `Bearer::read` and `is_bearer`; `Bearer::to_bytes` (new)
  writes a bearer back from its fields. The patch
  `vendor/patches/h2/0003-fuzz-docs.patch` lets the fuzz build of
  `pod-h2` pass. No change for a consumer.
- T-1351 (`pod-core`, `pod-cloudflare`, `pod-quic`, `pod-warp`): the
  text of these failures is the step, and the cause says the rest; read
  `Failure::chain` or join the chain. `Failure::io(stage, step, e)`
  takes a step (podssh: two tests). The changed texts for podnet:
  "provisioning quick tunnel: ERROR" (the text of a `ProvisionError`
  failure is "provisioning quick tunnel"); "no edge leg worked: LEG:
  CHAIN; LEG: STEPS" (the cause of the last leg is the source, not in
  the text); "TLS with the edge at ADDR: the connection was reset
  during the TLS handshake: ERROR" ("the peer sent a TLS alert" in place
  of "TLS alert from the peer: ALERT"); "REQUEST: TLS: WORDS: ERROR";
  "could not open a local UDP socket (QUIC needs UDP; where it is
  blocked, use PROTOCOL http2): no UDP socket for QUIC: ERROR"; "TLS
  handshake with the MASQUE front door DOOR" (the error is the source);
  "DOOR-A CHAIN; the other address of the front door failed too: DOOR-B
  STEPS" (the cause of the last door is the source).
- T-1343 (partial): the temp names of `pod-hostprobe`, `pod-enter`,
  `pod-actions-store`, `pod-link`, `pod-rootfs`, `pod-warp` and
  `pod-supervise` have a random part; `pod_core::files::new_temp_with`
  (new). The temp file of the table of `pod-supervise` is
  `containers.json-RANDOM.partial`; a sweep of podvm finds the suffix.

### What the next hand-off carries

- The takes of `v0.84.0` (#260): podnet and podrunner. podssh (`806e0a4`)
  and podvm (`04c8b7f4`) named their takes.
- podrunner: no open ask besides the take. T-1375 (a port of
  `source-driver.md` for `pod-testkit`) is open.
- podvm: T-1321 (a C backend of deflate for `pod-gzip`, measured).
- podnet: T-1343 is partial (the moves of the other crates of step 2 and
  step 3 follow); T-1377 and T-1378 (failures whose text holds the words
  of a typed cause).
- podssh: open: T-1169 (a test of the DNS carrier through `unbound`),
  T-1103 (iroh 1.5), T-1340 (a key file of `pod-tpm` in tpm2-openssl and
  go-tpm-keyfiles), T-1380 and T-1381 (`pod-tpm`: `Tbs`, `SoftTpm`),
  T-1382 (the trials of podssh) and T-1383 (`Mint` with an issuer that is
  not Ed25519).
- Entries of podcrates that no one has started: T-1115 (the copies of the
  bind at a long path), T-1146 (the comments of `pod-natprobe` are not in
  ASD-STE100), T-1150 (a watch of the facts of RFC 5780 of `easybell-stun`).
- T-963 is partial. T-975 and T-977 (podvm's asks #602 and #604; the ruling
  "Assets of other licences" allows their release part) are open. T-1159 is
  open (the ruling "A close by hand"): the code of `scripts/issue-sync.py`
  still waits 24 hours. T-1347 (the command of each patch of `pod-h2`) and
  T-1373 (the opt-in fetch of Sigstore roots from TUF) are open. The names
  `pod-natprobe`, `pod-tpm`, `pod-gzip` and `pod-h2` wait for the owner's
  local agent: PodUtils/podcrates#261 (T-1372).

## Unfinished work

- No agent work is in flight and no worktree is left. Each branch of the
  agents is merged into `main`.
- `origin/wip/t1073` is merged into `main` (T-1073, T-1070). The proxy
  refuses `git push origin --delete`, so the owner deletes the branch on
  GitHub (`TODO/PROGRESS.md`, "Owner actions"). No other `wip/` branch
  exists.

## What waits

- podvm: T-330 (its pair commands). T-207 (`pod-netlab`) does not wait for
  podvm: podvm closed PodUtils/podvm#260 and took the offline part; T-345 to
  T-347 (the probe program, the world and the run on a kernel) are open, and
  podvm's lab guest (PodUtils/podvm@T-1662 to PodUtils/podvm@T-1664) takes
  them when they are done.
- podswarm: its asks wait until its session starts, but for T-444, T-447
  and T-448 (the ruling "podswarm").
- `pod-git` (T-217 to T-220) is blocked (`PodUtils/podforge`, deferred).
  The rest of M8 waits (the ruling "Priority"); the entries for the CI of
  githome and cfb are in the work order.
- T-386 (the live run of `pod-devtunnel`) waits for the owner's token and
  tunnel (`TODO/PROGRESS.md`, "Owner actions").
