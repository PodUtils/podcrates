# Handoff

The state of unfinished work, for a session with no memory of the last
one. Read this page first, then `AGENTS.md`, `TODO/PROGRESS.md` (the
state, the work order, the questions), `docs/maintenance.md` (the
procedures, the tools and the rules of the work), `docs/design.md` and
`docs/decisions.md`. Keep this page current: remove a line when its work
is done. Git and the record keep finished work.

## The first steps of the next session

1. Start the watcher (`sh scripts/agent/watch-org.sh 90` under the Monitor
   tool) and read the open COORD issues `(to podcrates)`.
2. Read the answer of podnet on #139 (the profile of `kcp`, T-526) and the
   takes of `v0.40.0` on its hand-off issue #140.
3. Take the work order of `TODO/PROGRESS.md`: the red run first (T-537),
   then T-539 (the termios mode of `pod-term`, in work on the local branch
   w-term539) and the link carriers (item 3c).

## In flight

- The last release: `v0.40.0` at `1335e9a` (ci 43467923338, adoption
  43467923371, release 43474412563; the hand-off #140, COORD-080): the
  poll, quic, udp, dns and icmp carriers of `pod-link`, `cpu_s` and
  `peak_rss_mib` of `pod-job`, the forms of the file hosts. The takes of
  `v0.39.0`: each consumer (#137, closed).
- Each agent of this session is merged into `main`. The open threads of
  this session:
  - T-539 (open): the termios mode of `pod-term` (PodUtils/podssh#232).
    An agent works on it in the worktree `.claude/worktrees/term539`.
  - T-533 (open): scripts/agent/check-macos.sh, a local check of the
    build for macOS.
  - T-526 (partial): the fix of `kcp` upstream. podnet sets the profile of
    `kcp` with its take of the udp carrier (#139, PodUtils/podnet#527).
- The red runs that the record knows:
  - `nightly.yml` at `72632b2` (run 43451625236): macOS failed to build
    the icmp carrier (`3d6bbf0` fixes it); riscv64 and armv7 fail bounds
    of the tests of `pod-link` (T-535).
  - `ci.yml` at `3d6bbf0` (run 43459943301): a random failure of the
    test of `pod-oci` against the distribution registry (T-536).
  - `services.yml` at `4c2d17c` (run 43402100735): a tox node gives no
    TCP relay (T-537). The run had no first "no answer", so T-351 still
    waits for its proof.
  - `plants.yml` at `6b183f6` (run 43448941361): the plant
    `link-udp-fragment-taken` was missed; its `EXPECT` takes the stall
    now (T-521, Correction). The next run of `plants.yml` proves it.
- podswarm's tracker is PodUtils/podswarm#1 (moved from #107). githome
  builds its side of a live test (#83): it waits for T-106 (the
  conformance binary of `pod-testkit`) and T-100 (`pod-broker`), which
  stay in M8 under the ruling "Open questions".
- `pod-warp`: podnet and podssh took the crate in `v0.37.0`; their WARP
  code holds fixes only (#135). T-339 (partial) holds the rest.
- The disk of a cloud session is small: `target/debug/incremental` grows
  to some GB. Run the workspace tests with `CARGO_INCREMENTAL=0` when the
  disk is short.
- T-040 (partial): the trials of podvm, with each import for podvm.

## What waits

- podvm (on `v0.39.0`, `19892c50`, PodUtils/podvm#309; `v0.2.0-beta.8`
  out): T-330 (its pair commands); T-207 (`pod-netlab`, #70) for
  PodUtils/podvm@T-1662; `exec_attached` of `pod-incus` if
  PodUtils/podvm@T-1722 asks. Write to podvm by COORD issues only.
- podnet (on `v0.39.0`, `306d80b`, PodUtils/podnet#526; `pod-warp`
  taken): the rest of its link workstream (#133: T-494, T-500, then group
  d), then T-029 (`pod-cloudflare`) on the chain to WARP (the owner's
  order). Write to podnet by COORD issues only.
- podssh (on `v0.39.0`, `5228b65`, PodUtils/podssh#229; `pod-warp`
  taken): the live runs of its PodUtils/podssh@T-737 (the brokers) and
  PodUtils/podssh@T-571 (`syncthing-relay`); `sandhole` and
  `openbare-xyz` wait for PodUtils/podssh@T-743 and PodUtils/podssh@T-740.
- podrunner (on `v0.39.0`, `69d7b14`, PodUtils/podrunner#84): its session
  runs with podvm. Its T-108 fills `cpu_s` and `peak_rss_mib` with
  `v0.40.0` (PodUtils/podrunner#86). Write to it by an issue with
  `(to podrunner)` in the title.
- podswarm (#107 to #125): the owner's ruling "podswarm" (2026-11-13).
  T-439, T-444, T-447 and T-448 help a sibling and are in the work order;
  the other asks wait until the podswarm session starts.
- `pod-git` (T-217 to T-220) is blocked: `PodUtils/podforge`, deferred
  (the owner; #75, #131). M8 (cfb and githome, #35 and #36) waits (the
  ruling "Priority").

## Channels

- Each session talks with the others by COORD issues alone, no direct
  message (the owner, 2026-11-12, the ruling "Tonight"). A session writes
  only to its own repository: link a consumer's COORD issue from the
  hand-off issue of podcrates. Do not start a session of a consumer.
- Workflows: dispatch them with the GitHub MCP tool
  (`docs/maintenance.md`, "A run of a workflow"). `services.yml` with the
  kind `forward-url` reads the banners of the SSH forward relays (no
  session has SSH).
