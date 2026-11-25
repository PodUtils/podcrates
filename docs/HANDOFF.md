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
   order changes. COORD-157 (#263, T-1387) asks each repository for one
   `docs/hosted-sessions.md` and one role table: record each answer on its
   body, and close it when the four sessions answered.
3. Answer the open COORD issues `(to podcrates)`. Map each new one in the
   same commit.
   - The hand-off of `v0.85.0` is COORD-156 (#262). It answers
     PodUtils/podvm#664 (T-1379). Record each take on its body (the
     "Takes" lines, as #260), and close it when each consumer names the
     tag (call C780). The take of podnet is `cb695dd` (PodUtils/podnet#757,
     verified, on #262) and answered COORD-157 (#263: its role table is
     in its `docs/hosted-sessions.md`).
   - Done on `main` after `v0.85.0`; answer each with the next tag:
     PodUtils/podvm#667 (COORD-250, T-1389), PodUtils/podnet#753 to #755
     (COORD-123 to COORD-125, T-1392 to T-1394).
   - Done on `main` after `v0.85.0`: PodUtils/podnet#756 (COORD-126,
     T-1395); answer it with the next tag.
   - Open asks: PodUtils/podssh#358 (COORD-148) is T-1390;
     PodUtils/podvm#668 (COORD-251) is T-1391; PodUtils/podssh#357
     (COORD-147) asks T-1380, T-1381 and T-1383; PodUtils/podssh#349
     (COORD-139) is T-1169.
   - Answered, waiting for the take of the consumer: PodUtils/podssh#340,
     #349, #351 to #353, #355, #356; PodUtils/podvm#657 (T-1193 to
     T-1196). podrunner took PodUtils/podrunner#352 (T-1362) and #353
     (T-1370) at `3391edc` and `f2716a1`; its session closes them.
   - Mapped, no step now: PodUtils/podssh#350 (T-1191, T-1192);
     PodUtils/podvm#659 (T-1205), #661 (T-1213); PodUtils/podnet#709
     (T-1209).
4. Read the runs of the head of `main` (`ci`, `plants`, `adoption`,
   `secrets`, `deny`, `issue-sync`). A new red run needs an entry before
   other work. The open red runs and the runs to read:
   - `nightly.yml` 45603454952 at `cd7cb5a`: T-1444 (`pod-core`,
     `files::temp` on Windows: `PermissionDenied` at the name of a
     directory) and T-1445 (`pod-tpm`, `tests/socket.rs` on macOS: the
     timeouts of a closed Unix stream) are open; T-1446 (`pod-warp`, a
     socket path over 104 bytes on macOS) and T-1447 (the doc tests of
     `pod-h2`) are fixed on `main`. `riscv64` and `loongarch64` fail in
     `pod-iroh` (T-916, T-938, the ruling "Emulated nightly jobs"). T-1359
     and T-1360 held. Open: T-789, T-766, T-1044.
   - `ci.yml`: T-1325 closes with the times of five runs of the two macOS
     rows of `static` (limit 60 minutes): read the next `ci` runs.
   - `services.yml`: read the next scheduled run (T-1415 reopens on a
     second 5xx of `magic-wormhole-mailbox`; T-1096 reopens when
     `freedos-image` times out again). `live.yml`: T-963 (partial).
   - `upstream.yml` (job `sources`): read the next daily run.
   - The dry run of `publish.yml` on the tag `v0.85.0` (45631623920):
     read it (step 5 of "A release").
   - A plant that a run misses once with no change of its crate runs its
     test five times in its `.plant` (as `link-quic-drop-closes-at-once`).
5. The work order of `TODO/PROGRESS.md`: the next tag (T-1389, T-1392 to
   T-1394), then T-106 (the CI of githome and cfb), then the asks of the
   consumers. `docs/research/hard-problems.md` has the hard open problems
   (H-01 to H-18); T-978 (partial) holds the findings that no other entry
   has.
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

## Release: v0.85.0

`v0.85.0` (`1d1c9fc`, `docs/releases/v0.85.0.md`) is released: the GitHub
release, the signed `SHA256SUMS` and `SHA256SUMS.sigstore.json`, 14 assets
and the images of `ghcr.io/podutils` (`podlab`, `pod-fake-github`,
`pod-fake-githome` and `pod-release`; each index passes `gh attestation
verify`). `ci` 45620169994 at `1d1c9fc`, the dry run of `release.yml`
45628517649, the dispatch run 45629559471 and the run on the tag
45630869703 passed. It carries T-1379 (the config PATCH of `pod-incus` on
an ETag), T-1351 (the step of a failure), T-1343 (random temp names, step
2 in part), T-1375 (the port of podrunner `fcff5bc`), T-1369, T-1382,
T-1385 and T-1415.

The hand-off is COORD-156 (#262), with the digest of each image and the
section of each consumer. It closes when each consumer names `v0.85.0` or
a later tag (call C780). The hand-off COORD-155 (#260) of `v0.84.0` is
closed: podrunner `72cd0ab`, podvm `04c8b7f4`, podnet `405d47c` and podssh
`806e0a4` took it. COORD-153 (#258) is closed: podssh wires the codec at
`97cebe6` (PodUtils/podssh#358).

### On `main` after `v0.85.0` (for the next tag)

- T-1392 (`pod-dns`, PodUtils/podnet#753: the line of a
  name that does not resolve names each group of resolvers once
  (`N resolvers failed: K with CAUSE; K with CAUSE`) and shortens a cause
  at a part boundary, never inside it. No change of the public API.
- T-1393 (`pod-proxy`, PodUtils/podnet#754: the stderr
  of an exec command goes to a pipe (`STDERR_KEPT` 512 bytes,
  `ChildIo::stderr_tail`); the new `DialError::ProxyExited { word, status,
  said }` (breaking: `DialError` is not `non_exhaustive`); a command that
  exits 126 or 127 is held for `EXEC_HOLD` (10 s); `secret_parts` of an
  exec spec keeps the program word readable.
- T-1394 (`pod-cloudflare`, PodUtils/podnet#755: the
  edge discovery has `DISCOVERY_LIMIT` 12 s, `DOH_LIMIT` 8 s, `DOH_EACH`
  3 s and the event `edge.discovery` after `SLOW_SOURCE` 3 s; the legs of
  one start share one discovery.
- T-1389 (`pod-enter`, PodUtils/podvm#667 for ask 5 of
  PodUtils/podrunner#296: `Plan::tmpfs` takes
  `TmpfsMount { guest, bytes: NonZeroU64 }`; the namespace rung mounts
  each with `size=N` and reads the limit back (steps 49 to 52). Breaking:
  each struct literal of `Plan` adds `tmpfs: Vec::new()`. T-1430 (a rule of
  the confinement on such a tmpfs) is open.
- T-1395 (`pod-session`, PodUtils/podnet#756): `resume::after_mark(window)`,
  `resume::move_under(cap, window)` and `Settings::under_cap(cap)` (class
  `Usage`). The bound of the code is larger than the bound of the ask: a
  `DATA` record has a head of 13 bytes, so a far end that writes one byte
  at a time sends 14 bytes for each byte of its window; the handshake and
  the small records of the hold count too. With `PAIR_WINDOW`, a cap of
  64 MiB takes a move of 53.4 MiB (the default 48 MiB fits) and a cap of
  1 MiB takes no move: podnet's test of 1 MiB needs a window of 64 KiB (a
  move at 54,988 bytes). The writer reads the hold under the lock of the
  state before it sends an `ACK`.
- T-1446 (`pod-warp`): a test socket path fits the 104 bytes of macOS.
  T-1447 (`scripts/ci/test-limited.sh`): the doc run of the nightly
  excludes `pod-h2`. No change for a consumer.

### What the next hand-off carries

- The takes of `v0.85.0` (#262), and the answers to COORD-157 (#263).
- podrunner: the tmpfs of ask 5 of PodUtils/podrunner#296 (T-1389).
- podvm: T-1389 (PodUtils/podvm#667) on `main`; T-1391 (a static
  `mkfs.erofs`, PodUtils/podvm#668); T-1430; T-1321 (a C backend of
  deflate for `pod-gzip`, measured).
- podnet: T-1392 to T-1394 (PodUtils/podnet#753 to #755) on `main`;
  T-1395 (PodUtils/podnet#756) on `main`; T-1450 to T-1452 (the findings
  of T-1395: record heads against the window, `Offer::fit` of `pod-relay`,
  the wait of a small window); T-1343 is partial (the moves of the other crates of step 2
  and step 3 follow); T-1377 and T-1378 (failures whose text holds the
  words of a typed cause).
- podssh: open: T-1169 (a test of the DNS carrier through `unbound`),
  T-1103 (iroh 1.5), T-1340 (a key file of `pod-tpm` in tpm2-openssl and
  go-tpm-keyfiles), T-1380 and T-1381 (`pod-tpm`: `Tbs`, `SoftTpm`),
  T-1383 (`Mint` with an issuer that is not Ed25519) and T-1390 (the rows
  of the pane in the transfer codec, PodUtils/podssh#358).
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

- One worker is in flight: T-1444 and T-1445 (the red nightly) on the
  local branch `nfix1125` in `.claude/worktrees/nfix1125`. Each other
  branch of the agents is merged into `main`.
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
