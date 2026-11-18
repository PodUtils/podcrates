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
   - `nightly.yml` 44275593101 at `2b11314` (the proof of T-916): the
     cross jobs of armv7, arm, aarch64, loongarch64 and powerpc64le pass.
     Open: T-916 (i686 runs natively and counts 3 switches; two
     `pod-iroh` tests on riscv64), T-934 (an exec proxy through `sh -c`
     on Windows), T-935 (two renames onto one name on Windows), T-938 (the
     riscv64 job meets its 90 minutes). T-933 (three tests of
     `pod-cloudflare` on Windows) and T-936 (`refused()` on macOS) wait
     for the proof of the next nightly run. T-789 and T-766 stay.
   - `ci.yml`: T-943 builds the arm64 probe of macOS on the Intel runner;
     it is partial until three runs of `ci.yml` in a row are green.
   - `upstream.yml`, the job `sources`: red on `pod-manual` until the
     port review of podssh's changes (T-863).
   A new red run needs an entry before other work. A plant that a run
   misses once with no change of its crate runs its test five times in
   its `.plant` (as `link-quic-drop-closes-at-once`).
4. The work of podcrates (`TODO/PROGRESS.md`, "Work order"): the red
   nightly above, then the entries for the CI of githome and cfb (T-106,
   T-104 with T-880, T-100, T-105, T-103, T-102).
5. T-922 is on `main`, for the next tag: `pod_job::{Status, SourceStatus,
   KindStatus, PinStatus}` and `pod_job::source` (the source wire of
   podrunner at `ca8c308`), `schemas/status.v1`, `source-request.v1`,
   `source-reply.v1`, and `schemas/CHANGES.md` with its check
   (`scripts/check-schemas.py`). The hand-off of the next tag tells
   podrunner what changes at its take (`crates/pod-job/README.md`, the
   adoption map: `Done {}`, `Minted`, the strict `status.v1`).
   T-921 is on `main` for the next tag too: the fake GitHub
   `pod-fake-github` (feature `github` of `pod-testkit`, call C785), a
   release asset for x86_64 and aarch64 musl. The hand-off tells githome
   to pin it by its line of `SHA256SUMS` (G-30), and podrunner that its
   routes can take the place of `podrunner-broker-fake` for a job of
   `run:` steps (`crates/pod-testkit/README.md`).
   T-948 is on `main` for the next tag too: the fake of githome's source
   `pod-fake-githome` (feature `githome` of `pod-testkit`, call C786), a
   release asset for x86_64 and aarch64 musl. The hand-off tells
   podrunner, for PodUtils/podrunner@T-509 (G-38,
   PodUtils/podrunner#306), to pin it by its line of `SHA256SUMS`, to run
   `pod-fake-githome serve`, and to name `pod-fake-githome source` as the
   program of a `[[source]]` with `POD_FAKE_GITHOME`
   (`crates/pod-testkit/README.md`).
6. The channel of cfb and githome is #223. podcrates keeps one reply
   comment to each there (cfb: 6970315549; githome: 6970683028) and edits
   it, with the UTC time of each update and of each acknowledgement. cfb
   is offline until the CI of podrunner and githome is complete; it reads
   `docs/research/fleet/asks.md` and the block of #223 at its next start.
   githome's asks G-30 to G-35 are T-921 to T-923, T-099, T-100 and T-104.
7. The hand-off of `v0.72.0` is open (COORD-138, #229): podrunner takes
   T-922 (the adoption map of `crates/pod-job/README.md`) and T-882;
   podvm takes T-944; podssh takes T-909 and T-939. Record each take on
   #229, and close it when each consumer names the tag (call C780).
8. T-767 (PodUtils/podssh#283, partial) and T-939 (done as a guard, call
   C784: a link is lost when a PING waits 30 s and nothing moves): the
   eighth failure of podssh's gate did not come again on the loopback;
   T-945 (done, on `main` for the next tag) gives the client's own line,
   `resume::Note::Stall`, which tells its cause; podssh shows it at `-v`
   at its take. T-768 goes with T-767.
9. Then the work order of `TODO/PROGRESS.md`.
10. Run `scripts/agent/check.sh` with a short `TMPDIR` (for example
    `TMPDIR=/tmp`): a long one makes the socket paths of `pod-guest` too
    long for `sun_path` (T-920).

## Unfinished work

- #208 asks the owner's agent to delete the `wip/` branches whose work is
  on `main` (`wip/t909` is on `main` too). The proxy of a cloud session drops
  `git push origin --delete`. The branch
  `dependabot/docker/images/trufflehog/trufflesecurity/trufflehog-3.99.8`
  of the closed pull request #79 also stays.

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
