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
   - `nightly.yml` 44739339618 at `8a2cdd9`: its new failures have entries
     on `main`: T-1040 (a doc test of `pod-control` on Windows), T-1043 (a
     race of a test of `pod-link` on armv7), T-1041 and T-1042 (macOS: a
     timeout of a closed Unix socket; Node of Homebrew reads
     `SSL_CERT_FILE`). The next nightly proves them, and T-1021 and T-1030
     (PATHEXT on Windows). T-1044: the checks of Windows and macOS here
     compile no doc test. Open: T-916 (two `pod-iroh` tests on riscv64
     and loongarch64), T-938. T-789 and T-766 stay.
   - `nightly.yml` 44848003241 at `a7fb465` (#252): two red jobs. i686:
     `a_turn_of_the_arq_blocks_on_nothing` counted 3 switches; T-1082 is
     done (each gate of `switches::count` in a test binary of its own).
     riscv64: `pod-iroh` `road.rs` and `share.rs` ("early eof"), T-916;
     the job took 111 of its 150 minutes (T-938). The next nightly proves
     T-1082.
   - `nightly.yml` 44871383085 at `ab73e75` (#255): it was running at the
     end of the last session. It is the first live run of T-1026 (the
     step "A Windows service of pod-unit" on windows-2025 and
     windows-11-arm) and of the Windows path of `install_system`. Read it
     first: a red step of it is an entry before other work. `ci`,
     `adoption` and `plants` of #255 passed on `ab73e75`.
   - `ci.yml`: T-943 is done: the arm64 probe of macOS builds on the
     Intel runner.
   - `docs/research/hard-problems.md`: the hard open problems (H-01 to
     H-18), with the questions that research must answer. T-978 has its
     findings that no other entry had.
   - `upstream.yml`, the job `sources`: red on `pod-manual` until the
     port review of podssh's changes (T-863).
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
   G-34 and G-35 are done; the last part of G-34 (T-964) goes in the next
   tag. cfb's A-X-18 (T-106, partial) waits for cfb's answer on #223. Both
   reply comments are current.
7. The hand-off of `v0.81.0` is open: COORD-151 (#254). podrunner
   (PodUtils/podrunner#340), podnet (PodUtils/podnet#654) and podssh
   (PodUtils/podssh#337) took it at 2026-11-21; podvm has no take. Write the take of
   podvm in its "Takes" list, and close #254 when podvm took `v0.81.0` or a
   later tag (call C780). The pushes of a cloud session of this
   account start no run of GitHub Actions: each gate, nightly and release
   is a request `[gh action] requested` (#255 is the last); one
   request can run the workflows of PodUtils/static-pkgs too. COORD-142 (#236) relays the ruling "ghcr.io" (call C789):
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

## The next tag

`v0.82.0` is prepared on `main` (`docs/releases/v0.82.0.md`) for push, green
`ci.yml`, and `release.yml`. It carries each fix of `main` after `v0.81.0`
plus the five consolidated crates and transport/security updates:

- T-1086: `pod-extract` sets the ACLs of a layer of GNU tar `--acls`
  (`SCHILY.acl.*` to `system.posix_acl_*`, counted in `xattrs`); a GNU
  sparse file now refuses its layer (T-1087); call C899.
- T-1080: `pod-enter` gives 126 to a file that `execve` found and refused
  (`E2BIG`, `EINVAL`, `ELIBBAD` and five more), and the search of the path
  keeps the errno of the refused file (PodUtils/podvm#639). podvm drops its
  read of the text `E2BIG (7)` at its take.
- T-1026: `pod_unit::windows::serve`, the dispatcher of the service
  control manager. The README of `pod-unit`, "The dispatcher of a Windows
  service", has the map of podrunner's loop (`ready`, `stop`, `stopping`,
  the exit code).
- T-1060: the server of `pod-control` wipes each answer that it drops.
- T-1082: each gate of no blocking call runs alone in its test binary.
- Consolidated crates and updates: `pod-shape`, `pod-cover`, `pod-harness`,
  `pod-sidecar` (`sidecar` feature off by default), `pod-offline`,
  `pod-link` (`ntp`/`smtp`/`imap` + IMAP `max_fetch` and `command_timeout`),
  `pod-e2e` (shape + destination envelope), `pod-turn` (T-501 RFC 6062 TCP
  relay), `pod-quic` (`migrate`), `pod-warp` (`provider`, H3 shape with
  open control stream), `pod-egress` (`coalesce`, `replay`, `vless::early`,
  Trojan UDP refusal, VMess nonce-wrap bound), `pod-ws` (HTTP/2 shape and
  fallback), `pod-testkit` (`vectors`), `pod-core` (`Class::Permission`,
  `Class::Suite`), `pod-manual` (`failure_classes`), and `url` 2.5.9
  (RUSTSEC-2026-0592).

T-966 (podnet's half waits for PodUtils/podnet@T-494) and T-963 are
partial. T-975 and T-977 (podvm's asks #602 and #604; Q18 blocks their
release part) are open. Before the release, read `docs/maintenance.md`, "A
release": the steps of `release.yml` that run on a tag only are not tested
by the dry run.

## Unfinished work

- `wip/t1073` (base `991c600b`): T-1073 (one deadline of the server for
  a call back of `pod-control`) and T-1070 (`Server::active` counts a
  refused connection for a moment). Its first commit has the code, the
  tests and four plants (`control-refused-counts-as-place`,
  `control-call-back-blank-lines-renew`,
  `control-line-write-timeout-each-write`,
  `control-write-after-part-of-line`); fmt and clippy passed, and each new
  test passed 20 of 20 runs. Its second commit is the record of done. The
  workspace tests, the doc, `cargo todo check`, `lint.sh`,
  `check-windows.sh` and `check-macos.sh` did not run. Next: merge the
  branch into `main` locally, run each check of AGENTS.md, fix a fault,
  and push; then delete the branch by a request `[gh action] requested`
  (the proxy drops `git push origin --delete`). The new API is
  `Context::call_within`; no consumer must change.

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
