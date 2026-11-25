# Handoff

The state of unfinished work, for a session with no memory of the last
one. Read this page first, then `AGENTS.md`, `TODO/PROGRESS.md` (the
state, the work order, the questions), `docs/maintenance.md` (the
procedures, the tools and the rules of the work), `docs/design.md` and
`docs/decisions.md`. Keep this page current: remove a line when its work
is done. Git and the record keep finished work.

## The first steps of the next session

1. Start (`docs/hosted-sessions.md`, "Start"). The Claude Code hook runs
   `scripts/session-start.sh`; in another harness, run it by hand, then
   `sh scripts/cloud-setup.sh` on a new host and `. ./scripts/cloud-env.sh`
   in each shell. Read `.work/watch.log` and `.work/runs.log` after each
   step. Give the work to workers by the role table and the brief of
   `docs/hosted-sessions.md`, "Lead and workers". Work until each task is
   done or blocked on a thing out of the control of the session (the
   ruling "Work until done").
2. Coordinate the five sessions by COORD issues alone (the rulings
   "Coordinator" and "COORD only"). COORD-136 (#227) gives the order of
   each session; route each ask of podrunner first, and edit #227 when the
   order changes. Send the COORD issue `(to all)` of T-1387 (one
   `docs/hosted-sessions.md` and one role table for each repository).
3. Answer the open COORD issues `(to podcrates)`. Map each new one in the
   same commit.
   - Open asks: PodUtils/podvm#664 (COORD-248, for
     PodUtils/podrunner#354) is T-1379, the first entry of the work order;
     PodUtils/podssh#357 (COORD-147) asks T-1380 to T-1383;
     PodUtils/podnet#711 (COORD-119) is T-1343 (partial);
     PodUtils/podssh#349 (COORD-139) is T-1169.
   - The takes of `v0.84.0` (the hand-off #260, COORD-155): podssh
     (`806e0a4`, #357), podvm (`04c8b7f4`, #666) and podrunner (`72cd0ab`,
     PodUtils/podrunner#356) are verified; podnet has none. T-1375 ports
     the four files of podrunner that changed in `pod-testkit`.
   - Answered by `v0.84.0`, waiting for the take of the consumer:
     PodUtils/podnet#712 (T-1365), #726 (T-1366); PodUtils/podssh#340,
     #349, #351 to #353, #355, #356; PodUtils/podvm#657 (T-1193 to
     T-1196). podrunner took PodUtils/podrunner#352 (T-1362) and #353
     (T-1370) at `3391edc` and `f2716a1`; its session closes them.
   - Mapped, no step now: PodUtils/podssh#350 (T-1191, T-1192);
     PodUtils/podvm#659 (T-1205), #661 (T-1213); PodUtils/podnet#709
     (T-1209).
4. Read the runs of the head of `main` (`ci`, `plants`, `adoption`,
   `secrets`, `deny`, `issue-sync`). A new red run needs an entry before
   other work. The open red runs and the runs to read:
   - `nightly.yml`: `riscv64` and `loongarch64` fail in `pod-iroh`
     (`road.rs`, `share.rs`; T-916, T-938, the ruling "Emulated nightly
     jobs"). Read the macOS and Windows ARM64 jobs of the next run (T-1359
     and T-1360 are done on `main`). Open: T-789, T-766, T-1044.
   - `ci.yml`: T-1325 closes with the times of five runs of the two macOS
     rows of `static` (limit 60 minutes): read the next `ci` runs.
   - `services.yml`: read the next scheduled run (T-1096 reopens when
     `freedos-image` times out again). `live.yml`: T-963 (partial).
   - `upstream.yml` (job `sources`): read the next daily run.
   - A plant that a run misses once with no change of its crate runs its
     test five times in its `.plant` (as `link-quic-drop-closes-at-once`).
5. The work order of `TODO/PROGRESS.md`: T-1379, then T-106 (the CI of
   githome and cfb), then the asks of the consumers.
   `docs/research/hard-problems.md` has the hard open problems (H-01 to
   H-18); T-978 (partial) holds the findings that no other entry has.
6. Rules of the session that the documents do not repeat:
   - The channel of cfb and githome is #223. podcrates keeps one reply
     comment to each there (cfb: 6970315549; githome: 6970683028) and
     edits it, with the UTC time of each update. cfb is offline until the
     CI of podrunner and githome is complete; cfb's A-X-18 (T-106,
     partial) waits for its answer on #223.
   - A step of githome's answers that podrunner writes and that differs
     from `pod-fake-githome` or `pod-fake-github` is an entry here.
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

Takes of `v0.84.0`: podssh, podvm and podrunner are verified. The body of #260 has one
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
`72cd0ab` (PodUtils/podrunner@T-664). `72cd0ab` is on `main` of podrunner, followed by
`3391edc` (T-1362) and `f2716a1` (T-1370). Each `Cargo.toml` of podrunner
at `72cd0ab` names `tag = "v0.84.0"` (27 lines) and no other tag, and
`Cargo.lock` names no other. The line of podrunner in #260 is the take
line. `check-sources.py` at `72cd0ab`: `pod-actions-results`,
`pod-actions-store`, `pod-job` and `pod-time` give 0 changes;
`pod-testkit` gives four changed files, which T-1375 ports.

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
- podrunner: took the fields `ref` and `repo_id` of `RealJob`
  (PodUtils/podrunner#352, T-1362) at `3391edc` and the warm units of
  `run-report.v1` and `status.v1` (PodUtils/podrunner#353, T-1370) at
  `f2716a1`, after its take `72cd0ab` (PodUtils/podrunner#356).

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

- The take of `v0.84.0` (#260): podnet. podssh (`806e0a4`), podvm
  (`04c8b7f4`) and podrunner (`72cd0ab`) named their takes.
- podrunner: no open ask. T-1375 (the port of four files of podrunner
  into `pod-testkit`) is open.
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
  local agent: PodUtils/podcrates#261 (T-1372; the owner's registration is in progress).

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
