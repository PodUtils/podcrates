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
   - The hand-off of `v0.86.0` is COORD-158 (#272). It answers
     PodUtils/podvm#667; PodUtils/podnet#753 to #756, #758 and #776;
     PodUtils/podssh#359 to #363 and #367. Record each take on its body
     (the "Takes" lines, as #262), and close it when each consumer names the
     tag (call C780). The take of podnet is `144a5ee`
     (PodUtils/podnet#778, verified, on #272). COORD-157 (#263): podnet,
     podvm and podrunner took the role table; it waits for podssh.
   - On `main` after `v0.86.0`: PodUtils/podvm#675 (COORD-256, T-1424) and
     PodUtils/podvm#677 (COORD-258, T-1425); answer each with the next tag.
   - On `main` after `v0.86.0`: PodUtils/podssh#358 (COORD-148, T-1390).
   - Open asks:
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
     time limits of a closed Unix stream, `pod_core::sockopt`) are done on
     `main`; the next nightly is their proof (no run on Windows or macOS
     here). T-1446 (`pod-warp`, a
     socket path over 104 bytes on macOS) and T-1447 (the doc tests of
     `pod-h2`) are fixed on `main`. `riscv64` and `loongarch64` fail in
     `pod-iroh` (T-916, T-938, the ruling "Emulated nightly jobs"). T-1359
     and T-1360 held. Open: T-789, T-766, T-1044.
   - `ci.yml`: T-1325 closes with the times of five runs of the two macOS
     rows of `static` (limit 60 minutes): read the next `ci` runs.
   - `services.yml`: read the next scheduled run (T-1415 reopens on a
     second 5xx of `magic-wormhole-mailbox`; T-1096 reopens when
     `freedos-image` times out again). `live.yml`: T-963 (partial); T-1504 (the upload of temp.sh
     answered 500 on 2026-11-26: read the next scheduled run).
   - `upstream.yml` (job `sources`): read the next daily run.
   - The dry run of `publish.yml` on the tag `v0.86.0` (45786405631):
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

## Release: v0.86.0

`v0.86.0` (`22d1a75`, `docs/releases/v0.86.0.md`) is released: the GitHub
release, the signed `SHA256SUMS` and `SHA256SUMS.sigstore.json`, 14 assets
and the images of `ghcr.io/podutils` (each index passes `gh attestation
verify`). `ci` 45779678814 at `22d1a75`, the dry run of `release.yml`
45784561978 and the run on the tag 45785761766 passed. It carries the
tmpfs of `pod-enter` (T-1389), the asks of podnet #753 to #758 and #776
(T-1392 to T-1395, T-1404, T-1416, T-1474, call C973), the asks of podssh
#359 to #363 and #367 (T-1396, T-1399 to T-1412, T-1421 to T-1423), the
fixes of the nightly (T-1444 to T-1447) and the Dependabot updates
(T-1414, with `incompatible-rust-versions = "allow"`). The notes name each
breaking change with the files of each consumer.

The hand-off is COORD-158 (#272). The hand-off COORD-156 (#262) of
`v0.85.0` is closed: each consumer took it.

### On `main` after `v0.86.0` (for the next tag)

- T-1424 (`pod-hostprobe`, PodUtils/podvm#675): `Settings::emulator`
  (`Option<PathBuf>`) and `Settings::with_emulator`; the machine legs run
  the emulator at that path. Breaking: `podvm-probe` builds `Settings`
  with each field and adds `emulator: None`.
- T-1390 (`pod-term`, PodUtils/podssh#358): `Transfer::set_size(cols,
  rows)`, `pane_chunk`, `CHUNK_FLOOR` (256), `SPARE_ROWS` (2): a `DATA`
  frame takes at most `rows - 2` lines; each end sends its pane cap in the
  `chunk` of `HELO` and takes the smaller cap; a lost `DATA` goes again
  built from its first `CHUNK_FLOOR` bytes. The codec stays version 2 (the
  `chunk` of `HELO` was a cap already), so old and new ends pair. podssh:
  in `crates/podssh-ssh/src/zmodem/far.rs` call `codec.set_size(cols,
  rows)` at the start and at each resize in place of `set_width`, drop
  `PACE` and the parts of `put`, and name the cap in `docs/terminal.md`.
- T-1488 (`pod-link`): the loopback, UDP and split lines show no
  " (os error N)" (`pod_core::text::io_words`).
- T-1473 (`pod-relay`), breaking: `grant::openssh_public` is removed. podssh
  (`crates/podssh-cli/src/grants.rs`, three places): `grant::openssh_public(&k)`
  becomes `k.openssh()` (`pod_identity::PublicKey::openssh`, the same type).
  `grant::parse_public` reads with `PublicKey::from_openssh`.
- T-1480 (`pod-core`): `pod_core::abort::AbortOnDrop` (feature `async`) in
  place of the copies of `pod-relay`, `pod-cloudflare` and `pod-warp`.
- T-1425 (`pod-oci`, PodUtils/podvm#677): `spec::Config::rootfs`,
  `spec::RootFs`, `spec::check_layers`, `MAX_LAYERS`. No build change.

### What the next hand-off carries

- The takes of `v0.86.0` (#272), and the answer of podssh to COORD-157
  (#263).
- podvm: T-1424, T-1425 (on `main`); T-1391 (a static `mkfs.erofs`,
  PodUtils/podvm#668); T-1430; T-1321.
- podnet: T-502 (PodUtils/podnet#760); T-1450 to T-1452; T-1343
  (partial); T-1377, T-1378.
- podssh: T-1390 on `main`; T-1413 (optional); T-1169,
  T-1103, T-1340, T-1380, T-1381, T-1383.
- podcrates: T-1463 (a check that stops
  a `to_string()` of a failure), T-1460, T-1453, T-1484 (iroh 1.5).

## Unfinished work

- No worker is in flight and no worktree is left. Each branch of the
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
