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
   `(to podcrates)` of each repository of PodUtils.
2. Coordinate the five sessions by COORD issues alone (the rulings
   "Coordinator" and "COORD only"): podcrates, podrunner (the flagship),
   podssh, podnet and podvm. The owner starts the sessions; send no direct
   message. COORD-136 (#227) gives the order of each session. Read each
   answer `(to podcrates)`, route each ask of podrunner to its owner at
   once, and edit #227 when the order changes.
3. The red scheduled runs and their entries:
   - `nightly.yml` 44739339618 at `8a2cdd9`: T-1040, T-1041, T-1042, T-1043,
     T-1021 and T-1030 are done. Open: T-1044 (the checks of Windows and
     macOS here compile no doc test), T-916 (partial; `pod-iroh` on
     riscv64 and loongarch64), T-938 (partial), T-789 and T-766.
   - `nightly.yml` 44848003241 at `a7fb465` (#252): T-1082 (`80fa507a`)
     moved each gate of `switches::count` into a test binary of its own,
     and `i686` passed in `nightly.yml` 44891581672 and 45009375271.
   - `nightly.yml` 44871383085 at `ab73e75` (#255), plus scheduled runs
     44891581672 (2026-11-21) and 45009375271 (2026-11-22) at `50aa5e0b`:
     - `test (windows-2025)` and `test (windows-11-arm)` failed in both
       `Test` and `A Windows service of pod-unit`
       (`crates/pod-unit/tests/service_live.rs`): `win::serve` returned
       `Class::Unavailable` (`StartServiceCtrlDispatcherW: An instance of
       the service is already running. (os error 1056)`) on the second
       call in `a_program_that_the_manager_did_not_start_gets_a_usage_failure`
       instead of `Class::Usage`. T-1089 (done, in `v0.82.1`) fixes it;
       the proof is the next run of `nightly.yml` on Windows.
     - `cross (riscv64gc-unknown-linux-musl)` failed in `pod-iroh`
       (`road.rs`, `new_address.rs`, `share.rs`, T-916, T-938) and in
       45009375271 also in `pod-quic` (`datagrams.rs`). `loongarch64`
       (44891581672, 45009375271) and `powerpc64le` (45009375271) failed
       in `pod-iroh` (`road.rs`, T-916).
     - `ci` 44871374413, `adoption` 44871377124 and `plants` 44871380137
       passed on `ab73e75`.
   - Current `main` runs (`51c09390` `v0.82.0` and `07d4cc4f` `HEAD`):
     - At `51c09390` (`v0.82.0`): `publish.yml` 45122406747 on `v0.82.0`
       (`dry-run`, `crates-io = false`), `plants.yml` 45122401716,
       `adoption.yml` 45122401663, `secrets.yml` 45122401645,
       `vmess interop` 45122401640, `issue-sync.yml` 45122401611 and
       `deny.yml` 45122401588 passed. `ci.yml` 45122401625 failed in
       `test` on `scripts/ci/turn-tests.sh` (`coturn_tcp` output match).
     - At `07d4cc4f`: `secrets.yml` 45144882194, `issue-sync.yml`
       45144882108 and `deny.yml` 45144881966 (proving `url` 2.5.9 after
       `deny.yml` 45035185940 failed on RUSTSEC-2026-0592) passed.
       `ci.yml` 45144882072 and `plants.yml` 45144882160 are running;
       read their results first.
   - `services.yml` 45009160652 (2026-11-22): failed (`syncthing-relay`
     `relay-dvwou27` moved to `1.168.95.18`, plus `freedos-image`,
     `localtunnel`, `gosuda-portal`, `openbare-xyz`, `opentunnel` and
     `node-trojan-66-163-112-121`; T-1093; T-1096 for the hosts that
     gave no answer to the runners, which held on 45159148662).
   - `ci.yml`: T-943 is done: the arm64 probe of macOS builds on the
     Intel runner.
   - `docs/research/hard-problems.md`: the hard open problems (H-01 to
     H-18), with the questions that research must answer. T-978 has its
     findings that no other entry had.
   - `upstream.yml` 45224210077 (`sources`): red on `pod-testkit` and
     `pod-unit`. T-1111 and T-1122 are done: each crate gives 0 changes
     at the consumers' heads (podrunner `1f04680`, podssh `444b518`). No
     drift remains.
   A new red run needs an entry before other work. A plant that a run
   misses once with no change of its crate runs its test five times in
   its `.plant` (as `link-quic-drop-closes-at-once`).
4. The work of podcrates (`TODO/PROGRESS.md`, "Work order"): the red
   nightly above, then the entries for the CI of githome and cfb (T-106,
   T-104 with T-880, T-100, T-105, T-103, T-102).
5. The fakes of the CI of podrunner and githome are release assets of
   each tag from `v0.73.0`: `pod-fake-github` (T-921, call C785;
   githome's G-30 and G-40) and `pod-fake-githome` (T-948, call C786;
   githome's G-38 and PodUtils/podrunner@T-509), each for x86_64 and
   aarch64 musl in the signed `SHA256SUMS`, and from `v0.75.0` an image
   of each on ghcr.io (`crates/pod-testkit/README.md`). When podrunner
   writes a step of githome's answers that differs from the fake, it is
   an entry here.
6. The channel of cfb and githome is #223. podcrates keeps one reply
   comment to each there (cfb: 6970315549; githome: 6970683028) and edits
   it, with the UTC time of each update and of each acknowledgement. cfb
   is offline until the CI of podrunner and githome is complete; it reads
   `docs/research/fleet/asks.md` and the block of #223 at its next start.
   githome's asks G-30 to G-35 are T-921 to T-923, T-099, T-100 and T-104.
   G-34 (T-104, T-880, T-964, T-965) and G-35 (T-100) are done and shipped
   in `v0.77.0` to `v0.79.0`. cfb's A-X-18 (T-106, partial) waits for
   cfb's answer on #223. Both reply comments are current.
7. Release and hand-off state:
   - `v0.81.0` (`0707cfa`) is the last GitHub release with `release.yml`
     assets and `ghcr.io` images. Its hand-off COORD-151 (#254) is closed
     (call C780): podrunner took `v0.81.0` in PodUtils/podrunner#340,
     podnet in PodUtils/podnet#654, podssh in PodUtils/podssh#337, and
     podvm took `v0.82.0` in PodUtils/podvm#645.
   - `v0.82.0` (`51c09390`) is tagged directly without `release.yml`:
     `publish.yml` 45122406747 passed on `v0.82.0`, while `ci.yml`
     45122401625 failed on `turn-tests.sh` (fixed at `07d4cc4f`). Thus
     `v0.82.0` has no GitHub release, no signed `SHA256SUMS`, no `podlab`
     or fake assets, no `ghcr.io` images and no hand-off COORD issue.
     podnet (`4df28952`), podssh (`e728db9f`) and podvm (`4fa56ec` to
     `a43e80cd`) pin `v0.82.0`; podrunner stays on `v0.81.0`.
   - Next release: the release commit of `v0.82.1` is on `main` and
     waits for `release.yml` ("Releases: v0.82.0 and v0.82.1", below).
   - The pushes of a cloud session of this account start no run of
     GitHub Actions: dispatch workflows via the GitHub MCP tool first,
     and use `[gh action] requested` (#255 is the last) when dispatch
     fails; one request can run the workflows of PodUtils/static-pkgs
     too. COORD-142 (#236) relays the ruling "ghcr.io" (call C789):
     podcrates, podvm and podnet publish in its form, podssh pushes at its
     next tag, and podrunner's T-553 is open.
8. T-767 (PodUtils/podssh#283, partial) and T-939 (done as a guard, call
   C784: a link is lost when a PING waits 30 s and nothing moves): the
   eighth failure of podssh's gate did not come again on the loopback;
   T-945 (done, in `v0.73.0`) gives the client's own line,
   `resume::Note::Stall`, which tells its cause; podssh shows it at `-v`
   at its take. T-959 (done) gives the channel's own line above the
   layer, the event `e2e.wait` of `pod-e2e`: a write of the carry to the
   application or to the layer that takes no byte for 20 s, with the bytes
   of its way. With both lines, the eleventh failure (podssh's run
   44506565889) names podssh's own SSH client as the reader that stopped;
   podssh takes it (PodUtils/podssh@T-780), with no new ask of podcrates.
   T-768 goes with T-767.
9. Then the work order of `TODO/PROGRESS.md`.
10. Run `scripts/agent/check.sh` with a short `TMPDIR` (for example
    `TMPDIR=/tmp`): a long one makes the socket paths of `pod-guest` too
    long for `sun_path` (T-920).

## Releases: v0.82.0 and v0.82.1

`v0.82.0` (`51c09390`, `docs/releases/v0.82.0.md`) is tagged without a
`release.yml` run: it has no GitHub release, no signed `SHA256SUMS`, no
assets, no `ghcr.io` images and no hand-off. Its content is in `v0.82.1`
(call C818).

The release commit of `v0.82.1` (`docs/releases/v0.82.1.md`) is on `main`:
the version of the workspace and of each workspace dependency is 0.82.1,
and `schemas/CHANGES.md` sets the `run-report.v1` row to `v0.82.1`. The
tag waits for `release.yml`:

1. Wait for the green `ci.yml` run of the release commit (`release.yml`
   refuses a commit with no green `ci`).
2. Run `release.yml` with no input (a dry run), then with
   `version=0.82.1` (`docs/maintenance.md`, "A release", steps 3 to 5).
   The steps that run on a tag only are not tested by the dry run.
3. Open the hand-off COORD issue `(to all)` (call C780). It carries the
   parts of `v0.82.0` that podrunner has not taken, and the asks of each
   consumer of `docs/releases/v0.82.1.md`, "How a consumer takes it":
   podrunner T-1110 (`service_account()`) and PodUtils/podrunner@T-595
   (T-1098); podvm T-1124 (`pod_enter::joinable`), `Plan::relay_title` and
   `pod_hostprobe::usage`; podnet T-1117 (the limits 110 s and 210 s);
   podssh iroh 1.4.2, `Unit::public`, `older_mark`, `write_checked` and
   `bind_staged`.

T-963 is partial. T-975 and T-977 (podvm's asks #602 and #604; Q18 blocks
their release part) are open.

## Unfinished work

- `origin/wip/t1073` is merged into `main` (T-1073, T-1070; the new API
  `Context::call_within` of `pod-control`; no consumer must change). The
  coordinator deletes the branch on GitHub by a request `[gh action]
  requested` (the proxy drops `git push origin --delete`).

## What waits

- podvm: T-330 (its pair commands); T-207 (`pod-netlab`, #70) for
  PodUtils/podvm@T-1662.
- podswarm: its asks wait until its session starts, but for T-439,
  T-444, T-447 and T-448 (the ruling "podswarm").
- `pod-git` (T-217 to T-220) is blocked (`PodUtils/podforge`, deferred).
  The rest of M8 waits (the ruling "Priority"); the entries for the CI of
  githome and cfb are in the work order.
- T-386 (the live run of `pod-devtunnel`) waits for the owner's token and
  tunnel (`TODO/PROGRESS.md`, "Owner actions").
