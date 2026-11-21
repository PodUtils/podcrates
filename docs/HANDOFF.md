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
     compile no doc test. Open: T-916 (i686 counts 3 switches; two
     `pod-iroh` tests on riscv64 and loongarch64), T-938. T-789 and T-766
     stay.
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
7. The open hand-off is COORD-150 (#250, `v0.80.0`); its "Takes" list
   holds each take. Write each take of the next tag in the "Takes" list of its issue, and
   close it when each consumer took its tag or a later one (call C780). The hand-off of the
   next tag names T-1014 (the static QEMU 11.1.3) for podvm. The pushes of a cloud session of this
   account start no run of GitHub Actions: each gate, nightly and release
   is a request `[gh action] requested` (#249 is the last, closed); one
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

It carries each fix of `main` after `v0.80.0`, and T-1014 (done, proven
by `services.yml` 44806797116 and `guest.yml` 44809029300): the pin of the
static QEMU 11.1.3 of PodUtils/static-pkgs (release `qemu-11.1.3`, built
from QEMU `v11.1.3` with glib and libaio from pinned sources and the source
of each LGPL part as an asset), for the save of an NVMe disk (T-1035).
T-966 (podnet's half waits for PodUtils/podnet@T-494) and T-963 are
partial. T-975 and T-977 (podvm's asks #602 and #604; Q18 blocks their
release part) are open. Before the release, read `docs/maintenance.md`, "A
release": the steps of `release.yml` that run on a tag only are not tested
by the dry run.

## Unfinished work

- #208 asks the owner's agent to delete the `wip/` branches whose work is
  on `main`: `wip/T-717-installer-sign`, `wip/T-746-way-out`,
  `wip/egress-part2`, `wip/egress-part3` and `wip/t909`. The proxy of a
  cloud session drops `git push origin --delete`. The two branches
  `dependabot/docker/images/coturn/coturn/coturn-4.19.0` and
  `dependabot/docker/images/trufflehog/trufflesecurity/trufflehog-3.99.8`
  also stay: their pull requests (#232, #79) are closed, and `main` has
  the newer pins.

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
