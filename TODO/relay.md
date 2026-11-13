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
- Released in `v0.22.0` (`93de04a`). podssh took it at `75a50318`
  (PodUtils/podssh#191, COORD-024 of podssh): `src/e2e/` is the glue of
  the process; its hints follow `NotChannel` and `NO_EXIT`. Each step of
  `pod-e2e.run` passed there but the zig musl build (no zig in its
  container; its release workflow builds it). `check-sources.py` with a
  clone at `75a5031`: 22 files, 0 changes. The trial files are removed.

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
**Status:** done

## Problem

podnet's WireGuard leg of WARP has its own initiator, sans-IO, on ring.

## Premise

- Read: podnet `PodUtils/podnet:src/warp/wg.rs` (1,078): it imports only
  ring.

## Approach

The procedure of `docs/design.md`, section 8.

## Decision

Call C56: the cut of PodUtils/podnet#447, podnet's answer to GitHub #67
(COORD-042). The sans-IO parts of `src/warp/wg.rs` come; `Peer`, the
socket, `connect`, `connect_inside`, `connect_on`, `WgTx`, `WgRx`,
`WgKeepalive` and their lock stay in podnet; no driver over a trait of
datagrams is built now.

## Prove

```sh
cargo test -p pod-wg --all-features
sh scripts/agent/check.sh
```

## Start condition

T-003 is done.

## Correction

2026-11-11, read at podnet `54d7ed4`: `src/warp/wg.rs` has 965 lines, and
`src/warp/wg/standin.rs` 240. `wg.rs` imports ring (ChaCha20-Poly1305),
curve25519-dalek (X25519), getrandom, anyhow, and for its driver tokio,
tracing, bytes and base64 (`Peer::from_exported`); the stand-in takes
`tcp_packet` of `src/warp/tun/probe.rs`. It has no cookie code: a cookie
reply is an error, and no initiation has a mac2.

## Done

2026-11-11. Commits `22adb56` (the import of `src/warp/wg.rs` and
`src/warp/wg/standin.rs` at podnet `54d7ed4`, with `SOURCES`), `0b91685`
(edition 2024: `cargo fix --edition` in a scratch crate with stubs changed
no line; one `tail_expr_drop_order` in `WgRx::recv_packet`, safe),
`8d4c58a` (the adaptation to the cut of PodUtils/podnet#447), `1f5bf23`
(the fuzz targets and five plants), `f105d34` (README, the static probes,
the rows of `docs/design.md` and `README.md`, call C56), `145ff0c` and
`3f65c98` (the trial, `adoption/podnet/pod-wg.*`), `0bb640e` (the stand-in
under load) and the commit of this record. The release `v0.22.0` and
podnet's take remain.

- Cut (PodUtils/podnet#447, call C56): BLAKE2s, HMAC, the KDF, X25519,
  ChaCha20-Poly1305, TAI64N, `Keys`, `Initiation`, mac1, `Session` with its
  window of counters, and the timers of `State` and of
  `WgKeepalive::run` as `Tunnel`, each on the caller's `now`; the
  stand-in as the feature `standin` (`start` gives the keys and the
  address; podnet makes its `Peer` of them). podnet keeps `Peer` with
  `from_exported` (the device record of WARP), `Sock`, `connect`,
  `connect_inside`, `connect_on`, `WgTx`, `WgRx`, `WgKeepalive` and the
  lock, and the tests `no_answer_is_an_error_that_names_the_door`,
  `a_tunnel_over_udp_carries_packets_both_ways` and
  `wireguard_probe_renews_a_dead_leg` (they run its driver).
- New: the cookie reply and mac2 (`Cookie`, `Initiation::add_mac2`,
  `Received::Cookie`; XChaCha20-Poly1305 is HChaCha20 of the crate before
  ring's AEAD, checked against RustCrypto's), `Stamps`, `initiate_from`
  (the caller's `Entropy`), `standin::start_under_load`. API: `Keys`,
  `initiate`, `initiate_from`, `initiate_with`, `Initiation` (`finish`,
  `mac1`, `add_mac2`, `sender`), `Cookie`, `Session`, `Tunnel` (`rekey`,
  `send`, `receive`, `tick`), `Outgoing`, `Received`, `Stamps`, the
  message types, lengths and timers, `Error` (a `Failure` at the stage
  `Warp`). With no feature the crate has no tokio
  (`examples/static-probe-lean` links it). 13 source files, each under 500
  lines.
- Defects of podnet's `wg.rs` fixed in the crate, each with a test that
  fails on the old code (run once): a response that failed its check set
  the times of the waiting rekey to now, so bad responses held a dead path
  open and delayed each resend (`a_bad_response_keeps_the_dead_time`);
  `State::rekey` read the age of the session from the system clock, not
  from the given `now` (`the_timers_of_a_tunnel`); the TAI64N timestamp
  read the wall clock at each initiation, so a step back of the clock or
  two initiations at one instant gave a timestamp that the responder drops
  (`stamps_only_go_forward`); the stand-in read the type byte of an empty
  datagram, and the panic ended its task
  (`an_empty_datagram_does_not_stop_the_standin`); no random bytes of the
  system was a panic (`Error::NoRandom`); a responder under load refused
  each handshake, as no initiation had a mac2. The keys of an initiation,
  a session and a cookie are wiped on drop. In podnet's driver (the
  trial patch): `connect_on` read the type byte of an empty datagram from
  the buffer of the datagram before; it reads it only when it has one.
- Tests: `cargo test -p pod-wg --all-features`: 19 passed; the four tests
  of `wg.rs` keep their names (`blake2s_vectors`, `hmac_and_kdf_vectors`,
  `a_handshake_and_transport_messages`, `the_replay_window`), 15 are new.
  The fuzz targets `wg_response`, `wg_cookie` and `wg_transport` with 14
  seeds: `cargo test --manifest-path fuzz/Cargo.toml --lib`, 7 passed. The
  plants `wg-mac1-unchecked`, `wg-replayed-counter-taken`,
  `wg-timestamp-goes-back`, `wg-bad-response-resets-the-dead-time` and
  `wg-cookie-mac2-missing`: `sh scripts/plant-each.sh`, 5 caught.
- Static: `examples/static-probe` (feature `standin`) runs one handshake
  with the stand-in in memory, a packet each way and a rekey; with
  `static-probe-lean` it is static on `x86_64-unknown-linux-musl`
  (`cargo zigbuild`, `scripts/check-static.sh`). ring stays (C8).
- Trial: the patch `pod-wg.patch` of `adoption/podnet/` (removed after
  podnet's take) applies at podnet `54d7ed4`; with
  the crates of this tree, podnet's record check, `check-size.py`,
  `check-layers.py`, clippy of the workspace, `cargo fmt --check`,
  `cargo deny check bans sources`, and `cargo test -p podnet --lib`: 457
  passed (a front door under load among them). Each podcrates tag of the
  patch moves from `v0.20.0` to `v0.22.0`; podnet took `v0.21.0` at
  `54bd958` after `54d7ed4`, so its take moves from there.
- `check-sources.py --clone PodUtils/podnet=CLONE pod-wg`: 3 files, 0
  changes. In the run of `sh scripts/agent/check.sh`,
  `a_session_moves_while_both_ends_send_at_full_speed` of `pod-session`
  failed one time (T-204).
- Released in `v0.22.0` (`93de04a`). podnet took it at `089e1c9`
  (PodUtils/podnet#447): `git apply --3way`; the manifests conflicted only
  on the tags; each line of `pod-wg.run` passed there but the zigbuild;
  `cargo test -p podnet --lib` 492 passed; its `check.sh` 0. The cookie
  path of `connect_on` and `Peer` whole in podnet are as the patch has
  them. `check-sources.py` with a clone at `089e1c9`: 3 files, 0 changes.
  The trial files are removed.

# T-025: `pod-relay`: the relay client, the pool, the roads and the reverse road (the base of podssh)

**Source:** `docs/design.md`, section 4 (M2).
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** L
**Status:** done

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

## Done

2026-11-12: podssh took the crate at `0872c2c6` (PodUtils/podssh#206),
from `v0.27.0`, with each line of the `.run` of its trial; the `taken`
line is in `SOURCES`, and the trial files are gone. The import, on
2026-11-11: Commits `b4dd345` (the import of
`PodUtils/podssh:crates/podssh-relay/src/` and its tests at `75a5031`, with
`SOURCES`: 117 files), `f74b805` (the adaptation of the library), `cb122b3`
(the tests), `18a73a0` (the README, the plants, the fuzz targets, the
static probe, the comments), `c479ab0` (the doc links, a test of
`pod-broker`) and this record. Edition 2024 needs no
commit: podssh's workspace is in edition 2024.

- The cut, as the tree at `75a5031` holds it: the files of
  `docs/design.md`, section 4, but `cache/` (no token or pool part: its
  files are `pod-core`'s), `tls_groups.rs` (podssh's glue over `pod-tls`),
  and podssh-ws `h2_road/` and `cleartext/` (podssh's glue over `pod-ws`:
  its process pool and the way of a proxy choice). Of `cache.rs`, the token
  entries; of `code/`, `exchange.rs` and `run.rs` (`mod.rs`, the choice of
  the mailboxes, stays in podssh). Of the tests: each test of these files
  that runs no other module of podssh-relay, with its harness, the
  fixtures of the grants and the relay's document of the close table.
  `tests/session_relay_resume.rs` runs the relay role: it goes with
  `pod-relay-serve` (T-036), with `relay_serve_*.rs`, `reverse_carrier.rs`,
  `reverse_restart.rs` and `http_road.rs`.
- The cycles of the source: `reverse/wire.rs`'s `serve::reverse::carrier::Door`
  is the trait `reverse::wire::Door` behind `Carrier`; `quic_road/` uses
  `pod_quic::{probe, endpoint, connect::tls}`; the legs and the answer of a
  peer are `pod-session`'s and `pod-e2e`'s; `reverse/` takes `pod-e2e`'s
  `Node` (its docs, its holds and its events are the caller's);
  `cache::valid_token` is one rule here, which `pod-e2e`'s
  `legs_tests.rs` copies for its own test (`pod-e2e` does not depend on
  this crate).
- The globals of the source, each a value of the caller (call C60):
  `gate::process()`, `pool::IN_USE`, `rank_auto::RACES` and `RACE`, the
  QUIC road's memory and podssh-ws's `h2_road::POOL` are in the
  `Context`; `pool::signed::BY_SETTINGS`, `pool::members::BY_NET`,
  `rank_list::OFF_BY_SETTINGS` and `rank_auto::AUTO_BY_SETTINGS` are
  `Settings`; `reverse/node.rs`'s `UPGRADING`, `reverse/legs.rs`'s
  `ROTATES` and `reverse/spares.rs`'s `SAY` and `SAID` are a
  `reverse::NodeState`; `addresses::INSTALLED` and podssh-ws's process
  resolver are the caller's `Context::resolver`; the budget's sink of
  `reverse/layered.rs` is `Layered::telling`. The variables
  (`PODSSH_RELAY_TOKEN`, `PODSSH_OFFLINE`, `PODSSH_RELAY_KEYS`,
  `PODSSH_RELAY_RANK`, `PODSSH_RELAY_RANK_PACE`, `PODSSH_H2`,
  `PODSSH_QUIC`, `PODSSH_CLEARTEXT`, `PODSSH_CACHE_DIR`) are `Settings`
  and the `Cache`; the crate reads none.
- The crate: 87 Rust files of `src/` (17,500 lines) and 40 of `tests/`
  (9,000 lines), each under 500 lines; features `pair`, `blocking`, `code`,
  `grant`, `quic`, `plain-ws`, `fuzzing`. Each error converts to a
  `pod_core::Failure` (`failure.rs`). The comments name podssh's entries
  as `podssh T-NNN` and its issues as `PodUtils/podssh#N`, with no date of
  a measurement.
- Defects of podssh's code: none found. The test
  `the_switch_turns_the_order_off` read the process's variable and set a
  process global; it now reads the `Settings` that it makes.
- Tests: 356 pass and 19 live tests are ignored (`cargo test -p pod-relay
  --all-features`): 180 unit tests and 176 integration tests. Of the 380
  tests of the imported files, 370 are here with their names, and 2 are
  new (the classes of the failures); the other 10 are `pod-core`'s (7 of
  the places of the cache) and `pod-wormhole`'s (3 of magic-wormhole's
  Python peer).
- Plants: 28 of podssh's plants of these files (group `relay`,
  scripts/plants/relay-*), each caught by its test here; the 9 others
  check a test that stays in podssh (its CLI, its interop, the role).
- Fuzz: the targets `relay_list` and `relay_pool` (podssh's) run on
  `pod-relay`, with seeds written here; the stable test of `fuzz/` passes
  (`cargo test --manifest-path fuzz/Cargo.toml --lib`: 7 passed).
- Checks: `sh scripts/agent/check.sh` with a `TMPDIR` of 85 bytes: fmt,
  clippy, lint, doc, record, size, paths, static and test each 0; the
  workspace's tests: 2,562 passed, 0 failed, 35 ignored. The first run
  found two defects, repaired in place: three doc links of the crate
  (ambiguous or private), and `pod-broker`'s test
  `a_socket_path_that_is_another_file_is_refused`, which bound a socket
  under `TMPDIR` with no short path and failed there (`SUN_LEN`): it takes
  `pod_testkit::host::socket_dir`. `python3 -I scripts/check-sources.py`
  with a clone of podssh: 127 files, 0 changes on its `main`.

- The trial of podssh (`docs/design.md`, section 8, step 7):
  the patch `pod-relay` of `adoption/podssh/` (removed after the take),
  its `.commit` (`75a5031`, which podssh held to fixes) and `.run`. The patch changes 318 files of podssh (7
  new, 173 removed, 138 changed; 1,325 lines added, 25,812 removed;
  `Cargo.lock` aside). podssh-relay keeps each module name of the cut as
  glue over `pod_relay`: `src/context.rs` makes the `Context` of the
  process (`context::process()`, `context::of(&proxy)`: the way of
  `podssh_ws::client::way`, the proxy of each host, the places of
  `PODSSH_CACHE_DIR`, each `PODSSH_*` variable read at each call with
  `Names::podssh()`, the keys, the members and the rank that the settings
  file and the net give, the process resolver, one shared state); each
  free function keeps its old signature over it; `open::Request` and
  `open` keep podssh's form; `reverse::node_state()` and
  `reverse::settings()` keep a node's upgrade, rotations and lines the
  process's (the spares to the sink of `spares::tell`, a relay's close to
  `session::budget`); `reverse::tcp::handler` gives a node's target
  podssh's dialer; the role's carrier `Door` implements
  `reverse::wire::Door`. podssh-cli: 32 struct literals take `ctx` in
  place of `proxy`, 11 targets `tcp::handler`, 12 nodes
  `reverse::settings()`, 5 layered handlers `Layered::telling` with the
  budget's sink, and the ends of a code take their way out again after the
  pins (an `Ends` made before `--relay-addr` held the resolver of no
  pins: `podssh-cli --test relay_code_role` failed). Out of podssh: 31
  test files of the cut and their harnesses, the token tests of
  `tests/cache.rs`, 37 plants (28 are `relay-*` here; T-036 takes 3, T-229
  the other 6), the fuzz targets `relay_list` and `relay_pool` with their
  seeds, and the live steps of the moved tests (T-228). The role's tests,
  `pair_code.rs` (the role's mailbox, T-036) and the places of the cache
  stay. The record of podssh: 338 citations of removed code name
  `PodUtils/podssh:` paths, the citations of each changed file move by its
  diff, and 16 done entries name in a Correction the test targets that
  moved.
- The trial found two defects, repaired: `failover::MODE_ENV` named
  podssh's variable in the crate, and `Names::failover` was read by no
  word (`Mode::from_setting(value, name)` now; the test
  `a_word_that_is_no_mode_is_refused_with_the_name_of_the_caller_s_setting`
  failed on the code before); and podssh's test of a signed list read
  "this podssh", where the crate says "this client" (T-212 lists the
  words that `Names::app` can give back).
- Local results, podssh `75a5031` with the patch and the crates of this
  tree: the patch applies to a fresh checkout; `podssh-todo check` (706
  entries), `PodUtils/podssh:scripts/check-repo.py`,
  `PodUtils/podssh:scripts/check-scripts.py`, rustfmt,
  `cargo deny check bans licenses sources`, the consumer checks of
  toolchains and test targets: each 0; clippy with `-D warnings` on the
  crates of the gate, on podssh-relay with each feature and with
  `serve,plain-ws,quick,quic`, and on podssh-cli, podssh-iroh and
  podssh-warp with `iroh-test` and `warp`: clean; `cargo test -p
  podssh-relay --all-features`: 611 passed, 18 ignored (51 binaries);
  podssh-cli, its library and the 20 test targets of the `.run` (a node,
  an operator, a pair, a code, a token, the pool, the rank, the failover,
  a forward session, the role): 1,266 passed. The first run of the CLI
  failed 3 tests: the variable of the failover that podssh's manual names
  (repaired in the crate and the glue), the words of a signed list (the
  test takes the crate's words), and the ends of a code made before the
  pins (repaired in the CLI). The steps of the `.run` on the
  fresh checkout: each 0, but the three builds that the disk of this host
  did not take.
- `adoption.yml` ran the whole `.run` of the trial in a clean checkout
  (run 43130443273, `main` at `99099e4`): it passed, with the build of the
  release's features with zig, podssh's static check and the build with
  `iroh,warp`.
- The patch names the tag `v0.27.0` (the fix of `Mode::from_setting` is
  not in `v0.26.0`). T-224 moved `dashboard-host` of `pinggy` into
  `url-skip`: the patch changes `pinggy::dashboard()` of podssh-relay to
  the first text of `url-skip` (the same line count, so no citation
  moves).

What remains, in order:

1. The release, and the take of podssh (`docs/design.md`, section 8,
   step 8); podvm takes the facade `blocking` (Q103).
2. T-212: the lines for the user as events, and the words of podssh's
   commands as the caller's.

# T-026: `pod-relay`: podnet's relay client and its plain reverse mode

**Source:** `docs/design.md`, section 8, step 5.
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

podnet's relay code (the mint, the token cache, `RelayBudget`, the race
of hosts, the reverse mode on `/v1/node/NAME` with 32-hex ids) must keep
each capability in `pod-relay`.

## Premise

- Read: podnet `PodUtils/podnet:src/ws_relay.rs` (995; `active()` is a
  process global), `PodUtils/podnet:src/relay_pool.rs` (380; a copy of
  podssh's pool), `PodUtils/podnet:src/relay_reverse.rs` (1,139).
- podnet holds these three files to fixes from `7c5f87e`
  (PodUtils/podnet#449); import from that commit or a later one that a
  COORD issue of podnet names.

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

The trial of podnet (What remains, step 1) starts after T-234, which is
done.

## Correction

2026-11-12: at `7c5f87e`, `PodUtils/podnet:src/ws_relay.rs` has 994
lines, `PodUtils/podnet:src/relay_pool.rs` 406, and
`PodUtils/podnet:src/relay_reverse.rs` 891 with
`PodUtils/podnet:src/relay_reverse/pairfile.rs` (183) and
`PodUtils/podnet:src/relay_reverse/session.rs` (232). podnet's `main`
(`3657539`) holds the same blobs: no fix came after `7c5f87e`. The work
started while T-025 was partial: its import and its adaptation are done.

## Done

2026-11-12, partial: the trial of podnet remains (T-234 is done). Commits
`d012f20` (the capabilities and their tests) and this record (the plants,
the README, `SOURCES`, the calls C73 to C76).

- The parity list is in `crates/pod-relay/README.md` ("Parity", "podnet"):
  each item of podnet, its item here or the place where it stays, and its
  test. `SOURCES` has a block of podnet at `7c5f87e` (7 files):
  `python3 -I scripts/check-sources.py --clone PodUtils/podnet=CLONE
  --clone PodUtils/podssh=CLONE pod-relay`: 127 and 7 files, 0 changes.
- Added, with tests: `RelayBudget` (`budget`, call C73); `stream::of` (a
  forward session as a byte stream); the tokens of a run in the `Context`
  (`token::remember`, `forget`): a run mints once for each token's life,
  also with no file of the cache; `reverse::local` (podnet's targets:
  `host:port`, `tcp://`, `[IPv6]:port`, `unix:/path`, and `LocalHandler`,
  whose refusal names the target); `reverse::Settings::half_close` and
  `HALF_CLOSE_IDLE` (podnet T-196); `OperatorLimits::eof_idle` and
  `EOF_IDLE` (podnet issue 109); `pair::newest` (the stored pair with the
  most time left, and the next one). The plain mode is these settings,
  with podssh's behaviour by default (call C74). `reverse/serve.rs` keeps
  the loop and the writer; the local side of a session is
  `crates/pod-relay/src/reverse/route.rs`. The labels of the store moved to
  `crates/pod-relay/src/pair/stored.rs` (the same paths, re-exported).
- Defects of the base, repaired (call C75); each test fails on its plant,
  which puts the code before back: after the relay's `close`, the copy to
  a local side that does not read stayed in its write while the node ran
  (`Settings::drain`, `DRAIN`; `a_relay_close_ends_a_session_whose_target_does_not_read`);
  a second `open` of a live id opened a second local side
  (`a_second_open_of_a_live_session_is_no_new_session`); a `401` made a
  node connect again for ever (`a_node_whose_token_is_refused_with_401_stops`);
  `pool::same_deployment` took a host under an IP address of the primary,
  and a host with an empty label (podnet's `hosts_of_one_deployment`).
- The differences that stay as the base has them: call C76. What stays in
  podnet (its lines of an expiry, its ready document, its instances, its
  variables): call C74.
- Tests: `cargo test -p pod-relay --all-features`: 374 passed, 19 ignored
  (191 unit tests; `tests/reverse_plain.rs` 7). clippy with `-D warnings`,
  with all features and with none: clean. `python3 -I
  scripts/check-size.py`: 0 faults.
- Plants: 9 new (group `relay`, scripts/plants/relay-t026-*), each caught
  on this host by its test. Two plants of T-025 were stale after the split
  of `serve.rs` and the `401` (relay-t643-426-reconnects,
  relay-t648-one-queue-node): their patches are made again on the same
  lines, and each is caught. `sh scripts/plant-each.sh --apply`: 0 stale.
- The trial of podssh (T-025) names no public item that changed: its
  literals of `reverse::Settings` and `OperatorLimits` end with `..`. It
  was not run again here.
- Not measured: the mint path that keeps its token in the run (the mint is
  HTTPS, and no test has a TLS stand-in of the mint); the drain and the
  half close against the live relay; podnet's own tests on the crate.

2026-11-12, partial: the trial of podnet started; no file of the trial is
committed, and no line of a `.run` ran.

- The commit of the trial: podnet's `main` at `1abfa91`
  (`1abfa9150c72c1c073ba951ebfdd1d0728313859`). It holds the same blob of
  each of the 7 files of the block of podnet in `SOURCES` as `7c5f87e`;
  `git diff 7c5f87e 1abfa91` of these files is empty, and no file of them
  moved. No fix of podnet to port.
- The calls of the glue: C101 (podnet turns on `plain-ws` in its
  dependency), C102 (podnet keeps the relay of its process), C103 (the
  wire of a stored pair, and the next pair in podnet's loop).
- What the patch must change, from a reading of podnet at `1abfa91`:
  - The manifests: each podcrates tag of the 6 manifests with one
    (`PodUtils/podnet:Cargo.toml` and the 5 crates) to `v0.29.0`;
    `pod-relay` with `pair` and `plain-ws`; the lock of
    `PodUtils/podnet:fuzz/Cargo.lock`.
  - `PodUtils/podnet:src/ws_relay.rs`: a `Relay` of a `RelayList`, the
    wire and a `Context` (`new`, `single`, `from_env`, `url` with the
    scheme, `urls`, `host`, `port`, `connect` over `open` or
    `open_on_loopback` and `stream::of`; an IPv6 target stays refused);
    one `Context` of the process: the way of `crate::trust::context()` and
    `crate::egress::egress(None)`, `Trust::Default` (as podnet's
    `ws::dial`), its state directory as `Cache::in_dirs` (`Cache::none()`
    with `PODNET_STATE_DIR=none`), `PODNET_RELAY_TOKEN` as
    `Settings::token`; `RelayBudget` and a `meter(io)` over
    `RelayBudget::meter_for` for `PodUtils/podnet:src/warp/session.rs`;
    `SESSION_BYTES` and `SESSION_AGE` as the crate's budgets for
    `PodUtils/podnet:src/edge_h2.rs`.
  - `PodUtils/podnet:src/relay_pool.rs` goes; its users are
    `PodUtils/podnet:src/ws_relay.rs` and
    `PodUtils/podnet:src/relay_reverse/pairfile.rs` alone.
  - `PodUtils/podnet:src/relay_reverse.rs`: the pairs as `pair-NAME.json`
    of the store in the directory `relay` of the state directory; podnet's
    `load` keeps its exit codes (64, 69, 78) and its refusal of a directory
    that others can write (the store of `pod-core` makes it 0700); a named
    pair is read with `pair::read_file` after a check for a symbolic link;
    with no name, `pair::newest`; `run_node` over `reverse::run` with
    `LocalHandler` (a direct `pod_proxy::Dialer` over podnet's
    `resolve::plain()`, as podnet dialed its target with no proxy),
    `half_close: Some(HALF_CLOSE_IDLE)`, a stop at the pair's end, the
    expiry lines, `detach::ready` at the line `online` of
    `NodeConfig::say`, and a fault of podnet for each `Exit`; `connect`
    over `operator::run` with `eof_idle: Some(EOF_IDLE)` and
    `tokio::io::join(input, output)`; `pair` and `stop` over
    `pair::create` and `pair::stop`. Its callers
    (`PodUtils/podnet:src/cli/cmd/relay.rs`,
    `PodUtils/podnet:src/cli/cmd/serve_relay.rs`,
    `PodUtils/podnet:src/cli/cmd/git_serve.rs`,
    `PodUtils/podnet:src/roads.rs`) read the tokens of a pair by its
    methods.
  - The tests: those that the crate holds leave (the README, "Parity",
    "podnet"); the six of its last row, `the_instances_of_a_pair`,
    `the_node_ready_document_names_the_pair`, `the_expiry_words_and_parts`
    and the exit codes of a pair file stay. A test writes its pair as a
    file of the store's form: `pair::Pair` has no public constructor, and
    `pair::parse` asks for 10 min left and tokens of 8 characters or more.
    The fake relay of `PodUtils/podnet:src/dialers/tests.rs` takes the
    token `good`: `token::kept` refuses a given token of fewer than 8
    characters (`TokenError::BadEnvironment`), so the test needs a longer
    one.
  - podnet's checks: `PodUtils/podnet:scripts/layers-allow.txt` loses
    `ws_relay -> edge_h2` and `ws_relay -> ws` (a row with no reference is
    a fault); `PodUtils/podnet:scripts/size-allow.txt` lowers or removes
    the counts of `ws_relay.rs` (994) and `relay_reverse.rs` (891); the
    citations of `relay_pool.rs` with a line in its record
    (`PodUtils/podnet:TODO/fixes.md`, `PodUtils/podnet:TODO/crates.md`,
    `PodUtils/podnet:docs/protocol.md`, `PodUtils/podnet:SECURITY.md`)
    name their commit or leave the backticks, and its documents name the
    files of a token and of a pool as the crate keeps them.
- Not measured: each step above; podnet's build on the crates of this
  tree.

2026-11-12, partial: the trial of podnet passed on this host (step 1 of
the list before). Commits `a176a93` (`pod-relay`: the notes of the opener
are `Send`) and this record with adoption/podnet/pod-relay.patch, `.commit`
and `.run`.

- The commit of the trial: podnet's `main` at `98d8a71`
  (`98d8a7126c1ca1d448ab35e4a485c690adb82b3e`, its take of `v0.29.0`).
  `git diff 1abfa91 98d8a71` changes no file of the block of podnet in
  `SOURCES` and no caller. The patch moves each tag from `v0.29.0` to
  `v0.30.0`, as `pod-hold-syn.patch` moved them to the next release.
- The patch (32 files): the 6 manifests, and `pod-relay` with `pair` and
  `plain-ws` (C101); `PodUtils/podnet:src/ws_relay.rs` (353 lines, was 994) as the glue:
  one `Context` of the process (`ws_relay::context()`: the way of
  `trust::context()` and `egress::egress(None)`, the state directory as
  the cache, `PODNET_RELAY_TOKEN` as `Settings::token` by `from_env`,
  podnet's `Names`), `set` and `active` kept (C102), `Relay::{new, single,
  from_env, url, urls, host, port, connect}` over `open` or
  `open_on_loopback` and `stream::of`, `meter` over
  `RelayBudget::meter_for`, `SESSION_BYTES` and `SESSION_AGE` of the
  crate; `PodUtils/podnet:src/relay_pool.rs` and `PodUtils/podnet:src/relay_reverse/session.rs` removed;
  `PodUtils/podnet:src/relay_reverse.rs` (532 lines, was 891) over `pair::{create, stop}`,
  `reverse::run` with `LocalHandler` (a direct `pod_proxy::Dialer` over
  `resolve::plain()`), `half_close: Some(HALF_CLOSE_IDLE)`, a stop at the
  end of the pair, `detach::ready` at the line `online`, and a fault for
  each `Exit`, and `operator::run` with `eof_idle: Some(EOF_IDLE)`; the
  pair file `relay/pair-NAME.json` of the store (`PodUtils/podnet:src/relay_reverse/pairfile.rs`:
  the checks and the exit codes 64, 69 and 78, the next pair over
  `pair::newest`, the relay text of the file for the wire, C103); the
  callers; the tests (`PodUtils/podnet:tests/relay_reverse.rs` keeps its six tests and its
  fake relay, which now forwards the node's `ready` with its id; the
  pairfile tests keep the exit codes; the dialer test takes a token of 8
  characters or more); `PodUtils/podnet:scripts/layers-allow.txt` (two rows less),
  `PodUtils/podnet:scripts/size-allow.txt` (two rows less), `PodUtils/podnet:scripts/check-layers.py`
  (`relay_pool` out); `PodUtils/podnet:SECURITY.md`, `PodUtils/podnet:docs/protocol.md`, `PodUtils/podnet:docs/relay.md`,
  `PodUtils/podnet:llms-full.txt` (`PodUtils/podnet:scripts/gen-llms.py`); the record of podnet (the remap
  of the edited files, and each citation of a line of a removed or
  rewritten file as "lines N at `COMMIT`" of the tree that it read).
- The `.run` (22 steps), each run here in a working copy that equals the
  patched checkout of `98d8a71` (`diff -r` with no difference but the
  locks), with the manifests on the crates of this tree as
  `scripts/adoption.sh` makes them: the remap and `podnet-todo check` (the
  record agrees), `check-size.py` (0 faults), `check-layers.py` (0 faults),
  the build, `cargo deny --locked check bans sources` (exit 0; warnings
  only), the features `pair` and `plain-ws` in the tree, the removed files,
  a grep for no definition of the moved items, clippy of the workspace with
  `-D warnings`, `cargo test -p podnet --lib` (513 passed), the tests
  `relay_reverse` (6) and `relay_listen` (2), the tests `exit_codes` and
  `llms` of `cli` (17), `docs` and `api_surface` (16), the lock and the
  build of `fuzz/`, rustfmt, `check-consumer-toolchains.py`, `cargo
  zigbuild` for x86_64 musl, and `check-static.sh`: each passed.
  `scripts/adoption.sh` itself and `adoption.yml` did not run here: the
  disk of the session had no room for a second checkout and its build
  (`adoption.yml` runs it at the push). No step needs root or the
  network beyond crates.io.
- `pod-relay` (`a176a93`): the future of `open`, `open_on_loopback` and
  `forward_layer::client::Dialer::link` was not `Send`, as its notes were
  `&mut dyn FnMut(&str)`; podnet's edge leg, its warp client and its relay
  dialer spawn an open. The notes are `&mut (dyn FnMut(&str) + Send)`
  (call C112). Test `the_open_of_a_session_can_run_in_a_spawned_task`: it
  does not compile with the old type. `cargo test -p pod-relay
  --all-features`: 386 passed, 19 ignored; clippy with all features:
  clean.
- Calls: C110 (the bound of `Relay::connect`), C111 (podnet's old pair
  files are not read), C112 (the notes are `Send`).
- Findings. (1) The defect of the crate above, repaired. (2) podnet's fake
  relay gave the operator `{"type":"ready"}` with no id; the crate's
  operator reads a `ready` with its id (`NodeOutbound::Ready`), as the
  relay forwards the node's own text, so the session waited 20 s; the
  patch repairs the fake. (3) podnet's behaviour that changes with the
  base (C76): a node whose name another node holds exits 69 after 30 s
  (`Settings::first_conflict`; podnet connected again for ever); a Close
  `1008` with "token" no longer stops the node (the crate connects again;
  a `401` or `403` to its upgrade stops it); a lost socket that served is
  opened again at once, then with the crate's backoff; a forward session
  takes the crate's roads (HTTP/2, the cleartext road last), its rank and
  its record of failures, whose files go to podnet's state directory; a
  bad pair name of `relay connect` exits 64. (4) The take adds 9 packages
  to podnet's lock (`pod-relay`, `pod-session`, `pod-e2e`,
  `pod-identity`, `snow`, `chacha20poly1305`, `x25519-dalek`, `wasip2`,
  `wit-bindgen`): `pod-relay` needs the resumable layer and the
  end-to-end channel, which podnet's plain mode does not use (T-290).
  (5) podnet's T-200 (the reverse road on the alternate hosts) stays
  blocked: `pair` asks the control host only, as no pool host was
  measured to serve `/v1/*`.
- Not measured: the trial in `adoption.yml`; the live relay through the
  glue (the mint, the pool, a pair of `podnet relay pair`, a node and an
  operator at `tcp.ssh.relay.ajam.dev`); the size of podnet's release
  binary after the take.

2026-11-12, partial: the words of `plain-ws` follow call C101 in
`crates/pod-relay/src/open.rs`, `crates/pod-relay/src/reverse/wire.rs`,
`crates/pod-relay/Cargo.toml`, `crates/pod-relay/README.md`,
`crates/pod-ws/Cargo.toml` and `crates/pod-ws/README.md`: podnet's binary
turns the feature on for a relay on the loopback; a host off the loopback
stays refused.

2026-11-12, done: podnet took it. `adoption.yml` run 43191076065 passed
the trial at podnet `98d8a71`; `v0.30.0` is at `613058c` (GitHub #89).

- podnet `83e0c93` (PodUtils/podnet#481) takes `tag = "v0.30.0"` with
  adoption/podnet/pod-relay.patch (a three-way apply on its `main`; the
  conflicts were the tag lines and the feature `ipv4-fragmentation` of
  `pod-netstack` of PodUtils/podnet@T-248, kept); `pod-relay` with `pair`
  and `plain-ws`. podnet ran the steps of the `.run` but
  `check-consumer-toolchains.py` and the zig build (its CI builds the
  static binaries); `cargo test --workspace --no-fail-fast` had 0
  failures.
- The `taken` line of the block of `7c5f87e` in `crates/pod-relay/SOURCES`:
  `python3 -I scripts/check-sources.py --clone PodUtils/podnet=CLONE
  --clone PodUtils/podssh=CLONE pod-relay` gives 0 changes in each block.
  The trial files adoption/podnet/pod-relay.* are gone; the adoption map
  of `crates/pod-relay/README.md` names the take. The freeze of podnet's
  `src/ws_relay.rs`, `src/relay_pool.rs` and `src/relay_reverse.rs` ends.
- The next trial of podssh (T-025) and podvm's take of the facade see the
  changes of calls C75, C80 and C112; T-290 (the layer and the channel as
  a feature) is open.

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
- Read on 2026-11-12 (PodUtils/podnet#482, after podnet's take of `v0.30.0`):
  the relay contract (`llms-full.txt`, the fact `contract-sha256` of
  `ajam-relay`) has no renew path. The crate has
  `pair::renew::renew`: `POST /v1/renew/NAME` with the stop token, which
  podssh's relay role serves (PodUtils/podssh@T-592) and a relay of the contract
  answers with 404 or 405 (no refusal, no renewal). `reverse::run` renews
  nothing. podnet's T-161 asks for `POST /v1/pair/NAME/renew` with the node
  token.

## Approach

A setting of `reverse::run` that renews the pair before its end with the
call of the crate (`POST /v1/renew/NAME`, the stop token, where the pair
file of the node holds it): at half the TTL, and again in the first
warning band; a relay that answers 404 or 405 renews nothing, and the node
says so once. A test with the stand-in relay of `pod-testkit`. The call of
the public relay (a path, the node token as podnet's T-161 asks) waits for
the owner of the relay: it adds the path to the contract, and the crate
then takes it beside the call of podssh's role.

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
**Status:** done

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

## Done

2026-11-12. No new code: `crates/pod-relay/src/failures.rs` (the import
of T-025, PodUtils/podssh@T-059 and PodUtils/podssh@T-563) is the health
memory. Each run keeps a record for each road and host in the cache
(`relay-failures.json`): a host that failed with an error that another
host could repair, or that said nothing while a host after it opened (a
lost race), is tried last for `WINDOW` (10 minutes); a host whose last
`STRIKES` (2) sessions were cut within `SHORT` rests for `REST`; a host
that opened within `WINDOW` goes first; an open clears its failure. No
host is left out, and the order is stable otherwise; a note names what
moved a host. The difference from the ask: one failure or lost race
moves a host, not two; a stalled primary then costs the stagger once in
10 minutes. podnet takes the rule with `pod-relay` (T-026); a count of
two comes by a COORD issue. Tests: `failures::tests` (10 of the
import), the plant `relay-t059-faults-recorded`.

# T-123: `pod-relay`: the session budget from the pool document (PodUtils/podnet#413)

**Source:** PodUtils/podnet#413, ask 3 (PodUtils/podnet@T-163, PodUtils/podnet#360)
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

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

## Done

2026-11-12. No new code: the import of T-025 (PodUtils/podssh@T-342) and T-026
hold it. `pool::parse_limits` reads `limits` of `/relays.json`
(`idle_timeout_ms`, `max_session_ms`, `max_session_bytes`,
`max_frame_bytes`; a value that is missing, not a whole number or 0 is
left out); the cached pool keeps them (`pool::cached_limits`).
`relay::byte_budget_under` and `relay::time_budget` keep the margins
(64 - 60 MiB, 12 - 11.5 h): a smaller cap of the relay lowers the budget
by the same margin, and a larger one never raises it.
`budget::RelayBudget::meter_for` gives podnet's meter (`pod_core::Metered`)
the budgets of the relay in use. Tests:
`pool::tests::a_relays_json_limits_are_read_and_cached_with_the_pool`,
`budget::tests::the_budget_of_a_run_goes_under_the_limits_of_its_relay`.

# T-124: `pod-relay`: failover on the reverse road (PodUtils/podnet#413)

**Source:** PodUtils/podnet#413, ask 4 (PodUtils/podnet@T-200, PodUtils/podnet#397)
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** blocked

## Problem

`pair`, `node` and `stop` of the reverse road reach host 0 of the pool only.

## Premise

- Read: the ask of podnet.
- Not measured: a node socket on host B that serves a pair minted on host A.
- Read on 2026-11-12 (PodUtils/podnet#482): the relay contract makes a pair
  on the control host only ("Agent-created pairs need no admission
  (control host only)"), and names no move of a pair to another host.

## Approach

Measure the cross-host case first. Then one host loop for `connect`, `websocket` and `post_json`
of the reverse road, with the health memory of T-122.

## Prove

```sh
cargo test -p pod-relay --all-features
```

## Blocker

The relay contract (`ajam-relay`, `llms-full.txt`) makes a pair on the
control host only and moves none. The owner of the relay adds a pair on
(or a move to) another host of the pool; the daily watch of the contract
(`services.yml`) shows the change, and this entry then starts.

## Start condition

T-025 or T-026 is done.

# T-150: `pod-relay`: a handler target for the relay node (PodUtils/podnet#413)

**Source:** PodUtils/podnet#413, ask 11 (PodUtils/podnet@T-083)
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

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

## Done

2026-11-12. `aa919d0`. `reverse::channel(depth)` gives a `ChannelHandler`
and the receiver of its sessions: each session that the relay opens is
one end of a pipe (64 KiB each way), and the other end goes with the
session's id to a service of the process (podnet's file server, a git
SSH server, a transfer), with no socket file. A session waits for room in
the channel within the node's limit for opening it; a closed channel
refuses it ("the service of the process is gone"). A handler, not a
variant of `Target`: `Target` names a place to dial, and the node takes
any `Handler`. Test:
`a_service_of_the_process_answers_each_session_through_a_channel`
(`tests/reverse_plain.rs`: two sessions answered in turn, then a refusal
after the service is gone); the plant `relay-t150-channel-drops-the-session`
is caught.

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

# T-204: `pod-session`: `a_session_moves_while_both_ends_send_at_full_speed` lost its link one time in a run of the workspace's tests

**Source:** T-024 (a run of `sh scripts/agent/check.sh`).
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

In a run of `cargo test --workspace --all-features` (through
`sh scripts/agent/check.sh`, 2026-11-11), the test
`a_session_moves_while_both_ends_send_at_full_speed` of
`crates/pod-session/tests/session_move.rs` failed:
`no loss, no failed move: [Lost { why: "the link ended with no word from
the far end" }, Resumed { resent: 196608, after: 385.325µs }]`. Both ends
send at full speed and the session moves each 256 KiB; one link ended
before the far end's word, and the session resumed. A random failure is a
defect of the test or of the code (`docs/development.md`).

## Premise

- Measured: 0 failures in 20 runs of the test alone, and 0 in 60 runs of
  its binary six at a time. The failing run had each test binary of the
  workspace in turn, and another session may have built in the main
  checkout at that time.
- Measured on 2026-11-12, the test's binary alone: 0 failures in 150
  runs at a load of 3; 1 failure in 200 runs, eight runs at a time beside
  six busy loops on four cores (`[Lost { why: "the link ended with no
  word from the far end" }, Resumed { resent: 65536, after: 382µs }]`).
  With a line printed at each end of a stand-in link and at each end of
  the far end's resume: 0 failures in 1,120 runs under the same load.
- Read: the far end stops the old link when a resume of the session
  comes (`Keeper::take`), with no `RETIRE` and no linger; the client
  sends the offsets of a move only after its old link stopped.
- Not measured: the order of the end of the old link and the far end's
  word at a move under load.

## Approach

1. Run the test in a loop under the load of the workspace's tests (or
   under `stress` with each core busy) until it fails, with the events of
   the keeper.
2. Find the race: the old link ends before the word of the far end, or
   the test's link drops it.
3. Fix the cause in the code or in the test, and give the loop count in
   the commit message.

## Prove

```sh
for i in $(seq 200); do cargo test -q -p pod-session --test session_move -- a_session_moves_while_both_ends_send_at_full_speed || break; done
```

# T-212: `pod-relay`: the lines for the user as events, and the words of podssh's commands as the caller's

**Source:** T-025 (the adaptation of `pod-relay`).
**Category:** chore
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

`docs/design.md`, section 1, rules 8 and 10: a line for the user is a
typed event, and a name of a consumer is a parameter. After T-025 the
opener of `pod-relay` still gives its notes to a callback of text
(`open`'s `notes`), as do a node (`NodeConfig::say`) and a UDP handler
(`UdpHandler::say`). Some words of the crate still name podssh's commands
and flags: the kinds of an exchange by code (`podssh relay pair NAME
--code`, `podssh relay join NAME`, `podssh net invite NAME`), the end of a
grant (`podssh node --grant-for`), a node's spares (`--spares`),
"podssh's" in the refusals of an exchange, and the note of an IPv6 target
that the relay closed at once (`relay::ipv6_note`: `podssh relay serve`,
`--relay-host`, `-J node://NAME`; found by T-026). The other way: where podssh's
words named podssh, the crate says "this client" (an untrusted signed
list, a list of a newer form, a critical option of a grant, a frame that
the relay refused, a control frame), "a relay role of a deployment" (a
member's list with no signed list) and "a node" (a pair under its 10
minutes), where `Names::app` would give podssh its words back; podssh's
trial of T-025 takes the words of the crate in its test of a signed list.
podssh's take of `v0.28.0` (PodUtils/podssh#208) found two lines of
`OpenError::lines` that do not read `Settings::names`, although `Names`
has `offline` and `token`: an offline run ("the offline setting is on, so
no connection goes anywhere") and a token of the environment that the
relay rejected ("the relay token that was given was rejected"). podssh's
glue `podssh_relay::open::words` replaces the two texts with
`PODSSH_OFFLINE` and `PODSSH_RELAY_TOKEN`.

## Premise

- Read: `crates/pod-relay/src/open.rs`, `crates/pod-relay/src/reverse/node.rs`,
  `crates/pod-relay/src/reverse/udp.rs`, `crates/pod-relay/src/code/exchange.rs`,
  `crates/pod-relay/src/grant/door.rs`, `crates/pod-relay/src/reverse/spares.rs`,
  `crates/pod-relay/src/relay.rs`.
- Not measured: the words that podssh's tests read.

## Approach

The notes, the lines of a node and of a UDP handler become events
(`relay.note`, `reverse.online`, `reverse.udp`) of the caller's sink, with
the text of podssh. The commands of a kind of exchange become a value of
the caller (`exchange::Kind` takes them), and the flags in the words go to
`Settings::names`. `OpenError::lines` takes the `Names` (as
`forward_layer::offer` does), so podssh's `open::words` can go. podssh's
trial keeps its words through its glue.

## Prove

```sh
cargo test -p pod-relay --all-features
```

## Start condition

T-025 is done.

# T-214: `pod-wormhole`, `pod-transit`: the file transfer of magic-wormhole (PodUtils/podnet#450)

**Source:** PodUtils/podnet#450 (COORD-032 of podnet, for PodUtils/podnet@T-103 to PodUtils/podnet@T-106)
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** L
**Status:** open

## Problem

podnet's `send` and `receive` need the file-transfer layer of
magic-wormhole. `pod-wormhole` (T-022) holds the mailbox, SPAKE2 and the
phases only, and `pod-transit` (T-027) takes podssh's own transit words
only. No crate holds the offer and the answer (a file, a directory, a
text), the transit hints, the relay handshake and the records (SecretBox),
a directory as zip with an inflate decoder (`pod-extract` reads no zip),
or podnet's own hint kinds (the podnet relay as a carrier of a transfer,
PodUtils/podnet@T-104; a piping server as a transit of last resort,
PodUtils/podnet@T-252).

## Premise

- Read: PodUtils/podnet#450; PodUtils/podnet#452, the words of podnet's
  hint kinds: `podnet-relay-v1` (`relay`, an https base URL; `pair`, the
  name of a relay pair; `resume`, a bool; the connect token of the pair
  goes in its own sealed phase with the key `podnet-relay-token`, after
  the hints) and `piping-v1` (`url`; the path is the first 32 hex digits
  of HKDF-SHA256(transit key, info `podnet piping path v1`); the sender
  PUTs the sealed records, the receiver GETs them). A client of
  magic-wormhole ignores a hint kind that it does not know.
- The hosts for `pod-services` (PodUtils/podnet#452): the piping servers
  `ppng.io` and `piping.onrender.com`; the mailboxes
  `wss://relay.magic-wormhole.io/v1` and `wss://mailbox.winden.app/v1`.
- Not read: the file-transfer and transit documents of magic-wormhole
  (`file-transfer-protocol.md`, `transit.md` of the Python project); the
  current Python release, for the interop test.

## Approach

The offer and the answer, the zip of a directory (write and read, inflate
in Rust, a bound of each size and path as `pod-extract` has) go to
`pod-wormhole` under a feature `transfer`. The transit hints (direct and
relay, and a kind of the caller for podnet's carrier), the relay
handshake and the records go to `pod-transit` (call C64). The hint type
is open (a kind and a JSON object), so that podnet adds its kinds with no
change of the crate; `podnet-relay-v1` and `piping-v1` are tests of it.
The piping servers and the mailboxes become services of `pod-services`.
A stand-in of the transit relay for the tests. An interop test against
the Python magic-wormhole in `live.yml` (PodUtils/podnet@T-106).

## Prove

```sh
cargo test -p pod-wormhole --all-features
cargo test -p pod-transit --all-features
```

## Start condition

T-027 is done.

# T-215: `pod-wormhole`: the mailbox server (PodUtils/podnet#450)

**Source:** PodUtils/podnet#450 (the owner's ruling of 2026-11-11 in podnet: `podnet relay serve --mailbox`)
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

podnet runs its own mailbox server (`podnet relay serve --mailbox`, and
`send` and `receive --mailbox URL`). `pod-wormhole` has the client side
of the mailbox protocol only.

## Premise

- Read: PodUtils/podnet#450.
- Not read: the server of the Python `magic-wormhole-mailbox-server`
  (the nameplates, the mailboxes, the expiry, the welcome message).

## Approach

The server side of the mailbox protocol in `pod-wormhole` under a feature
`server` (call C64): it shares the message types and the phases with the
client. The state as a value of the caller (no global); the storage in
memory with a bound of each count; the WebSocket of `pod-ws`. Tests: the
client of the crate against the server of the crate; an interop test of
the Python client against it in `live.yml`.

## Prove

```sh
cargo test -p pod-wormhole --all-features
```

# T-222: `pod-netstack`: any IP, a TCP listener and a UDP socket on any address, and the checksums of a link device (PodUtils/podnet#454)

**Source:** PodUtils/podnet#454 (COORD-036 of podnet, for PodUtils/podnet@T-249)
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

podnet's netns router answers for each destination of the program: the
real addresses and the fake DNS addresses. PodUtils/podnet@T-249 moves the
router onto `pod-netstack`, and then starts step 2 of T-170. podnet read
`crates/pod-netstack/src/stack/reactor.rs` and
`crates/pod-netstack/src/device.rs` at `v0.22.0` (podnet's record: commit
`98bc9bb`, PodUtils/podnet@T-249, Correction) and asks for four calls:

1. Any IP. The stack never calls `Interface::set_any_ip(true)`, and
   `Core::listen` takes only an address that the stack owns. Ask:
   `StackConfig.any_ip: bool`.
2. A TCP listener on any destination. Each listen socket has
   `addr: Some(..)`. The router needs the wildcard (`addr: None`) and the
   destination of the SYN, the target of the flow. Ask:
   `NetStack::listen_any(port) -> TcpListener`; the `local_addr()` of each
   accepted `TcpStream` is the destination that the program dialed.
3. The checksums of the link device. `LinkDevice::capabilities` keeps the
   default of smoltcp (`Checksum::Both`). The veth wire
   (PodUtils/podnet@T-174) needs `Checksum::Tx` for `ipv4`, `tcp`, `udp`
   and `icmpv4`: the program's end of a veth pair can send partial
   checksums. Ask: `LinkDevice::with_checksum(ChecksumCapabilities)`.
4. UDP on any address. The UDP flows of the router need a socket on any
   address that sends from a source that is not the stack's own address
   (the answer of a remote host to the program). Ask:
   `NetStack::bind_udp_any(port)`, and a source address on `send_to`.

The hold of a SYN (`HeldSyn`) and podnet's flow table need no change: the
pump on the TUN or the veth descriptor sees each SYN before the stack.

## Premise

- Read: PodUtils/podnet#454; podnet `main` (2026-11-11):
  `PodUtils/podnet:crates/podnet-netns/src/netns/backend.rs` (on smoltcp
  0.12: `set_any_ip(true)` and a default IPv4 route; for each destination
  port a listen socket with `addr: None`; the `local_endpoint` of a socket
  is its target), `PodUtils/podnet:crates/podnet-netns/src/netns/tun.rs`
  (its device is `Medium::Ip` on both wires; the veth wire sets
  `Checksum::Tx` for `ipv4`, `tcp`, `udp` and `icmpv4`) and
  `PodUtils/podnet:crates/podnet-netns/src/netns/udpflow.rs`.
- Read: smoltcp 0.14. With any IP, `has_ip_addr` of the interface is true
  for each address (0.12 checked any IP and a route in `process_ipv4`;
  0.14 needs no route). `Interface::has_ip_addr` gives the same, so the
  stack must read `Interface::ip_addrs` to own the address of a listener.
  A TCP socket that listens with `addr: None` takes a SYN to each address
  of its port, and its `local_endpoint` is the destination of the SYN. A
  UDP socket bound with `addr: None` takes each address;
  `UdpMetadata::local_address` is the destination of a received datagram
  and the source of a sent one. smoltcp gives a packet to the first socket
  of its set that takes it (`process_tcp`, `process_udp`). `IpRepr::new`
  panics on a source and a destination of two families. The interface
  reads `DeviceCapabilities::checksum` once, in `Interface::new`. With
  `Medium::Ip`, the egress looks up no route.
- Not measured: podnet's router on the crate (PodUtils/podnet@T-249).

## Approach

1. `StackConfig::any_ip` (default false) calls `Interface::set_any_ip`.
   The listen of one address owns that address by the list of the
   interface, not by `has_ip_addr`.
2. `NetStack::listen_any(port)` and `NetStack::bind_udp_any(port)`: a
   local end with no address. Their `local_addr` is `[::]:port`, and the
   admit sees `[::]:port`. A socket on each address and a socket on one
   address of the same port are `AddrInUse` both ways (they would race in
   smoltcp). The gateway service is the first socket of the set, so a UDP
   socket on each address of its port is allowed, and a bind at its
   address is `AddrInUse`.
3. An accepted stream takes its local address from
   `tcp::Socket::local_endpoint`.
4. `UdpSocket::recv_from_to(buf) -> (n, from, to)` and
   `UdpSocket::send_to_from(buf, remote, source)`. `UdpDatagrams` gives
   the destination in `RecvMeta::dst_ip` and takes `Transmit::src_ip` as
   the source. A remote of the other family than the source is
   `InvalidInput`.
5. `LinkDevice::with_checksum(ChecksumCapabilities)`, and
   `StackConfig::checksum`, which `NetStack` gives to its device.
6. Tests through two stacks over a link: a router, and a program whose
   subnet holds each address. Plants for any IP and for the owned address.

## Prove

```sh
cargo test -p pod-netstack
sh scripts/plant-each.sh netstack-any-ip-not-set netstack-listener-owns-by-has-ip-addr
sh scripts/agent/check.sh
```

## Done

2026-11-11. `7c20ad2` (the code, the tests, the README) and the commit of
this section (the plants, the static step of `scripts/agent/check.sh`).

- The calls: `StackConfig::any_ip: bool` (default false);
  `StackConfig::checksum: ChecksumCapabilities` (default: each sum
  computed and verified); `NetStack::listen_any(port: u16) ->
  io::Result<TcpListener>`; `NetStack::bind_udp_any(port: u16) ->
  io::Result<UdpSocket>`; `UdpSocket::send_to_from(buf, remote, source:
  IpAddr) -> io::Result<usize>`; `UdpSocket::recv_from_to(buf) ->
  io::Result<(usize, SocketAddr, SocketAddr)>` (the length, the source,
  the destination); `LinkDevice::with_checksum(ChecksumCapabilities) ->
  LinkDevice`. `UdpDatagrams` gives the destination in `RecvMeta::dst_ip`
  and takes `Transmit::src_ip` as the source. A listener or a UDP socket
  on each address has the `local_addr` `[::]:port`, and the admit sees
  `[::]:port`.
- Breaking: `StackConfig` has two new fields (a struct literal with no
  `..StackConfig::default()` does not compile). A socket on each address
  and a socket on one address of the same port are `AddrInUse`. A UDP
  bind at the address and the port of the gateway service is `AddrInUse`
  (that socket got no datagram). A UDP send to the other family than the
  source is `InvalidInput`.
- smoltcp 0.14: with any IP, `has_ip_addr` is true for each address, so
  the listen of one address reads `Interface::ip_addrs` to own it.
- Defect fixed in place: a UDP send to an address of the other family than
  the socket's went to smoltcp, which panicked in the reactor
  (`IP version mismatch`), and the stack stopped (podnet's stack had the
  same code). `a_datagram_to_the_other_family_is_refused_and_the_stack_runs`
  fails on the old code and passes now.
- `cargo test -p pod-netstack`: 38 passed (29 before). New:
  `a_listener_on_any_address_takes_each_destination` (TCP to 1.1.1.1,
  10.66.0.77 and 2606:4700::1111 between a router and a program over a
  link), `with_any_ip_off_a_listener_on_any_address_takes_only_the_own_addresses`,
  `with_any_ip_a_listener_on_another_address_stays_in_the_stack`,
  `a_udp_socket_on_any_address_answers_from_each_destination`,
  `a_socket_on_any_address_shares_no_port_and_the_gateway_service_keeps_its_address`,
  `a_partial_checksum_passes_with_tx_and_is_dropped_by_default`,
  `a_tcp_exchange_with_partial_checksums_works_with_tx`,
  `a_datagram_to_the_other_family_is_refused_and_the_stack_runs`,
  `with_checksum_sets_the_capabilities`. The new tests passed 30 runs in a
  loop.
- `sh scripts/plant-each.sh netstack-any-ip-not-set
  netstack-listener-owns-by-has-ip-addr`: 2 caught.
- `sh scripts/agent/check.sh` (its own `CARGO_TARGET_DIR`): fmt 0, clippy 0,
  lint 0, doc 0, record 0, size 0, paths 0, static 0, test 0 (the
  workspace: 2,608 passed, 0 failed, 37 ignored).
- Fixed in place: the static step of `scripts/agent/check.sh` read the
  probes in `target/`, not in `CARGO_TARGET_DIR`.
- Found: T-223 (a SYN sent again can reach a newer listen socket of the
  pool of a listener).
- PodUtils/podnet@T-249 can start on the first tag with `7c20ad2`.

# T-223: `pod-netstack`: a SYN sent again can reach a newer listen socket of the pool of a listener

**Source:** T-222 (a reading of smoltcp 0.14 for the listener on any address).
**Category:** defect
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

A listener of `NetStack` keeps a pool of listen sockets (`accept_pool`,
8 by default). smoltcp gives a TCP segment to the first socket of its set
that takes it, in the order of the slots, and a socket in `Listen` takes
each SYN with no ACK on its port: it does not look for a socket that holds
the 4-tuple. When the reactor adopts a connection, it adds a new listen
socket into the first free slot. Thus a listen socket can come before a
socket of the same port in `SynReceived`. When the peer sends its SYN
again (its SYN-ACK was lost or late), the newer listen socket takes it:
two sockets hold one 4-tuple with two initial sequence numbers, and the
first shadows the second. The handshake can then end with a RST, and the
second socket can stay in `SynReceived` in the pool.

## Premise

- Read: smoltcp 0.14: `process_tcp` (the first socket that takes the
  segment), `tcp::Socket::accepts` (a socket in `Listen` with no 4-tuple
  takes each SYN with no ACK on its port), `SocketSet::add` (the first
  free slot); `crates/pod-netstack/src/stack/service.rs` (`accept_inbound`
  adds the new listen socket) and `crates/pod-netstack/src/stack/reactor.rs`
  (`drop_socket` frees a slot).
- Not measured: a run with a lost SYN-ACK; what the peer and the two
  sockets do then. A link in the process loses a packet only when a
  bounded queue is full; a pump over a real link can lose one.

## Approach

1. A test: two stacks over a pump that drops the first SYN-ACK of a
   connection, with a free slot before the socket of its handshake (a
   closed connection, then an accept of another connection).
2. A fix: in smoltcp, `process_tcp` looks for the 4-tuple before the
   listen sockets (report it with the test); or in the reactor, a SYN
   whose 4-tuple a socket in `SynReceived` holds does not reach a listen
   socket of the pool.

## Prove

```sh
cargo test -p pod-netstack --lib
```

## Done

2026-11-12. `dc0e807` (with T-232).

- The look at each inbound SYN of T-232 is the place of the fix: a SYN
  whose 4-tuple a socket holds (in a state other than SYN-SENT) does not
  reach smoltcp. smoltcp ignores such a SYN in the socket that holds the
  4-tuple, and that socket sends its SYN-ACK again at its own time. Call
  C72. No report to smoltcp: the crate keeps the release of smoltcp 0.14.
- T-232 adds a listen socket at each SYN that finds none, into the first
  free slot of the set, while the socket of that SYN is in SYN-RECEIVED:
  the case of the Problem can come at each SYN. The fix covers it.
- `cargo test -p pod-netstack --lib`: 51 passed. The test
  `a_syn_sent_again_reaches_its_own_socket`: a pump drops the first
  SYN-ACK, and a closed listener frees the first slot of the set before
  the socket of the handshake. With the check planted out
  (`netstack-syn-again-to-a-listen-socket`, caught), the newer listen
  socket takes the SYN, and the server accepts no connection in 5 s.

# T-229: `pod-relay`: a test of the crate for each of six plants of podssh whose tests stay in podssh

**Source:** T-025 (the trial of podssh).
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

Six plants of podssh change code that is `pod-relay`'s now, and only a
test of podssh catches each: `t059-faults-recorded` (`failures.rs`,
podssh's interop step `faults`), `t173-no-room-not-taken`
(`forward_layer/client.rs`, `podssh-cli --test relay_serve`),
`t421-backoff-after-a-fault` (`reverse/node.rs`, `--test node_cut`),
`t617-ask-skipped` (`open.rs`) and `t617-member-key-unchecked`
(`pool/members.rs`), both `--test relay`, and `t620-offers-a-flag`
(`reverse/legs.rs`, `--lib node_rotate`). The trial of T-025 removes them
from podssh, as their patches change no file of podssh. Until a test of
the crate catches each defect, no check guards that code.

## Premise

- Read: `PodUtils/podssh:scripts/plants/` at `75a5031` (the six `.plant`
  and `.patch` files), `crates/pod-relay/tests/`.
- Not measured: whether a test of the crate over the stand-ins of
  `crates/pod-relay/tests/` reaches each defect; T-036 takes the three
  plants whose tests run the relay role.

## Approach

As T-021 did with `crates/pod-e2e/tests/door.rs`: for each defect, a test
of `pod-relay` over loopback stand-ins (the record of failures and its
order, the answer of a role that has no room, the backoff of a node after
a socket that served, the ask of a net's relay member and the check of its
key, the count of the sockets that heard the offer of a rotation), and the
plant as `relay-tNNN-*` of the group `relay`, which the test catches.

## Prove

```sh
cargo test -p pod-relay --all-features
sh scripts/plant-each.sh --group relay
```

## Done

2026-11-12. `9e774f2`. Each of the six patches of podssh, moved to the
code of the crate, is a plant of the group `relay`:
`relay-t059-faults-recorded`, `relay-t173-no-room-not-taken`,
`relay-t421-backoff-after-a-fault` and `relay-t617-member-key-unchecked`
are caught by the unit tests that came with the import
(`failures::tests`, `forward_layer::client::tests::only_a_busy_refusal_in_the_handshake_is_no_room`,
`reverse::node::tests::a_socket_that_served_is_taken_again_at_once_once_each_10_s`,
`pool::members::tests::a_list_of_another_key_is_not_the_member_s_even_when_trusted`).
Two needed a test: `tests/members_ask.rs`
(`a_run_with_the_default_list_asks_the_net_s_relay_member_first`: a run
with the default list connects to the role of its net's member on the
loopback before its race, through `open_on_loopback`) for
`relay-t617-ask-skipped`, and
`reverse::legs::tests::the_next_pair_s_socket_that_heard_the_offer_raises_the_count`
for `relay-t620-offers-a-flag`. `sh scripts/plant-each.sh` on the six: 6
caught.

# T-228: `pod-relay`: the live tests of the relay client in `live.yml`

**Source:** T-025 (the trial of podssh).
**Category:** chore
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The 19 live tests of `pod-relay` (`#[ignore]`: `live` 2,
`live_backpressure` 1, `pair_live` 1, `reverse_live` 4,
`reverse_idle_live` 6, `reverse_drops_live` 1, `blocking_live` 2,
`grant_live` 1, `pair_code_live` 1) run in no workflow of podcrates.
podssh ran them by hand: `relay-live.yml` (`live_backpressure`,
`reverse_live`), `reverse-drops.yml` (the drops of reverse sockets, from
GitHub's network beside a run from another network) and `pair-code.yml`
(a short run through each public mailbox). The trial of T-025 removes these
steps and `reverse-drops.yml` from podssh with the tests.

## Premise

- Read: `PodUtils/podssh:.github/workflows/relay-live.yml`,
  `PodUtils/podssh:.github/workflows/reverse-drops.yml` and
  `PodUtils/podssh:.github/workflows/pair-code.yml` at `75a5031`;
  `.github/workflows/live.yml`.
- The live tests read the variables of their workflow (call C60):
  `PODSSH_RELAY`, `PODSSH_LIVE_RELAY_ADDR`, `PODSSH_DROPS_UNTIL` and the
  proxy.

## Approach

A step of `live.yml` for each test file, chosen by an input of the run by
hand, with the variables of its podssh workflow; the run of the drops with
its end time as an input. The run of the drops through podssh's relay role
on a runner waits for T-036.

## Prove

```sh
cargo test -p pod-relay --all-features -- --ignored --list
python3 -I scripts/check-workflows.py
```

## Done

2026-11-12. `b3dfc88`: `live.yml` has the job `relay` (by hand; the
input `relay` names one of `live`, `live_backpressure`, `pair_live`,
`reverse_live`, `reverse_idle_live`, `reverse_drops_live`,
`blocking_live`, `grant_live`; the input `until` gives
`PODSSH_DROPS_UNTIL`). The job `live` runs the tests of `pod-wormhole` and
`pair_code` of `pod-relay` against magic-wormhole's own mailbox server and
library each day (`POD_WORMHOLE_TEST_SERVER=required`; magic-wormhole
0.24.0, magic-wormhole-mailbox-server 0.8.0), and `live` of `pod-wormhole`
and `pair_code_live` of `pod-relay` on Thursdays and by hand. Fixed on the
way: no workflow set `POD_WORMHOLE_TEST_SERVER=required`, so the tests
against magic-wormhole's server skipped in CI. Run 43153607165 (job
`live`): 20 + 11 + 14 tests of `pod-wormhole` (none skipped), 7 of
`pair_code`, both public mailboxes met and paired. Run 43153611375 (job
`relay`, `live`): passed. The run of the drops through podssh's relay
role on a runner waits for T-036.

# T-230: `pod-netstack`: a listener frees its listen sockets, its port and its address at its close and at its drop (PodUtils/podnet#457)

**Source:** PodUtils/podnet#457 (COORD-039 of podnet, for PodUtils/podnet@T-249)
**Category:** defect
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

podnet's netns router opens one `listen_any(port)` for each destination
port that the program dials. PodUtils/podnet#457, item 1: "A `TcpListener`
has no close. With no close (and a drop that does not free the listen
sockets), the router must keep each listener for the life of the pod.
podnet caps them at 256 destination ports, and a SYN to a 257th port is
dropped. The old loop had no such cap (4096 sockets at one time). Ask:
`TcpListener::close()`, or a drop that frees its listen sockets, so the
router opens a listener for a port while a flow to it lives, and frees it
after."

The drop of a `TcpListener` drops only the receiver of its queue of
accepted streams. The reactor keeps the `Listener` with its `accept_pool`
listen sockets (each with two buffers of `tcp_buffer`), keeps its port (a
new listen on it is `AddrInUse`), and keeps the address that `Core::own`
added for it (the stack owns at most `MAX_ADDRESSES`).

## Premise

- Read: PodUtils/podnet#457; `crates/pod-netstack/src/stack/socket.rs`
  (`TcpListener` holds the receiver of its queue and has no `Drop`);
  `crates/pod-netstack/src/stack/reactor.rs` (`Core::listen`, `Core::own`:
  no call removes a listener, a listen socket or an owned address).
- Read: smoltcp 0.14 `tcp::Socket::dispatch` resets a socket whose local
  address the interface does not have. Thus the stack can drop an owned
  address only when no connection uses it.
- Not measured: the memory of the 256 listeners of podnet.

## Approach

1. The reactor removes a listener whose queue receiver is gone (the drop
   of `TcpListener`), at each pass and before each listen. It removes each
   socket of the listener that it did not adopt. `TcpListener::close(self)`
   drops the listener and kicks the reactor.
2. The streams that the listener accepted stay. The streams in its queue
   close as dropped streams do (T-231).
3. The stack drops the address that `own` added for the listener when no
   listener and no connection uses it.
4. Tests: open and close listeners on 1,000 ports in turn, and read
   `socket_count`; a listen on the port after its close; an accepted
   stream after the close of its listener; listeners on more addresses
   than the stack can own, one after the other.

## Prove

```sh
cargo test -p pod-netstack --lib stack::tests::listener
```

## Done

2026-11-12. `dc0e807`.

- `TcpListener::close(self)` and its `Drop`: the reactor removes the
  listener at its next pass (the drop kicks it) and before each listen.
  Its listen sockets and its half-open sockets go; its established
  connections that the app did not accept, and the streams in its queue,
  close as dropped streams do (T-231); the streams that it accepted stay.
  The address that `own` added goes when no listener and no connection
  uses it (`LinkDevice::remove_local` is new). Call C71.
- `cargo test -p pod-netstack --lib stack::tests::listener`: 3 passed.
  `dropped_listeners_free_their_sockets_and_their_ports` (1,000 ports one
  after the other, each listened on two times; the live count stays at
  one listener and goes back to its rest),
  `an_accepted_stream_outlives_its_listener` (on 10.66.0.3, which the
  listener made the stack own; a connect after the drop is refused),
  `dropped_listeners_free_their_addresses` (20 addresses one after the
  other). On the code before `dc0e807` the three fail: `AddrInUse` at
  port 1, a connection to the dropped listener, 8 sockets left at
  10.66.0.10.
- `closed_connections_leave_no_socket` keeps its listener to the end: its
  task dropped the listener, and a dropped listener now frees its sockets.
- The plant `netstack-dropped-listener-stays` is caught.
- Breaking: none in the signatures. A dropped listener frees its port: a
  consumer that dropped a listener and kept its port held now loses it.

# T-231: `pod-netstack`: a dropped or closed stream sends its queued bytes, then the FIN, and frees its socket (PodUtils/podnet#457)

**Source:** PodUtils/podnet#457 (COORD-039 of podnet, for PodUtils/podnet@T-249)
**Category:** defect
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

PodUtils/podnet#457, item 2: "A drop of a `TcpStream` loses the bytes that
wait in its channel. A drop closes the socket at once. The test of a
48 MiB download lost about 1 MB until the router waited for
`StackEvent::Closed` (up to 30 s) before the drop. Ask: a graceful close:
`TcpStream::close()` (or `shutdown(Write)` and a drop) that sends each
queued byte, then the FIN, and frees the socket at the end, with no wait in
the caller."

The drop of a `TcpStream` drops the receiver of the channel to the app.
`move_to_app` (`crates/pod-netstack/src/stack/service.rs`) then sees a
closed channel and calls `tcp::Socket::close` at once. The socket goes to
FIN-WAIT-1, `may_send` is false, and `move_to_socket` moves no more bytes:
the chunks in the channel from the app (at most 64) and the part of a
chunk that the socket did not take are lost. Also, a dropped stream to a
dead peer keeps its socket for ever: no time limit ends it.

## Premise

- Read: `crates/pod-netstack/src/stack/socket.rs` (`Drop` of `TcpStream`
  sends `ConnCmd::Close`); `crates/pod-netstack/src/stack/service.rs`
  (`move_to_app`, `close_and_check`); smoltcp 0.14 `tcp::Socket::close`,
  `may_send` and `set_timeout`. The timeout of smoltcp counts from the
  last packet of the peer, also on an idle connection: a timeout set at
  the drop of a stream that was idle for longer aborts it at once.
- Not measured before the test: the bytes that the drop loses.

## Approach

1. A test that fails: one stack writes 16 MiB and drops the stream at
   once; the peer reads each byte, then the end. The same with `close()`.
2. A reader that has gone does not close the socket at once. The reactor
   marks the stream as dropped, moves each queued byte into the socket,
   discards the bytes that come in, and closes the socket (a FIN) when no
   byte waits to go.
3. `TcpStream::close(self)`: the same close, by name.
4. A bound: a dropped stream that makes no progress (no change of its
   state and of its send queue) for `StackConfig::linger` (default 60 s,
   the FIN-WAIT-2 time of Linux) is aborted (a RST), and its socket is
   freed. A test with a dead peer and a short linger.

## Prove

```sh
cargo test -p pod-netstack --lib stack::tests::close
```

## Done

2026-11-12. `dc0e807`.

- The cause: `move_to_app` closed the socket at once when the reader had
  gone, and smoltcp took no byte to send after it. Now the reactor marks
  the stream, moves each queued byte into the socket, discards the bytes
  that come in, and closes the socket (a FIN) when no byte waits.
  `TcpStream::close(self)` is the same close, by name.
- The bound: `StackConfig::linger` (default 60 s). A dropped stream with
  no change of its state, its send queue and its queued bytes for that
  time gets a RST, and its socket goes. Call C69.
- Fixed in place, with a test each: `abort` sent no RST (the reactor
  removed the socket before smoltcp sent it; now the socket stays until
  then); a RST of the peer read as a clean end (now `ConnectionReset`);
  the end of a connection was lost when the channel to the app was full
  (now it waits for a place after the data).
- `cargo test -p pod-netstack --lib stack::tests::close`: 6 passed.
  `a_dropped_stream_sends_each_queued_byte_then_the_end` and
  `a_closed_stream_sends_each_queued_byte_then_the_end` (16 MiB between
  two stacks over a link, the end at once after the last write),
  `a_dropped_stream_to_a_dead_peer_is_freed_after_the_linger`,
  `a_dropped_stream_whose_peer_never_closes_is_freed_after_the_linger`
  (FIN-WAIT-2; linger 300 ms), `an_abort_sends_a_reset`,
  `a_reset_after_a_full_channel_reads_as_a_reset`. On the code before
  `dc0e807`, the drop lost 4,233,864 of 16,777,216 bytes, and the abort
  sent no RST in 3 s. With the bound planted out, the two tests of the
  linger fail; with the wait for a place planted out, the app read
  255,936 bytes and a clean end.
- The plants `netstack-dropped-stream-closes-at-once`,
  `netstack-dropped-stream-never-lingers` and `netstack-reset-with-no-room`
  are caught.
- Breaking: `StackConfig::linger` is a new field. A RST of the peer reads
  as `ConnectionReset`, not as an end.
- Not measured: podnet's 48 MiB download over the router with a drop of
  the stream and no wait (PodUtils/podnet@T-249).
- PodUtils/podnet#464, ask 1 (2026-11-12, `3de30c7`, `75fe91d`): the abort
  sends its RST on `586fe83` too.
  `an_abort_after_the_handshake_sends_a_reset_on_the_link` (podnet's case
  `guest_wire_abort_after_the_handshake_sends_no_reset`: a router with any
  IP accepts and aborts) counts the RST on the link; the peer reads
  `ConnectionReset`, and its next write fails. The plant
  `netstack-abort-frees-the-socket-at-once` is caught (no RST in 3 s).

# T-232: `pod-netstack`: a burst of SYNs to one port gets no RST: a listener adds a listen socket for each SYN (PodUtils/podnet#457)

**Source:** PodUtils/podnet#457 (COORD-039 of podnet, for PodUtils/podnet@T-249)
**Category:** defect
**Milestone:** M2
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

PodUtils/podnet#457, item 3: "The accept pool answers a SYN with a RST when
each listen socket is busy. A listener has `accept_pool` listen sockets; a
SYN with no free one gets a RST. podnet sets the pool to 8 and lets at most
8 SYNs of one port wait for their handshake (the test
`netns_backend_takes_a_burst_of_connects_to_one_port`: 48 connects at once
to one port; with no limit only 8 of 48 got through). Ask: a pool that
grows on demand, or a SYN that waits (no RST) until a listen socket is
free, or a size of the pool in `StackConfig`."

`StackConfig::accept_pool` exists. A larger pool costs two buffers for
each listen socket of each listener, also with no connection. The need is
that a burst of SYNs to one port gets no RST.

Also: when the accept queue of a listener (`accept_pool * 4` streams) is
full, `accept_inbound` drops an established connection
(`NetStats::dropped`). A burst of 48 connects that one pass of the reactor
completes fills a queue of 32.

## Premise

- Read: `crates/pod-netstack/src/stack/service.rs` (`accept_inbound` puts
  a new listen socket in the pool after it adopts one; a full queue drops
  the connection). smoltcp 0.14: `Interface::poll` takes each inbound
  packet before the pass of egress, and `Interface::poll_ingress_single`
  takes one; `process_tcp` gives a segment to the first socket that
  `accepts` it, and answers a SYN that no socket takes with a RST; a
  socket in LISTEN takes each SYN with no ACK on its port.
- Not measured: the cost of a look at the head of each inbound packet.

## Approach

1. A test: 48 connects at once to one port with the default settings;
   each connects, and each gets its answer.
2. The reactor takes the inbound packets one at a time
   (`poll_ingress_single`) and reads the head of each in the queue: before
   a SYN goes to smoltcp, the listener of its port gets a free listen
   socket. A listener keeps one free listen socket.
3. The bound: the stack holds at most `max_connections` connections, open
   and half-open. A SYN past it waits: the stack drops it, the peer sends
   it again, and `NetStats::dropped` counts it.
4. A full accept queue drops no connection: its socket waits in the
   listener, and `accept` kicks the reactor.
5. `StackConfig::accept_pool` goes: its pool and its limit are gone.
6. T-223 uses the same look at a SYN.

## Prove

```sh
cargo test -p pod-netstack --lib stack::tests::burst
```

## Done

2026-11-12. `dc0e807`.

- The reactor takes the inbound packets one at a time
  (`Interface::poll_ingress_single`, then the passes of egress, as
  `Interface::poll` does) and reads the head of each in the queue
  (`PacketQueue::peek` is new): a SYN that finds no free listen socket of
  its listener gets a new one. A listener keeps one free listen socket.
  The bound is `max_connections`, open and half-open; a SYN past it waits
  (the stack drops it and counts it in `NetStats::dropped`). A full accept
  queue (64 streams) drops no connection: the connection waits in the
  listener, and `accept` kicks the reactor. Call C70.
- `cargo test -p pod-netstack --lib stack::tests::burst`: 4 passed.
  `a_burst_of_connects_to_one_port_gets_no_reset` (48 SYNs to one port in
  one burst with the default settings; each connects and gets its byte
  back), `a_burst_past_the_accept_queue_loses_no_connection` (100, accepted
  after each handshake), `a_syn_past_the_connection_limit_waits`
  (`max_connections` 4: 4 SYNs wait with no RST and connect after the
  first 4 close), `a_syn_sent_again_reaches_its_own_socket` (T-223). On
  the code before `dc0e807` the four fail: client 8 refused (a pool of 8),
  no wait at the limit, the counts of the dropped listener.
- The cost of the look at each packet: `a_large_download_crosses_a_bounded_link`
  (16 MiB, debug build) took 1.13 s to 1.15 s before and 1.12 s to 1.21 s
  after, 3 and 5 runs.
- The plant `netstack-syn-finds-no-listen-socket` is caught.
- Breaking: `StackConfig::accept_pool` is gone. `max_connections` counts
  the half-open connections, also at a connect. `NetStats::dropped` counts
  the SYNs that waited at `max_connections`, no longer the connections that
  a full accept queue dropped.
- PodUtils/podnet#464, ask 2 (2026-11-12, `3de30c7`): the burst tests count
  the RST segments on the link: 0 past the accept queue (100 connects) and
  0 at `max_connections`. podnet's ask for `StackConfig::accept_queue`
  needs no setting: a full queue drops no connection (it waits in the
  listener, bounded by `max_connections`), so the size of the queue decides
  no loss.

# T-234: `pod-relay`: a relay host as an IPv6 address in brackets, and plain `ws://` to a loopback relay on the forward road

**Source:** T-026 (the parity of podnet's relay client).
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podnet's `Relay::new` takes `https://[IPv6]:port` (its issue 273: the host
keeps its port) and `http://` or `ws://` to a relay on the loopback (its
tests). `pod_relay::relay::parse_relay` refuses both: a relay host is a TLS
name with no IPv6 literal (`ipv6_literals_are_targets_and_zones_are_not`
asserts it), and the forward opener (`open::open`) has no plain road; the
feature `plain-ws` serves the reverse road (`reverse::Wire::PlainLoopback`)
and the forward session of the blocking facade only. podnet's tests of its
forward road over a loopback relay (`bytes_go_through_a_local_relay` of
`PodUtils/podnet:src/ws_relay.rs`, `PodUtils/podnet:src/dialers/tests.rs`)
cannot run on the crate in the trial of T-026.

## Premise

- Read: `PodUtils/podnet:src/ws_relay.rs` at `7c5f87e` (`Host::parse`,
  `Host::url`, `authority_host`, the tests `relay_urls` and
  `an_ipv6_relay_host_keeps_its_port`); `crates/pod-relay/src/relay.rs`
  (`parse_relay`); `crates/pod-relay/src/open.rs` (`try_host`);
  `crates/pod-relay/src/reverse/wire.rs`.
- Not measured: a relay at an IPv6 address with a certificate for it; the
  file names of the cache for such a host (`pod_core::files::safe_name`
  maps `:` to `_`).

## Approach

1. `Relay` takes an IPv6 literal from `[ADDR]` or `[ADDR]:PORT` (with
   `https://` or `wss://`), and keeps the bare address; the URL, the `Host`
   header (`pool::host_header`), `token::relay_name` and the paths of the
   cache write it in brackets. A bare IPv6 literal stays refused: an
   address and a port cannot be told apart.
2. Under the feature `plain-ws`, `open::Request` takes a `reverse::Wire`
   (or the forward opener takes `Wire::PlainLoopback`), so that a forward
   session goes to a stand-in relay on the loopback over `ws://`, with the
   checks of `pod_ws::plain` (a host that is not the loopback is refused
   before any connection).
3. podnet's tests `relay_urls`, `an_ipv6_relay_host_keeps_its_port` and
   `bytes_go_through_a_local_relay` come with their names.

## Prove

```sh
cargo test -p pod-relay --all-features
```

## Done

2026-11-12: commits `7e5a270` (the code, the tests, the plants, the README)
and this record (the calls C80 to C82, T-245, T-246).

- `relay::parse_relay` takes `[ADDR]` and `[ADDR]:PORT` with `https://`,
  `wss://` or no scheme, and keeps the address with no brackets in its
  canonical form (RFC 5952); a bare address and a zone stay refused (call
  C80). `Relay::name` and `Relay::url` write it in brackets;
  `token::relay_name`, `pool::host_header` and the `Host` of the HTTP road
  are `Relay::name`; the lines of the opener, the gate, the record of
  failures, a kept offer and the QUIC road write a host in brackets. The
  cache files of `[2001:db8::1]:8443` are `relay-token-_2001_db8__1__8443.json`
  and `relay-pool-2001_db8__1.json`, for each spelling of the address.
- `open::open_on_loopback` (feature `plain-ws`, call C81) is the forward
  opener on plain `ws://` to stand-in relays on the loopback
  (`crates/pod-relay/src/open_plain.rs`): the order of the hosts, the
  failover and the notes of `open`, no other road, and no mint: the token
  of the settings, the run or the cache (`token::kept`, which `obtain`
  uses). A host off the loopback is refused before any connection. With
  the feature, `parse_relay` takes `http://` and `ws://` for a host on the
  loopback (call C82). `open::Request` has no new field: the one literal
  that the podssh trial patch of `pod-relay` adds (`grep -n "Request {"`, in
  podssh-relay's `open`) has no `..`, and stays valid.
- podnet's tests, with their names: `relay_urls` and
  `an_ipv6_relay_host_keeps_its_port` (`crates/pod-relay/src/relay_tests.rs`),
  `bytes_go_through_a_local_relay` (`crates/pod-relay/tests/forward_plain.rs`,
  with a stand-in relay of `pod_ws::server` in place of podnet's
  tungstenite). A host with no scheme stays a relay of TLS (podnet refused
  it). New tests: `an_ipv6_relay_has_one_name_in_brackets`,
  `a_failure_names_an_ipv6_host_in_brackets`,
  `a_relay_off_the_loopback_is_refused_before_any_connection`,
  `the_plain_wire_takes_a_given_token_and_mints_none`. The test
  `ipv6_literals_are_targets_and_zones_are_not` and the properties
  `a_list_keeps_its_order_and_drops_its_duplicates` (IPv6 items in brackets)
  and `any_text_parses_only_to_hosts_that_pass_the_check` take the new rule.
  On the code before, with stubs of `Relay::url`, `url_host` and
  `open_on_loopback` (as `open`), each new test fails: 5 unit tests, the 3
  of `forward_plain.rs`, and `a_list_keeps_its_order_and_drops_its_duplicates`.
- The tests of `relay.rs` moved to `crates/pod-relay/src/relay_tests.rs`
  (the file came near 500 lines). Defect found and repaired: the sweep
  `the_default_relay_is_named_in_exactly_one_place`
  (`crates/pod-relay/tests/default_relay.rs`) knew a test module by
  `mod NAME;` only, so it read `relay_tests.rs` as library code and failed;
  it takes a module that `#[path]` names under `#[cfg(test)]` too.
- Tests: `cargo test -p pod-relay --all-features`: 381 passed, 0 failed, 19
  ignored (195 unit tests; `tests/forward_plain.rs` 3), each test target in
  turn, as the disk of this host held one at a time. clippy with
  `-D warnings`, all targets, with all features and with none: clean.
  `python3 -I scripts/check-size.py`: 0 faults.
- Plants: 6 new (group `relay`, scripts/plants/relay-t234-*), each caught
  on this host by its test with the command of its RUN: `http://` to a host
  off the loopback, a name with no brackets, an address kept as spelled,
  a mint on the plain wire, a plain dial of any host, and a literal of the
  default relay in `relay.rs` (the sweep). relay-t173-foreign-name-taken is
  made again on its line (its context changed), and caught.
  `sh scripts/plant-each.sh --apply`: 0 stale.
- Not measured: a relay at an IPv6 address (a certificate for an address,
  the relay's answer to a `Host` in brackets); a mint or a pair at an IPv6
  relay on port 443, whose `Host` has no brackets (T-245); podnet's own
  tests on the crate, and `PodUtils/podnet:src/dialers/tests.rs` (the trial
  of T-026).
- Found: T-245 (the `Host` of an HTTPS request to an IPv6 address on port
  443), T-246 (two hosts with one name of a cache file).

# T-245: `pod-ws`, `pod-relay`: the `Host` of an HTTPS request to an IPv6 relay on port 443 has no brackets

**Source:** T-234 (found in the work).
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`https_request` of `crates/pod-ws/src/client.rs` took the host alone
for the `Host`: an HTTPS request of
`pod-ws` (`https_post_json`, `https_with_token`) to an IPv6 address on
port 443 sends `Host: 2001:db8::1`, with no brackets (RFC 9110, section
7.2, takes the `host` of RFC 3986: an IPv6 address in brackets).
`keyed::host_header` of `crates/pod-relay/src/keyed.rs` copies the rule, as
the proof of a key signs the `Host` that goes, and the cleartext road
(`crates/pod-relay/src/cleartext.rs`, `mint_once`) sends it. Since T-234 a
relay host can be an IPv6 address: a mint and a pair at `[ADDR]` on port
443 send a `Host` that a strict server refuses. The upgrade of a session
(`pod_ws::client::connect`) writes the `Host` with its port, in brackets.

## Premise

- Read: the files above, and `pod_proxy::host::host_header`, which writes
  the address in brackets.
- Not measured: a relay at an IPv6 address on port 443, and what the relay
  and podssh's relay role answer to `Host: 2001:db8::1`.

## Approach

In one change: `pod_proxy::host::host_header(host, port)` in `https_request`
of `pod-ws` and in `keyed::host_header`. A test of `pod-ws` reads the `Host`
of an HTTPS request to an IPv6 address on port 443: a stand-in proxy on the
loopback answers the `CONNECT`, then serves TLS with a certificate of the
address (as `crates/pod-ws/tests/h2_road.rs` serves TLS). A test of
`keyed::host_header("::1", 443)`. Each fails on the code before.

## Prove

```sh
cargo test -p pod-ws --all-features
cargo test -p pod-relay --all-features
```

## Done

2026-11-12. `30ba101`. `https_request` of `pod-ws` takes its `Host` from
`client::https_host`, which is `pod_proxy::host::host_header`: an IPv6
address in brackets, also on port 443; `keyed::host_header` is the same.
The tests `the_host_of_an_https_request_to_an_ipv6_address_is_in_brackets`
(`pod-ws`, `client::cut_tests`) and
`the_host_of_an_ipv6_relay_on_443_is_in_brackets` (`pod-relay`,
`keyed::tests`); the plants `ws-https-host-no-brackets` and
`relay-t245-key-host-no-brackets` are caught. A unit test of the rule in
place of a stand-in proxy with TLS at an IPv6 address: a runner cannot
bind port 443, and the request reads its `Host` from the one function.
Not measured: a relay at an IPv6 address on port 443.

# T-246: `pod-relay`: two relay hosts can have one name of a cache file

**Source:** T-234 (found in the work).
**Category:** defect
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`pod_core::files::safe_name` (`crates/pod-core/src/files.rs:149`) writes
each character other than `[a-z0-9.-]` as `_`, and `check_host` takes `_`
in a name. Thus a name with `_` and an IPv6 address, or a name with a port,
can give one file name: the host `__1` and the address `::1` share
`relay-pool-__1.json` (`pool::file_name`) and `relay-offer-__1-PORT.json`
(`crates/pod-relay/src/forward_layer/kept.rs`). The token files of
`___1__8443` and `[::1]:8443` share one name too, but `token::usable`
refuses an entry whose minting relay has another key. The cached pool, its
limits or a kept offer of one host can go to the other.

## Premise

- Read: `crates/pod-core/src/files.rs`, `crates/pod-relay/src/pool.rs`,
  `crates/pod-relay/src/forward_layer/kept.rs`, `crates/pod-relay/src/cache.rs`.
- Not measured: a run with two such hosts. A public certificate carries
  no name with `_`.

## Approach

A file name that is one to one with the host: each character outside
`[a-z0-9.-]` as `_` and its two hex digits (`_3a` for `:`, `_5f` for `_`),
in a new function of `pod_core::files`, for the files of the token, the
pool and the offers of `pod-relay`. The old names are not read: one more
mint and one more read of a pool (no compatibility, the owner's rule). A
test: `__1` and `[::1]` give two names of each file.

## Prove

```sh
cargo test -p pod-core
cargo test -p pod-relay --all-features
```

## Done

2026-11-12. `30ba101`. `pod_core::files::exact_name`: each byte outside
`[a-z0-9.-]` as `_` and its two hex digits. The files of the token
(`cache::file_name`), the pool (`pool::file_name`) and the offers
(`forward_layer::kept::file_name`) take it: `__1` is `_5f_5f1`, `::1` is
`_3a_3a1`, `[2001:db8::1]:8443` is `_5b2001_3adb8_3a_3a1_5d_3a8443`. The
old names are not read (one more mint, one more read of a pool). Tests:
`exact_name_is_one_to_one` (`pod-core`), `each_host_and_port_has_a_file_of_its_own`
(`pod-relay`); the plant `core-exact-name-not-one-to-one` is caught.
`cargo test -p pod-relay -p pod-ws -p pod-core -p pod-identity
--all-features` passes.

# T-257: `pod-netstack`: UDP counters for a datagram lost at a slow app, at a full queue or buffer, and over the size that the stack carries (PodUtils/podnet#464)

**Source:** PodUtils/podnet#464 (COORD-044 of podnet, for PodUtils/podnet@T-248)
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

PodUtils/podnet#464, ask 3 (podnet's note
`PodUtils/podnet:docs/research/podnet/guest-wire-router.md`, section 3):
"UDP counters in `NetStats`: a datagram lost at the full channel to the
app, a datagram lost at a full smoltcp send buffer, and a datagram that the
send path cannot carry (over the MTU)." podnet's T-176 measured on v0.26.0:
a burst of 500 datagrams with no reader gives 113 to the app, and
`NetStats::dropped` and `rx_dropped` stay 0, with 500 packets in
`rx_packets`. "Count in the router. The stack gives no signal."

On `586fe83`, each UDP loss of the stack is silent: `service_udp` loses a
datagram at a full channel to the app (T-259) and a datagram that
`send_slice` refuses at a full send buffer; `UdpDatagrams::try_send` drops
a datagram at a full queue to the stack; the gateway service drops a
request over `GATEWAY_DATAGRAM` and an answer at a full send buffer;
smoltcp drops a datagram at a full receive buffer with a trace only.

## Premise

- Read: `crates/pod-netstack/src/stack/service.rs` and
  `crates/pod-netstack/src/stack/datagrams.rs` at `586fe83`; smoltcp 0.14
  `udp::Socket::process` (a full receive buffer drops the datagram with
  `net_trace!` only), `udp::Socket::send_slice` (`BufferFull`).
- Measured on `586fe83`: the tests of T-259 (the app got 64 of 500).
- Not measured: the counts under the load of many sockets.

## Approach

1. Three counters in `NetStats` (call C86): `udp_rx_dropped` (an inbound
   datagram past the bound of its socket, or a request to the gateway
   service over `GATEWAY_DATAGRAM`), `udp_tx_dropped` (a `try_send` at a
   full queue to the stack, or an answer of the gateway service at a full
   send buffer), `udp_oversize` (T-258). `dropped` stays the count of the
   SYNs at `max_connections`.
2. A datagram of an app that the send buffer does not take waits for the
   next pass: a full buffer holds the app, and loses nothing.
3. The reactor takes each datagram out of smoltcp at the packet that brings
   it (T-259), so that smoltcp never drops one with no count.
4. Tests with exact sums: the datagrams that the app got and the count make
   the burst; the sent and the counted make the sends; the answers and the
   count make the requests.

## Prove

```sh
cargo test -p pod-netstack --lib stack::tests::udp
cargo test -p pod-netstack --lib --all-features stack::tests::udp
```

## Done

2026-11-12. `3de30c7`, `75fe91d` (with T-258, T-259).

- `NetStats::udp_rx_dropped`, `udp_tx_dropped`, `udp_oversize` (call C86),
  each in `NetStatsSnapshot`. A datagram of an app that the send buffer of
  smoltcp does not take waits in `UdpConn::pending` and goes first at the
  next pass: `send_to` loses nothing at a full buffer. The gateway service
  counts a request over `GATEWAY_DATAGRAM` (and goes on with the next one;
  it stopped the pass before) and an answer at a full send buffer.
- `cargo test -p pod-netstack --lib stack::tests::udp`: 10 passed, with no
  feature and with `--all-features`. `a_burst_with_no_reader_counts_each_lost_datagram`
  (500 sent; the app got 128, and 372 are counted: the sum is 500),
  `a_full_queue_to_the_stack_counts_each_lost_datagram` (200 `try_send`
  with no pass of the reactor: 64 sent, 136 counted),
  `a_burst_to_the_gateway_counts_each_lost_answer` (100 requests in one
  pass: 64 answers, 36 counted), `the_stack_counts_each_datagram_over_its_limit`.
  The tests do not build on `586fe83` (no counter).
- The plants `netstack-udp-loss-not-counted` and
  `netstack-udp-send-loss-not-counted` are caught.
- Breaking: `NetStats` and `NetStatsSnapshot` have three new fields (a
  literal of `NetStatsSnapshot` needs them or `..Default::default()`).
- Not measured: the counters under the load of many sockets.

# T-258: `pod-netstack`: a UDP send over what the stack carries is `Ok` and lost; the IPv4 fragmentation of smoltcp as a feature (PodUtils/podnet#464)

**Source:** PodUtils/podnet#464 (COORD-044 of podnet, for PodUtils/podnet@T-248)
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

PodUtils/podnet#464, ask 4 (podnet's note
`PodUtils/podnet:docs/research/podnet/guest-wire-router.md`, section 3):
"`UdpSocket::send_to` returns an error (`InvalidInput`) for a datagram over
`mtu - 28` when the stack has no fragmentation, or over `udp_payload`."
podnet measured on v0.26.0: a datagram over `mtu - 28` bytes is lost, up
and down: 1252 bytes pass and 1253 fail at MTU 1280, 1472 pass and 1473
fail at MTU 1500; `send_to` returns `Ok`; the sender counts the datagram as
sent. With the smoltcp features `proto-ipv4-fragmentation`,
`reassembly-buffer-size-65536` and `fragmentation-buffer-size-65536`, the
sizes 1253, 4000 and 65507 pass, up and down, at MTU 1280. The lead asks
for a call: pod-netstack turns the features on, gives them as its own
features, or refuses a large datagram; with a look at the memory of each
stack and at IPv6 (no IPv6 fragmentation in smoltcp: the limit is
`mtu - 48`).

## Premise

- Read: smoltcp 0.14 `InterfaceInner::dispatch_ip` (an IPv4 packet over the
  MTU is dropped with no fragmentation feature; an IPv6 packet over the MTU
  is dropped in each build), `process_ipv4` (with no reassembly, a fragment
  goes on as a whole packet), `iface/fragmentation.rs` (with `alloc`, the
  buffer of a reassembly is a `Vec` that grows; the fragmenter holds an
  array of `FRAGMENTATION_BUFFER_SIZE` bytes in each `Interface`;
  `REASSEMBLY_BUFFER_COUNT` is 1 and the time limit of a reassembly is
  60 s), `build.rs` (two values of one size feature in one build stop the
  build).
- Measured on `586fe83`: `a_datagram_over_what_the_stack_carries_is_refused`
  fails: the send of 1253 bytes at MTU 1280 is `Ok`.
- Not measured: the memory of a stack with the feature in a process of
  podnet; a lossy link with fragments.

## Approach

1. `StackConfig::max_datagram(to)`: IPv6 `mtu - 48`; IPv4 `mtu - 28`, or
   65507 with the feature; each at most `udp_payload`. The same on
   `UdpSocket` and `UdpDatagrams`.
2. `send_to`, `send_to_from` and `try_send` refuse a datagram over it with
   `InvalidInput`, and count it in `udp_oversize` (T-257).
3. The feature `ipv4-fragmentation` of pod-netstack turns on
   `proto-ipv4-fragmentation` and `fragmentation-buffer-size-65536`. Off by
   default (call C85).
4. Inbound: with no reassembly, the reactor drops an IPv4 fragment before
   smoltcp, and counts the first fragment of a datagram to a socket of the
   stack in `udp_oversize`; the same count for an IPv6 fragment.
5. `UdpDatagrams::may_fragment` tells the truth of the build.
6. CI runs the tests of the crate with no feature too.

## Prove

```sh
cargo test -p pod-netstack --lib stack::tests::udp
cargo test -p pod-netstack --lib --all-features stack::tests::udp
```

## Done

2026-11-12. `3de30c7`, `75fe91d` (with T-257, T-259).

- `StackConfig::max_datagram(to)`, `UdpSocket::max_datagram`,
  `UdpDatagrams::max_datagram`: IPv4 `mtu - 28`, IPv6 `mtu - 48`, each at
  most `udp_payload`; IPv4 65507 with the feature `ipv4-fragmentation`
  (call C85; `pod_netstack::IPV4_FRAGMENTATION`). `send_to`,
  `send_to_from` and `try_send` refuse a datagram over it with
  `InvalidInput` and count it in `udp_oversize`. With no reassembly, the
  reactor drops an inbound IPv4 fragment before smoltcp and counts a first
  fragment to a socket of the stack; an IPv6 first fragment is counted.
  `UdpDatagrams::may_fragment` is `IPV4_FRAGMENTATION` (it was true). The
  static probe turns the feature on and sends 4000 bytes through a loopback
  stack at MTU 1280. CI runs `cargo test -p pod-netstack --lib` with no
  feature too.
- `cargo test -p pod-netstack --lib stack::tests::udp`: 10 passed, with no
  feature and with `--all-features`. `a_datagram_over_what_the_stack_carries_is_refused`
  (1252 and 1232 bytes cross at MTU 1280, one more is `InvalidInput`; with
  the feature, 65507 bytes cross in fragments) fails on `586fe83`: the send
  of 1253 bytes is `Ok`. `an_inbound_datagram_in_fragments` (3000 bytes in
  two IPv4 fragments: delivered with the feature, dropped and counted with
  none; an IPv6 first fragment counted), `the_limits_follow_the_mtu_and_the_buffer`.
- The plant `netstack-udp-over-the-limit-goes` is caught.
- Breaking: a UDP send over `max_datagram` is `InvalidInput` (it was `Ok`,
  and the datagram was lost). `UdpDatagrams::may_fragment` is false with no
  feature. With no feature, the stack drops each inbound IPv4 fragment
  (smoltcp took it as a whole packet).
- Not measured: the memory of a stack with the feature in a process of
  podnet; fragments over a lossy link (smoltcp puts together one datagram at
  a time, and a lost fragment holds that place for 60 s).

# T-259: `pod-netstack`: the reactor loses a datagram that comes while the channel to the app is full (PodUtils/podnet#464)

**Source:** PodUtils/podnet#464 (COORD-044 of podnet, for PodUtils/podnet@T-248)
**Category:** defect
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

PodUtils/podnet#464, ask 5 (podnet's note
`PodUtils/podnet:docs/research/podnet/guest-wire-router.md`, section 3):
"`service_udp` does not take a datagram from the smoltcp socket
(`peek_slice`, then `recv_slice`) when the channel to the app is full:
smoltcp then applies its own bound, and no datagram is lost in the
reactor." podnet's guest-wire router (PodUtils/podnet@T-248) waits for it.

On `586fe83`, `service_udp` takes a datagram from smoltcp (`recv_slice`),
then gives it to the channel to the app (`try_send`, 64 datagrams); a full
channel loses it. Also, a read of the app does not kick the reactor: the
datagrams that stay in smoltcp wait for the next tick
(`max_poll_interval`).

## Premise

- Read: `crates/pod-netstack/src/stack/service.rs` at `586fe83`
  (`service_udp`); smoltcp 0.14 `udp::Socket::process` (a full receive
  buffer drops a datagram with a trace only), `PacketBuffer::enqueue`,
  `Interface::poll_ingress_single`.
- Measured on `586fe83`: `a_burst_with_no_reader_keeps_each_datagram_that_fits`
  (500 datagrams over a link, `max_poll_interval` 10 s): the app gets 64,
  the numbers 0 to 63.
- Not measured: a load of many sockets with slow apps.

## Approach

1. A test: 500 datagrams over a link to a socket whose app reads nothing;
   the app then reads the first `64 + udp_packets`, in order, at once.
2. The reactor gives the channel a place first, then the datagram. smoltcp
   counts no datagram that its full receive buffer drops, so the reactor
   takes each datagram out of smoltcp at the packet that brings it, into a
   backlog of the socket with the bound of the buffer of smoltcp
   (`udp_packets`, `udp_payload`): T-257 counts each datagram past it
   (call C84).
3. The next receive of the app after a full channel kicks the reactor.
4. Plants: the backlog lost at a full channel; no kick.

## Prove

```sh
cargo test -p pod-netstack --lib stack::tests::udp
cargo test -p pod-netstack --lib --all-features stack::tests::udp
```

## Done

2026-11-12. `3de30c7`, `75fe91d` (with T-257, T-258).

- The reactor reads the head of each inbound packet (`Head::parse`) and
  takes each datagram out of smoltcp at the packet that brings it, into a
  backlog of its socket: at most `udp_packets` datagrams and `udp_payload`
  bytes. The backlog goes into the channel to the app with a place first.
  The next receive of the app after a full channel kicks the reactor
  (call C84).
- Fixed in place, with a test: a dropped `UdpSocket` or `UdpDatagrams`
  kept its smoltcp socket and its port for the life of the stack. Now its
  drop kicks the reactor, which sends its queued datagrams, then frees both;
  a link that takes nothing for the linger frees them too (call C87).
- `cargo test -p pod-netstack --lib stack::tests::udp`: 10 passed, with no
  feature and with `--all-features`; `cargo test -p pod-netstack --lib`: 61
  passed in each build; 25 runs of the UDP, close, burst and hooks tests in
  each build passed. On `586fe83`:
  `a_burst_with_no_reader_keeps_each_datagram_that_fits` (500 over a link,
  `max_poll_interval` 10 s) fails: the app got 64 (now 128, the numbers 0
  to 127, at once); `a_dropped_udp_socket_sends_its_queue_and_frees_its_port`
  fails: 2 sockets stay.
- The plants `netstack-udp-backlog-lost-at-a-full-channel`,
  `netstack-udp-backlog-waits-for-the-tick` and
  `netstack-dropped-udp-socket-stays` are caught.
- Breaking: a UDP socket whose app reads slowly keeps 64 + `udp_packets`
  datagrams, and counts the rest; a dropped UDP socket frees its port after
  its queue went.
- Not measured: a load of many sockets with slow apps; the cost of the look
  at the head of each packet (one lock of the inbound queue for each
  packet, as T-232 does for a SYN).

# T-261: `pod-wormhole`: a meeting point of the pairing over public MQTT brokers on 443 (PodUtils/podssh#205)

**Source:** PodUtils/podssh#205 (COORD-031 of podssh, for PodUtils/podssh@T-737)
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

The two ends of a code of `pod-wormhole` meet only through the
mailboxes of magic-wormhole (`meet.rs`, `mailbox.rs`). When no mailbox
answers, the pairing fails. PodUtils/podssh@T-737 asks for a second
meeting point: MQTT 3.1.1 over a WebSocket on port 443 (subprotocol
`mqtt`), through a public broker with no account. podssh took
`pod-wormhole` at `v0.26.0`, so the work is podcrates'.

## Premise

- Read: PodUtils/podssh#205. The messages: `CONNECT` (a clean session,
  a random client id, keep-alive 60 s), `SUBSCRIBE`, `PUBLISH` at QoS 0
  with no retained message, `PINGREQ`, `DISCONNECT`. A topic of letters
  and digits comes from the code's nameplate; the waiting end
  subscribes, the other publishes first; the replies use the same topic
  with a tag of direction; a message that reaches no subscriber is sent
  again. The PAKE of the pairing holds off a stranger on the topic.
- The brokers that PodUtils/podssh@T-547 read: the Eclipse sandbox (`/mqtt`;
  it answered 522 and 504 then) and the public instance of shiftr.io
  (64 KB payloads, topics of 128 characters, 100 subscriptions). Not
  read: the WebSocket path of shiftr.io, and the rate limits of each.
- Not measured: either broker from this sandbox.

## Approach

1. A service of `pod-services` for each public broker (host, path,
   public credentials, limits), each read from its source, with a
   `websocket-upgrade` watch (subprotocol `mqtt`).
2. In `pod-wormhole`, a module of the MQTT meeting point over `pod-ws`:
   the codec of the five messages, the topic of a nameplate, the tag of
   direction, the send again of a message with no subscriber, and the
   same messages of the pairing as the mailbox carries. Each source file
   stays under 500 lines.
3. The tests that podssh asks: against Eclipse Mosquitto with a
   `protocol websockets` listener (in CI), two ends pair through a
   topic; a message on the topic of another code is not delivered; a
   planted topic from a constant fails the test. An offline stand-in of
   the broker for `cargo test`.
4. One short live pairing through each public broker in `live.yml`.
5. podssh wires it into its order of the pairing (`PODSSH_MQTT=no`,
   `PODSSH_LAST_RESORT=no`): podssh's glue.

## Prove

```sh
cargo test -p pod-wormhole --all-features
cargo test -p pod-services --test mqtt
cargo test -p pod-services-watch --test watches -- ws::
sh scripts/ci/mosquitto-tests.sh
cargo run -q -p pod-services-watch -- --service shiftr-mqtt
cargo test -p pod-wormhole --test live -- --ignored
```

## Correction

2026-11-12, read and measured from a cloud session, through its HTTPS
proxy. The WebSocket path of shiftr.io: its JavaScript manual dials
`wss://` at `public.cloud.shiftr.io` with no path, so `/`; the upgrade of
`/` and of `/mqtt` answered 101 with `Sec-WebSocket-Protocol: mqtt`, and
two ends paired through `/`. The rate limits of shiftr.io: its error codes
name a limit for each user and for each connection, with no value. The
Eclipse sandbox reset each TLS handshake (three tries, and each watch);
the Wayback Machine's capture of its page (2025-03-15) names port 443 and
the path `/mqtt`; its page of the sandboxes of Eclipse IoT states no limit.

## Done

2026-11-12, partial. `f73cd93` (the code, the tests, the services, the
fuzz target, CI), and the commit of the plants and this record.

- `pod_wormhole::mqtt`: `opened(&Brokers, versions)` and
  `joined(&Brokers, &code, versions, giver_wait)`, the steps of `meet` at
  a list of brokers (`Brokers { points, config, resend }`,
  `Point { url, login }`, `Login`), for a caller whose mailboxes do not
  answer; `mqtt::broker::Broker`, MQTT 3.1.1 over `pod-ws` with the
  subprotocol `mqtt` (call C92); `mqtt::packet`, the codec of the eight
  packets; `mqtt::topic(appid, nameplate)` and a nameplate of 4 digits
  that the giver draws (call C93); `place::Place`, so that a `Wormhole`
  opens at a `Mailbox` or a `Broker`. Each source file has less than 400
  lines.
- `pod-services`: `shiftr-mqtt` and `eclipse-mqtt`, of the kind
  `mqtt-broker`, each value with its source and the date (call C94).
  `pod-services-watch`: `websocket-upgrade` takes the argument `protocol`.
- `cargo test -p pod-wormhole -p pod-services -p pod-services-watch
  --all-features`: each passed. `pod-wormhole`: 33 unit tests (13 new),
  10 of `tests/broker.rs` over the stand-in broker and the Eclipse
  Mosquitto 2.0.18 of this host (two ends pair through a topic; a message
  on the topic of another code is not delivered; a stranger on the topic
  fails the PAKE and spends the code; the taker sends again until the
  giver answers; a refused login, a broker that is not there, a broker
  that breaks the protocol; the bounds of the public brokers; a
  `PINGRESP` that waited answers its ping; a flood of packets does not
  hold the deadline), 11 of
  `tests/faults.rs`, 14 of `tests/mailbox.rs` (Python skipped).
  `pod-services`: 2 new (`tests/mqtt.rs`). `pod-services-watch`: 38, 1
  new.
- `sh scripts/ci/mosquitto-tests.sh`, with a stand-in of `docker` that
  starts the Mosquitto 2.0.18 of this host with
  `images/mosquitto/mosquitto.conf`: passed, and said
  `Eclipse Mosquitto: two ends paired`.
- Live, through the session's proxy: `two_ends_meet_at_each_public_broker`
  paired two ends at shiftr.io; the Eclipse sandbox reset the TLS
  handshake. `two_ends_meet_at_each_public_mailbox` passed at both
  mailboxes. `pod-services-watch -- --service shiftr-mqtt`: 3 hold;
  `--service eclipse-mqtt`: 2 failed (the TLS reset).
- Plants: `wormhole-mqtt-topic-constant`, `wormhole-mqtt-no-resend` and
  `services-upgrade-protocol-dropped` are caught (each run by hand: the
  test FAILED); `services-upgrade-refusal-dropped` is made again for the
  new `ws.rs`, and is caught. `sh scripts/plant-each.sh --apply`: 0 stale.
- Fixed in place: no workflow ran the live test of the public mailboxes
  (`live.yml` runs it now, with the brokers); the live tests take the
  proxy of the environment; `Failed::Unreachable` said "no mailbox" for a
  broker; at `f73cd93` the wait of `Broker::message` took its timer or a
  packet that was there by chance, so a `PINGRESP` that came while the
  caller did not read could end a live connection (the read goes first
  now, and the deadline holds after each packet;
  `a_pingresp_that_waited_answers_its_ping` fails 10 runs of 10 there,
  and passed 50 of 50 with `a_flood_of_packets_does_not_hold_the_deadline`).
  Found: no workflow runs the tests against magic-wormhole's Python server
  and library (T-263).
- Fuzz: the target `mqtt_packet` with 10 seeds. `cargo clippy
  --manifest-path fuzz/Cargo.toml --all-targets -- -D warnings`: clean;
  `cargo test --manifest-path fuzz/Cargo.toml --lib`: 9 passed.
- Checks: `cargo fmt --all --check`, `cargo clippy --workspace
  --all-targets --all-features -- -D warnings`, `cargo todo check`,
  `python3 -I scripts/check-design-paths.py`, `python3 -I
  scripts/check-workflows.py`, `python3 -I scripts/check-size.py`, `sh
  scripts/plant-each.sh --apply`: each exit 0.
- Breaking: `Wormhole::open` takes `impl Into<Place>` (each caller of a
  `Mailbox` compiles unchanged); the words of `Failed::Unreachable`.
- Remains, for the lead: the step `pod-wormhole against Eclipse
  Mosquitto` of `ci.yml` (the pinned image, Mosquitto 2.1.2: this host has
  no docker daemon) and the step of `live.yml`; then `done`.
  podssh wires the meeting point into its order of the pairing
  (PodUtils/podssh@T-737).
- Not measured: the Eclipse sandbox (no answer from here); Mosquitto 2.1;
  a rate limit of either broker; a pairing while other clients load a
  public topic.
- The runs of the lead: `ci.yml` at `1c9b738` (run of the push; the step
  against Eclipse Mosquitto 2.1.2 of `images/mosquitto/` passed); `live.yml`
  run 43172235777 at `cc51bee`: shiftr.io paired two ends through a topic
  from GitHub; `mqtt.eclipseprojects.io:443` timed out after 19 s (here its
  TLS handshake reset). A public broker that answers from neither network
  leaves: `eclipse-mqtt` is gone, and `shiftr-mqtt` is the one broker.
  The tests against magic-wormhole's server ran in the same job (none
  skipped) once `POD_WORMHOLE_TEST_MQTT=required` named the need of
  Mosquitto apart from `POD_WORMHOLE_TEST_SERVER`. `live.yml` run
  43174276510 at `52e461a`, with `shiftr-mqtt` alone: passed.

# T-263: `pod-wormhole`: no workflow runs the tests against magic-wormhole's own mailbox server and Python library

**Source:** T-261 (a finding)
**Category:** defect
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`tests/mailbox.rs` runs the wormhole against magic-wormhole's own mailbox
server and a peer of its Python library where Python has
`magic-wormhole-mailbox-server` and `magic-wormhole`; else the tests skip.
`POD_WORMHOLE_TEST_SERVER=required` turns a skip into a failure. The
harness said that a workflow sets it: no workflow sets it, and no runner
has the packages. So no check of CI proves that this crate and the Python
implementation meet (T-022 ran it by hand).

## Premise

- Read: `.github/workflows/` at `f73cd93`: no step names
  `POD_WORMHOLE_TEST_SERVER` or installs the two packages;
  `crates/pod-wormhole/tests/harness/mod.rs` (the skip and the variable).
- Not measured: the time and the size of the packages on a runner
  (Twisted, Autobahn, PyNaCl, spake2 and their dependencies).

## Approach

1. A file of requirements with each package, its version and its hash,
   for `pip install --require-hashes` in a virtualenv of the job.
2. A step of `ci.yml` that makes the virtualenv and runs
   `cargo test -p pod-wormhole --test mailbox` with
   `POD_WORMHOLE_TEST_PYTHON` and `POD_WORMHOLE_TEST_SERVER=required`.
3. Dependabot (`pip`) keeps the file new.

## Prove

```sh
POD_WORMHOLE_TEST_PYTHON=.venv/bin/python POD_WORMHOLE_TEST_SERVER=required cargo test -p pod-wormhole --test mailbox
python3 -I scripts/check-workflows.py
```

## Done

2026-11-12. T-228 did it (`b3dfc88`): the job `live` of `live.yml`
installs `magic-wormhole-mailbox-server` 0.8.0 and `magic-wormhole` 0.24.0
in a virtualenv and runs the tests of `pod-wormhole` and `pair_code` of
`pod-relay` with `POD_WORMHOLE_TEST_SERVER=required`, each day. Run
43153607165: 14 tests of `tests/mailbox.rs` against the server and the
library, none skipped; 7 of `pair_code`.

# T-277: `pod-relay`: the runs of a code meet at the MQTT brokers after the mailboxes (PodUtils/podssh#210)

**Source:** PodUtils/podssh#210 (COORD-036 of podssh, for PodUtils/podssh@T-737)
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podssh pairs through `pod_relay::code::run::{give, take, give_with,
take_with}`. They meet only through `run::opened` and `run::joined`, which
call `pod_wormhole::opened` and `pod_wormhole::joined` over
`Ends.mailboxes` alone. `run::Ends` has no field for brokers, so podssh
cannot use `pod_wormhole::mqtt` (T-261) from its glue. podssh took
`pod-relay`, so the change is podcrates' (the ruling Q3). podssh's ask:

1. `run::Ends` takes the brokers too (for example `brokers:
   Vec<pod_wormhole::mqtt::Point>`; empty: no MQTT), with the default of
   `Brokers::resend`.
2. `run::opened`: the mailboxes in order; when each failed
   (`Failed::Unreachable`), `mqtt::opened` at the brokers. `run::joined`:
   the same order for the taker, with `waits.giver` for each broker.
3. The shown name of the meeting point stays in `Given::mailbox` and
   `Taken::mailbox` (a `Place::shown()`), so that podssh's lines name the
   broker.
4. One event (a `pod_core::Event`) before the first broker: a third party
   that nobody vouches for carries the messages, and the proof of the
   pairing keeps the secret. podssh shows the events of `Config::events`
   as lines.

## Premise

- Read: `crates/pod-relay/src/code/run.rs` at `a4d3630` (`Ends`,
  `opened`, `joined`); `pod_wormhole::mqtt::{opened, joined, Brokers,
  Point}` (T-261, calls C92 to C95).
- Not measured: a pairing of the runs through a broker.

## Approach

`Ends` gets `brokers` and the resend of `Brokers`. `opened` and `joined`
try the mailboxes, then, on `Failed::Unreachable` alone, send the event
and try `mqtt::opened` or `mqtt::joined` with `Brokers { points, config,
resend }`. The shown name of a broker goes where the mailbox's goes. A
test against Eclipse Mosquitto (`POD_WORMHOLE_TEST_MQTT=required` in
`ci.yml`) pairs two runs with no mailbox that answers, and a test with no
broker keeps `Failed::Unreachable`.

## Prove

```sh
cargo test -p pod-relay --all-features code::
```

## Done

2026-11-12. The commit that names T-277 (the work, the tests, the plants
and call C115).

- `crates/pod-relay/src/code/run.rs`: `Ends` has `brokers:
  Vec<pod_wormhole::mqtt::Point>` and `resend` (`mqtt::RESEND`);
  `Ends::new(mailboxes, config)` gives no broker; `Ends::brokers()` gives
  the `mqtt::Brokers`. `run::opened`: the mailboxes, then on
  `Failed::Unreachable` the brokers. `run::joined`: the mailboxes, then on
  `Failed::Unreachable` or `Failed::Nobody` the brokers, each within
  `waits.giver` (call C115). Before its first broker each end tells the
  event `relay.code.broker` (a warning; `run::BROKER_WORDS`). Where the
  mailboxes and the brokers fail to answer, `Failed::Unreachable` names
  both. `Given::mailbox` and `Taken::mailbox` name the broker
  (`Wormhole::shown`). Breaking: a literal of `Ends` needs the two fields,
  or `Ends::new`.
- Tests (`crates/pod-relay/tests/pair_code.rs`, with the stand-in broker
  of pod-wormhole's harness included in `mailbox_harness`):
  `where_no_mailbox_answers_the_ends_meet_at_a_broker`,
  `a_taker_that_finds_no_giver_at_a_mailbox_looks_at_the_brokers`,
  `with_no_broker_a_run_that_reaches_no_mailbox_fails_as_before`. `cargo
  test -p pod-relay --all-features --test pair_code`: 10 passed. clippy of
  `pod-relay` with all features and with none: clean.
- Plants: `relay-t277-brokers-not-tried` (the giver does not try the
  brokers) and `relay-t277-nobody-stops-the-taker` (the taker stops at
  "nobody"); each is caught by its test.
- Not measured: a pairing through Mosquitto or `shiftr-mqtt` from the
  runs (PodUtils/podssh@T-737 tests them with its glue).

# T-279: `pod-session`: the resume window and the single use of a resume token as settings (PodUtils/podnet#479)

**Source:** PodUtils/podnet#479 (COORD-048 of podnet, ask A2, part 1, for PodUtils/podnet@T-263)
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

podnet's link resumes a session within 10 minutes with a token that works once. `pod-session` holds its resume window and its token rule as constants of the crate; podnet asks for both as settings.

## Premise

- Read: PodUtils/podnet#479 and its ruling "The link workstream" (podnet's
  `docs/decisions.md`, 2026-11-12); the workstream is P3 in podnet.
- Not measured: podnet's use of it; podnet writes no code for it now.
- Read `crates/pod-session/src` for the window of a resume and for the reuse of a resume token before the change.

## Approach

A field of the settings of a session for the window (default the value of today) and one for a token of one use (default the rule of today); tests that a resume after the window fails, and that a second use of a token of one use fails.

## Prove

```sh
cargo test -p pod-session --all-features
```

## Start condition

podnet starts its PodUtils/podnet@T-263.

## Done

2026-11-12. No code: what the ask names holds in the crate (call C119).

- The window: `Settings::resume_deadline` (`resume::RESUME_DEADLINE`,
  600 s); a far end names it in its `GREETING`; `client_deadline` only
  shortens it. After it the far end answers `REFUSE` `UNKNOWN_SESSION`
  (`Class::Unavailable`), and the client says that the far end forgot the
  session and how long it was away (`deadline::forgotten`). Tests:
  `crates/pod-session/tests/session_deadline.rs` (4).
- One use: no bearer token exists; a resume is an HMAC over both fresh
  nonces of the new link, and a replayed proof is refused
  (`a_replayed_proof_is_refused` in
  `crates/pod-session/tests/session_handshake.rs`, 12 tests). `cargo test
  -p pod-session --all-features --test session_deadline --test
  session_handshake`: 16 passed.
- podnet maps `UNKNOWN_SESSION` to its `ResumeExpired`; the token of the
  RESUME record of its record layer is T-301.

# T-290: `pod-relay`: the resumable layer and the end-to-end channel as a feature, so that a plain client builds without them

**Source:** T-026 (a finding of the trial of podnet)
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

`pod-relay` depends on `pod-session` (the resumable layer), `pod-e2e`
(the end-to-end channel, with `snow`) and `pod-identity` with no feature.
podnet's relay client uses the forward road and the plain mode of the
reverse road: no layer and no channel. Its take adds 9 packages to its
lock (`pod-relay`, `pod-session`, `pod-e2e`, `pod-identity`, `snow`,
`chacha20poly1305`, `x25519-dalek`, `wasip2`, `wit-bindgen`) and their
code to its static binary.

## Premise

- Read: `crates/pod-relay/Cargo.toml`; the modules that name `pod_e2e`,
  `pod_session` or `pod_identity`: `forward_layer/client.rs`,
  `reverse/{e2e, layered, legs, spares, control, node, serve, routes, udp,
  close_say}.rs`, `pair/{leg, once}.rs`, `pool/{signed, members, kept}.rs`,
  `keyed.rs`, `cleartext.rs`, `settings.rs`, `grant/`, `code/`,
  `blocking/`.
- Measured in the trial of T-026: the lock of podnet with and without
  the take (9 packages).
- Not measured: the size of podnet's release binary with and without
  them; which of the uses of `pod-identity` (the keys of a signed pool,
  `keyed`) a plain client needs.

## Approach

1. Measure the size of the release binary of podnet with the take, and
   with the code of the layer and the channel cut out by hand.
2. When the gain is worth it: a feature `layer` (the resumable layer, the
   end-to-end channel, the legs of one token, the spares), on in podssh's
   and podvm's dependency, off in podnet's; the node's handlers and the
   control frames of the layer behind it, the plain mode and the forward
   road outside it.
3. CI builds and tests the crate with `pair,plain-ws` alone, and with all
   features.

## Prove

```sh
cargo test -p pod-relay --no-default-features --features pair,plain-ws
cargo test -p pod-relay --all-features
! cargo tree -p pod-relay --no-default-features --features pair -e normal -i snow
```

# T-300: `pod-link`: the stream multiplexer with credit flow control, the carriers and the ARQ, as a shared crate (PodUtils/podnet#479)

**Source:** PodUtils/podnet#479 (COORD-048 of podnet, ask A2, part 2, for PodUtils/podnet@T-263)
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** L
**Status:** partial

## Problem

podnet asks whether podcrates wants a shared crate for the stream multiplexer of its link with credit flow control, the carriers and an ARQ. `pod-session` has no multiplexer and no credits. Until an answer, podnet plans them in a local crate crates/podnet-link.

## Premise

- Read: PodUtils/podnet#479 and its ruling "The link workstream" (podnet's
  `docs/decisions.md`, 2026-11-12); the workstream is P3 in podnet.
- Not measured: podnet's use of it; podnet writes no code for it now.
- `pod-session` holds one stream of a session, its resume and its layer; `pod-relay` and `pod-e2e` take it.
- Not read: podnet's design of the link (`PodUtils/podnet:TODO/link.md` at `060da00`).

## Approach

Call C116: the crate is `pod-link` in podcrates, not `podnet-link`. podcrates writes it when podnet starts the workstream: the frame of a stream (open, data, credit, close, reset), the credit window of each stream and of the link, the ARQ for a carrier that loses frames, and the carriers as a trait over `pod-session`. A fuzz target of the frame, and a test with 3 % of the frames lost.

## Prove

```sh
cargo test -p pod-link --all-features
```

## Start condition

podnet starts its link workstream and names its first carrier (a COORD issue).

## Correction

2026-11-12: read `PodUtils/podnet:TODO/link.md` at `main` (PodUtils/podnet@T-262 to PodUtils/podnet@T-277), PodUtils/podnet#466 and PodUtils/podnet#480. podnet's rules: a window of 16 KiB at the start and 8 MiB at most; a record of 1 MiB before the carrier splits it; the unix carrier with a path of 1 to 107 bytes, the mode 0600, the peer's uid unless `--allow-other-uid`, a regular file refused (64), a live socket in use (69), a stale socket replaced; the memory carrier with a ring of 1 MiB (64 KiB to 64 MiB) that makes the writer wait. The ARQ has its own rules and start in podnet (PodUtils/podnet@T-277): T-310. The crate runs over any byte stream and does not depend on `pod-session` (call C120): T-311 tests the resume.

## Done

2026-11-12, partial: the ARQ (T-310) and the test of a resume over `pod-session` (T-311) remain. Commit `b7aac60`; calls C120 to C124.

- `crates/pod-link`: the frame codec (`frame`), `Link` and `Stream` with the windows of each stream and of the link, the limits of streams and of a frame, the half close, the reset, the `GOAWAY` and the keepalive; the trait `Carrier` with `available()`, the unix carrier and the memory carrier. The README has the API, the wire, the settings, the parity list against PodUtils/podnet#466 and #467, and the adoption map of podnet's entries.
- `cargo test -p pod-link --all-features`: 49 passed (`frames` 6, `carriers` 8, `keepalive` 4, `limits` 13, `streams` 12, `windows` 6); each test binary passed 60 runs in a loop. Clippy of `pod-link` with all features and with none, rustdoc with `-D warnings`, `check-size.py`, `check-design-paths.py` and `check-probe.py` pass.
- The fuzz target `link_frame` with 6 seeds; its body ran on the seeds and on 600,000 mutations with no panic.
- Before the release: the main session reserves the name `pod-link` on crates.io (`check-crates-io-names.py` reads it from `docs/design.md`; crates.io answered 404 for it on 2026-11-12).

# T-301: `pod-e2e`: the handshake hash and the transport keys as an interface (PodUtils/podnet#479)

**Source:** PodUtils/podnet#479 (COORD-048 of podnet, ask A3, for PodUtils/podnet@T-298)
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

A record layer of several hops in podnet's link takes its keys from the end-to-end handshake of `pod-e2e`, not from the TLS exporter of one hop. podnet asks for the handshake hash and the transport keys as an interface.

## Premise

- Read: PodUtils/podnet#479 and its ruling "The link workstream" (podnet's
  `docs/decisions.md`, 2026-11-12); the workstream is P3 in podnet.
- Not measured: podnet's use of it; podnet writes no code for it now.
- Read `crates/pod-e2e/src` (the Noise handshake of snow) before the change: which values snow exposes after the handshake, and which the crate keeps.

## Approach

A method of the finished handshake that gives the handshake hash, and an exporter that derives a key for a label from it (no raw transport key leaves the crate); tests that both ends derive the same key, and another label another key.

## Prove

```sh
cargo test -p pod-e2e --all-features
```

## Start condition

podnet starts its PodUtils/podnet@T-298.

# T-303: `pod-relay`: research: the relay contract and an exchange of NAT traversal candidates between two peers (PodUtils/podnet#479)

**Source:** PodUtils/podnet#479 (COORD-048 of podnet, ask A5, for PodUtils/podnet@T-284)
**Category:** chore
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

podnet asks whether the relay contract can carry the exchange of NAT traversal candidates (addresses and ports) between two peers, so that they try a direct path before the relay carries their bytes.

## Premise

- Read: PodUtils/podnet#479 and its ruling "The link workstream" (podnet's
  `docs/decisions.md`, 2026-11-12); the workstream is P3 in podnet.
- Not measured: podnet's use of it; podnet writes no code for it now.
- The contract is `https://tcp.ssh.relay.ajam.dev/llms-full.txt` (watched daily, T-253). Not read for this question.

## Approach

Read the contract: a message of a pair or of a session that carries opaque data between the two ends before the stream. Write the answer in this entry (yes with the message, or no with what the contract needs), and the answer to podnet.

## Prove

```sh
grep -n -i 'candidate\|signal\|offer' docs/research/*.md
```

## Done

2026-11-12. The answer to ask A5 of PodUtils/podnet#479, from a read of
`https://tcp.ssh.relay.ajam.dev/llms-full.txt` (18359 bytes, the bytes of
the fact `contract-sha256` of `ajam-relay`). No code.

- Yes, with no change of the contract: two peers of a pair meet in a
  reverse session (the operator opens `/v1/connect/NAME`, the node answers
  `ready`), and the relay carries the bytes of a session verbatim in both
  directions ("The operator leg carries no framing at all"). The node can
  give such a session to a service of its process (`reverse::channel`,
  T-150), which reads and writes the candidates in podnet's own form,
  inside the layer of `pod-e2e`, so that the relay does not read them.
- The contract has no message for data between the two ends outside a
  session: the controls of a node are `hello`, `open`, `close`, `ready`
  and `reject`, and the operator sends binary data alone.
- The contract gives no reflexive address: `/trace` names the colo, the
  city, the country and the ASN of the client, and `?egress=1` the
  address of the relay's egress. The peers learn their public addresses
  from STUN (the services of the kind `stun` in `pod-services`).
- The relay carries TCP only, and takes no inbound TCP: a direct path is
  the peers' own (for example a UDP hole punch with the candidates), and
  the relay stays the carrier when no direct path opens. A session needs
  a node online and its `ready` within 15 s; it lasts at most 720 min and
  64 MiB.

# T-310: `pod-link`: a selective-repeat ARQ for a carrier that loses frames, with a test at 3 % loss (PodUtils/podnet#480)

**Source:** T-300 (the ARQ part of its Approach); PodUtils/podnet#468, row E2-2 (PodUtils/podnet@T-277)
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** L
**Status:** open

## Problem

T-300 asks for an ARQ for a carrier that loses frames, and a test with 3 % of the frames lost. The first step of `pod-link` has the reliable carriers only (the unix socket and the memory pair). podnet's rules for the ARQ (row E2-2 of PodUtils/podnet#468):

- A frame carries a 32-bit sequence, a 16-bit fragment index and a fragment count.
- SACK is a 32-byte bitmap (256 bits) for a window of 256 frames. Retransmission is for each fragment.
- The RTO floor is 200 ms and the ceiling is 8 s. There are 8 retries.
- A stream that exhausts its retries closes with `arq-timeout`, not the session, unless the control channel is also lost.
- A 32-bit sequence wraps after 2^32 frames. The wrap forces a rekey and never a silent reuse.

## Premise

- Read: `crates/pod-link/src/link.rs` (the link runs over one ordered byte stream) and `crates/pod-link/src/carrier.rs` (`Facts` says whether a carrier is `reliable` and its unit, `mtu`).
- Read: `PodUtils/podnet:TODO/link.md` (T-276, T-277): the first lossy carrier is UDP with KCP; the rekey is the job of podnet's record layer (PodUtils/podnet@T-298, which needs T-301 here).
- Not measured: the loss and the order of a real lossy carrier.

## Approach

1. A sans-IO layer between a datagram carrier and the link: it turns datagrams (the unit of `Facts::mtu`) into an ordered byte stream, so that `Link` stays as it is. The sequence, the fragment index and count, the SACK bitmap of 256 bits, the RTO with its floor, ceiling and retries as settings.
2. A trait of a datagram carrier beside `Carrier`, and a lossy shim of the memory carrier for the tests (a loss rate and a reorder rate from a seed).
3. Tests: 8 MiB over 3 % loss and 1 % reorder, whole and in order; the fragment count; the end of the link after the retries; a start sequence near 2^32 asks the caller for a rekey and never reuses a sequence.
4. The close of one stream on `arq-timeout` needs the ARQ to know the streams: write the choice (a stream-aware ARQ, or the end of the link) as a call.

## Prove

```sh
cargo test -p pod-link --all-features
```

## Start condition

podnet starts PodUtils/podnet@T-277 or names a lossy carrier (a COORD issue).

# T-311: `pod-link`: a test of a link over a resumable session of `pod-session` across the loss of its carrier (PodUtils/podnet#480)

**Source:** T-300 (call C120); PodUtils/podnet#466, row E0-2 (PodUtils/podnet@T-263: "1 MiB resume digest equal")
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** M
**Status:** done

## Problem

Call C120 puts the resume of a link in `pod-session`: the link runs over the application side of a session. No test proves that composition: the carrier of a session is lost while streams of the link move, the session resumes on a new carrier, and each stream goes on with no loss. podnet's row E0-2 is done when 1 MiB crosses a resume with an equal digest.

## Premise

- Read: `crates/pod-session/src/resume.rs` (the client's session across links takes an `app` byte stream) and `crates/pod-session/src/far.rs` (`serve`, the far end).
- Read: `crates/pod-link/src/link.rs`: `Link::new` takes any byte stream.
- Not measured: the time of a resume under the keepalive of a link (10 s by default): a resume that takes longer than `dead_after` ends the link.

## Approach

1. A test in `crates/pod-link/tests/` with `pod-session` as a dev-dependency: a `tokio::io::duplex` pair at each end, one side to the session's `app`, the other to `Link::new`.
2. Drop the session's link (its carrier) while 1 MiB moves on two streams; give a new link; compare the SHA-256 of each stream.
3. Write in `crates/pod-link/README.md` how a consumer composes the two, and the bound between `dead_after` and the resume deadline of `pod-session`.

## Prove

```sh
cargo test -p pod-link --all-features
```

## Start condition

T-279 is done (the resume window and the single use of a token as settings), or podnet starts PodUtils/podnet@T-263.

## Done

2026-11-12. `crates/pod-link/tests/resume.rs`
(`two_streams_cross_two_resumes_of_the_session_whole`): a session of
`pod-session` over a carrier that a stand-in cuts twice with no word
(after 700 000 and 1 300 000 bytes), two streams of a link over the
application side move 1 MiB each way, and the SHA-256 of each stream is
equal at both ends; two `Note::Lost` and two `Note::Resumed`; the link
lives across each resume. 20 runs passed. `pod-session` and `sha2` are
dev-dependencies of `pod-link`. `crates/pod-link/README.md`, "A link that
survives the loss of its carrier": the composition and the bound between
`dead_after` and `resume_deadline`.

# T-327: `pod-relay`: podnet's `relay connect` hangs on a runner with a direct egress after its take (PodUtils/podnet#502)

**Source:** PodUtils/podnet#502 (COORD-052), PodUtils/podnet@T-315.
**Category:** defect
**Milestone:** M5
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

In podnet's `live.yml` (run 43203358263, at podnet `59c0313`, after its
take of `pod-relay`), `podnet relay node NAME --to 127.0.0.1:22` connects,
then `ssh -o ProxyCommand="podnet relay connect NAME"` waits about 2
minutes and gets "Connection closed by UNKNOWN port 65535". The step
passed at podnet `64f1ed0`, before the take. In podnet's sandbox (an HTTP
proxy) the same three commands pass against the live relay. The runner
has a direct egress and open UDP; podnet gives no `h2` and no `quic`
setting.

## Premise

- Read: podnet's operator glue (the file pod-relay.patch of `adoption/podnet/` at
  `v0.30.0`) calls `reverse::operator::run` with `relay.wire()`;
  `reverse::wire::open` with `Wire::Tls` dials through `pod_ws`.
- Measured: `live.yml` 43221128869 (`reverse_live`, 4 passed) and
  43221537246 (5 passed, with `the_node_speaks_first_as_sshd_does`, a
  server-first session as SSH through `ProxyCommand`), on runners with a
  direct egress: the crate's node and operator open and carry 1 MiB each
  way through the live relay.
- Read: podnet's answers on PodUtils/podnet#502 (live 43220567832): a
  pipe that writes before it reads gets sshd's banner and KEXINIT; the
  `ssh` after it fails as before. podnet's glue gives
  `tokio::io::join(tokio::io::stdin(), tokio::io::stdout())` and
  `eof_idle`; the way is direct.
- Read: `reverse::operator::carry` writes each frame of the node with
  `write_all` and flushes only at the end. `tokio::io::stdout()` wraps the
  line buffer of the process's stdout: sshd's banner ends in a newline,
  its binary KEXINIT does not, so it stays in the buffer; `ssh` waits for
  it, and sshd ends the session after `LoginGraceTime` (120 s). In the
  pipe test the exit of the process flushed it. The live tests of the
  crate use a duplex, which holds nothing.

## Approach

1. Run `reverse_live` of `live.yml` (the node and the operator of the
   crate through the live relay, from a runner with a direct egress).
2. Compare the dial of the operator with and without a proxy: the road
   (HTTP/1.1, HTTP/2, QUIC), the upgrade, and the first frames.
3. A test over a stand-in that reproduces the hang; the fix; a plant.

## Prove

```sh
cargo test -p pod-relay --all-features
```

`live.yml` with `relay = reverse_live`; podnet's `live.yml` at its next
tag.

## Done

2026-11-12. `reverse/operator.rs`: `write_now` writes each frame of the
node to the local side and flushes it. `reverse/route.rs`: the node's
writer to a session's stream flushes after each frame too (a handler's
stream can have a buffer). Test `the_node_bytes_reach_a_buffered_local_side_at_once`
(`tests/reverse_operator.rs`: a local side that holds each write until a
flush; the old code timed out); plant `relay-t327-operator-no-flush`.
The live test `the_node_speaks_first_as_sshd_does` stays. podnet proves
the fix with its `live.yml` at the next tag.

# T-354: `pod-wormhole`, `pod-relay`: magic-wormhole's mailbox server and transit relay from a hashed requirements file, on each push

**Source:** `docs/live-calls.md` (row L6); `live.yml` run 43164673537.
**Category:** chore
**Milestone:** M5
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The tests of `pod-wormhole` and of `pair_code` of `pod-relay` against
magic-wormhole's own mailbox server and library run only in the daily
`live.yml`. They reach no service: the server runs on the loopback. Only
its install reaches PyPI (pip with pinned versions and no hashes). A
defect of the pairing waits for the next day, not the push. The step also
runs the broker tests of `pod-wormhole` with no Mosquitto: they print
"did not run: no Mosquitto here" (run 43172235777), and in run
43164673537 they failed on `POD_WORMHOLE_TEST_SERVER=required`. The
transit relay of T-027 needs a server of the same kind.

## Premise

- Read: `.github/workflows/live.yml` (the steps of the virtualenv and of
  the pairing by a code), `crates/pod-relay/tests/mailbox_harness/mod.rs`,
  `crates/pod-wormhole/tests/harness/mod.rs`.
- Read (2026-11-12): https://pypi.org/pypi/magic-wormhole-mailbox-server/json
  (0.8.0, MIT, wheel SHA-256
  `e03f6a60284c6174baf4d94ee880f90071c1d01afe7f65380fa5f20ff0f01ad4`),
  .../magic-wormhole-transit-relay/json (0.5.0, MIT, wheel SHA-256
  `e2a378a84e6bdb3e317d1e0dfaf02e9f4a92e30aa5686c5e4ef27209c3ac0b57`),
  .../magic-wormhole/json (0.24.0, MIT, wheel SHA-256
  `312d7d55cba76fe443fd207b1e006d12b7c56699961ed6dce46bed765945e712`).
  No official image of the servers exists (search of 2026-11-12).
- Measured: the logs of `live.yml` runs 43164673537 and 43172235777.
- Not measured: the time of the install with the cache of pip.

## Approach

1. A requirements file of the test servers with the hash of each package
   and of each dependency (made with `pip-compile --generate-hashes`),
   installed with `pip install --require-hashes`, with the cache of pip in
   actions/cache keyed by the hash of the file (call C152).
2. scripts/ci/mailbox-tests.sh: the step of the job `test` of `ci.yml`
   with `POD_WORMHOLE_TEST_SERVER=required`; it runs the mailbox tests of
   `pod-wormhole` and the target `pair_code` of `pod-relay`, not the
   broker tests (Mosquitto has its own step, C95). The two steps leave
   `live.yml`.
3. magic-wormhole-transit-relay 0.5.0 is in the same file, for the tests
   of `pod-transit` (T-027).

## Prove

```sh
sh scripts/ci/mailbox-tests.sh
gh workflow run ci.yml
```

## Done

2026-11-12. `scripts/ci/wormhole-servers.txt`: the mailbox server 0.8.0,
the transit relay 0.5.0 and the library 0.24.0 with each dependency and
its SHA-256 (33 packages, `uv pip compile` for Python 3.12 on
x86_64-unknown-linux-gnu; the three top wheels have the hashes of the
Premise). `scripts/ci/mailbox-tests.sh`: a virtualenv, `pip install
--require-hashes`, then the targets `mailbox` and `faults` of
`pod-wormhole` and `pair_code` of `pod-relay` with
`POD_WORMHOLE_TEST_SERVER=required`. A step of the job `test` of
`ci.yml`; the two steps leave `live.yml`. The run here: 11, 14 and 10
passed, no skip. The cache of pip (actions/cache) is T-355's.

# T-390: `pod-identity`: a limit of entries for `Allowlist`, and a line in the display form (PodUtils/podnet#512 A11)

**Source:** PodUtils/podnet#512 (COORD-056), ask A11; PodUtils/podnet@T-261.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podnet keeps a wrapper around `Allowlist` for two things: a limit of the
entries of a file (4096 in podnet), and a line that names a key in the
display form of T-278 (`WQC4LSJV-...`), not as its OpenSSH fingerprint or
iroh's form.

## Premise

- Read: `crates/pod-identity/src/allow.rs` (`Allowlist::parse`,
  `KeyName::parse`) and `crates/pod-identity/src/display.rs`.

## Approach

`Allowlist::parse_with(text, limit)` (the lines past the limit are named
as bad lines, so a node does not grow without a bound); `parse` keeps no
limit. `KeyName` reads the display form (five groups of eight characters
with its checksum) and matches it to a key by `DisplayFingerprint::matches`.
Tests for each.

## Prove

```sh
cargo test -p pod-identity --all-features
```

## Done

2026-11-12. `Allowlist::parse_with(text, limit)` and `read_with(path,
limit)` (`parse` and `read` keep no limit): a line of a new key past the
limit is a bad line; a key already listed still adds its routes.
`KeyName::Display(DisplayFingerprint)`: `KeyName::parse` reads the display
form after a fingerprint and a key (the checksum must match; case is
free) and matches it by `DisplayFingerprint::matches`; its `fingerprint()`
is the display form. Test `an_allowlist_reads_the_display_form_and_holds_a_limit`.

# T-391: `pod-session`: a typed cause in `End::GaveUp` (PodUtils/podnet#512 A12)

**Source:** PodUtils/podnet#512 (COORD-056), ask A12; PodUtils/podnet@T-263.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`End::GaveUp(String)` holds the reason as text alone. podnet maps the end
of a resume to its `ResumeExpired` by the words of the text ("no such
session", "no new link within"), which break when a text changes.

## Premise

- Read: `crates/pod-session/src/pump.rs` (`End`) and
  `crates/pod-session/src/resume.rs` (the places that give up).

## Approach

`End::GaveUp(GaveUp)` with `GaveUp { cause: GiveUp, reason: String }`;
`GiveUp` names each cause: the far end does not know the session
(`REFUSE` `UNKNOWN_SESSION`), the window ended with no new link, the
transport said not to come back, and the far end refused for another
reason. Breaking for a caller that matches `GaveUp(text)`. Tests for each
cause.

## Prove

```sh
cargo test -p pod-session --all-features
```

## Done

2026-11-12. `pod_session::End::GaveUp(GaveUp)`, `GaveUp { cause: GiveUp,
reason: String }` (Display: the reason), `GiveUp::{UnknownSession,
WindowEnded, Stopped, Refused}`: `Failed::Forgotten` gives `UnknownSession`,
`Failed::Final` `Refused`, `Next::Stop` `Stopped`, the timeout and the
backoff past the window `WindowEnded`. The four tests of a give-up assert
the cause; a new test `a_stop_of_the_caller_ends_the_session_with_its_cause`.
No other crate of the workspace matches `GaveUp`. Breaking: a caller that
matched `GaveUp(text)` reads `.reason`, and podnet maps `.cause` to its
`ResumeExpired`.

# T-392: `pod-session`: the single use of the resume secret as a setting (PodUtils/podnet#512 A13)

**Source:** PodUtils/podnet#512 (COORD-056), ask A13; T-279 (call C119).
**Category:** research
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

podnet asks for the single use of the resume secret as a value of
`Settings`.

## Premise

- Read: call C119 (T-279): the layer has no resume token to spend; each
  resume proves the secret with an HMAC over both fresh nonces of the new
  link, so a proof works once, on its own link, and a replayed one is
  refused (`BAD_PROOF`).

## Approach

No setting: a proof of one use is the protocol, not a choice. The answer
on the COORD issue names C119 again; `Settings::resumes = false` ends a
session with its link, for a caller that wants no resume at all.

## Prove

```sh
cargo test -p pod-session --all-features
```

## Done

2026-11-12. No code: C119 stands. The answer is on the COORD issue of
podcrates to podnet.

# T-417: `pod-wormhole`: a board of retained messages over `mqtt::Point`, for PodUtils/podssh@T-738 (PodUtils/podssh#220)

**Source:** PodUtils/podssh#220 (COORD-045), ask 2.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

PodUtils/podssh@T-738 keeps the URL of each piece as a sealed note on a
board. A caller that holds one connection at a time needs to put a
retained message at a topic, take the retained message of one topic, and
clear a topic. `pod_wormhole::mqtt` pairs two ends over a broker, and has
no board of retained messages.

## Premise

- Read: PodUtils/podssh#220; `crates/pod-wormhole/src/mqtt*`; the test
  of the crate against Eclipse Mosquitto (`scripts/ci/mosquitto-tests.sh`,
  T-261).
- Not measured: whether shiftr.io's public instance clears a retained
  message on an empty retained `PUBLISH` (its docs say that it keeps
  retained messages in memory).

## Approach

`mqtt::Board` over `mqtt::Point`, each call on a connection of its own:
`put(topic, payload)` (`PUBLISH` with RETAIN, QoS 0), `take(topic, wait)`
(`SUBSCRIBE`, the first `PUBLISH` within the wait, `DISCONNECT`; none
within the wait is `Ok(None)`), `clear(topic)` (an empty retained
`PUBLISH`). A topic is letters and digits, at most 42 characters; a
payload is under 64 KB; others are refused before a connection. Tests
against the stand-in of the crate and against Mosquitto in
`scripts/ci/mosquitto-tests.sh` (put, take, clear, take after a clear);
one short live run against shiftr.io in `live.yml`.

## Prove

```sh
cargo test -p pod-wormhole --all-features
sh scripts/ci/mosquitto-tests.sh
```

## Done

2026-11-13, commit e65b766. `pod_wormhole::mqtt::Board { point, config }`
(`mqtt::board`): `put(topic, payload)` (a retained `PUBLISH` at QoS 0),
`take(topic, wait)` (`SUBSCRIBE`, the first non-empty `PUBLISH` of the
topic within the wait, else `Ok(None)`), `clear(topic)` (an empty retained
`PUBLISH`), each on a connection of its own with `DISCONNECT` at the end; a
`PINGREQ` and its `PINGRESP` after each `PUBLISH`. `check_topic` (1 to
`TOPIC_MOST` = 42 letters and digits) and `check_payload` (1 to 64,000
bytes) refuse with `Failed::Mine` before a connection (C221).
`Packet::Publish` carries `retain` (breaking). The stand-in broker keeps
retained messages and records the flag of each publish.

`cargo test -p pod-wormhole --all-features`: 74 passed (`tests/board.rs` 4:
put, take, a second put, clear and a take after the clear; a note put
while a take waits; the bounds refused before a dial; a refused login).
`sh scripts/ci/mosquitto-tests.sh` with dockerd of this sandbox: passed,
with the lines `Eclipse Mosquitto: a board put, took and cleared a note`
and `Eclipse Mosquitto: two ends paired through the topic of a code`
(the script now checks both). The plants
`wormhole-board-clear-not-retained` and `wormhole-board-clear-not-empty`
fail `a_board_puts_takes_and_clears_a_note` on the stand-in and on
Mosquitto, checked by hand. `tests/board_live.rs` (`#[ignore]`) runs in the
new step of `live.yml` each Thursday: whether shiftr.io clears a retained
message is its first run (`docs/live-calls.md`, L38).

# T-420: `pod-link`: the WebSocket carrier with its server half (PodUtils/podnet#515)

**Source:** PodUtils/podnet#515 (COORD-059), podnet's next carriers.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

podnet starts its stream carriers of the link in an order, each a take of
`pod-link` (the ruling of #88). Its PodUtils/podnet@T-269 needs the WebSocket carrier, its server half (podnet uses `pod-ws`). `pod-link` has
the `tcp` carrier with the pin of an Ed25519 key (T-396) and the unix and
memory carriers, and not this one.

## Premise

- Read: PodUtils/podnet#515; `crates/pod-link/src/tcp.rs` and
  `tcp/listen.rs` (the form of a carrier with a pin, T-396);
  `docs/decisions.md`, C175 to C179.
- Not measured: none here.

## Approach

`ws::Ws` behind a feature `ws`: the dial of `wss://HOST/PATH` through the caller's way out with `pod-ws`, the accept half on a listener of the caller (the HTTP upgrade), each side with the pin of an Ed25519 key as `tcp` has it (`pod_tls::peer`), the frames of the link in binary messages; loopback tests as `tests/tcp.rs` has them (a refused key on each side, a silent peer).

## Prove

```sh
cargo test -p pod-link --all-features
```

# T-421: `pod-link`: the HTTP/2 carrier, one bidirectional POST (PodUtils/podnet#515)

**Source:** PodUtils/podnet#515 (COORD-059), podnet's next carriers.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

podnet starts its stream carriers of the link in an order, each a take of
`pod-link` (the ruling of #88). Its PodUtils/podnet@T-270 needs the HTTP/2 carrier, one bidirectional POST. `pod-link` has
the `tcp` carrier with the pin of an Ed25519 key (T-396) and the unix and
memory carriers, and not this one.

## Premise

- Read: PodUtils/podnet#515; `crates/pod-link/src/tcp.rs` and
  `tcp/listen.rs` (the form of a carrier with a pin, T-396);
  `docs/decisions.md`, C175 to C179.
- Not measured: none here.

## Approach

`h2::H2` behind a feature `h2`: one POST with a request body and a response body that stay open, over TLS 1.3 with the pin of `tcp`, through the caller's way out; the accept half; loopback tests, and a test through an HTTP/2 proxy stand-in that buffers no body.

## Prove

```sh
cargo test -p pod-link --all-features
```

# T-422: `pod-link`: the HTTP/1.1 long-poll carrier, and the SSE and chunked-stream carrier (PodUtils/podnet#515)

**Source:** PodUtils/podnet#515 (COORD-059), podnet's next carriers.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** L
**Status:** open

## Problem

podnet starts its stream carriers of the link in an order, each a take of
`pod-link` (the ruling of #88). Its PodUtils/podnet@T-271, PodUtils/podnet@T-272 needs the HTTP/1.1 long-poll carrier, and the SSE and chunked-stream carrier. `pod-link` has
the `tcp` carrier with the pin of an Ed25519 key (T-396) and the unix and
memory carriers, and not this one.

## Premise

- Read: PodUtils/podnet#515; `crates/pod-link/src/tcp.rs` and
  `tcp/listen.rs` (the form of a carrier with a pin, T-396);
  `docs/decisions.md`, C175 to C179.
- Not measured: none here.

## Approach

`poll::Poll` and `sse::Sse` behind features: the down path as a chunked or `text/event-stream` answer, the up path as POSTs with a sequence number; each side with the pin of `tcp`; the order of the frames kept across POSTs; loopback tests through a stand-in proxy that closes idle connections.

## Prove

```sh
cargo test -p pod-link --all-features
```

# T-423: `pod-link`: the QUIC carrier (PodUtils/podnet#515)

**Source:** PodUtils/podnet#515 (COORD-059), podnet's next carriers.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

podnet starts its stream carriers of the link in an order, each a take of
`pod-link` (the ruling of #88). Its PodUtils/podnet@T-273 needs the QUIC carrier (podnet uses `pod-quic`). `pod-link` has
the `tcp` carrier with the pin of an Ed25519 key (T-396) and the unix and
memory carriers, and not this one.

## Premise

- Read: PodUtils/podnet#515; `crates/pod-link/src/tcp.rs` and
  `tcp/listen.rs` (the form of a carrier with a pin, T-396);
  `docs/decisions.md`, C175 to C179.
- Not measured: none here.

## Approach

`quic::Quic` behind a feature `quic`: one bidirectional stream of a `pod-quic` connection over the caller's `Datagrams`, the TLS of QUIC with the pin of `tcp`; the accept half; loopback tests.

## Prove

```sh
cargo test -p pod-link --all-features
```

# T-424: `pod-link`: the UDP carrier with KCP (PodUtils/podnet#515)

**Source:** PodUtils/podnet#515 (COORD-059), podnet's next carriers.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** L
**Status:** open

## Problem

podnet starts its stream carriers of the link in an order, each a take of
`pod-link` (the ruling of #88). Its PodUtils/podnet@T-276 needs the UDP carrier with KCP (the selective-repeat ARQ of a lossy carrier is T-310). `pod-link` has
the `tcp` carrier with the pin of an Ed25519 key (T-396) and the unix and
memory carriers, and not this one.

## Premise

- Read: PodUtils/podnet#515; `crates/pod-link/src/tcp.rs` and
  `tcp/listen.rs` (the form of a carrier with a pin, T-396);
  `docs/decisions.md`, C175 to C179.
- Not measured: none here.

## Approach

`udp::Udp` behind a feature `udp`: datagrams over the caller's `Datagrams`, KCP (or the ARQ of T-310) for the order and the loss, a handshake that proves the pinned key; loopback tests at 0 % and 3 % loss.

## Prove

```sh
cargo test -p pod-link --all-features
```

# T-442: `pod-link`: a first stream whose frames go before the data of the others, and a change of the windows on a live link (GitHub #88)

**Source:** GitHub #88 (podswarm P-4).
**Category:** feature
**Milestone:** M8
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

podswarm runs each road on `pod-link`. Its control stream (`hello`, the
heartbeats, the fence, the signals) must not wait behind a large file on
a slow road, and its view serves three classes of streams in order. It
computes the windows from the bandwidth-delay product of the road, which
changes in a session, so the caps must change on the live link.

## Premise

- Read (2026-11-13): the comment of podswarm on GitHub #88; podswarm
  spec, file 21, sections 6.9 and 7 (ask P-4, milestone M1); file 04,
  section 7.2; file 06; file 26 (the unknown "whether `pod-link` can
  change a window on a live link").
- In podcrates now: `Settings` (`crates/pod-link/src/settings.rs`) is
  fixed at `Link::new`; `crates/pod-link/src/link.rs` has `new`, `open`,
  `accept`, `close`, `rtt` and no priority of a stream and no change of
  the settings. The writer bounds its queue (`queue_limit` in
  `crates/pod-link/src/state.rs`).
- The workstream of the crate is T-300 (partial); T-310 and T-420 to
  T-424 are podnet's next parts. This ask is new.

## Approach

`Link::open_first()`: the writer sends the frames of that stream before
the `DATA` of each other stream; a class number for each stream (lower
first, equal classes in turn) when it costs no more.
`Link::set_windows(stream_window, link_window)`: the ranges of `Settings::check`; a new cap
takes effect for the credit that this end grants next; a refused value
keeps the old values. The wire does not change.

## Prove

```sh
cargo test -p pod-link --all-features
```

The tests (the same `Link` and streams throughout): with `frame_size` 4
KiB, a carrier shaped to 1 Mbit/s and a bulk stream with data, at most 4
KiB of bulk `DATA` and the headers pass between the write of 64 bytes on
the first stream and its frame; after `set_windows` from 1 MiB to 64 KiB
the unread bytes of an unread stream stay at most 64 KiB once the old
credit is used; after a raise the stream reaches the new window; a value
that `Settings::check` refuses gives an error.

## Start condition

The owner answers Q15.

# T-476: `pod-relay`: the invariants 6 to 10 of the HTTP road, client and server, with a test of lost acknowledgements (PodUtils/podssh#221)

**Source:** PodUtils/podssh#221 (COORD-046; from PodUtils/podnet#438, the owner's review of 2026-11-11, beside the five invariants of PodUtils/podssh#55).
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

PodUtils/podssh@T-319 takes `pod_relay::http_road`, so the protocol of the HTTP
road is podcrates' work. Five invariants of the review are not in the
record: (6) a stored `seq` received again is answered with the same
acknowledgement, never dropped in silence; (7) an acknowledgement goes
only after the chunk is stored, and a full window is a refusal; (8) a
refusal is a frame with its reason (`full`, `unknown session`, `not
allowed`); (9) the close of a direction carries its count, and a
mismatch fails the session; (10) a retry log on disk is keyed by
(session, `seq`).

## Premise

- Read: PodUtils/podssh#221 (2026-11-13); `crates/pod-relay/src/http_road/`
  and the server of the role that podssh hosts.
- Not measured: how the code answers a `seq` received again today.

## Approach

Each invariant in the client and in the server of `http_road`, each with
its test. The test of the ask: a stand-in that drops every third
acknowledgement ends a 20 MiB session with equal digests, with at most
one resend for each lost acknowledgement; with (6) removed it fails (a
plant). podssh's tests `http_road` and `http_road_quick` run again at its
take.

## Prove

```sh
cargo test -p pod-relay --all-features
```

## Done

2026-11-13, commit 1fb1a7e. podcrates holds the client of the road and,
for the role's server, `http_road::window` (`Up`, `Refusal`), which
podssh's `serve::http_road` takes now and `pod-relay-serve` (T-036) takes
with the role (C230). Each invariant, in the client and in the window:
(6) a change: a batch stored before gets `Stored::Again`, the same `204`
with `X-Seq`; the client fails an acknowledgement that names no batch or
another. Before, podssh's role answered a duplicate `204` already, and
the client took any `204`. (7) a test of what held: the window stores
before it acknowledges and refuses `full` with no move; the client's
offset down names only bytes that the caller's pipe holds. (8) a change:
`X-Refuse` names `full`, `unknown session` or `not allowed`; the client
sends the same batch again after `full` and stops at once for the other
two. (9) a change: the last batch carries `total=BYTES`, and a total that
differs ends the session `1002 count mismatch`; the `410` of `1000`
carries `X-Total`, and the client fails a `1000` with no count or another
count. (10) a test only: each batch is keyed by (session, `seq`), and a
resend carries the key and the body of its first send; the road keeps no
retry log on disk (C232). A defect, fixed in place: in the split form,
the bytes down went on after the bytes up ended the session, so the
caller's stream did not end. `tests/http_road_acks.rs` (6 tests) runs
the client against a stand-in role in memory over the window, on a
paused clock: with each third acknowledgement dropped after the batch is
stored, 20 MiB each way end with equal digests in each form, and with
one resend at most for each lost acknowledgement. The plant
`relay-t476-duplicate-not-acknowledged` removes (6) from the window; the
test fails (the session ends `1002 count mismatch`). The change of the
wire (C231): `X-Seq`, `X-Refuse`, `total=` and `X-Total`; the role's
server must send them at its take of this version, else each session of
the road fails. The README of `pod-relay` lists the ten invariants and
the change. `cargo test -p pod-relay --all-features` passes.

# T-477: `pod-relay`: the serial form of the HTTP road deadlocks when the bytes down block the bytes up (PodUtils/podssh#222)

**Source:** PodUtils/podssh#222 (COORD-047), the CLI test of PodUtils/podssh@T-319.
**Category:** defect
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** open

## Problem

`http_road::serial::run` carries both directions in one task. It writes
the bytes of each answer to the app with `to_app.write_all(..).await`.
When the app does not read them at once, that write blocks, and the task
reads no more bytes up. An SSH client sends its window adjustments up
before it reads more, so an `ssh` session over the serial form logs in
and then moves nothing (podssh: 20 MiB each way, no progress in 240 s;
the split form takes 23 s).

## Premise

- Read: PodUtils/podssh#222 (2026-11-13), with podssh's tested change
  (a task of its own writes the bytes down, by an unbounded channel; the
  serial `ssh` test passes in 25 s); `crates/pod-relay/src/http_road/serial.rs`.
- Not measured: the memory that the bytes down take when the app is slow.

## Approach

A task of its own writes the bytes down to the app, by a bounded channel:
when it is full, the session sends no new exchange (it holds the bytes
down that it took), and it never stops reading the app's bytes up. The
end of the session drops the channel and waits for the writer. A test in
`pod-relay`: a client whose writer waits for its reader, as SSH's does,
ends a session of 20 MiB each way over the serial form; a plant that
writes down in the same task fails it.

## Prove

```sh
cargo test -p pod-relay --all-features
```
