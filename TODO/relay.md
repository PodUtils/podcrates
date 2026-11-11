This file holds the entries of milestone M2: the QUIC stack, the layer,
the end-to-end channel, the code words, the userspace stack, WireGuard and
the relay client. Each crate follows the procedure of `docs/design.md`,
section 8.

# T-018: One QUIC stack: quinn against noq, measured

**Source:** `docs/decisions.md`, call C10.
**Category:** research
**Milestone:** M2
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

podnet uses quinn 0.11 (the edge of Cloudflare, the quic+warp leg, MASQUE
over HTTP/3, QUIC over WARP through `WarpUdpSocket`). podssh uses noq 1.4,
the fork of quinn by n0 (the quick tunnel, the QUIC road, iroh). One
binary must carry one QUIC stack.

## Premise

- Read: podnet `PodUtils/podnet:src/warp/quic.rs`,
  `PodUtils/podnet:src/cloudflare/quic.rs`; podssh
  `PodUtils/podssh:crates/podssh-relay/src/quick/quic.rs`,
  `PodUtils/podssh:crates/podssh-relay/src/quic_road/`.
- noq is the stack of iroh, which `pod-iroh` keeps.

## Approach

1. List each use of each stack: datagrams, a custom UDP socket, the
   transport configuration, the provider.
2. Port the smallest use to the other stack, both ways, and measure the
   build size and the throughput on the loopback.
3. Write the choice in `docs/decisions.md`.

## Prove

```sh
cargo todo check
python3 -I scripts/check-design-paths.py
```

`docs/research/quic/stack.md` gives the measurements of both stacks.

## Start condition

T-016 is done.

## Done

2026-11-10, the commit of this entry. The choice is noq 1.4 (C43,
`docs/research/quic/stack.md`). Two scratch crates ran one client (custom
UDP socket in the shape of `WarpUdpSocket`, datagrams, a stream) on
quinn 0.11.12 and on noq 1.4.0, with rustls on ring. Both link static on
x86_64, aarch64, i686, armv7, arm, riscv64gc, powerpc64le and loongarch64
musl (cargo-zigbuild). x86_64 musl: 1,857,400 bytes (quinn) against
2,077,344 (noq); 130 against 185 lines of `cargo tree`, 60 against 84
unique crates; a 1 GiB stream on a stock socket 322 against 286 MiB/s, on
the custom socket 253 against 236 MiB/s; datagrams on a stock socket
395,000 against 332,000 for each second, on the custom socket 284,000
against 250,000 (3 runs each, medians). iroh pins noq and needs its
multipath and NAT traversal; podnet's HTTP/3 is its own code on the raw
connection, so it needs no `h3` binding. Defect found: the send poller of
`WarpUdpSocket` is in the Approach of T-019.

# T-019: `pod-quic`: the QUIC endpoint, the probe and QUIC over datagrams

**Source:** `docs/design.md`, section 4 (M2).
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

`pod-relay`, `pod-warp` and `pod-cloudflare` share the QUIC endpoint, the
QUIC probe and QUIC over `Datagrams`.

## Premise

- Read: podssh `quick/quic.rs`, `quick/probe.rs`; podnet
  `PodUtils/podnet:src/warp/quic.rs` (the client configuration is an
  argument: podnet's edge 10 of its architecture study).

## Approach

The procedure of `docs/design.md`, section 8, on noq (T-018, C43). Map
`Datagrams` to noq's `AsyncUdpSocket` and `UdpSender` (`create_sender`,
`poll_recv(&mut self)`). Give each sender its own waker (for example
`tokio_util::sync::PollSender`): the send poller of podnet's
`WarpUdpSocket` creates a `Notified` for each poll and drops it on
`Pending`, and tokio removes a dropped `Notified` from the list of
`notify_waiters`, so a full queue can lose its wake-up. Test a full queue
that drains. Implement the reset key and the token key in pure Rust.

## Prove

```sh
cargo test -p pod-quic
sh scripts/agent/check.sh
```

## Start condition

T-018 is done.

## Done

2026-11-10, partial: the release `v0.14.0` and the takes of podssh and
podnet remain. The patches are made again at the heads of podssh
(`c5636c9`, after its take of `v0.13.0` at `a083593e`) and podnet
(`5465007`), and both trials pass there. podnet's fix of T-159
(`1d556fb`) is not on podnet's `main` at `5465007`; it changes
`src/warp/quic.rs`, which the podnet patch replaces, so the patch must be
made again (and its trial run again) when that commit reaches `main`
before the take. Commits
`8a39c0e` (the import of podssh `quick/quic.rs`, `quick/probe.rs`, the
`TokenKey` of `serve/quic.rs` and the tests at `af214a6`, and podnet
`src/warp/quic.rs` at `2fed3ff`, after the fix of PodUtils/podnet@T-225,
with `SOURCES`), `e8f36bd` (the adaptation on noq, the tests), `1f4d436`
(a test of `pod-enter` that skips when nobody cannot enter the temporary
directory, found by the checks), `f81f441` (README, the static probe, the
fuzz target, the row of `docs/design.md`, C44), `009df49` (the trials),
the commit that gives `connect::tls` rustls's own verifier (the guard
`no_other_code_sets_its_own_verifier` of `pod-tls` found the call of
`dangerous()` of podssh's form), and the commit of this record.

- API: `Settings` (cloudflared's by default; `transport(v6)`), `keys`
  (`ResetKey`, `TokenKey`: HMAC-SHA-256 in pure Rust, from an `Entropy`),
  `endpoint` (`config`, `server_config`, `on_socket`, `bind`, `system`,
  `server`, `over` any `pod_core::Datagrams`), `connect` (`tls` on the
  caller's provider, `client_config`, `connect`, `handshake` as a race of
  addresses), `probe` (`packet`, `answers`, `probe`), `queued`
  (`QueuedDatagrams` over a `Link`), `socket` (`DatagramSocket`, noq's
  socket over a `Datagrams`). Each failure is a `pod_core::Failure`. noq
  1.4 with no default feature: no ring, no aws-lc. The crate depends on
  `pod-core` alone (C44); 10 source files, each under 500 lines.
- Cut: podssh keeps `server_name`, `alpn`, `connect_edge`,
  `EdgeConnection`, the resolution of the regions of an edge (`handshake`,
  `probe::edge`) and the role's server until `pod-cloudflare` and
  `pod-relay`. podnet keeps `warp::quic::endpoint` and `client_config`
  (its trust, the MTU of its tunnel) over pod-quic, and moves each user of
  quinn to noq in the same patch (`src/cloudflare/{mod,quic,rpc}.rs`,
  `src/proxy.rs`, `src/tunnel.rs`, `src/warp/{h3,l4,quic,session}.rs`,
  `src/ws.rs`): no quinn stays in podnet.
- Defects: noq makes one sender for each task and a `Datagrams` keeps one
  waker, so a second sender that parks on a full way takes the waker of
  the first. `socket.rs` gives the way a fan-out waker;
  `two_senders_on_a_full_way_each_wake` fails with the task's waker given
  to the way (the first sender is not woken) and passes with the fan-out.
  The same holds for podnet on quinn now: T-159. The test
  `resolve_past_a_denied_listing` of `pod-enter` failed with a `TMPDIR`
  under `/root` (EACCES of the child as nobody); it skips now with a
  reason (`1f4d436`).
- Tests: 24, each with its name from its source: `probe` (3 unit, 2 of
  podssh's `quick_probe.rs`), `a_token_opens_only_as_it_was_sealed`, the 3
  `doctors_handshake_*` of podssh's `quick_tunnel_quic.rs`, podnet's
  `send_poller_reports_ready_when_the_queue_has_room` and
  `a_drain_after_pending_wakes_the_send_poller`; new: the keys (2), the
  settings (2), the socket (3), the queue over a link (2), an untrusted
  server, and a stream of 1 MiB with a burst of datagrams over the
  system's `Datagrams`, over a queued link and over a queued link of two
  datagrams. The same 24 tests of the four test binaries pass as i686
  musl binaries and, under qemu-user, as aarch64, riscv64gc and
  loongarch64 musl binaries (static). The fuzz target
  `quic_probe_answer` passes the stable test of `fuzz/`.
- Trials at the heads (the hand-off): podssh at `c5636c9` (17 files, 69
  lines in, 466 out; the tags of each manifest move from `v0.13.0` to
  `v0.14.0`): each step of `sh scripts/adoption.sh podssh pod-quic`
  passed, with 11 test binaries and 1,697 tests of `podssh-relay` and
  `podssh-cli`, 0 failed. podnet at `5465007` (18 files, 157 lines in, 405
  out; the patch of `2fed3ff` applies with no change but its context):
  each step of `sh scripts/adoption.sh podnet pod-quic` passed, with no
  quinn in its tree and 6 test binaries and 512 tests. Each has the
  static check of its musl binary. The first trials, at podssh `af214a6`
  and podnet `2fed3ff` (`009df49`), passed with the same counts. The
  imported files did not change between the two heads of each consumer
  (`check-sources.py`: 0 changes).
- Checks: `sh scripts/agent/check.sh` (fmt, clippy, doc, record, size,
  paths, the static probes, the tests); `python3 -I
  scripts/check-sources.py` on `pod-quic` (podssh 5 files, podnet 1 file,
  0 changes); `python3 -I scripts/check-probe.py`.

- Released in `v0.14.0` (`c8e2fae`, release 42833358262; GitHub #50).
  podssh took `pod-quic` at `525f8907` (PodUtils/podssh#185; no conflict;
  each line of the `.run` passed; 3,377 tests in a namespace with the
  loopback alone) and podnet at `b7b0aef` (PodUtils/podnet#421; it kept the
  patch's `src/warp/quic.rs` over its own fix of PodUtils/podnet@T-228; no quinn is left).
  Both `taken` lines are in `crates/pod-quic/SOURCES`; the snapshot of
  podnet's file moves to the version with PodUtils/podnet@T-228 (`1d556fb`), which the
  crate has in its own form (check-sources: 0 changes). The trial of
  podnet ends; podssh's `pod-quic.patch` stays while `pod-wormhole.needs`
  names it.

# T-020: `pod-session`: the resumable layer and UDP in a stream

**Source:** `docs/design.md`, section 4 (M2).
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** L
**Status:** done

## Problem

podssh's resumable layer (64-bit offsets each way, resume through any
road, replay, heartbeat, the move before the limits of the relay,
handover to a new process) is the base of the relay roads of podssh and
of the transfer of podnet.

## Premise

- Read: `PodUtils/podssh:crates/podssh-relay/src/session/` (7,468, with
  `handover/`), `udp/` (352), `answer.rs`, `transit/link.rs`.
- Tests: 21 files `PodUtils/podssh:crates/podssh-relay/tests/session_*.rs`
  and `udp_framing.rs`.
- `session/` changed 38 times in the last 50 commits of podssh: the
  freeze of `docs/decisions.md` (Who edits) applies from the import.

## Approach

The procedure of `docs/design.md`, section 8. The variables
(`PODSSH_REPLAY_BUFFER` and the others) become fields of the settings.

## Prove

```sh
cargo test -p pod-session
sh scripts/agent/check.sh
```

## Start condition

T-010 and T-016 are done.

## Done

2026-11-10, partial: the take of podssh remains (the release `v0.15.0` is
at `27a4ccd`; the hand-off is GitHub #54)
(podnet takes the crate later, PodUtils/podnet@T-086). Commits `e1a1256`
(the import of `PodUtils/podssh:crates/podssh-relay/src/session/`, `src/udp/`,
`src/answer.rs`, `src/transit/link.rs` and the tests at `c5636c9`, with
`SOURCES`), `4e6522b` (the adaptation), `b736f04` (README, the static
probe, the fuzz targets, the row of `docs/design.md`), `70c7f99` (the
plants of podssh's layer and the test of the budget), `70f2a0c` (the
trial, `adoption/podssh/pod-session.*`) and the commit of this record.

- API: `Settings` (with `pair_window`, `move_bytes`, `move_age`: the
  limits of the relay), the records, the decoder, the offsets, the replay
  buffer, the secrets, the handshakes (sans-IO), the link, the two ends,
  the pump, the resume, the spare link, the race of roads, the keeper and
  its budget (`Keeper::with_tell`), the handover (`Movable` for a TCP
  target, `Keeper::hand_over_with`), `udp` (the framing and a flow through
  the caller's `Resolve`), `ws_link` (a link over a `pod_ws::RelaySession`),
  `answer`. Each error converts to a `pod_core::Failure` of the stage
  `Session` (`failure.rs`). The crate depends on `pod-core` and `pod-ws`
  (C47); 37 source files, each under 500 lines; no variable of the
  environment and no global (the task-local id of a handover aside).
- Cut: podssh keeps a glue `src/session/` (`pub use pod_session::*`, its
  sink of the budget's lines `budget::{tell, say, told}` with its words,
  and its own `handover::Movable`, which the pipe of `e2e/hold` takes),
  `PODSSH_REPLAY_BUFFER`, the tests `session_policy.rs` and
  `session_relay_resume.rs` (until `pod-relay`) and the handover test
  with the channel (until `pod-e2e`). podnet has no copy of the layer.
- Defects: the orphan rule stops a consumer from giving the crate's
  `Movable` to tokio's `DuplexStream`, which podssh's end-to-end channel
  moves at a handover: `Keeper::hand_over_with` takes the move as a
  function, and podssh keeps its trait. The plant of PodUtils/podssh@T-586 (a
  closing session charged a whole buffer) had its check in podssh's
  `reverse_layered.rs`: no test of the crate caught it. The new test
  `a_session_whose_target_ended_holds_only_its_unacknowledged_bytes`
  (`tests/session_budget.rs`) fails on the plant and passes without it.
  The budget's lines were a process global (`budget::tell`): a field of
  the keeper now.
- Tests: 147 pass (`cargo test -p pod-session`): 143 of podssh's 144 with
  their names, and 4 new (the classes of the failures, a host that refuses
  UDP, the sink of a `Tell`, the budget of a closing session). The tests
  that bind start with `need_loopback!`. The plants of podssh's layer
  (14, group `session`): each caught (`sh scripts/plant-each.sh --group
  session`). The fuzz targets `session_record`, `session_handover` and
  `udp_frame` pass the stable test of `fuzz/` (3 tests).
- Trial (the hand-off): podssh at its `main` `9975fe92` (after its take
  of `pod-quic`), after `pod-wormhole.patch` (`pod-session.needs`): 118
  files, 306 lines in, 13,745 out; the tags move to `v0.15.0`. Each step
  of `sh scripts/adoption.sh podssh pod-session` passed: the record (the
  citations of the removed files as `PodUtils/podssh:PATH`, a
  `## Correction` on 15 done entries whose Prove names a moved test
  target), `check-repo.py`, the builds, clippy with each feature of
  `podssh-relay`, 25 test binaries of `podssh-relay`, `podssh-cli` and
  `podssh-iroh` with 1,772 tests and 0 failed, rustfmt, the toolchains,
  the 16 test targets of the gate, and the static check of the musl
  binary of the release's features. An earlier run at podssh `f9113862`
  after the patches of `pod-quic` and `pod-wormhole` passed with 1,789
  tests. The imported files did not change on podssh's `main`
  (`check-sources.py`: 56 files, 0 changes).
- Cross: the 22 test binaries of the crate, built with cargo-zigbuild,
  are static; as i686 musl binaries 147 tests passed; under qemu-user
  (`QEMU_LD_PREFIX=/`) as aarch64 and riscv64gc musl binaries 146 passed
  and `a_session_moves_before_each_cap` failed at random (a link met its
  cap on the slow host): T-167.
- Checks: `sh scripts/agent/check.sh`: fmt, clippy, doc, record, size,
  paths and the static probes passed; the tests passed with `/tmp` (1,900
  passed, 0 failed); with a `TMPDIR` of 75 bytes `pod-gsr`'s
  `blocking.rs` failed at the bind of a Unix socket (T-168), and
  `pod-enter`'s `a_missing_workdir_in_a_volume_names_the_volume`
  expected a host that the message cuts at 64 bytes (repaired,
  `1177350`). `python3 -I scripts/check-sources.py` on `pod-session`
  (podssh, 56 files, 0 changes; a planted change found);
  `python3 -I scripts/check-probe.py`; `cargo test --manifest-path
  fuzz/Cargo.toml --lib` (3 passed).
- 2026-11-11, done: podssh took the crate at `6f78a7c7` (PodUtils/podssh#187,
  COORD-020 of podssh): `session/` is glue over the crate; each line of
  `pod-session.run` and podssh's full suite passed (3,279 passed in a
  namespace with only the loopback). `crates/pod-session/SOURCES` has
  `taken 6f78a7c`; `check-sources.py` with a clone reads 56 files, 0
  changes. The trial files are removed.

# T-167: `pod-session`: a link meets its cap under qemu-user in `a_session_moves_before_each_cap`

**Source:** T-020 (the cross tests of `pod-session`).
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`a_session_moves_before_each_cap` (`crates/pod-session/tests/session_move.rs`)
fails at random as an aarch64 or riscv64gc musl binary under qemu-user:
`assert_eq!(capped, 0, "no link met its cap")`. The session moves at 768
KiB of a link with a cap of 1 MiB; on a slow host the old link carries
more than 256 KiB while the new link shakes hands. The nightly tests of
the other Linux targets run under qemu-user (`docs/design.md`, section 7),
so the nightly can fail at random.

## Premise

- Measured: 2 runs of 5 failed as an aarch64 binary, 1 of 1 as a
  riscv64gc binary; each x86_64 and i686 run passed.
- Not measured: the bytes that the old link carries after the move
  starts, under qemu-user.
- The relay's margin is 16 MiB (a move at 48 MiB of 64 MiB): the code can
  be right and the margin of the test too small.

## Approach

Count the bytes that the old link carries after the move starts, under
qemu-user. When the writer keeps sending on the old link after the move
point, stop new `DATA` there until the new link takes the session (a
defect of the code, with a test). Otherwise give the test a margin that
holds on a slow host, and say why.

## Prove

```sh
cargo test -p pod-session --test session_move
QEMU_LD_PREFIX=/ qemu-aarch64 target/aarch64-unknown-linux-musl/debug/deps/session_move-HASH
```

## Done

2026-11-10, the commit of this entry. Cause: the test paces each writer at
about 16 MiB/s with a fixed sleep, and the margin before a cap is 256 KiB
(about 8 ms of traffic in both ways). The handshake of the new link takes
more time under qemu-user (the crypto), so the old link met its cap. The
crate is right: the move starts at the move point and the old link carries
only the bytes that the writers send meanwhile. The test now slows its
pace with `pod_testkit::host::scaled`, so the margin keeps its time under
an emulator; the native check is the same. Measured after the change:
`a_session_moves_before_each_cap` 0 failures in 15 runs under qemu-aarch64,
0 in 12 under qemu-riscv64 (and 0 in 15 of an earlier run), 0 in 10 runs
native. One native run in 10 failed while the emulator runs loaded the
machine; 55 later runs passed.

# T-021: `pod-e2e`: the end-to-end channel (Noise XX)

**Source:** `docs/design.md`, section 4 (M2).
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

The relay sees plain bytes unless the end-to-end channel runs above the
layer.

## Premise

- Read: `PodUtils/podssh:crates/podssh-relay/src/e2e/` (3,376, on snow
  0.10); tests `e2e`, `e2e_faults`, `e2e_layered`, `e2e_net`,
  `e2e_harness/`.

## Approach

The procedure of `docs/design.md`, section 8.

## Prove

```sh
cargo test -p pod-e2e
sh scripts/agent/check.sh
```

## Start condition

T-020 is done.

## Done

2026-11-11. Commits `772fca0` (the import of
`PodUtils/podssh:crates/podssh-relay/src/e2e/` and its tests at `74760e8`,
with `SOURCES`), `625dab4` (the adaptation), `7fe46ba` (the fuzz targets
and nine plants), `8d06e1d` (the tests of the door and six plants),
`040f3ea` (`scripts/adoption.sh` and `scripts/agent/togit.sh` take a crate
name with a digit), `5cc147c` (README, the static probe, the rows of
`docs/design.md` and `README.md`), `258c08f` (the trial,
`adoption/podssh/pod-e2e.*`) and the commit of this record. The release
`v0.22.0` and podssh's take remain.

- Cut (call C55): all of podssh's `e2e/` (15 files). Each file names only
  the identity (`pod-identity`), the layer, its handover and a peer's
  answer (`pod-session`), and the names and the safe text (`pod-core`), so
  `ask.rs` (the word of a proof, which the handshake reads) and
  `person.rs` (the gate of a node's person; the caller's `Asker` answers)
  come too. Of the tests: `e2e.rs`, `e2e_faults.rs`, `e2e_net.rs`,
  `e2e_harness/`, `routes.rs`, `person.rs` and the channel's test of
  `session_handover.rs`. `tests/e2e_layered.rs` stays in podssh until
  `pod-relay` (T-025): it runs podssh's `reverse::{Layered, E2e}`. podssh
  keeps a glue `src/e2e/`: its process documents of the net, its process
  holds with its `Movable` of the pipe, and its lines of a node's events.
- API: the handshake (`initiate_with`, `respond_with`, `Offered`,
  `Asked`), `Channel` and its carry, `Ask`, `ends` (`Node` with `events`,
  `docs` and `holds`; the three serves; the four operators), `person`,
  `net`, `legs`, `hold` (`Holds`, `resume`, `ChannelState`), the feature
  `fuzzing`. Each `Error` converts to a `pod_core::Failure` of the stage
  `Session` (`failure.rs`); a node's lines are events `e2e.*`. The crate
  depends on `pod-core`, `pod-identity`, `pod-session` and snow 0.10 with
  podssh's features (pure Rust); 17 source files and 8 test files, each
  under 500 lines.
- Defects and findings: the two process globals of podssh's `e2e/` (the
  installed documents of the net, the registry of the holds of a
  handover) are values of the caller now. Three plants of podssh's `e2e/`
  (t226-route-unmarked, t615-seed-unread, t638-grant-skips-the-person) ran
  tests that stay in podssh, so no test of the crate caught them:
  `crates/pod-e2e/tests/door.rs` (3 tests) catches each. `adoption.sh`
  and `togit.sh` named a crate by `pod-[a-z-]+`, which does not match
  `pod-e2e`: the trial failed at the resolve of the tag `v0.22.0`; fixed.
  `TODO/PROGRESS.md` named the calls "C1 to C50" where C54 was the last;
  fixed. snow 0.10 brings a second ChaCha20-Poly1305 stack (T-199). No
  defect of the channel's code was found: the readers of the frames, the
  verdict, the proof and the state of a handover keep their bounds under
  the fuzz targets, and `pod_identity::PublicKey::signed` refuses a weak
  key (`verify_strict`), so no other key signs a Noise key. Nothing to
  tell podssh but the hand-off.
- Tests: 58 pass (`cargo test -p pod-e2e --all-features`): 54 of podssh
  with their names (22 unit tests; `e2e` 10, `e2e_faults` 7, `e2e_net` 6,
  `person` 5, `routes` 3, `session_handover` 1) and 4 new (the classes of
  the failures; `door` 3). The test that binds starts with
  `need_loopback!`. The thirteen plants of podssh's `e2e/` (as
  `e2e-tNNN-*`) and two new (`e2e-proof-signature-skipped`,
  `e2e-frame-length-trusted`): each caught (`sh scripts/plant-each.sh`,
  9 and 6, group `e2e`). The fuzz targets `e2e_frame`, `e2e_proof` and
  `e2e_handover` pass the stable test of `fuzz/` (6 tests, with 2000
  mutations of each seed).
- Static: `examples/static-probe` with `pod-e2e` for
  `x86_64-unknown-linux-musl` (`cargo zigbuild`): `scripts/check-static.sh`
  passed, and the probe ran a Noise XX handshake. `cargo deny check`:
  advisories, bans, licences and sources ok (snow adds the warnings of
  two duplicates).
- Checks: `sh scripts/agent/check.sh`: fmt, clippy, lint, doc, record,
  size, paths, static and test each 0 (the workspace: 2,151 passed, 0
  failed, 16 ignored); `python3 -I scripts/check-sources.py --clone
  PodUtils/podssh=CLONE pod-e2e`: 22 files, 0 changes.
- Trial (the hand-off): podssh at its `main` `74760e8`: 96 files, 378
  lines in, 4,939 out; the tags move to `v0.22.0` and snow leaves podssh's
  workspace. Checked here with the crates of this tree: the patch applies;
  `cargo check` of podssh-relay (each feature, with its tests), podssh-cli
  and podssh-iroh, clippy with the lints of the gate, rustfmt,
  `podssh-todo check` (706 entries agree, with a `## Correction` on
  PodUtils/podssh@T-089, PodUtils/podssh@T-643 and PodUtils/podssh@T-668),
  `check-repo.py`, `check-consumer-tests.py` (16 targets) and
  `check-consumer-toolchains.py`. Its tests, its release build and the
  static check of its binary run in `adoption.yml`.
- The trial in CI: adoption 43008801789 at `e2752f9` passes for podssh,
  podnet and podvm. Two fixes of the glue came first: the patch grew
  `PodUtils/podssh:crates/podssh-cli/src/node_derp.rs` over 500 lines
  (`67300aa`), then moved three lines that podssh's record cites
  (`025c9ab`); `podssh_relay::e2e::node` builds a node of the process now.
- podssh accepts the cut and the words (C55) and holds `e2e/` to fixes
  until its take of `v0.22.0`.

# T-022: `pod-wormhole`: the mailbox client, SPAKE2 and the code words

**Source:** `docs/design.md`, section 4 (M2).
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

podssh pairs with a code of words through a magic-wormhole mailbox;
podnet's transfer needs the same rendezvous.

## Premise

- Read: `PodUtils/podssh:crates/podssh-relay/src/code/` (1,824):
  `mailbox.rs`, `wormhole.rs`, `phrase.rs`, `words.rs`, `mod.rs`; the
  pairing exchange (`exchange.rs`, `run.rs`) uses the pairs of the relay
  and goes to `pod-relay`.
- The AppID is a parameter (`docs/design.md`, section 1, rule 10).

## Approach

The procedure of `docs/design.md`, section 8, with the mailbox harness.

## Prove

```sh
cargo test -p pod-wormhole
sh scripts/agent/check.sh
```

## Start condition

T-016 is done.

## Done

2026-11-10, partial: the take of podssh remains (the release `v0.15.0` is
at `27a4ccd`; the hand-off is GitHub #54)
(podnet takes the crate with T-102; the exchange goes to `pod-relay`,
T-024). Commits `22850e3` (the import of podssh `code/` at `c5636c9`
with `SOURCES`), `bb631ec` (the adaptation and the tests), `074b798` (the
fuzz target), `922b1b9` (the static probe, the row of `docs/design.md`,
call C45), `836a93f` (README), `93dce3c` (the plants), `d38b8e2` (the
trial), and the commit of this record.

- API: `Config` (the AppID, the client's name and version, the `pod_ws::Way`,
  the trust, the bounds, the events), `Mailbox` (the server protocol of
  version 1 over a `pod-ws` WebSocket), `Wormhole` (SPAKE2 keyed by the
  whole code, the sealed phases), `meet::{Ends, opened, joined}` (the
  giver's new code, the taker's search over a list of mailboxes), `Code` and
  the PGP words, `message` (the pure reading of a mailbox's messages),
  `Failed` (each variant a `pod_core::Failure` at `Stage::Rendezvous`). The
  crate depends on `pod-core` and `pod-ws`, and not on `pod-identity` (C45);
  pure Rust, no feature; 8 source files, each under 400 lines.
- Cut: podssh keeps `APPID`, `ENV`, `LIFETIME`, `mailboxes`,
  `mailboxes_for`, `public_mailboxes`, `Say`, the flag `--pair-mailbox` and
  `exchange.rs` with the rest of `run.rs` (`give`, `take`, `give_with`,
  `take_with`; they go to `pod-relay`, T-024) until then; its `code::config`
  builds the `Config`. podnet has no wormhole code: its transfer (T-102)
  takes the crate.
- Defects: a mailbox could make the client keep an unbounded number of
  messages (the answers' wait kept each message that came meanwhile; the
  wormhole held each message of a phase not asked for): both are bounded at
  256 now (`a_flood_of_messages_while_a_command_waits_is_broken_not_kept`,
  `a_mailbox_that_sends_more_than_it_was_asked_for_ends_the_exchange`; plant
  `wormhole-held-unbounded`). The frames of a mailbox took the 16 MiB of the
  relay's limits; they are 256 KiB and 1 MiB now (`Config::limits`). A
  command with no answer within 300 ms said `within 0 s`; it names the limit
  with its fraction. `Failed::Nobody` said node in a crate that is no
  podssh's pairing; it says giver. A frame, a message and a `pake` body had
  no test of their own and no fuzz target: `message` has 5 tests and the
  target `mailbox_message`. The stand-in mailbox found that magic-wormhole's
  server counts a side that released a nameplate as one of its two until
  the nameplate is free; `a_third_end_is_refused_while_two_hold_the_code`
  passes on both. spake2 0.4 keeps hkdf 0.12, sha2 0.10 and rand_core 0.6
  in the tree beside the 0.13, 0.11 and 0.10 of the workspace (`cargo deny`
  warns, as podssh's lock did); no newer spake2 exists.
- Tests: 45 and 1 live (`#[ignore]`, each public mailbox of `pod-services`).
  20 unit tests: 17 of the source with their names (the test of the public
  mailboxes of `mod.rs` and `a_code_lives_10_minutes_*` of `run.rs` stay in
  podssh), 3 new. 14 of `tests/mailbox.rs`: the wormhole over a stand-in
  mailbox in Rust and over magic-wormhole's own mailbox server, with a peer
  of its Python library giving and taking the code; 4 of them are tests of
  `pair_code.rs` that run no exchange (the three tests of the Python
  library lose the word podssh in their names), 10 are new. 11 of
  `tests/faults.rs`: a scripted mailbox that goes wrong. With
  `POD_WORMHOLE_TEST_SERVER=required` and a virtualenv with
  `magic-wormhole-mailbox-server` and `magic-wormhole`, none is skipped. The
  tests of `pair_code.rs` that run an exchange go with `run.rs` to
  `pod-relay`. The same tests pass as i686 musl binaries and, under
  qemu-user (`QEMU_LD_PREFIX=/`), as aarch64 musl binaries (static). The
  plants `wormhole-version-unchecked` and `wormhole-held-unbounded` are
  caught. The fuzz target `mailbox_message` passes the stable test of
  `fuzz/`.
- Trial: podssh at `f9113862`, after the patch of `pod-quic`
  (`pod-wormhole.needs`, then; 15 files, 60 lines in, 250 out;
  the four modules leave `podssh-relay`, and `spake2`, `crypto_secretbox`
  and `rand_core` leave its manifest): each step of `sh scripts/adoption.sh
  podssh pod-wormhole` passed, with 564 lib tests, `pair_code` (10 tests,
  Python skipped), 1,071 lib tests of `podssh-cli`, `net_invite`,
  `relay_code` and `relay_code_role`, and the static check of the musl
  binary. The plant `t086-version-unchecked` moves to podcrates.
- Checks: `sh scripts/agent/check.sh` (fmt, clippy, doc, record, size,
  paths, the static probes, the tests); `cargo deny check`; `python3 -I
  scripts/check-sources.py` on `pod-wormhole` (podssh 10 files, 0 changes);
  `python3 -I scripts/check-probe.py`; `python3 -I
  scripts/check-design-paths.py`.
- The trial again, clean: at podssh `f9113862` after `pod-quic.patch`, each
  step of `sh scripts/adoption.sh podssh pod-wormhole` passed in one run
  (the run of the import was put together in parts after the disk filled).
  podssh took `pod-quic` at `525f8907`: the patch is made again on podssh's
  `main` `9975fe92` (no conflict; no `.needs`), and each step passed there.
- 2026-11-11, done: podssh took the crate at `baa00a06` (PodUtils/podssh#187,
  COORD-020 of podssh): `code/` keeps the exchange and a thin `mod.rs` over
  the crate; each line of `pod-wormhole.run` passed. `crates/pod-wormhole/SOURCES`
  has `taken baa00a0`; `check-sources.py` with a clone reads 10 files, 0
  changes. The trial files are removed.

# T-023: `pod-netstack`: the userspace TCP and UDP stack on smoltcp 0.14

**Source:** `docs/design.md`, section 4 (M2).
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

WARP and the virtual networks of podnet carry TCP and UDP in user space,
on smoltcp 0.12 now.

## Premise

- The cut of PodUtils/podnet#427 (COORD-018, podnet's answer to GitHub #61),
  at podnet `main` `da6705f` or later: to `pod-netstack`,
  `PodUtils/podnet:crates/podnet-vnet/src/stack.rs` (469 lines: `Clock`,
  `StackConfig`, `StackEvent`, `NetStats`, `NetStack`), `stack/reactor.rs`
  (447), `stack/service.rs` (362), `stack/socket.rs` (380),
  `stack/datagrams.rs` (108), the tests of `stack/tests.rs` (219) and
  `device.rs` (679). The fields `acl` and `dns` of `StackConfig` become
  hooks of the consumer; the recorder becomes a trait `PacketTap`.
  `record.rs`, `policy.rs`, `dns.rs`, `switch/`, `framing.rs`, `proxy.rs`,
  `shim.rs`, `control.rs`, `dialer.rs`, `door.rs`, `stack/proof.rs` stay
  in podnet; the netns router of `podnet-netns` comes after podnet's
  T-174, and `src/warp/ip.rs` goes to `pod-warp`.
- podnet relies on: the medium `ip`; the close of a socket after its
  queues are empty and `TimeWait` read as closed (`stack/service.rs`);
  buffers of 64 KiB and a large download at full speed with a bounded
  buffer (PodUtils/podnet@T-226).
- smoltcp 0.14 has each feature that podnet uses.

## Approach

The procedure of `docs/design.md`, section 8; the move from smoltcp 0.12
to 0.14; the traits `Admit`, `GatewayService` and `PacketTap` for the
policy of the consumer.

## Prove

```sh
cargo test -p pod-netstack
sh scripts/agent/check.sh
```

## Start condition

T-005 is done. podnet named the cut in PodUtils/podnet#427 (the answer to
GitHub #61, COORD-036, 2026-11-11). Step 2 of T-170 waits for the netns
router (podnet's T-174).

## Done

2026-11-11. The import (`88e7efd`) and the adapt (`3a223ba`) of podnet `f7ed86b`
(`main`; the files of the cut are the same at `da6705f`): `stack.rs`,
`stack/reactor.rs`, `stack/service.rs`, `stack/socket.rs`,
`stack/datagrams.rs`, `stack/tests.rs`, `device.rs` (2,664 lines), of
`addr.rs` only `VirtualSubnet` (as `Subnet`) and of `lib.rs` the build check
of `SMOLTCP_IFACE_MAX_ADDR_COUNT`; `SOURCES` (9 files, 0 changes with
`check-sources.py --clone`). The crate has 2,851 lines; no file is over
500.

- The hooks: `Admit::admits(&self, SocketAddr) -> bool` (at each listen,
  connect, UDP bind and UDP send), `GatewayService::{port, answer(&self,
  SocketAddr, &[u8]) -> Option<Vec<u8>>}` (a UDP service at the IPv4
  gateway), `PacketTap::packet(&self, Direction, &[u8])` (called by
  `RecordingDevice<D, T>`). A closure is an `Admit` and a `PacketTap`.
- The endpoints are `SocketAddr` (each call takes `impl Into<SocketAddr>`);
  `VnetDatagrams` is `UdpDatagrams` (a `pod_core::Datagrams`).
- smoltcp `=0.12.0` moves to 0.14 with podnet's features (root
  `[workspace.dependencies]`); no code changed for it, and the crate
  re-exports it. `.cargo/config.toml` has `SMOLTCP_IFACE_MAX_ADDR_COUNT =
  "8"`; the crate fails a build with less.
- Fixed in place: a receive that found the channel to the app full lost
  its bytes (podnet's stack had the defect; a slow reader got a stream
  with a hole); now the stack takes no bytes with no place, and the next
  read kicks the reactor; the end and the handshake are sent again. A
  refused listen, connect or bind freed no socket. Plant
  `netstack-receive-with-no-room`.
- Tests: 25 of `cargo test -p pod-netstack` (the 7 tests of the stack and
  the 8 of the device of the cut with their names; the two hook tests
  `virtual_dns_answers_at_the_gateway` and `acl_refuses_connections` with
  stand-ins; `async_loopback_round_trip` with no proof; new:
  `the_admit_sees_each_target`, `closed_connections_leave_no_socket`,
  `a_large_download_crosses_a_bounded_link` (16 MiB over two bounded
  links), `a_slow_reader_gets_each_byte_and_the_end`,
  `a_refused_bind_frees_its_socket`, `a_recording_device_taps_each_packet`,
  `a_subnet_holds_its_prefixes_and_its_gateways`). The static probe
  sends a datagram through a loopback stack.
- The trial `adoption/podnet/pod-netstack.{patch,run,commit}` (podnet
  `f7ed86b`, tag `v0.20.0`; 2,428 lines of Rust leave podnet): each step of
  `sh scripts/adoption.sh podnet pod-netstack` passed: the record, size,
  layers, build, cargo-deny bans and sources, one smoltcp (0.14 in the root,
  `podnet-netns` and `podnet-vnet`), clippy, the docs, the tests of
  podnet-vnet (94), podnet-netns, podnet-core, podnet-serve, the library of
  podnet (455, WARP on 0.14), the integration tests of the vnet, the
  window, the shims and the pod (the netns backend moved 48 MiB in 2 s),
  the fuzz targets, rustfmt, the toolchains, and the static musl binary
  (zig). podnet's `tests/tcp_window.rs` sets `CongestionControl::None`, as
  podnet's stacks: Reno and CUBIC of 0.14 gave 1.8 MB/s there (T-188).
- The checks of `AGENTS.md`, section 11, pass (fmt, clippy, the tests of
  the workspace, the record), with `check-size.py`, `check-probe.py`,
  `check-publish-order.py`, `check-design-paths.py`,
  `check-workflows.py`, `cargo deny check`, and the static probes of
  `x86_64-unknown-linux-musl` (zig). CI runs at the merge to `main`.
- The take: podnet took the crate at `d23ce4e` (tag `v0.20.0`,
  PodUtils/podnet#445, the answer to GitHub #64). It keeps
  `CongestionControl::None` in `tests/tcp_window.rs` (T-188). `SOURCES`
  has the `taken` lines; `lib.rs` reads the parent of the take, `193bd36`
  (podnet added `pub mod routes`). `check-sources.py` with a clone: 9
  files, 0 changes. The trial files are removed.

# T-024: `pod-wg`: the WireGuard initiator

**Source:** `docs/design.md`, section 4 (M2); `docs/decisions.md`, call C8.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

podnet's WireGuard leg of WARP has its own initiator, sans-IO, on ring.

## Premise

- Read: podnet `PodUtils/podnet:src/warp/wg.rs` (1,078): it imports only
  ring.

## Approach

The procedure of `docs/design.md`, section 8.

## Prove

```sh
cargo test -p pod-wg
sh scripts/agent/check.sh
```

## Start condition

T-003 is done.

# T-025: `pod-relay`: the relay client, the pool, the roads and the reverse road (the base of podssh)

**Source:** `docs/design.md`, section 4 (M2).
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** L
**Status:** open

## Problem

podssh's relay client (hosts, pool, rank, failover, tokens, the roads to
a relay, the reverse road, the pairs, the blocking facade of podvm) is
the base of the relay crate of the three consumers.

## Premise

- Read: the files of `PodUtils/podssh:crates/podssh-relay/src/` of
  `docs/design.md`, section 4 (about 18,000 lines), and podssh-ws
  `h2_road/` and `cleartext/`.
- Cycles of the source: `reverse/wire.rs` names `serve::reverse::carrier::Door`
  (a trait of `pod-relay`); `quic_road/` uses `quick::{probe, quic}`
  (`pod-quic`); `e2e/legs.rs` uses `answer::offers` (`pod-session`).
- `reverse/` changed 34 times in the last 50 commits of podssh.

## Approach

The procedure of `docs/design.md`, section 8, with podssh's files only;
the blocking facade behind the feature `blocking`. podssh's tests that
wait for this crate come with it: `tests/e2e_layered.rs` (the channel of
`pod-e2e` over `reverse::Layered`, T-021), `tests/session_policy.rs` and
`tests/session_relay_resume.rs` (T-020). `cache::valid_token` comes with
the cache; `crates/pod-e2e/src/legs_tests.rs` holds a copy of its rule.

## Decision

The owner, 2026-11-06 (Q103): podvm takes the feature `blocking` of
`pod-relay` from podcrates by a tag; podssh's test of two clients at once
comes with the facade.

## Prove

```sh
cargo test -p pod-relay --all-features
sh scripts/agent/check.sh
```

## Start condition

T-019, T-021 and T-022 are done.

# T-026: `pod-relay`: podnet's relay client and its plain reverse mode

**Source:** `docs/design.md`, section 8, step 5.
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** M
**Status:** open

## Problem

podnet's relay code (the mint, the token cache, `RelayBudget`, the race
of hosts, the reverse mode on `/v1/node/NAME` with 32-hex ids) must keep
each capability in `pod-relay`.

## Premise

- Read: podnet `PodUtils/podnet:src/ws_relay.rs` (995; `active()` is a
  process global), `PodUtils/podnet:src/relay_pool.rs` (380; a copy of
  podssh's pool), `PodUtils/podnet:src/relay_reverse.rs` (1,139).

## Approach

Each capability of podnet that the base does not have, with its tests;
the plain reverse mode as a mode of the API of the reverse road; the
parity list; the trials of podssh, podnet and podvm.

## Prove

```sh
cargo test -p pod-relay --all-features
sh scripts/agent/check.sh
```

## Start condition

T-025 is done.

# T-110: `pod-session`: the held link to a server that is not a podssh relay (GitHub #35)

**Source:** GitHub #35 (A-C-08, A-S-05)
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** M
**Status:** open

## Problem

The node agent of cfb needs one WebSocket to a cfb URL: pings each 25 s, reconnect with jitter, resume by
sequence and a replay window, a CONNECT proxy and a private CA, upgrade headers from files or from a
command, and a local Unix socket of frames.

## Premise

- Read: the ask of A-C-08. podssh has the WebSocket roads and the resumable layer, with no link to a server
  that is not a podssh relay.
- Not measured: the link.

## Approach

A `Link` over `pod-ws` with the layer of `pod-session`: pings at a set interval, reconnect with jitter,
resume by sequence and a replay window, upgrade headers from files or from a command, a frame API for a
local socket. The wire is T-120. The command `podssh link` stays in podssh.

## Prove

```sh
cargo test -p pod-session --all-features
```

A test drops 3 % of the frames for a long run. No frame is lost or repeated.

## Start condition

T-016 and T-020 are done.

# T-111: `pod-relay`: a lease header, Access headers from files on each WebSocket road, a heartbeat that a relay offers (GitHub #35)

**Source:** GitHub #35 (A-S-14, A-C-04, A-S-05)
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

A-S-14, A-C-04 and A-S-05 change what each relay road sends: a lease in place of a token, Access service
token headers from files, and a heartbeat that lets a Durable Object relay hibernate.

## Premise

- Read: the asks. podssh sends `X-Relay-Token` only, and pings each 10 s.
- Not measured: a relay that offers the heartbeat.

## Approach

A lease header from a file. Access headers from files on each WebSocket road, with exit class Refused and
a name of the missing variable. The heartbeat that a `hello` offer sets: WebSocket Ping, or 60 s or more.

## Prove

```sh
cargo test -p pod-relay --all-features
```

## Start condition

T-025 is done.

# T-120: The wire documents of the layer, the reverse dialect and the role's extensions, with test vectors (GitHub #35)

**Source:** GitHub #35 (A-S-15, A-X-04, A-C-08)
**Category:** docs
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

A port of the relay to wasm (`podrelay-worker`, A-X-04) and the link of A-C-08 need the wire as a
document with test vectors. The owner, 2026-11-09: the wire document comes first.

## Premise

- Read: the prose in `PodUtils/podssh:docs/relay.md` and `PodUtils/podssh:docs/design.md`.
- Not measured: a vector test.

## Approach

One wire document for `pod-session` and one for `pod-relay`, each with vector files. A test reads each
vector. The Python stand-in of podssh reads the same vectors. podssh can write the documents first, and
the import takes them.

## Prove

```sh
cargo test -p pod-session --all-features
cargo test -p pod-relay --all-features
```

## Start condition

T-020 and T-025 are done.

# T-121: `pod-relay`: renew a pair (PodUtils/podnet#413)

**Source:** PodUtils/podnet#413, ask 1 (PodUtils/podnet@T-161, PodUtils/podnet#358)
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

A pair of the reverse road ends at its TTL. The published relay contract has no call that
extends it, so a node cannot stay paired for months.

## Premise

- Read: the ask of podnet; the relay contract of `pod-relay` (T-025, T-047).
- Not read: a renew endpoint on the relay server; the owner of the relay adds it.

## Approach

A call `POST /v1/pair/NAME/renew`, authorized by the node token only, that extends the TTL and
keeps the pair id. The node renews at half the TTL and again in the first warning band. A test
with the stand-in relay of `pod-testkit`.

## Prove

```sh
cargo test -p pod-relay --all-features
cargo todo check
```

## Start condition

T-025 or T-026 is done, and the relay server has the renew endpoint.

# T-122: `pod-relay`: a health memory of the relay hosts (PodUtils/podnet#413)

**Source:** PodUtils/podnet#413, ask 2 (PodUtils/podnet@T-162, PodUtils/podnet#359)
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

The alternate relay hosts are chosen once at the start. A stalled primary costs the stagger of the
race (2 s) on each dial.

## Premise

- Read: the ask of podnet.
- Not measured: the cost over a day of dials.

## Approach

Count the failures and the lost races of each host. Demote a host after 2; restore it on a
success; keep the order stable otherwise. Tests with planted failures.

## Prove

```sh
cargo test -p pod-relay --all-features
```

## Start condition

T-025 or T-026 is done.

# T-123: `pod-relay`: the session budget from the pool document (PodUtils/podnet#413)

**Source:** PodUtils/podnet#413, ask 3 (PodUtils/podnet@T-163, PodUtils/podnet#360)
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

The budget of a relay session (60 MiB, 11.5 h) is a constant. The default relay publishes `limits`
in `/relays.json`.

## Premise

- Read: the ask of podnet; `crates/pod-services/services/ajam-relay.toml` (the watch of `/limits`).
- Not measured: the values of other relays.

## Approach

Parse `limits` with the published key names, keep them with the cached pool, and give them to the
meter (`pod_core::Metered`). The floor is the current constants; clamp each value; keep the margins
60/64 MiB and 11.5/12 h.

## Prove

```sh
cargo test -p pod-relay --all-features
```

## Start condition

T-025 or T-026 is done.

# T-124: `pod-relay`: failover on the reverse road (PodUtils/podnet#413)

**Source:** PodUtils/podnet#413, ask 4 (PodUtils/podnet@T-200, PodUtils/podnet#397)
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

`pair`, `node` and `stop` of the reverse road reach host 0 of the pool only.

## Premise

- Read: the ask of podnet.
- Not measured: a node socket on host B that serves a pair minted on host A.

## Approach

Measure the cross-host case first. Then one host loop for `connect`, `websocket` and `post_json`
of the reverse road, with the health memory of T-122.

## Prove

```sh
cargo test -p pod-relay --all-features
```

## Start condition

T-025 or T-026 is done.

# T-150: `pod-relay`: a handler target for the relay node (PodUtils/podnet#413)

**Source:** PodUtils/podnet#413, ask 11 (PodUtils/podnet@T-083)
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

The relay node serves one target: a TCP address or a unix socket. A
service in the same process (podnet's file server, a git SSH server, a
transfer) must open a socket file to be reached through the relay.

## Premise

- Read: the ask of podnet.
- podnet's users of it (PodUtils/podnet@T-091, PodUtils/podnet@T-094,
  PodUtils/podnet@T-104) come later: the ask has a low priority.

## Approach

A `Target` variant that hands each session's stream to a handler in the
process (a trait object or a channel of streams), beside the address and
the unix socket. A test with the stand-in relay of `pod-testkit`: a
handler in the test process answers one session and then another.

## Prove

```sh
cargo test -p pod-relay --all-features
cargo todo check
```

## Start condition

T-025 or T-026 is done.

# T-159: podnet: two QUIC connections on one WARP socket share one waker of the full send queue

**Source:** T-019 (the review of PodUtils/podnet:src/warp/quic.rs at `2fed3ff`).
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podnet's `WarpUdpSocket` gives each poller of quinn a clone of one
`SendReady`, one `AtomicWaker` (the fix of PodUtils/podnet@T-225). quinn
0.11 makes a poller for each connection (`create_io_poller` in
`connection.rs`), and the HA connections of the `quic+warp` leg share one
endpoint (`WarpLink::endpoint` in `src/tunnel.rs`). When the send queue
of 1024 datagrams is full and two connections wait, the second register
takes the waker of the first; the drain wakes one connection, and the
other waits for its next timer or packet.

## Premise

- Read: PodUtils/podnet:src/warp/quic.rs at `2fed3ff` (`SendReady`,
  `create_io_poller`), quinn 0.11.12 `src/connection.rs` (a poller for each
  connection), PodUtils/podnet:src/tunnel.rs (`WarpLink`, `EndpointSource::Warp`).
- Measured on pod-quic, not on podnet: `two_senders_on_a_full_way_each_wake`
  fails when two senders give the way their own wakers. Not measured: a
  stall of a podnet leg.

## Approach

The take of `pod-quic` (`pod-quic.patch` of podnet at `v0.14.0`) removes the
defect: `pod_quic::socket` gives the way one fan-out waker that wakes each
sender. Tell podnet in the hand-off of `v0.14.0` (a COORD issue of
podcrates `(to podnet)`), with this entry. The file is frozen in podnet
(`crates/pod-quic/SOURCES`), so podnet takes no fix of its own unless it
cannot take the crate.

## Prove

```sh
cargo test -p pod-quic --lib two_senders_on_a_full_way_each_wake
python3 -I scripts/check-sources.py pod-quic
```

The taken line of the podnet block of `crates/pod-quic/SOURCES`.

## Start condition

The release `v0.14.0` (T-019).

## Done

2026-11-10. podnet fixed the defect in its own code (PodUtils/podnet@T-228,
podnet `1d556fb`): `SendReady` keeps a list of wakers, and the writer
wakes each; its test `two_senders_on_a_full_queue_each_wake` failed on the
single waker and passes. A fix of a frozen file is allowed; `pod-quic`
has the same fix (`two_senders_on_a_full_way_each_wake`). When `1d556fb`
reaches podnet's `main` (after podnet's `v0.4.0-beta.2`),
`check-sources.py` shows `src/warp/quic.rs` as changed: compare the two
fixes and move the snapshot of `crates/pod-quic/SOURCES` (M5).

# T-184: `pod-wormhole`: the words of a mailbox can end with a space

**Source:** the run 42958692411 of `fuzz.yml` (the target `mailbox_message`)
**Category:** defect
**Milestone:** M1
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`message::words` makes the mailbox's words one safe line, then cuts it to
`WORDS_MOST` characters. A cut on the space between two words left a line
that ends with a space, which is not the one safe line that the function
promises.

## Premise

- Read: `crates/pod-wormhole/src/message.rs`; the input of the run.

## Approach

Trim the end of the cut. Keep the input as a test and as a seed.

## Prove

```sh
cargo test -p pod-wormhole --lib message
cargo test --manifest-path fuzz/Cargo.toml
```

## Done

2026-11-11. `words` trims the end of the cut. The test
`the_mailboxs_words_are_one_short_line` holds a cut on a space; the seed
`cut-on-a-space` of `mailbox_message`. The stable test of `fuzz/` passes
with it.

# T-188: `pod-netstack`: Reno and CUBIC of smoltcp 0.14 grow by one segment for each round trip on a link that gives one ACK for each round trip

**Source:** the trial of T-023 (podnet's `tests/tcp_window.rs`).
**Category:** measurement
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

On smoltcp 0.14, podnet's `tests/tcp_window.rs` (two smoltcp stacks over
two tunnel devices of `pod-netstack`, 20 ms in each direction on a virtual
clock, buffers of 512 KiB, 8 MiB) gives 1.79 MB/s with the default
controller (CUBIC) and with Reno, and 11.18 MB/s with
`CongestionControl::None`. On smoltcp 0.12 the default (CUBIC) gave more
than 8 MB/s (the limit is 12.6 MB/s). The stacks of podnet set `None`
(the vnet, and WARP with no `--cc`), so their default is not slower. A
consumer that sets Reno or CUBIC (podnet's `warp --cc reno|cubic`) gets
about one seventh of the rate on such a link.

## Premise

- Measured (2026-11-11, the trial of T-023): no packet was sent again
  (the server sent 6,769 packets with each controller); the client sent
  118 packets with Reno and 65 with `None`, about one ACK for each round
  trip. The bytes that arrive in each round trip grow by 1,240 (one
  segment) from the first round trip.
- Read: smoltcp 0.14 `src/socket/tcp/congestion/reno.rs` and `cubic.rs`:
  in slow start, `cwnd` grows by `min(len, mss)` for each ACK (RFC 3465
  with L = 1 MSS). The link of the test delivers each burst at one time,
  so the receiver takes it in one poll and gives one ACK.
- Not measured: a link with a serial delay (the packets of a burst one by
  one), where the receiver gives an ACK for each two segments; podnet's
  WARP with `--cc` over a real edge.

## Approach

1. A test of `pod-netstack` over two stacks with a delay link (a pump
   that holds each packet for a time) that measures the rate of each
   controller, with one burst at one time and with a serial delay.
2. When the rate stays low with a serial delay too, give the stack a
   setting for the controller with the measured rate of each in its
   README, and report the slow start of 0.14 to smoltcp with the test.
3. Tell podnet the result for its `--cc` of WARP (a COORD issue).

## Prove

```sh
cargo test -p pod-netstack --lib link
```

## Done

2026-11-11, the commit of this entry. `crates/pod-netstack/src/link_rate.rs`
(with `crates/pod-netstack/src/link_rate/tests.rs`) measures a download of
8 MiB between two interfaces of smoltcp 0.14 over `LinkDevice::link`: 20 ms
in each direction, buffers of 512 KiB, MTU 1280, on a virtual clock (no
wall time; each run gives the same numbers). The burst link delivers each
burst that one poll sends at one time, as podnet's `tests/tcp_window.rs`.
The serial link sends the packets one by one at 100 Mbit/s, then holds
each for the delay. With the end of podnet's loop (the next event after
the last byte), the burst link gives the numbers of the trial of T-023:
11.18 MB/s with no controller, 1.79 with Reno and CUBIC, 6,769 packets
from the server, 65 and 118 from the client. The tests end at the last
byte. In MB/s:

| Link | Limit | None | Reno | CUBIC |
| --- | --- | --- | --- | --- |
| Burst | 13.11 | 11.34 | 1.80 | 1.80 |
| Burst, the second half | 13.11 | 10.20 | 2.98 | 2.98 |
| Serial | 12.50 | 11.00 | 6.72 | 6.72 |
| Serial, the second half | 12.50 | 10.58 | 10.58 | 10.58 |
| Serial, receiver with no ACK delay | 12.50 | | 7.90 | 7.90 |
| Burst, receiver with no ACK delay | 13.11 | | 1.80 | |
| Burst, receiver with a pass of egress after each packet | 13.11 | | 6.36 | |
| The same, no ACK delay | 13.11 | | 8.22 | |

The limit of the burst link is the window over the round trip (512 KiB in
40 ms); the limit of the serial link is its rate. No segment is sent
again: the server sends 6,769 packets (6,766 segments of data) in each
case. The client sends 118 ACKs with Reno on the burst link (one for each
round trip), 3,388 on the serial link (one for each two segments) and
6,770 with no ACK delay.

- Result: Reno and CUBIC are not slow on a serial link. Their slow start
  adds half of the window for each round trip, and the second half of the
  download moves at the rate of no controller. The slow start of 1.8 MB/s
  is an effect of a link that gives one ACK for each round trip. The stack
  keeps `CongestionControl::None` and gets no setting of the controller
  (step 2). The README of the crate has the table.
- Cause in smoltcp 0.14: in slow start, Reno and CUBIC add `min(len, mss)`
  to `cwnd` for each ACK (`src/socket/tcp/congestion/reno.rs`, `cubic.rs`,
  smoltcp-rs/smoltcp#1156 and #1157); 0.12 added `len`, each acknowledged
  byte. The TCP socket sends its ACK in `dispatch`, the pass of egress.
  `Interface::poll` takes each waiting packet before the egress, and
  `immediate_ack_to_transmit` (more than one MSS not acknowledged) only
  sets the delayed ACK to `Immediate`. One poll thus gives one ACK for all
  the segments that it takes.
- Fixes for a consumer: a sender with `CongestionControl::None` (the
  stacks of podnet); or, in a receiver of smoltcp, a pass of egress after
  each packet (`poll_ingress_single`, then `poll_egress`): 6.36 MB/s, and
  8.22 with `set_ack_delay(None)`. `set_ack_delay(None)` alone changes
  nothing on the burst link (1.80); on the serial link it gives 7.90.
- The result for podnet's `warp --cc reno|cubic`: the controller acts on
  the data that the smoltcp of WARP sends (an upload); the receiver is the
  edge. A receiver that gives an ACK for each second segment (RFC 9293,
  SHLD-19) gives the result of the serial link: a slow start that adds
  half of the window for each round trip, then the rate of no controller.
  A receiver that gives one ACK for each burst (for example after GRO)
  gives the result of the burst link. In a download, smoltcp sends only
  ACKs, and the controller of the edge acts. Not measured: WARP over a real
  edge (a live test of podnet). podnet's `tests/tcp_window.rs` gives
  1.8 MB/s with Reno and CUBIC because its receiver is smoltcp and its
  link delivers bursts. It can keep `CongestionControl::None`, or measure
  Reno and CUBIC on a serial link or with a receiver that takes each
  packet, as `link_rate` does. Step 3: the session of podcrates sends this
  item to podnet.
- Tests: `cargo test -p pod-netstack --lib link`: 8 passed (4 of
  `link_rate`, 3 of `stack::tests::link`, 1 of `device::tests`), 2.9 s in
  a debug build. The plant `netstack-link-rate-no-controller` (each socket
  keeps CUBIC) fails `each_controller_on_a_burst_link` and
  `each_controller_on_a_serial_link` with "None on a burst link" and "None
  on a serial link".
- Findings: the README of the crate named T-185 for this measurement
  (fixed: T-188). The reactor of `NetStack` gives one ACK for the segments
  that one poll takes: T-197.

The report to smoltcp (the owner posts it; not posted):

```text
Title: tcp: in 0.14, Reno and CUBIC grow cwnd by one MSS per round trip
when the receiver is smoltcp and one poll takes a burst

smoltcp 0.14.0; features std, medium-ip, proto-ipv4, proto-ipv6,
socket-tcp, socket-tcp-reno, socket-tcp-cubic.

Setup: two Interfaces (Medium::Ip) on a link that holds each packet 20 ms
in each direction. TCP buffers of 512 KiB, MTU 1280, a download of 8 MiB,
a virtual clock. The "burst" link delivers each packet that one poll sends
at one time. The "serial" link sends the packets one by one at
100 Mbit/s, then holds each for 20 ms. No packet is lost, and no segment
is sent again.

Rate in MB/s (limit: burst 13.11, the window in 40 ms; serial 12.50):

| Link                         | None  | Reno  | Cubic |
| burst                        | 11.34 |  1.80 |  1.80 |
| serial                       | 11.00 |  6.72 |  6.72 |
| serial, second half          | 10.58 | 10.58 | 10.58 |

With smoltcp 0.12.0, the burst link gave more than 8 MB/s with Cubic.

Cause:
1. Since 0.14.0 (#1156, #1157), the slow start of Reno and Cubic adds
   min(len, mss) to cwnd for each ACK (src/socket/tcp/congestion/reno.rs,
   cubic.rs). 0.12 added len.
2. The receiver sends its ACK in dispatch (egress). Interface::poll
   processes each queued packet before the egress, and
   immediate_ack_to_transmit() only sets the delayed-ACK timer to
   Immediate. One poll that takes N segments thus sends one ACK. On the
   burst link, the receiver sends one ACK per round trip (118 ACKs for
   6,766 segments), and cwnd grows by one MSS per round trip.

On the serial link, the receiver sends one ACK for each two segments
(3,388 ACKs), and cwnd grows by half per round trip. set_ack_delay(None)
on the receiver: one ACK per segment, 7.90 MB/s on the serial link, no
change on the burst link (1.80). A receiver that calls
poll_ingress_single and then poll_egress for each packet: 6.36 MB/s on
the burst link (8.22 with set_ack_delay(None)).

Proposals:
a. In slow start, add the acknowledged bytes, as 0.12 did, or at least
   use L = 2*SMSS (RFC 3465, section 2.2).
b. Send an ACK from the ingress path for at least each second full-sized
   segment (RFC 9293, SHLD-19), so that a poll of N segments sends N/2
   ACKs.

Reproduce: PodUtils/podcrates, crates/pod-netstack/src/link_rate.rs:
  cargo test -p pod-netstack --lib link_rate -- --nocapture
```

# T-197: `pod-netstack`: the reactor gives one ACK for the segments that one poll takes

**Source:** T-188 (the measurement of `link_rate`).
**Category:** measurement
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

The reactor of `NetStack` runs `Interface::poll`
(`crates/pod-netstack/src/stack/reactor.rs`). smoltcp takes each packet of
the inbound queue before its pass of egress, and its TCP socket sends the
ACK in the egress. When the pump puts a burst into the inbound queue
before the reactor polls, the stack gives one ACK for the burst. RFC 9293
(SHLD-19) asks for an ACK for at least each second full-sized segment. A
sender that adds one segment to its window for each ACK in slow start
(Reno and CUBIC of smoltcp 0.14) then grows by one segment for each round
trip: 1.80 MB/s, against 6.36 MB/s with a pass of egress after each
packet (T-188, the burst link of `link_rate`). No consumer has this case
now: the stacks of podnet set no controller.

## Premise

- Measured (T-188): the numbers above, with the two interfaces of
  `link_rate`, not with the reactor.
- Read (T-188): `Interface::poll` and the delayed ACK of `tcp::Socket` of
  smoltcp 0.14.
- Not measured: the reactor as a receiver; the cost of a pass of egress
  after each packet with many sockets (`socket_egress` of smoltcp visits
  each socket of the set); the senders that count the ACKs and not the
  acknowledged bytes.

## Approach

1. A test of the reactor as a receiver of a burst: the count of ACKs that
   one poll gives for a burst of segments. The `Clock` of the stack is
   real, so the test counts packets, not a rate.
2. Measure the cost of a pass of egress after each second packet
   (`poll_ingress_single`) with 4,096 sockets.
3. When the cost is low, poll so. Else keep `Interface::poll`, and follow
   the report of T-188 to smoltcp (an ACK from the ingress path).

## Prove

```sh
cargo test -p pod-netstack --lib link
```
