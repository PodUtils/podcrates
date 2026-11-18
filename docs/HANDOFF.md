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
2. The red scheduled runs and their entries:
   - `nightly.yml` at `62ce717`: T-914 (P1, an overflow of the reader of
     Cap'n Proto on a 32-bit target), T-915 (a test of `pod-cloudflare` on
     Windows and macOS), T-916 (the gate of no blocking call and eight
     time bounds under QEMU). T-789 (armv7) and T-766 (riscv64) stay.
   - `upstream.yml`, the job `sources`: red on `pod-manual` until the
     port review of podssh's changes (T-863).
   A new red run needs an entry before other work. A macOS arm64 job of
   `ci.yml` that no runner takes ("not acquired by Runner") is no defect:
   run the failed jobs again.
3. The open hand-off is COORD-132 (#222) for `v0.70.0` (`ed6d84c`). Record
   each take in its body, and close it when each consumer is on `v0.70.0`
   or later.
4. In flight: T-909 (PodUtils/podssh#314 ask 2) on the branch `wip/t909`.
   Part 1 (a byte budget under each cap, `relay::byte_budget_under`) is
   written and not tested. Part 2 (the cap in `forward_layer::Offer`, kept
   in the cache) is not started. Merge the branch into `main` after the
   checks pass, then ask for its delete.
5. `main` holds for the next tag: T-905, T-906, T-908, T-910, T-911,
   T-912, T-913 (COORD-132, section "On `main`").
6. T-767 (PodUtils/podssh#283, partial) waits for podssh's next stall with
   the lines `session.resume` and `session.stall`; T-768 goes with it.
7. Then the work order of `TODO/PROGRESS.md`.

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
  M8 waits (the ruling "Priority").
- T-386 (the live run of `pod-devtunnel`) waits for the owner's token and
  tunnel (`TODO/PROGRESS.md`, "Owner actions").
