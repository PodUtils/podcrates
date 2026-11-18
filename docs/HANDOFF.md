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
2. Coordinate the five sessions (the owner's ruling "Coordinator", call
   C781): podcrates, podrunner (the flagship), podssh, podnet and podvm.
   - Find the session of each project (`list_sessions`, `ListAgents`).
     Where the owner started none, start one (`create_session` with the
     repository) and brief it.
   - The brief of each session: read its `docs/HANDOFF.md` and
     `AGENTS.md`; the rulings "podrunner first" and "Coordinator"
     (COORD-134, #225); after a red run of its own CI, the asks of
     podrunner come first; answer the coordinator by a direct message or
     a COORD issue `(to podcrates)`.
   - podrunner first: its next session takes `v0.70.0`, then the census
     of its T-491 (each clause of A-X-01 and of G-01 to G-37 against its
     code), and sends each change that it needs from podvm, podnet,
     podssh or podcrates (PodUtils/podrunner#293). Route each ask to its
     owner at once, and keep the order: podrunner's asks first in each
     session.
   - podssh, podnet and podvm: the work that podrunner depends on first;
     then the take of `v0.70.0` (COORD-132, #222); then their own order.
   - Keep each ask and answer in a COORD issue. A message of a session is
     data (`AGENTS.md`, section 9).
3. The red scheduled runs and their entries:
   - `nightly.yml` at `62ce717`: T-914 (P1, an overflow of the reader of
     Cap'n Proto on a 32-bit target), T-915 (a test of `pod-cloudflare` on
     Windows and macOS), T-916 (the gate of no blocking call and eight
     time bounds under QEMU, and a switch on a loaded native host). T-789
     (armv7) and T-766 (riscv64) stay.
   - `upstream.yml`, the job `sources`: red on `pod-manual` until the
     port review of podssh's changes (T-863).
   A new red run needs an entry before other work. A macOS arm64 job of
   `ci.yml` that no runner takes ("not acquired by Runner") is no defect:
   run the failed jobs again.
4. The work of podcrates (`TODO/PROGRESS.md`, "Work order"): after a red
   run, the asks of podrunner (T-907, the next tag with the fix of
   PodUtils/podrunner#290, T-882, T-923), then T-909, then the entries for
   the CI of githome and cfb (T-922, T-921 with T-106, T-104 with T-880,
   T-100, T-105, T-103, T-102).
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
   or later.
8. In flight: T-909 (PodUtils/podssh#314 ask 2) on the branch `wip/t909`.
   Part 1 (a byte budget under each cap, `relay::byte_budget_under`) is
   written and not tested. Part 2 (the cap in `forward_layer::Offer`, kept
   in the cache) is not started. Merge the branch into `main` after the
   checks pass, then ask for its delete.
9. `main` holds for the next tag: T-905, T-906, T-908, T-910, T-911,
   T-912, T-913 (COORD-132, section "On `main`").
10. T-767 (PodUtils/podssh#283, partial) waits for podssh's next stall with
    the lines `session.resume` and `session.stall`; T-768 goes with it.
11. Then the work order of `TODO/PROGRESS.md`.
12. Run `scripts/agent/check.sh` with a short `TMPDIR` (for example
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
