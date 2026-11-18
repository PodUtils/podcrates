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
   - `nightly.yml` at `62ce717`: T-914 and T-915 are done. T-916 is
     partial: the fixes are on `main`; its proof is a run of `nightly.yml`
     (dispatch it, and read the cross jobs). T-789 (armv7) and T-766
     (riscv64) stay.
   - `upstream.yml`, the job `sources`: red on `pod-manual` until the
     port review of podssh's changes (T-863).
   A new red run needs an entry before other work. A macOS arm64 job of
   `ci.yml` that no runner takes ("not acquired by Runner") is no defect:
   run the failed jobs again.
4. The work of podcrates (`TODO/PROGRESS.md`, "Work order"): the tag
   `v0.71.0` (T-905, T-906, T-907 `pod-time`, T-908, T-910 to T-913,
   T-914, T-915, T-924 `FileLock`), then T-882, T-923, T-925 (`pod-incus`
   for podvm, PodUtils/podvm#582), then T-909, then the entries for the CI
   of githome and cfb (T-922, T-921 with T-106, T-104 with T-880, T-100,
   T-105, T-103, T-102).
5. podrunner's answer to COORD-135 (PodUtils/podrunner#294, at podrunner
   `3a0cbbb`) is in T-922 (the types and files of `status.v1` and of the
   source wire) and T-923 (the names and units of the new fields). Set
   the order of the move with podrunner: the wire moves after podrunner's
   T-486, T-493 and T-495.
6. The channel of cfb and githome is #223. podcrates keeps one reply
   comment to each there (cfb: 6970315549; githome: 6970683028) and edits
   it, with the UTC time of each update and of each acknowledgement. cfb
   is offline until the CI of podrunner and githome is complete; it reads
   `docs/research/fleet/asks.md` and the block of #223 at its next start.
   githome's asks G-30 to G-35 are T-921 to T-923, T-099, T-100 and T-104.
7. The open hand-off is COORD-132 (#222) for `v0.70.0` (`ed6d84c`). Record
   each take in its body, and close it when each consumer is on `v0.70.0`
   or later. Takes not yet in its body: podnet `7573016`
   (PodUtils/podnet#654), podrunner `9c67bb3` (PodUtils/podrunner#295).
8. In flight: T-909 (PodUtils/podssh#314 ask 2) on the branch `wip/t909`.
   Part 1 (a byte budget under each cap, `relay::byte_budget_under`) is
   written and not tested. Part 2 (the cap in `forward_layer::Offer`, kept
   in the cache) is not started. Merge the branch into `main` after the
   checks pass, then ask for its delete.
9. T-767 (PodUtils/podssh#283, partial) waits for podssh's next stall with
   the lines `session.resume` and `session.stall`; T-768 goes with it.
10. Then the work order of `TODO/PROGRESS.md`.
11. Run `scripts/agent/check.sh` with a short `TMPDIR` (for example
    `TMPDIR=/tmp`): a long one makes the socket paths of `pod-guest` too
    long for `sun_path` (T-920).

## Unfinished work

- #208 asks the owner's agent to delete the `wip/` branches whose work is
  on `main`. The proxy of a cloud session drops
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
