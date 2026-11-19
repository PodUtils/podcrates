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
**Status:** done

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
cargo test -p pod-relay --all-features --test wire_vectors -- --nocapture
python3 -I scripts/check-wire-vectors.py
python3 -I scripts/check-wire-vectors.py --plant
sh scripts/plant-each.sh wire-pong-fields-swapped
```

## Start condition

T-020 and T-025 are done.

## Done

2026-11-17, commits 6f0b31f89 (the import) and c121a2c8e (the adaptation),
and the commit of this section. podssh wrote the page first
(PodUtils/podssh#304, podssh T-723): one page, `docs/wire.md` (wire version
1), holds the layer, the forward road, `/v1/mint`, the reverse dialect, the
close codes and the grants, in place of one document for each crate. The
import takes it with its 117 vectors at podssh `04f141b`.

- `docs/wire.md`: the page; only the paths change (podcrates' paths, and
  `PodUtils/podssh:PATH` for podssh's documents). The digest of version 1
  holds.
- `crates/pod-relay/tests/vectors/` (6 files, 117 vectors), beside the
  test that reads them. `crates/pod-relay/SOURCES` records each imported
  file with its blob at `04f141b`: the vectors, the test, the page and the
  three scripts.
- `crates/pod-relay/tests/wire_vectors.rs`, `crates/pod-relay/tests/wire/`:
  the vectors through `pod-session`, `pod-relay` and `pod-ws`. 109 vectors
  run; 8 are skipped by kind with the reason, and the test prints the count
  (`117 in all, 109 run, 8 skipped`): the `mint` (3) and `check` (5)
  vectors of `tokens.json` need the relay role's tokens, which podcrates
  does not hold (T-036). T-856 runs them at that import. A kind that
  `wire/kinds.rs` does not know fails the test.
- `scripts/check-wire-vectors.py` (with `scripts/wire_check_parts.py` and
  `scripts/wire_ed25519.py`): the second reader, from the page alone; it
  reads all 117 vectors and the digest. `ci.yml` (the lint job) and
  `scripts/agent/check.sh` run it and its `--plant`.
- The plant `scripts/plants/wire-pong-fields-swapped.plant`: the PONG record of
  `pod-session` writes its offset before its value; the test of the records
  fails on the vector `pong`.

Results: `cargo test -p pod-relay --all-features --test wire_vectors`: 14
passed. `cargo test -p pod-session --all-features`: 161 passed.
`check-wire-vectors.py`: 117 vectors of 6 files, ok; `--plant`: a change
found in each of the 6 files. `sh scripts/plant-each.sh
wire-pong-fields-swapped`: caught; `--apply`: 0 stale. clippy of
`pod-relay`, `pod-session` and `pod-ws` (all targets, all features): no
warning. No source file of `pod-relay` changed, so its other test targets
did not run here; the workspace run before the push runs them.

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
**Status:** open

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

## Start condition

The roads of the relays in the set have their clients: `pod-relay` (the
owner's relay), `pod-gsr`, and the open entries of M4 for the others
(T-031 DERP, T-032 Piping, T-033 OpenTunnel). The first part works with
the roads that exist.

## Correction

2026-11-13, the owner's ruling "Open questions": the Problem names one
relay (the owner's) and its pool of hosts, so a failover only moves among
the hosts of one operator, and that relay stays a single point of
failure. The catalog of `pod-services` holds the relays of many
operators for a reverse road: `ajam-relay`, `croc`, `n0-iroh-relay`,
`ajam-ts-relay`, the DERP servers (`tailscale-derp`, `tailcat-derp`), the
SSH tunnels (`serveo`, `sandhole`, `pinggy`, `localhost-run`, `ssh-j`,
`tuns-sh`), `bore`, `localtunnel`, `trycloudflare`, `microsoft-devtunnel`,
`hackerschoice-gsr`, `qsocket-qsrn`, `gosuda-portal`, the Piping servers,
the mailboxes of magic-wormhole and the TCP relays of Tox and ZeroTier.
The new Approach: a node of the reverse road holds its place on the
relays of several operators at once (a set that the caller ranks, from
the catalog), and the dialer tries them in rank order; the node is named
by its key on each one (the pairing of each road, or a record under the
key, as `n0-pkarr` gives), so a lost relay costs a reconnect on the next
one, and no relay of one operator is needed. Measure first: the time of a
failover from a lost relay to the next, with two stand-ins of the test.

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
The source of the transfer layer when this entry starts: podnet's
src/transfer/ at its commit `d5c38c1` (the transfer messages, the transit
handshake and records, the hints, the zip writer and reader, the receive
rules of PodUtils/podnet#436); podnet then takes the crate and keeps fixes
only (PodUtils/podnet#535, ask 5). `Wormhole::derive_key` (T-545) gives
the transit key, `pod_wormhole::standin` (T-546) the mailbox of the tests,
and the service `magic-wormhole-transit` (T-548) the TCP transit relay.

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

`pod_core::files::safe_name` (`crates/pod-core/src/files.rs:150`) writes
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
**Status:** done

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

2026-11-13. The rest is done: `v0.32.0` released the crate (#93), the
resume over `pod-session` is T-311 (done), the tcp carrier is T-396
(done, `v0.34.0`), and podnet took `v0.34.0` at `ef8c901`
(PodUtils/podnet#515) with its PodUtils/podnet@T-268 on the tcp carrier.
crates.io answers `pod-link` at `0.0.0` with the repository of podcrates
(2026-11-13; `python3 -I scripts/check-crates-io-names.py`). The next
parts have entries of their own: the carriers T-420 to T-424, the ARQ
T-310, and the first stream of podswarm T-442.

# T-301: `pod-e2e`: the handshake hash and an exporter, and the record layer (PodUtils/podnet#479)

**Source:** PodUtils/podnet#479 (COORD-048 of podnet, ask A3, for PodUtils/podnet@T-298);
PodUtils/podnet#521 (COORD-064 of podnet)
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

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

## Correction

2026-11-13, call C246: PodUtils/podnet#521 names PodUtils/podnet@T-298
the AEAD record layer with a negotiated suite and a rekey, and
PodUtils/podnet@T-299 to PodUtils/podnet@T-301 build on it. As no raw
transport key leaves the crate (the answer #88), the record layer goes
into `pod-e2e` with the exporter: a record of a suite that the two ends
agree on in the handshake, a rekey by count and by time, padding of a
record to buckets (PodUtils/podnet@T-300), and a replay window with the
single-use RESUME record (PodUtils/podnet@T-301). The compression
(PodUtils/podnet@T-299) stays podnet's: it runs before the seal.

The wire of PodUtils/podnet#472 (the rows E6-1, E6-3, E6-4) applies: the
header of 24 bytes (`version` u8, `type` u8, `flags` u16, `stream` u32,
`counter` u64, `length` u32, `pad` u16, `reserved` u16) as the associated
data, the tag of 16 bytes, the suites `chacha20-poly1305` (the default)
and `aes-256-gcm`, a nonce of a 4-byte prefix of each direction and the
counter, a rekey at 2^31 records or 1 GiB, the buckets 0, 128, 512, 1024
and 2048, a bitmap of 1024 counters, and a RESUME with the token, the next
counter and a fresh prefix, valid 10 minutes, used once, and three
failures that end the link. Calls C255 to C258 give the rest.

## Done

2026-11-13, commit 0aee08a (the exporter, the suites and the records of
`crates/pod-e2e/src/exporter.rs`, `crates/pod-e2e/src/record/mod.rs` and
`Channel::agree`), and the commit after it (the seven plants).
`cargo test -p pod-e2e --all-features`: 77 passed (the lib 38, with 15
new; `tests/record.rs` 4, new: the same key at both ends, another key for
another label, the initiator's first suite and no suite in common, records
both ways over the stream of a real handshake, a document of the net
before the suites). The plants `e2e-record-window-takes-a-repeat`,
`e2e-exporter-of-the-hash-alone`, `e2e-record-resume-used-twice`,
`e2e-record-pad-outside-the-aad`, `e2e-record-phase-unflipped`,
`e2e-record-resume-never-late` and `e2e-agree-own-preference` each make
their test fail (each run by hand); `sh scripts/plant-each.sh --apply`: 0
stale. Not proven: a fuzz run of the opener (T-510); a carrier that loses
or moves records (the tests give each record by hand).

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
**Status:** done

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

## Done

2026-11-13. Commit `dcdbe01`; calls C262 to C265.

- `crates/pod-link/src/arq.rs` and `crates/pod-link/src/arq/`: the wire (`DATA` with the sequence, the fragment index and the count; `FIN`; `ACK` with the next frame, the window and a bitmap of 256 frames; `PROBE`), `Arq` (the state machine with no I/O), `ArqStream` and `ArqHandle` (the task over a datagram carrier, the byte stream of a `Link`), `ArqSettings` with podnet's values (256 frames, fragments of 1 KiB, an RTO of 200 ms to 8 s, 8 retries), `ArqError::Timeout` (`arq-timeout`). `crates/pod-link/src/datagram.rs`: the trait `Datagrams`, the memory pair and the seeded shim `Lossy`.
- Approach 4: the ARQ is under the link, so `arq-timeout` ends the link (call C262); the close of one stream is T-514. The congestion control is T-515, a fuzz target of the datagrams T-516.
- `cargo test -p pod-link --all-features`: 98 passed, with `arq_machine` 11 (the fragment count, the window, the peer's window, each lost frame again, the timer and its 8 retries, the RTO floor and ceiling, the probe of a closed window, the wrap with and without a rekey, the rekey ahead, datagrams of a bad form, a duplicate and the `FIN`) and `arq` 7 (8 MiB and 1 MiB at the same time at 3 % loss, 1 % reorder and 1 % duplicates with an equal SHA-256; a link of two streams of 4 MiB at the same faults; a silent peer ends with `arq-timeout` after 36.6 s; a link over a silent ARQ ends; the bytes wait at the wrap until the rekey; the shutdown and the `FIN`; the answer after a drop). The binary `arq` passed 200 runs in a loop.
- Eight plants (`link-arq-*`, group `link`): each was applied by hand, and its check failed with its text; `sh scripts/plant-each.sh --apply`: 0 stale.
- Clippy of `pod-link` with all features and with none, rustdoc with `-D warnings`, `check-size.py`, `check-design-paths.py` and `check-probe.py` pass. The tests of the ARQ use the memory pair alone, with no Unix item.

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
**Status:** done

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

## Done

2026-11-13, commit 3ee0a13: `ws::Ws` (feature `ws`) and `ws::WsListener`
on the accept loop of `pod-ws` (T-439); the parts of a pinned carrier in
`crates/pod-link/src/pin.rs`; call C253. `cargo test -p pod-link
--all-features`: 0 failed; `tests/ws.rs` 8 passed (a link with 3 streams
of 256 KiB, the keys of each side, a refused key on each side, a silent
peer, a silent listener, the address forms, the way out). The plants
`link-ws-server-key-unchecked`, `link-ws-client-key-unchecked`,
`link-ws-first-frame-skipped`, `link-ws-refusal-unnamed` and
`link-ws-handshake-unbounded` are caught.

# T-421: `pod-link`: the HTTP/2 carrier, one bidirectional POST (PodUtils/podnet#515)

**Source:** PodUtils/podnet#515 (COORD-059), podnet's next carriers.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

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

## Done

2026-11-13, commit efa3159: `h2::H2` (feature `h2`), `h2::H2Listener`
and `h2::PostUrl`; call C254. The stand-in of the Approach is a proxy of
the way out (a CONNECT tunnel of `pod-proxy`): a front that ends the TLS
would end the pin. `cargo test -p pod-link --all-features`: 0 failed;
`tests/h2.rs` 9 passed (a link with 3 streams of 256 KiB, a dial through
the proxy with 50 lines each answered while the POST is open, a refused
key on each side, a GET refused with 405, a silent peer, a silent
listener, the address forms, the way out); `cargo deny check bans
sources`: ok. The plants `link-h2-server-key-unchecked`,
`link-h2-client-key-unchecked`, `link-h2-settings-unread`,
`link-h2-request-ended` and `link-h2-handshake-inline` are caught.

# T-422: `pod-link`: the HTTP/1.1 long-poll carrier, and the SSE and chunked-stream carrier (PodUtils/podnet#515)

**Source:** PodUtils/podnet#515 (COORD-059), podnet's next carriers.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** L
**Status:** done

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

## Done

2026-11-13, commit bbc7cfd (with T-495). The feature `poll` holds one
carrier type, `poll::Poll`, whose form of the bytes down is a setting
(`Form::LongPoll` `poll`, `Form::Sse` `sse`, `Form::Chunked` `chunked`),
in place of the two types of the Approach (call C266); one listener,
`poll::PollListener`, serves each form. podnet's rules of
PodUtils/podnet@T-271 and PodUtils/podnet@T-272: the lanes `POST /up` and
`POST /down?since=N`, a hold of 1 s to 60 s (25 s), a batch of 256 KiB,
429 with Retry-After up to the third in a row, events of SSE in base64 of
8 KiB with a `: ping`, one POST up in flight, a raw chunked body; a
listener with no push answers 406 and the dialer takes the long poll
(podnet's `server_push=false`, call C269). The bytes are exact across a
lost answer and a cut by offsets, with no ARQ (call C267); the values
where podnet gives none are call C268. `Facts` gets `cost` (call C269).
`cargo test -p pod-link --all-features`: 0 failed; `tests/poll.rs` 12
passed (a link of 3 streams in each form through a way out that loses the
answer of a POST and cuts an answer down, a proxy stand-in that cuts each
idle connection, a refused key on each side, a silent peer, a silent
listener, the address forms and the settings, the names and the costs),
`tests/poll_lanes.rs` 6 passed (the key of a session, the end of a session
with no request on each side, the hold and the backoff, the long poll in
place of a refused push, 3 MiB of a push with no bytes up, 429), 10 runs
of both: 0 failed; `cargo deny check bans sources`: ok. The plants
`link-poll-duplicate-taken`, `link-poll-resend-dropped`,
`link-poll-refusal-unnamed`, `link-poll-session-key-unchecked`,
`link-poll-ack-unsent`, `link-poll-no-expiry`,
`link-poll-first-busy-fatal`, `link-poll-hold-unheld`,
`link-poll-handshake-inline` and `link-poll-push-refusal-fatal` are
caught. The findings are T-517 (a cap of the bytes of the sessions) and
T-518 (a fuzz target of the reader of HTTP/1.1).

# T-423: `pod-link`: the QUIC carrier (PodUtils/podnet#515)

**Source:** PodUtils/podnet#515 (COORD-059), podnet's next carriers.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

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

## Done

2026-11-13, commit 6a5a7d8. `quic::Quic` (feature `quic`, call C270):
one bidirectional stream of a connection of `pod-quic`, with the TLS of
`tcp` and the pin of each key. A dial sends from a UDP socket of the
system or over the datagrams of the caller's way out (`with_way`,
`pod_core::Dial::datagrams`); a name resolves through the caller's
resolver (`with_resolver`) or the system. A listener binds `IP:PORT` or
runs over the caller's datagrams (`bind_over`). The ALPN is the setting
`Quic::alpn` (`podlink/1` by default; podnet gives `podnet/1`). The
listener opens the one stream and writes the byte 1 on it, so a refused
key is an error of the dial (77). A drop waits for the acknowledgement of
each byte (at most `linger`, 10 s) before the close. `cargo test -p
pod-link --all-features --test quic`: 11 passed; `cargo test -p pod-link
--all-features`: 115 passed. Plants, each caught by hand:
`link-quic-refusal-unnamed`, `link-quic-handshake-inline`,
`link-quic-drop-closes-at-once`, `link-quic-way-unused`. The static probe
links to itself over QUIC. The tcp carrier now reads its address with the
shared `pin::key_at`, and a build of `tcp` alone is clean under clippy.

# T-424: `pod-link`: the UDP carrier with KCP (PodUtils/podnet#515)

**Source:** PodUtils/podnet#515 (COORD-059), podnet's next carriers.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** L
**Status:** done

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

## Done

2026-11-13, commit cd5acb5. `udp::Udp` (feature `udp`, calls C271 and
C272): the wire of KCP (the crate `kcp` 0.6.0, pure Rust, in stream mode)
over UDP or the caller's datagrams, with the TLS of `tcp` above it (the
pin of each key, the byte of acceptance). podnet's rules name KCP (a
window of 32 to 1024, its congestion window, `--kcp-nodelay`), so the
carrier does not run the ARQ of T-310. `udp::KcpStream` drives KCP over
any `datagram::Datagrams`; `Udp::connect_over` and `Udp::accept_over`
run the handshake over the datagrams of one peer (the `Lossy` shim). A
listener starts a session at a first segment alone, at most
`max_handshakes` at once. `cargo test -p pod-link --all-features --test
udp`: 10 passed; `--test udp_loss`: 5 passed (1 MiB each way at 0 % and
at 3 % loss with 1 % reorder and 1 % duplicates, 8 MiB each way at 5 %
loss and 1 % reorder with `nodelay`, a silent peer at the idle limit, a
datagram that breaks KCP); `cargo test -p pod-link --all-features`: 131
passed; 10 runs of the new targets in a loop passed. Plants, each caught
by hand: `link-udp-handshake-inline`,
`link-udp-stranger-starts-a-session`, `link-udp-drop-stops-at-once`,
`link-udp-idle-unchecked`, `link-udp-panic-hangs-the-stream`,
`link-tcp-alpn-unset`. The ALPN of `tcp` is now the setting `Tcp::alpn`
(call C273). Not held: podnet's run at 20 % loss within 3 times the clean
time (T-520, a question for podnet); a fuzz target of the input of KCP
is T-521.

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
- Measured by the study of zmux (`docs/research/zmux.md`, section 2;
  2026-11-16, one host, the memory carrier, the default settings): a
  round trip of 64 bytes takes p50 18 to 26 us and p99 49 to 66 us on an
  idle link, and p50 18 to 23 us and p99 46 to 62 us while a bulk stream
  of 128 MiB runs on the same link. On a fast carrier the bounded queue
  keeps the wait of a small write low; the wait of up to 64 KiB of bulk
  `DATA` shows only on a slow carrier, as the test of this entry has it.
- The workstream of the crate is T-300 (partial); T-310 and T-420 to
  T-424 are podnet's next parts. This ask is new.
- The owner's ruling "podswarm" (2026-11-13, `docs/decisions.md`; call
  C245): this ask helps podswarm alone: podnet asked for no priority and
  no change of the windows. It stays in M8 and waits until the podswarm
  session starts; an ask of podnet moves it to item 3.

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

The podswarm session starts, or a sibling asks for it (the owner's ruling
"podswarm", 2026-11-13).

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
**Status:** done

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

## Done

2026-11-13, commit c19f2ba. `http_road::serial::run` writes the bytes
down from a task of its own (`write_down`), by a channel of `HELD` = 4
answers, each `down_max` bytes at most: 4 MiB at the role's largest bound
(C233). The run task reserves a place in the channel before each
exchange. While the channel is full, no exchange goes, and each batch up
goes alone by `up` on the same connection, with no bytes down (C234), so
the client never stops reading the caller's bytes up. The offset of the
next exchange names the bytes that the channel holds; the end of the
session drops the channel and waits for the writer, so the caller reads
the end after the last byte (C235). The role's server of podssh takes
`up` and `x` in one session (`serve::http_road`). `tests/http_road_flow.rs`
(3 tests, a stand-in role in memory, a paused clock): a caller as SSH's
(after each 256 KiB down, a window adjustment up on the stream that its
20 MiB up share, before it reads on) ends a session of 20 MiB each way
with equal digests in the serial form and in the split form; a caller
that reads nothing gets each of its 4 MiB up to the target, by `up`, and
the client acknowledges 5.25 MiB down at most. The plant
`relay-t477-serial-writes-down` puts back the write in the same task: the
two tests of the serial form fail (no end in 900 s; no byte up read). The
6 tests of T-476 (`tests/http_road_acks.rs`) pass. The README of
`pod-relay` says the flow. A finding goes to T-478 (the client never
sends `close`). `cargo test -p pod-relay --all-features` passes.

# T-478: `pod-relay`: the client of the HTTP road never sends `close`, so a caller that drops its stream leaves the session at the role

**Source:** T-477 (a finding in `crates/pod-relay/src/http_road/`).
**Category:** defect
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

The wire has `POST /v1/http/s/ID/close`, which ends the session at once,
but `http_road::client` never sends it. When the caller drops its stream
before the end, the serial form returns at the next exchange with no
request to the role, and the split form's task down stops with no request
either (its task up sends the last batch). The role then holds the
session, its target connection and its windows until its idle limit or
the target's end.

## Premise

- Read: `crates/pod-relay/src/http_road/serial.rs`, `split.rs`,
  `client.rs` (2026-11-13); `mod.rs`, the wire of `close`.
- Not measured: how long podssh's role holds such a session.

## Approach

When the caller is gone and the session has no end, the client sends one
`POST close` with the session's key, within `LIMIT`, and sets the end
`Closed { code: 1000, reason: "client closed" }` or a failure that says
the caller went. Test against the stand-in role of `tests/`: a caller
that drops its stream in the middle of a session of each form makes the
stand-in record the close.

## Prove

```sh
cargo test -p pod-relay --all-features
```

## Done

2026-11-14, commit `19b4673`. When the caller drops its stream before
the session's end, the split form's task down (at the failed write of
the bytes down) and the serial form (at the closed channel to its
writer) send one `POST /v1/http/s/ID/close` with the session's key,
within `LIMIT` (20 s), and set the end `Closed { code: 1000, reason:
"client closed" }`. The end comes before the request, so that a `410`
that the task up gets after the close is no failure. `Shared::path`
gives no `?` with no query. The stand-in role counts each close and wakes
a waiter. `tests/http_road_gone.rs`, 2 passed, on a paused clock: a
caller drops its stream after 512 KiB of 4 MiB down, in each form; the
stand-in records one close within `LIMIT`, and no second one in 120 s.
Both tests fail on the code before the fix ("no close came within 20s").
The plants, each proved by hand: `relay-t478-serial-no-close` and
`relay-t478-split-no-close` (no request when the caller is gone; the test
of the form fails). `cargo test -p pod-relay --all-features`: 428
passed, no failure. podssh: no change of the role. Its `serve::http_road`
already serves `close` (`answers::close`: the end `1000 client closed`,
`204`). podssh's client sends the close once it takes the tag with this
commit.

# T-482: `pod-netstack`: a stream dropped before its handshake ends finishes the handshake later

**Source:** T-413 (the trial of podssh: ZeroTier's member on the stack).
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

A drop of a `TcpStream` is a graceful close (`ConnCmd::Close`). A caller
that gives up on a connect (a time limit on `established()`) and drops the
stream leaves the SYN in the stack: when the SYN-ACK comes later, the
handshake ends, and the stack then sends the FIN. The far end accepts a
connection that ends at once, before the one that the caller opens next.
podssh's ZeroTier road connects in short tries of 4 s: in the trial, the
first try ran out, the second one opened, and the far end read the end of
the first one ("early eof"); podssh's glue now aborts a try that runs out.
The stack of podssh before it closed a socket in `SynSent` with no
handshake (smoltcp's `close` in that state).

## Premise

- Measured: the trial of T-413, `cargo test -p podssh-zerotier --lib`
  (`member::tests::a_tcp_connection_over_the_ad_hoc_network_carries_64_kib_each_way`):
  failed with each try dropped, passed with `TcpStream::abort` on the try
  that ran out.
- Read: `crates/pod-netstack/src/stack/socket.rs` (`Drop for TcpStream`).

## Approach

The drop of a stream that is not established (no `Connected` event, no
data) is an abort: the reactor closes the socket in `SynSent` with no FIN,
and a SYN-ACK that comes later gets a RST. A test: a connect to a far end
that answers late, the stream dropped before the answer; the far end
accepts no connection.

## Prove

```sh
cargo test -p pod-netstack
```

# T-506: `pod-link`: the ws carrier with the rules of podnet's WebSocket carrier: a subprotocol, a text message as a protocol error, a Ping each 20 s with a Pong within 10 s (PodUtils/podnet#515)

**Source:** PodUtils/podnet#515 (COORD-059) with PodUtils/podnet@T-269 (row E1-2 of PodUtils/podnet#467); found by T-420.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

PodUtils/podnet@T-269 gives the rules of its WebSocket carrier (row E1-2
of PodUtils/podnet#467): "Binary frames only. The subprotocol is
`podnet/1`. Ping every 20 s, pong within 10 s." and "A text frame is a
protocol error." The ws carrier of T-420 offers no subprotocol, gives the
payload of a text message to the link as bytes (`pod_ws::bridge` writes
each data message), and sends no Ping of WebSocket: the link has its own
`PING`.

## Premise

- Read (2026-11-13): PodUtils/podnet:TODO/link.md, PodUtils/podnet@T-269
  and PodUtils/podnet@T-270, through the mirror of the GitHub API.
  PodUtils/podnet@T-270 names TLS 1.3 and "check the peer fingerprint in
  the TLS handshake", which the h2 carrier of T-421 does.
- In podcrates: `crates/pod-link/src/ws.rs` (`carry` on `pod_ws::bridge`);
  `crates/pod-ws/src/serve/conn.rs` answers with `server::switching`,
  which names no subprotocol; `crates/pod-ws/src/bridge.rs` takes a text
  and a binary message alike.
- Not measured: whether podnet keeps these rules over a carrier whose link
  has its own keepalive (`Settings::keepalive`, `dead_after`).

## Approach

- A setting `Ws::protocol: Option<String>`: the name is the consumer's,
  so that no name of a consumer is in the crate. The dial offers it
  (`pod_ws::url::offering`) and refuses a 101 that does not name it.
- `pod_ws::serve::Serve` gets an optional subprotocol: the loop refuses a
  request that does not offer it (426) and names it in the 101.
- A pump of the carrier in place of `pod_ws::bridge`: a text message
  sends a Close 1003 and ends the stream of the link with an error of
  protocol; a Ping each `ping_every` (20 s) and the end of the session when
  nothing comes within `pong_within` (10 s) after a Ping.
- Tests: a text message ends the link; a peer that answers no Ping ends
  the session (a paused clock); a dialer with no subprotocol gets 426; a
  plant for each.

## Prove

```sh
cargo test -p pod-ws --all-features --test serve
cargo test -p pod-link --all-features --test ws
```

## Done

2026-11-13, commit 0ef20aa. `Ws::protocol` (podnet's `podnet/1`): the
dial offers it and refuses a 101 that names another or none
(`pod_ws::handshake::check_chosen_protocol`); `pod_ws::serve::Serve`
got `protocol`, and its loop refuses a request that does not offer it
(426 with the name) and names it in the 101 (`server::check_protocol`,
`switching_to`). A name is visible ASCII with no comma, as `podnet/1` is
no token of RFC 6455 (call C259). The pump `ws::carry` replaces
`pod_ws::bridge`: a text message sends a Close 1003 and ends the stream
of the link with an error of `InvalidData`; a Ping each 20 s and no frame
within 10 s of it, while no message waits for the link, ends it with an
error of `TimedOut` (`Ws::beat`, call C260). `cargo test -p pod-ws
--all-features --test serve`: 11 passed; `cargo test -p pod-link
--all-features --test ws`: 14 passed (6 new); the lib of pod-ws: 36
passed. Plants, each caught by hand: `link-ws-text-message-carried`,
`link-ws-silent-peer-kept`, `link-ws-ping-unsent`,
`link-ws-waiting-message-unseen`, `link-ws-chosen-protocol-unchecked`,
`ws-serve-protocol-unchecked`, `ws-serve-101-unnamed`.

# T-493: `pod-link`: the DNS simplex carrier (PodUtils/podnet#521)

**Source:** PodUtils/podnet#521 (COORD-064), PodUtils/podnet@T-278.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podnet closes each of its issues before a complete beta, and its entry
PodUtils/podnet@T-278 is a take of a crate of podcrates: the DNS simplex carrier: the frames of the link go up in the names of queries and come down in the answers, one way at a time. The crate has
no such part.

## Premise

- Read: PodUtils/podnet#521 (2026-11-13). Not read yet: the text of
  PodUtils/podnet@T-278 and its tests; the agent of this entry reads them first.

## Approach

`dns::Simplex` behind a feature `dns`: the frames in base32 labels of queries to the caller's zone, the answers in TXT; a server half for the caller's authoritative listener; the ARQ of T-310 over it, as the carrier loses frames.

## Prove

```sh
cargo test --workspace --all-features
```

## Start condition

After T-305 (the reader of TXT and CNAME with EDNS0 and the TC bit).

## Done

2026-11-13, commit 5a78d3c. The dns carrier (feature `dns`,
`crates/pod-link/src/dns.rs` and `crates/pod-link/src/dns/`): one datagram
of the ARQ goes up as `session || counter || payload`, base32 of RFC 4648
in the labels of a query name under the zone (`crates/pod-link/src/dns/wire.rs`),
and the datagrams down ride in TXT records; the ARQ of T-310 recovers the
loss and the TLS of the tcp carrier runs over it (`crates/pod-link/src/dgram_tls.rs`,
shared with icmp; calls C274, C275). It takes `pod_dns::txt` (T-305), with
the new server halves `question` and `answer` in
`crates/pod-dns/src/txt.rs`, with a roundtrip test. The dialer
(`crates/pod-link/src/dns/client.rs`) starts a query for each datagram and
polls when idle; the listener
(`crates/pod-link/src/dns/server.rs`) is an authoritative server on a UDP
socket. `cargo clippy -p pod-link --features dns,icmp --all-targets --
-D warnings`: clean; `cargo test -p pod-link --features dns,icmp`: the
dns suite (`crates/pod-link/tests/dns.rs`, 7), the loss suite
(`crates/pod-link/tests/dns_loss.rs`, 3) and the wire unit tests passed;
`cargo test -p pod-dns --lib`: 51 passed. Plants
`dns-wire-base32-roundtrip` and `dns-down-needs-the-txt`, each caught.

# T-494: `pod-link`: the object-storage (S3) simplex carrier (PodUtils/podnet#521)

**Source:** PodUtils/podnet#521 (COORD-064), PodUtils/podnet@T-279.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podnet closes each of its issues before a complete beta, and its entry
PodUtils/podnet@T-279 is a take of a crate of podcrates: the object-storage simplex carrier: each side writes its frames as objects of a bucket that both can reach, and reads the objects of the other side. The crate has
no such part.

## Premise

- Read: PodUtils/podnet#521 (2026-11-13). Not read yet: the text of
  PodUtils/podnet@T-279 and its tests; the agent of this entry reads them first.

## Approach

`s3::Simplex` behind a feature `s3`: a PUT of each batch of frames as an object with a sequence in its key, a LIST and GET of the keys of the other side, a DELETE after the read; SigV4 signing; tests against a stand-in of the S3 API (the self-hosted stand-ins of `docs/live-calls.md`).

## Prove

```sh
cargo test --workspace --all-features
```

## Done

2026-11-14, commit `fa8adb4`. Read: PodUtils/podnet@T-279 (its rules of row
E2-4 of PodUtils/podnet#468; no test yet). `s3::S3` behind the feature `s3`
(with `poll` for its HTTP/1.1 and pipes; sha2 and hmac of the workspace for
SigV4, no new crate): the keys `open/SESSION`, `up/SESSION/SEQ` and
`down/SESSION/SEQ` under the prefix of a link, a PUT with
`If-None-Match: *`, a GET with `If-Match`, objects of 256 KiB, 8 in flight,
a poll of 100 ms to 5 s, a page of 1000 keys, a budget of 1 GiB for each
end of a session with a warning at 80 %, an opt-in prune; the TLS of the
tcp carrier over the objects; a listener takes the sessions that open
after its bind (calls C489 to C491). The endpoint, the key, the region and
the TLS of `https` are the caller's; the static probe binds the carrier.
Tests: `tests/s3.rs`, 8 passed (15 runs in a loop), against a stand-in of
the S3 API (`tests/common/s3.rs`) that checks SigV4 and the conditions;
the unit tests of `s3::sign` (the four examples of the documentation of
the S3 API), `s3::xml`, `s3::client` and `s3::stream`. The plants
`link-s3-put-unconditional`, `link-s3-run-past-a-gap`,
`link-s3-budget-unchecked`, `link-s3-old-session-taken`,
`link-s3-prune-skipped` and `s3-sign-query-unsorted` are caught.
`cargo test -p pod-link --all-features`: 203 passed. A test against a
server of another party on the loopback of CI is not part of this entry
(call C491).

# T-495: `pod-link`: the HTTP poll simplex carrier (PodUtils/podnet#521)

**Source:** PodUtils/podnet#521 (COORD-064), PodUtils/podnet@T-280.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podnet closes each of its issues before a complete beta, and its entry
PodUtils/podnet@T-280 is a take of a crate of podcrates: the HTTP poll simplex carrier: the client posts its frames and polls for the frames of the server, with no long-lived request. The crate has
no such part.

## Premise

- Read: PodUtils/podnet#521 (2026-11-13). Not read yet: the text of
  PodUtils/podnet@T-280 and its tests; the agent of this entry reads them first.

## Approach

`poll::Simplex` behind a feature `poll`: a POST of each batch, a GET at an interval that backs off when idle, a server half for the caller's HTTP listener; one shape with T-422 (the long poll) where it fits; loopback tests with a lost answer.

## Prove

```sh
cargo test --workspace --all-features
```

## Done

2026-11-13, commit bbc7cfd (with T-422). Read: PodUtils/podnet@T-280
(its rules are the ones of row E2-5 of PodUtils/podnet#468; it has no
test yet). The simplex poll is `Form::Simplex` (`httppoll`) of
`poll::Poll` (feature `poll`), one shape with T-422 (call C266), in place
of a type `poll::Simplex`: `POST /up` with 202 and the offset
acknowledged (`Pod-Ack`), `GET /out?since=N` with 204 when the listener
has nothing, 64 KiB for each request, one request in flight for each
lane, heads of 4 KiB, a backoff of 100 ms to 5 s while the link is quiet
that a POST up ends; `facts().cost` is `Cost::Highest` (call C269). The
listener of T-422 serves it; each lane is a request with its own answer,
so the split of T-500 can take one lane (call C267). `cargo test -p
pod-link --all-features`: 0 failed; the tests of the simplex poll:
`a_link_over_the_simplex_poll_carries_streams_across_a_lost_answer`,
`a_proxy_that_cuts_idle_connections_holds_no_push_back` (with the simplex
poll), `the_listener_refuses_a_client_of_another_key_and_stays_open`,
`the_simplex_poll_backs_off_while_the_link_is_quiet_and_the_long_poll_holds`,
`the_third_answer_429_in_a_row_ends_the_dial`. The plant
`link-poll-no-backoff` is caught.

# T-496: `pod-link`: the DNS stream carrier with a pipelined window of queries (PodUtils/podnet#521)

**Source:** PodUtils/podnet#521 (COORD-064), PodUtils/podnet@T-281.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** L
**Status:** open

## Problem

podnet closes each of its issues before a complete beta, and its entry
PodUtils/podnet@T-281 is a take of a crate of podcrates: the DNS stream carrier: several queries in flight, with the answers in TXT and CNAME, EDNS0 for the size and a fall back on the TC bit. The crate has
no such part.

## Premise

- Read: PodUtils/podnet#521 (2026-11-13). Not read yet: the text of
  PodUtils/podnet@T-281 and its tests; the agent of this entry reads them first.

## Approach

`dns::Stream` beside `dns::Simplex`: a window of queries in flight with their sequence, the size of an answer from EDNS0, a retry over TCP on TC; a test with lost and reordered answers.

## Prove

```sh
cargo test --workspace --all-features
```

## Start condition

After T-305 and T-493.

# T-497: `pod-link`: the ICMP echo carrier (PodUtils/podnet#521)

**Source:** PodUtils/podnet#521 (COORD-064), PodUtils/podnet@T-282.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podnet closes each of its issues before a complete beta, and its entry
PodUtils/podnet@T-282 is a take of a crate of podcrates: the ICMP echo carrier: the frames ride in the data of echo requests and replies. The crate has
no such part.

## Premise

- Read: PodUtils/podnet#521 (2026-11-13). Not read yet: the text of
  PodUtils/podnet@T-282 and its tests; the agent of this entry reads them first.

## Approach

`icmp::Echo` behind a feature `icmp`: an unprivileged ICMP socket (`SOCK_DGRAM`, `IPPROTO_ICMP`) where `ping_group_range` allows it, a raw socket otherwise; a server half that answers each request with the frames of its side; the ARQ of T-310 over it; a skip line where the host allows neither socket.

## Prove

```sh
cargo test --workspace --all-features
```

## Done

2026-11-13, commit 5a78d3c. The icmp carrier (feature `icmp`,
`crates/pod-link/src/icmp.rs` and `crates/pod-link/src/icmp/`): each
datagram of the ARQ rides in the data of an ICMP echo message with a
session (`crates/pod-link/src/icmp/wire.rs`,
`crates/pod-link/src/icmp/echo.rs`); the ARQ of T-310 recovers the loss and
the TLS of the tcp carrier runs over it
(`crates/pod-link/src/dgram_tls.rs`; calls C276, C277). The real path is a
raw ICMPv4 socket (`crates/pod-link/src/icmp/socket.rs`,
`crates/pod-link/src/icmp/listen.rs`) that needs `CAP_NET_RAW`; a server
needs a raw socket, since an unprivileged ping socket takes no echo
request, so `available()` checks for the raw socket and gives a typed
`Unavailable`, and the real test (`crates/pod-link/tests/icmp_raw.rs`, unix
only) prints a skip line and returns where the host refuses it. The echo
data holds `session || from || payload`: the `from` mark drops the kernel's
own reply to an echo request, which echoes the data verbatim and would
otherwise poison the ARQ (commit 86ff088). `EchoFrames` runs the same
framing over a memory pair with no privilege, so the framing, the ARQ, TLS,
the keys, a refused key and a silent peer run in
`crates/pod-link/tests/icmp.rs` on every host. `cargo clippy -p pod-link
--features dns,icmp --all-targets -- -D warnings`: clean; `cargo test
-p pod-link --features icmp --test icmp --test icmp_raw`: 8 and 1 passed
(the raw-socket link ran: the host allows `CAP_NET_RAW`). Plants
`icmp-wire-checksum` and `icmp-echo-session-filter`, each caught.

# T-498: `pod-link`: a WireGuard peer as a carrier, over `pod-wg` (PodUtils/podnet#521)

**Source:** PodUtils/podnet#521 (COORD-064), PodUtils/podnet@T-283.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

podnet closes each of its issues before a complete beta, and its entry
PodUtils/podnet@T-283 is a take of a crate of podcrates: a WireGuard peer as a carrier: the frames of the link go inside the tunnel of a peer. The crate has
no such part.

## Premise

- Read: PodUtils/podnet#521 (2026-11-13). Not read yet: the text of
  PodUtils/podnet@T-283 and its tests; the agent of this entry reads them first.

## Approach

`wg::Peer` behind a feature `wg`: a `pod-wg` device with one peer, the frames of the link in UDP datagrams inside the tunnel (the ARQ of T-310 over them), or a TCP stream of the stack of the device; loopback tests with two devices.

## Prove

```sh
cargo test --workspace --all-features
```

# T-499: `pod-link`: NAT traversal with a fall back to the relay (PodUtils/podnet#521)

**Source:** PodUtils/podnet#521 (COORD-064), PodUtils/podnet@T-284.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** L
**Status:** open

## Problem

podnet closes each of its issues before a complete beta, and its entry
PodUtils/podnet@T-284 is a take of a crate of podcrates: NAT traversal with a fall back to the relay: the two peers learn their public addresses from STUN, exchange candidates through a reverse session of the relay inside the layer of `pod-e2e`, punch a UDP hole, and keep the relay session where the punch fails (the answer of T-303). The crate has
no such part.

## Premise

- Read: PodUtils/podnet#521 (2026-11-13). Not read yet: the text of
  PodUtils/podnet@T-284 and its tests; the agent of this entry reads them first.

## Approach

`nat::Punch`: the candidates (local, reflexive by `pod-turn`'s `stun`), the exchange over a `pod_relay::reverse::channel`, the checks with a timer, the UDP carrier with the ARQ on success and the relay stream on failure; tests with two loopback peers and a stand-in relay, and a refused punch that falls back.

## Prove

```sh
cargo test --workspace --all-features
```

## Start condition

After T-301 (`pod-e2e`) and T-310 (the ARQ).

# T-500: `pod-link`: one session over an up carrier and a down carrier (PodUtils/podnet#521)

**Source:** PodUtils/podnet#521 (COORD-064), PodUtils/podnet@T-302.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podnet closes each of its issues before a complete beta, and its entry
PodUtils/podnet@T-302 is a take of a crate of podcrates: one session split across two carriers: the frames up go on one carrier and the frames down on another (for example DNS up and HTTP poll down). The crate has
no such part.

## Premise

- Read: PodUtils/podnet#521 (2026-11-13). Not read yet: the text of
  PodUtils/podnet@T-302 and its tests; the agent of this entry reads them first.

## Approach

`split::Pair`: a carrier made of a send half of one carrier and a receive half of another, with the session's acknowledgements on the down half; the two simplex halves of T-493 to T-495 fit it; a test over two memory halves with one lossy.

## Prove

```sh
cargo test --workspace --all-features
```

## Start condition

After T-493 to T-495 (the simplex carriers).

## Done

2026-11-14, commit `bdf5370`. Read: PodUtils/podnet@T-302 (its rules of row
E7-1 of PodUtils/podnet#473; no test yet). `split::Split` joins two
carriers of any kind, with no feature: a hello on each half, the join of
the halves of a session at the listener (two keys refused), `DATA` and
`END` on the main direction of each half, beats after 10 s of silence on
each direction, the loss of a half after 30 s of silence, a failure or an
early end, named on each side by `split::Lost` (`up-carrier lost`,
`down-carrier lost`) with a `LOST` frame on the other half (call C492). No
half takes over the other: podnet's "upstream fallback" of the control
channel is not done, as it is a promotion of the up half (call C493). The
pipes of the poll carriers are the module `pipes` of the crate now.
Tests: `tests/split.rs`, 5 passed (20 runs in a loop), among them 1 MiB
down a fast half and 64 KiB up a half of the ARQ at 3 % loss; the plants
`link-split-silence-unseen`, `link-split-keys-unchecked` and
`link-split-end-ignored` are caught. `cargo test -p pod-link
--all-features`: 209 passed.

# T-510: `pod-e2e`: a fuzz target of the opener of the records

**Source:** T-301 (the record layer, a finding of its work).
**Category:** chore
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`pod_e2e::record::Records::open` and `record::record_len` read the bytes
of each record that a carrier gives: the header, the length, the padding,
the counter of the replay window, and the RESUME record. A peer of a mesh
or a relay can send any bytes. The fuzz workspace (T-017) reads the frames
of the channel (`e2e_frame`), the proofs (`e2e_proof`) and the state of a
handover (`e2e_handover`), but no target reads a record.

## Premise

- Read: `crates/pod-e2e/src/record/open.rs`,
  `crates/pod-e2e/src/record/header.rs`, `fuzz/README.md` (the rows of
  the targets of `pod-e2e`).
- Not measured: no fuzz run of the opener.

## Approach

A target `e2e_record`: two ends of one exporter (an exporter of fixed keys
behind the feature `fuzzing`); the input is a list of records, each opened
at the receiving end. No panic; a record that opens has a payload of its
`length - pad` bytes; a record that fails leaves the next counter as it
was. Its seeds are sealed records of each suite (with padding, after a
rekey, a RESUME), written by a test. The row of `fuzz/README.md` and the
check of the fuzz workspace.

## Prove

```sh
cargo check --manifest-path fuzz/Cargo.toml --bins
cargo test -p pod-e2e --all-features
```

## Done

2026-11-13, commit 71dc3ca. The target `e2e_record`
(`fuzz/src/targets/e2e.rs`): a stream of records, each as long as
`record_len` says, opened at one end of an exporter of fixed keys
(`pod_e2e::fuzzing::records`); a first byte picks the suite, the time
between two records, the limit of failed RESUME records and the end that
opens. A record that opens has the fields of its header and a payload of
`length - pad` bytes, and moves the next counter to after its own and the
key at most one epoch on; a record that fails leaves the counters and the
epochs as they were; after the end of the layer each record fails. The
seeds (8: each suite, padding, two rekeys with a late record, a repeat, a
RESUME, a late one, one used twice, the responder's records) come from
`fuzz/src/record_seeds.rs`, whose test holds them to the files.
`cargo check --manifest-path fuzz/Cargo.toml --bins`: passes;
`cargo test --manifest-path fuzz/Cargo.toml --lib`: 10 passed;
`cargo test -p pod-e2e --all-features`: 0 failed. Plants, each caught by
hand: `e2e-record-fuzz-pad-kept`, `e2e-record-fuzz-window-before-the-check`.
No run of libFuzzer here: `fuzz.yml` runs the target each week.

# T-514: `pod-link`: a stream-aware ARQ, in which `arq-timeout` closes one stream and the link lives (PodUtils/podnet#468)

**Source:** T-310 (call C262, a finding of its work); PodUtils/podnet#468, row E2-2 (PodUtils/podnet@T-277)
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** L
**Status:** open

## Problem

podnet's row E2-2 says: "A stream that exhausts retries closes with `arq-timeout`, not the session, unless the control channel is also lost." Its gate is `tests/link_arq.rs` with "an isolated stream close". The ARQ of T-310 runs under the link as one sequence for each frame of each stream and of the control (call C262), so a frame with no retry left ends the ARQ and the link with it.

## Premise

- Read: `crates/pod-link/src/arq/send.rs` (one sequence for each direction), `crates/pod-link/src/link.rs` and `crates/pod-link/src/state.rs` (the frames of all streams go in one batch to one byte stream).
- Not measured: a loss that hits the frames of one stream and not the control frames. On one path, a peer that is gone loses each frame; a case is a path whose unit drops the large `DATA` frames and passes the small control frames.

## Approach

1. A channel id in the `DATA` and `ACK` datagrams of the ARQ, and a sequence, a window and a SACK bitmap for each channel: channel 0 carries the control frames of the link, and each stream has its own.
2. The link gives the frames of a stream to its channel, with a mode of `Link` for a datagram carrier (`Link::over_datagrams`), so that the byte-stream mode stays as it is.
3. A channel with no retry left resets its stream with a new reset code `ARQ_TIMEOUT`; channel 0 with no retry left ends the link.
4. Tests: a shim that drops each datagram of one channel; that stream ends with `arq-timeout`, and another stream and the link live.

## Prove

```sh
cargo test -p pod-link --all-features
```

## Start condition

podnet starts PodUtils/podnet@T-277 and asks for the close of one stream (a COORD issue), or a carrier of T-493 to T-499 shows the case.

# T-515: `pod-link`: a congestion window for the ARQ on a shared path (UDP, WireGuard, NAT traversal)

**Source:** T-310 (call C263, a finding of its work)
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

The ARQ of T-310 sends up to its window of 256 frames (256 KiB with fragments of 1 KiB) as soon as the peer's window lets it, and sends each lost frame again at once. It has no congestion control. On a path that other flows share (the UDP carrier T-424, a WireGuard peer T-498, NAT traversal T-499), a full window at each round trip with no back-off on loss is unfair to other flows, and on a slow path it fills the queues of the routers and adds loss. podnet's row E2-1 keeps the congestion control of KCP by default.

## Premise

- Read: `crates/pod-link/src/arq/send.rs` (`can_take`: the window of the settings and the window of the peer only) and PodUtils/podnet#468 (row E2-1: "Congestion control is the KCP default").
- Not measured: the ARQ over a real path with other flows; the tests use a memory pair with no rate limit.

## Approach

1. A congestion window in frames beside the two windows: NewReno of RFC 9002 (a slow start from 10 frames, a halving at a loss of a round trip, a growth of one frame for each round trip), as a setting with a choice of `none` for a carrier that has its own (the memory pair, a carrier with a rate limit).
2. Pacing of the frames at the congestion window over the smoothed round trip.
3. Tests: over a shim with a rate limit and a queue of a fixed size, the loss stays low and the throughput is near the rate; the gate of 3 % random loss still moves 8 MiB.

## Prove

```sh
cargo test -p pod-link --all-features
```

## Start condition

The first carrier over a shared path (T-424, T-498 or T-499) starts.

## Correction

2026-11-13, call C271: the udp carrier of T-424 runs the wire of KCP with
the congestion window of KCP, not this ARQ. The first carrier of the ARQ
over a shared path is T-498 or T-499.

# T-516: `pod-link`: a fuzz target of the datagrams of the ARQ

**Source:** T-310 (a finding of its work)
**Category:** chore
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`pod_link::arq::wire::Datagram::decode` and `Arq::on_datagram` read the bytes of each datagram that a lossy carrier gives, and a carrier that strangers reach (UDP, DNS, ICMP) can give any bytes. The test `datagrams_that_break_the_form_are_dropped` of `crates/pod-link/tests/arq_machine.rs` gives 20,000 random datagrams from one seed; no fuzz target reads them. The fuzz workspace has `link_frame` for the frames of the link.

## Premise

- Read: `crates/pod-link/src/arq/wire.rs`, `crates/pod-link/src/arq/recv.rs`, `fuzz/fuzz_targets/link_frame.rs`.
- Not measured: no fuzz run of the ARQ.

## Approach

A target `link_arq`: the input is a list of datagrams, each given to one `Arq` with a clock that moves by a byte of the input; after each, `poll_transmit` until `None` and `on_timeout` when its time passed. No panic; each datagram that `poll_transmit` gives decodes, and has at most the unit of the carrier; a `DATA` frame of a sequence keeps one payload. Seeds: the datagrams of a short transfer, written by a test. The row of `fuzz/README.md` and the check of the fuzz workspace.

## Prove

```sh
cargo check --manifest-path fuzz/Cargo.toml --bins
cargo test -p pod-link --all-features
```

## Done

2026-11-13, commit `0bfe4d6`. The target `link_arq` (`fuzz/src/targets/link.rs`,
`fuzz/fuzz_targets/link_arq.rs`): a byte of setup (the ARQ writes
`setup * 64` bytes), then datagrams, each a step of the clock in tens of
milliseconds, a length (u16) and its bytes. After each datagram the ARQ
sends what it has and fires its timer when its time passed. It checks: no
panic; each datagram that the ARQ sends decodes and fits the unit (1200
bytes); a `DATA` frame of one sequence always carries the same fragment,
also when it goes again; an error stays. The seeds (`fuzz/src/arq_seeds.rs`,
written by an ignored test and held to the files on disk): the datagrams
of a peer in a transfer of 3000 bytes, with a `FIN`, with acknowledgements
alone, a `PROBE`, and datagrams of a bad form. `cargo test --manifest-path
fuzz/Cargo.toml --lib`: 11 passed (the seeds and their mutations);
`cargo check --manifest-path fuzz/Cargo.toml --bins` and clippy of the fuzz
workspace pass. No plant: the target is the check, and `fuzz.yml` runs it
each week.

# T-520: `pod-link`: the default of the congestion window of KCP against podnet's gate of 20 % loss (PodUtils/podnet#515)

**Source:** T-424 (call C272, a finding of its work); PodUtils/podnet#468, row E2-1.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Row E2-1 of PodUtils/podnet#468 says "Congestion control is the KCP default" and "`--kcp-nodelay 1` [...] is not the default", and its gate is "an 8 MiB round trip over a 5% drop and 1% reorder shim; a 20% drop run within 3 times the clean time". The congestion window of KCP falls to one segment at each loss of a retransmission, so at random loss its throughput falls far below the clean one: the two rules disagree. The udp carrier of T-424 keeps the congestion window by default (`KcpSettings::nodelay` off) and runs the gate of 8 MiB with `nodelay`.

## Premise

- Measured (2026-11-13, a debug build, the `Lossy` shim of a memory pair, 2 MiB each way): with the congestion window 0.13 s clean, 3.1 s at 1 % loss, 12.4 s at 5 %, 75 s at 20 %; with `nodelay` 0.11 s clean, 0.16 s at 5 %, 0.44 s at 20 % (4.1 times the clean time).
- Not measured: a release build; UDP of the system with a real delay.

## Approach

Ask podnet (a COORD issue of podcrates `(to podnet)`) which default it wants: the congestion window of KCP (and a gate of 20 % with `nodelay`), or `nodelay` by default. Then set the default of `KcpSettings`, and add the run at 20 % loss with its bound of time (in a release build, or with a bound that a debug build holds) to `tests/udp_loss.rs`.

## Prove

```sh
cargo test -p pod-link --all-features --test udp_loss
```

## Start condition

podnet answers the question, or starts its PodUtils/podnet@T-276.

## Done

- 2026-11-13: podnet chose the congestion window of KCP by default (PodUtils/podnet#527); `KcpSettings::nodelay` stays off. `tests/udp_loss.rs` has the gate of row E2-1 in a release build: `eight_mib_cross_at_five_percent_loss_and_one_percent_reorder_with_the_defaults` (51 s measured, bound 120 s) and `twenty_percent_loss_with_nodelay_stays_within_the_factor_of_the_clean_time` (`nodelay` and a window of 1024 segments: 3.7 to 4.0 times measured here, 5.3 times on a GitHub runner, bound 8 times; call C284). A debug build ignores both. The job `gate` of `nightly.yml` runs them.

# T-521: `pod-link`: a fuzz target of the datagrams of the udp carrier (the input of KCP)

**Source:** T-424 (a finding of its work)
**Category:** chore
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The udp carrier of T-424 gives each datagram of its peer's address and conversation to `kcp::Kcp::input` (the crate `kcp` 0.6.0) before TLS checks a byte, and a stranger who knows the address and the conversation can send any bytes. The arithmetic of the crate overflows on some values: an `ACK` with an old time stamp gives a round trip near 2^31 ms, and `4 * rx_rttval` in `update_ack` overflows, which panics in a build with overflow checks (a debug build) and wraps in the release profile of the workspace. The task of `udp::KcpStream` ends the stream with an error when it ends by a panic (the test `a_datagram_that_breaks_kcp_ends_the_stream_with_an_error` of `crates/pod-link/tests/udp_loss.rs` gives one such `ACK`), but no fuzz target reads such datagrams.

## Premise

- Read: the source of `kcp` 0.6.0 (`src/kcp.rs`: `input`, `update_ack`, `parse_data`, `recv`); `crates/pod-link/src/udp/stream.rs`; `fuzz/fuzz_targets/link_frame.rs`.
- Not measured: no fuzz run; no forged datagram in a test.

## Approach

A target `link_kcp`: the input is a list of datagrams with a clock that moves by a byte of the input, each given to `Kcp::input` of one end in stream mode, with `update`, `flush` and `recv` after each. Find each panic; where one comes from the crate, take a datagram that holds it into a test of `crates/pod-link/tests/udp_loss.rs` (the stream ends with an error, no other stream is hurt), and send the fix upstream or keep a check before `input` (the time stamp of an `ACK` within the last minute). The row of `fuzz/README.md` and the check of the fuzz workspace.

## Prove

```sh
cargo check --manifest-path fuzz/Cargo.toml --bins
cargo test -p pod-link --all-features --test udp_loss
```

## Correction

2026-11-13: `plants.yml` run 43448941361 at `6b183f6` reports the plant
`link-udp-fragment-taken` as missed. The test fails, but with "the
segment, with no stall" ("KCP: the peer was silent for 20s"), a message
that the `EXPECT` of the plant did not name: since T-526, `kcp` has no
overflow checks in the test profile, so the fragment of the stranger
stalls the stream and does not panic. The `EXPECT` of the plant takes
that message too.

## Done

2026-11-13, commit `ee6f6bc`. The target `link_kcp`
(`fuzz/src/targets/link.rs`, `fuzz/fuzz_targets/link_kcp.rs`) drives
`udp::KcpStream` over scripted datagrams on a paused tokio clock: a byte
of setup (the stream writes `setup * 64` bytes), then datagrams, each a
step of the clock in tens of milliseconds, a length and its bytes. It
checks: no panic outside the code of `kcp`; the stream ends with an
error; no datagram that it sends passes 1200 bytes; after a panic in the
code of `kcp` (a hook of the target lets it unwind), the guard of the
carrier ends the stream with `BrokenPipe`, and no panic of `kcp` leaves
the task of the carrier (call C279). The plant
`link-udp-panic-hangs-the-stream` makes the fuzz test fail. The target
found two defects of the carrier, fixed (call C280): a fragment of a
message of a stranger made the reader of `kcp` overflow in the read of
the caller, outside the guard (a datagram with a fragment now does not
go in), and a segment larger than one of this end stopped the reader
until the silence limit (a read now takes a segment of any size). The
test `a_fragment_of_a_stranger_is_dropped_and_a_large_segment_is_read_whole`
fails before each fix (plants `link-udp-fragment-taken` and
`link-udp-large-segment-stalls`). The seeds (`fuzz/src/kcp_seeds.rs`): a
transfer of a peer, an acknowledgement of a time stamp 2^31 ms old (a
panic of `kcp` with overflow checks; the seed test checks the guard), and
datagrams of a bad form. `cargo test -p pod-link --all-features --test
udp_loss`: 6 passed; `cargo test --manifest-path fuzz/Cargo.toml --lib`:
13 passed; `cargo check --manifest-path fuzz/Cargo.toml --bins` passes.
The fix of `kcp` and its overflow checks are T-526.

# T-517: `pod-link`: a cap of the bytes that the sessions of a poll listener hold

**Source:** T-422 (a finding of its work)
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

A listener of the poll carriers (`crates/pod-link/src/poll/listen.rs`) caps the count of its sessions (`Poll::max_sessions`, 256), not their bytes. Each session keeps up to `WINDOW` (1 MiB) of bytes down that the dialer did not acknowledge, and two pipes of up to 256 KiB, so 256 sessions can hold about 384 MiB. The accept loop of `pod-ws` has a cap of bytes (`Serve::preauth_bytes`, call C251); the poll listener has none. Each session comes from a pinned key, so a stranger cannot open one.

## Premise

- Read: `crates/pod-link/src/poll/session.rs` (the window of each session, its pump), `crates/pod-ws/src/serve.rs` (`preauth_bytes`).
- Not measured: the memory of a listener with many sessions.

## Approach

A field `Poll::max_bytes` (64 MiB by default): a budget of the listener that each pump takes room from before it reads, and gives back at each acknowledgement; an open that cannot get the room of one batch answers 429. A test with a budget of 2 MiB and three sessions that do not read: the third open gets 429, and the bytes held stay within the budget.

## Prove

```sh
cargo test -p pod-link --all-features --test poll_lanes
```

## Done

2026-11-13, commit `f1c65fc`. `Poll::max_bytes` (64 MiB by default, at
least `MIN_BYTES`, one batch) caps the bytes down that the sessions of a
listener keep for their dialers (`crates/pod-link/src/poll/budget.rs`,
call C278). A pump reads, then waits for the room of the bytes that it
read; an acknowledgement and the end of a session give the bytes back; an
open with no room for one batch gets 429; `Stats::held` gives the bytes
kept now. The test
`the_sessions_keep_no_more_bytes_than_the_cap_and_an_open_with_no_room_gets_429`
has a cap of 1.5 MiB and three sessions that no dialer reads: the first
keeps its window (1 MiB), the second 512 KiB, the third open gets 429;
the acknowledgement of the first window gives its bytes back, the second
session takes the rest of its window, and the third open gets 201; no
count seen passes the cap. It passed 30 runs in a loop. The plants
`link-poll-bytes-uncapped` and `link-poll-open-room-unchecked` make it
fail. `cargo test -p pod-link --all-features --test poll_lanes`: 7
passed; `cargo test -p pod-link --all-features`: 156 passed. The bytes
outside the cap (the pipes of each session, the connections after their
handshake) are T-525.

# T-518: `pod-link`: a fuzz target of the HTTP/1.1 of the poll carriers

**Source:** T-422 (a finding of its work)
**Category:** chore
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

The poll carriers read heads of HTTP/1.1, chunked bodies and events of SSE (`crates/pod-link/src/poll/wire.rs`) from each peer whose key is pinned. Their unit tests give some forms by hand; no fuzz target reads them.

## Premise

- Read: `crates/pod-link/src/poll/wire.rs`, `fuzz/fuzz_targets/link_frame.rs`.
- Not measured: no fuzz run of the reader.

## Approach

A target `link_poll_wire`: the input is the bytes of one connection, given to `Conn::head`, `Conn::body` and `Conn::chunk` in turn, and to `Events::push` and `Events::next`. No panic; a head is at most 4 KiB; an event gives at most 8 KiB. The reader becomes public in a hidden module for the target, or the target holds a copy by `#[path]`. The row of `fuzz/README.md` and the check of the fuzz workspace.

## Prove

```sh
cargo check --manifest-path fuzz/Cargo.toml --bins
cargo test -p pod-link --all-features
```

## Done

2026-11-13, commit `ee6f6bc`. The target `link_poll_wire`
(`fuzz/src/targets/link.rs`, `fuzz/fuzz_targets/link_poll_wire.rs`) reads
the bytes of one connection with the reader of the poll carriers, public
in `poll::fuzzing` under the feature `fuzzing` of `pod-link`: the first
byte sets the size of each read (1 to 64 bytes, or whole for 0); each
head, then its body of a Content-Length (at most one batch) or its chunks
(at most two batches) into the reader of SSE. It checks: no panic; a head
takes at most 4 KiB with its blank line; a chunk is not empty and has at
most its limit; a body has its length; an event gives at most 8 KiB. It
found a defect: a head over 4 KiB that one read brought whole went in.
The reader now looks for the end of a head in the first 4 KiB alone; the
new case of `a_long_head_and_a_large_chunk_are_refused` fails before the
fix (plant `link-poll-long-head-taken`, which also makes the fuzz test
fail). The seeds (`fuzz/src/poll_seeds.rs`, written by an ignored test):
the requests of a dialer, the answers of a listener with a push of SSE, a
chunked answer of raw bytes, and refused forms. `cargo test
--manifest-path fuzz/Cargo.toml --lib`: 13 passed; `cargo check
--manifest-path fuzz/Cargo.toml --bins` and clippy of the fuzz workspace
pass; `cargo test -p pod-link --all-features`: 157 passed.

# T-525: `pod-link`: the bytes of a poll listener outside `max_bytes` (the pipes of each session, the connections after their handshake)

**Source:** T-517 (a finding of its work)
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`Poll::max_bytes` (T-517, call C278) caps the bytes down that the sessions of a poll listener keep for their dialers. Each session holds more bytes outside the cap: two pipes of `BATCH` (256 KiB each, `tokio::io::duplex` grows its buffer up to that size and keeps it) and the buffer of its pump (64 KiB), so 256 sessions can hold about 144 MiB besides the cap. A connection after its handshake is not counted: `max_handshakes` caps the handshakes alone, and each connection reads a body of up to `BATCH` before it answers. Each session and each connection comes from a pinned key, so a stranger cannot open one.

## Premise

- Read: `crates/pod-link/src/poll/session.rs` (the pipes and the pump), `crates/pod-link/src/poll/listen.rs` (the permit of a handshake drops after the handshake), `crates/pod-link/src/poll/answer.rs` (the body of a request).
- Not measured: the memory of a listener with many sessions or connections.

## Approach

Make the pipes of a session on the listener smaller (the pump drains the pipe down into the bytes kept, so 64 KiB is enough; the pipe up takes a POST in parts), and measure the throughput of `tests/poll.rs` before and after. Add `Poll::max_connections` (1024 by default): a permit for each connection that lasts as long as it does; a connection past the cap closes after its accept. Count the body of a request in `max_bytes`, or give it its own cap. A test with a cap of 2 connections: the third closes, the first two stay.

## Prove

```sh
cargo test -p pod-link --all-features --test poll_lanes
```

## Correction

2026-11-15: `nightly.yml` run 43699451813 at `98017c3`: `a_body_up_goes_into_the_link_by_parts_and_a_cut_keeps_its_part` fails on windows-2025 after 30 s at the read of the answer: the test reads the rest at the far end only after the answer, and the link on Windows holds less than the rest. T-681 has the fix of the test.

## Done

2026-11-14, commit `c3b3403`. A poll listener reads the body of a POST up
by parts of at most 64 KiB (`wire::Conn::part`) and writes each part into
the link at once; a cut in a body keeps the part that went in, and the
POST again skips it (call C488). Each other body is read by parts and
dropped, so no connection keeps a whole body, and a body needs no cap of
its own. The pipes of a session on a listener are 64 KiB, the buffer of
its pump 16 KiB. `Poll::max_connections` (1024) caps the connections at
once; one past it closes at its accept (`Stats::turned_away`). Throughput
on the loopback, 32 MiB each way, 3 runs: up 96 to 116 MiB/s before and
107 to 269 after; down 59 to 98 before and 54 to 100 after. Tests
`a_connection_past_the_cap_closes_and_the_others_stay` and
`a_body_up_goes_into_the_link_by_parts_and_a_cut_keeps_its_part`; the
plants `link-poll-connections-uncapped` and `link-poll-body-kept-whole`
make them fail; the plants `link-poll-duplicate-taken`,
`link-poll-handshake-inline` and `link-poll-open-room-unchecked` follow the
new code and are caught. `cargo test -p pod-link --all-features --test
poll_lanes`: 9 passed; the crate: 190 passed.

# T-526: `pod-link`: the arithmetic of the crate `kcp` wraps by design: no overflow checks for it, and the fix upstream

**Source:** T-521 (a finding of its work)
**Category:** chore
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** partial

## Problem

The crate `kcp` 0.6.0 is a port of ikcp, whose clock (32 bits of milliseconds) and sequence numbers wrap: `timediff` subtracts two `u32` as `i32`. With overflow checks (the dev and test profiles of the workspace, a fuzz build), this panics on a forged segment of a stranger (call C279), and also on true traffic when the clock of one stream passes 2^31 ms (24.8 days) or a sequence number passes 2^31. The guard of the task of `udp::KcpStream` ends the stream with an error, so a debug build of a consumer loses a long stream. The release profile has no overflow checks and wraps as ikcp does.

## Premise

- Read: the source of `kcp` 0.6.0 (`timediff`, `update_ack`, `input`, `peeksize`); `crates/pod-link/src/udp/stream.rs`; the profiles of `Cargo.toml` and `fuzz/Cargo.toml`.
- Not measured: a stream of more than 2^31 ms in a debug build.

## Approach

Set `[profile.dev.package.kcp] overflow-checks = false` in `Cargo.toml` (the test profile takes it from dev). The test `a_datagram_that_breaks_kcp_ends_the_stream_with_an_error` of `crates/pod-link/tests/udp_loss.rs` then needs another panic of the task (a carrier of datagrams that panics in `recv`), and its plant stays. Keep the overflow checks of `kcp` in `fuzz/Cargo.toml` so that `link_kcp` exercises the guard (C279). Send the fix to `kcp` upstream (`wrapping_sub` in `timediff`, `saturating_mul` in `update_ack`, a `u8` check in `peeksize`), and tell podnet to set the same profile in its workspace by a COORD issue.

## Prove

```sh
cargo test -p pod-link --all-features --test udp_loss
cargo test --manifest-path fuzz/Cargo.toml --lib
```

## Done

- Part, 2026-11-13: `Cargo.toml` sets `[profile.dev.package.kcp] overflow-checks = false`. The test `a_panic_of_the_task_ends_the_stream_with_an_error` uses a carrier that panics in `recv`; the plant `link-udp-panic-hangs-the-stream` moved to it and fails it. `fuzz/Cargo.toml` keeps the checks. Open: the fix upstream in `kcp`. #139 (COORD-079) asks podnet to set the same profile; podnet sets it with its take of the udp carrier (PodUtils/podnet#527).
- Part, 2026-11-14: `docs/upstream/kcp-0.6.0-wrapping.patch` holds the fix for `kcp` 0.6.0 (`timediff` with `wrapping_sub`; `update_ack` in 64 bits with saturation; `peeksize` with `usize::from(frg) + 1`); it applies to the source of the crate with `patch -p1`. Open: the owner sends it upstream ("Owner actions" of `TODO/PROGRESS.md`); a session cannot write outside PodUtils.

# T-531: `pod-netstack`: tests of a far end that closed first and of the packets held while the queue out is full (PodUtils/podssh#231)

**Source:** PodUtils/podssh#231 (COORD-054 of podssh).
**Category:** chore
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podssh removed two planted defects of PodUtils/podssh@T-599 (the stack inside WARP's tunnel), and their code is `pod-netstack`'s now. No test of `pod-netstack` holds either case:

1. A far end that closed first: each byte queued toward it must go before the end. The tests of `close.rs` close from this end.
2. Packets that the stack sends while the outbound queue is full: none must be lost when the queue drains. The tests of `device/tests.rs` check one token of the device, not the stack.

The record of podssh (PodUtils/podssh@T-771) must name a test of each case that fails on a planted defect.

## Premise

- Read: `crates/pod-netstack/src/stack/service.rs` (`close_and_check`: a close waits until no byte waits in either direction, also in CLOSE-WAIT), `crates/pod-netstack/src/device.rs` (`LinkDevice::link` holds while the outbound queue is full; `transmit` gives no token then).
- The code holds both cases. The gap is the tests.

## Approach

1. `close.rs`: the client shuts down its write side, and the server reads that end. Then the server writes 16 MiB and closes at once. The client must read each byte, then the end. The plant `netstack-close-wait-closes-at-once` closes the socket in CLOSE-WAIT with no wait: the bytes in the channel from the app are lost.
2. `hold.rs`: a pump from the server that stops and starts again. With a window of 2 MiB, the outbound queue of the server fills (`LINK_OUTBOUND` packets) while the pump stands. Then the pump moves again. The device must drop no packet, the stack must hold, the client must read each byte and the end, and each packet that the stack sent must cross the link. The plant `netstack-link-drops-at-a-full-queue` gives the device of a link a bounded queue with no hold: the stack sends into the full queue, and the queue drops the packets.

## Prove

```sh
cargo test -p pod-netstack --lib a_stream_whose_peer_closed_first_sends_each_queued_byte_then_the_end
cargo test -p pod-netstack --lib the_packets_held_at_a_full_link_each_go_when_it_drains
sh scripts/plant-each.sh netstack-close-wait-closes-at-once netstack-link-drops-at-a-full-queue
```

## Done

2026-11-13, commit `2f643b1`. `cargo test -p pod-netstack --all-features`: 79 passed. `close.rs`: `a_stream_whose_peer_closed_first_sends_each_queued_byte_then_the_end` (the client shuts down first; the server writes 16 MiB and closes; the client reads each byte, then the end). `hold.rs`: `the_packets_held_at_a_full_link_each_go_when_it_drains` (the outbound queue of the server fills to `LINK_OUTBOUND` while the pump stands; then `tx_dropped` is 0, `tx_held` is more than 0, the client reads 4 MiB and the end, and the pump moved each packet that the device counted in `tx_packets`). The plant `netstack-close-wait-closes-at-once` fails the first test ("the end after the end of the peer lost 4230144 bytes"). The plant `netstack-link-drops-at-a-full-queue` fails the second test ("4714 packets dropped at the full queue"); no other test of the crate catches it, as `a_full_tunnel_queue_holds_the_stack` uses `LinkDevice::tunnel_bounded`, not `LinkDevice::link`.

# T-550: `pod-wormhole`: Nostr's ephemeral events as a meeting point of the pairing by a code (PodUtils/podssh#235)

**Source:** PodUtils/podssh#235 (COORD-058 of podssh; PodUtils/podssh@T-545)
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** M
**Status:** done

## Problem

A pairing by a short code meets at magic-wormhole's mailboxes, then at the
public MQTT brokers (`pod_wormhole::mqtt`, T-261). podssh's ruling Q77
adds two more meeting points, in this order after the mailboxes. The first
is Nostr's ephemeral events: an event kind of the ephemeral range (20000
to 29999), tagged from the code's nameplate, at public relays over
`wss://` on 443. A peer subscribes with a filter for that tag, and the
code's PAKE messages go as the events' content. The ask:

- In `pod-wormhole`: the transport beside `mqtt`.
- In `pod-services`: the public Nostr relays, of a new kind, with the
  facts that say whose they are.
- In `pod-relay`'s runs of a code: the places in order (mailboxes, Nostr,
  trackers, brokers, as Q77 ruled), with the third party's line before
  the first byte, as `BROKER_WORDS` does.
- Tests against a stand-in or the software's own server (a Nostr relay
  such as `nostr-rs-relay`), and one short live pairing from GitHub
  Actions.

## Premise

- Read: `crates/pod-wormhole/src/mqtt.rs`. A broker is a `Place`
  (`place.rs`) that says the messages of the mailbox (`wormhole.rs`):
  SPAKE2 keyed by the whole code, then sealed phases. The topic comes from
  the AppID and the nameplate alone (`mqtt::topic`), never from the words;
  the giver draws the nameplate (`mqtt::nameplate`); each end reads the
  topic with the tag of its direction (`Role::topics`); the taker sends
  its messages again each `RESEND` until the giver answers, as a message
  at QoS 0 that reaches no subscriber is gone.
- Read: `crates/pod-relay/src/code/run.rs`. `Ends` has the mailboxes,
  then the brokers; the event `relay.code.broker` with `BROKER_WORDS`
  comes before the first broker.
- Known, not read for this entry: NIP-01 of Nostr. A client sends
  `["EVENT", event]`, `["REQ", id, filter]` and `["CLOSE", id]`; a relay
  sends `["EVENT", id, event]`, `["OK", ...]`, `["EOSE", id]` and
  `["NOTICE", ...]`. An event's id is the SHA-256 of
  `[0, pubkey, created_at, kind, tags, content]`, and its `sig` is a
  BIP-340 Schnorr signature on secp256k1 by `pubkey`. A relay keeps no
  event of the ephemeral range; it gives one to the subscribers of the
  moment. A relay's NIP-11 document (`Accept: application/nostr+json`)
  states its limits (`max_message_length`, `max_subscriptions`,
  `min_pow_difficulty`, `auth_required`, `payment_required`).
- The workspace has no secp256k1 Schnorr signer. Not measured: a public
  relay, from this sandbox.

## Approach

1. Read first: NIP-01, NIP-11, NIP-13 and NIP-42 at pinned commits of
   `nostr-protocol/nips`, and the source of `nostr-rs-relay` at the tag
   of the stand-in: the size limit of an event, the rate limits, and
   which public relays need neither proof of work nor AUTH.
2. `pod_wormhole::nostr`, beside `mqtt`, over `pod-ws` (`wss://`, 443):
   a key of secp256k1 made for each pairing (a pure-Rust signer, which
   links statically, with a call for the new dependency), events of one
   ephemeral kind, one single-letter tag whose value is `mqtt::topic` of
   the nameplate with the tag of the direction, and a `REQ` with that tag,
   the kind and `since` at the code's start. The content is the message of
   the mailbox, as `mqtt::Broker` carries it. The taker sends again each
   `RESEND` until the giver answers. `Place` gains the relay; the
   ends of `meet.rs` take the relays in order.
3. `pod-services`: the kind `nostr-relay` (endpoint `relay`, `wss`,
   443), with the facts of each relay's NIP-11 document and its party;
   watches: `websocket-upgrade` and an `http-json` of the NIP-11
   document; a table of the kind in `pod-services-watch` (`kind_facts`).
4. `pod-relay`: `Ends` takes an ordered list of places of each kind; the
   runs try the mailboxes, Nostr, the trackers (T-551), then the brokers
   (Q77), with an event before the first relay, `NOSTR_WORDS`, as
   `BROKER_WORDS`. podssh's glue gives the switches.
5. Tests: an offline stand-in relay for `cargo test` (the tag filter, an
   event of another nameplate not delivered, a bad signature refused), and
   `nostr-rs-relay` at a pinned tag in CI, as `scripts/ci/mosquitto-tests.sh`
   runs Mosquitto. A plant: the tag from a constant, and the test of two
   codes fails. One short live pairing through each public relay of the
   catalog in `live.yml`.

## Prove

```sh
cargo test -p pod-wormhole --all-features
cargo test -p pod-relay --all-features --test pair_code
cargo test -p pod-services --all-features
cargo test -p pod-wormhole --all-features --test live -- --ignored
```

## Correction

2026-11-14: the premise says that a relay keeps no event of the ephemeral
range. strfry 1.1.3 (strfry.conf at its commit 16c00df) keeps one for 300 s
(`ephemeralEventsLifetimeSeconds`) and refuses one older than 60 s
(`rejectEphemeralEventsOlderThanSeconds`); a `REQ` with `since` gets the
kept ones. The Approach named `nostr-rs-relay` at a pinned tag in CI:
neither it nor strfry publishes a release with a binary, so the stand-in
stays the relay of CI (C549). From a cloud session, relay.damus.io closed
each connection after the `REQ`, nos.lol and nostr.mom gave no answer to
an event, and relay.nos.social refused the kind (C548).

## Done

2026-11-14, commit `bf45447` (the code, the tests, the services) and the
commit of the plants and the record after it. Calls C546 to C551.

- `pod_wormhole::nostr` (`crates/pod-wormhole/src/nostr.rs`,
  `crates/pod-wormhole/src/nostr/relay.rs`,
  `crates/pod-wormhole/src/nostr/event.rs`): `Relays { points, config, resend }`, `opened`,
  `joined`, `KIND` 25431, `TAG` `t`, `RESEND` 2 s, `SINCE_SLACK` 60 s,
  `CONTENT_MOST` 64,000; `Relay::connect` proves the relay (`EOSE`, then
  its own empty event back with `OK` true); `event::Key` (a fresh key of
  secp256k1 for each pairing, `k256` 0.14 with the feature `schnorr`),
  `id_of`, `checked`, `said`, `harmless`. `Place::Relay`. The events
  `wormhole.nostr`.
- `pod-services`: the kind `nostr-relay`, `primal-nostr` (rank 1) and
  `offchain-nostr` (rank 2) with the facts of their NIP-11 documents;
  `pod-services-watch`: the argument `accept` of `http-json`, and
  `kind_facts("nostr-relay")`.
- `pod-relay`: `Ends::relays` and `Ends::relays()`; `opened` and `joined`
  try the mailboxes, the relays, then the brokers, with the event
  `relay.code.nostr` (`NOSTR_WORDS`) before the first relay. T-551 puts
  the trackers between the relays and the brokers.
- Tests: `cargo test -p pod-wormhole --all-features`: 46 unit tests (9
  new of `nostr`: BIP-340's vector 0, an id computed by Python, an event
  signed by a BIP-340 signer in Python that checks and that this crate
  signs the same, a changed event refused, the messages of a relay) and
  `crates/pod-wormhole/tests/nostr.rs` (7) on the stand-in relay of
  `crates/pod-wormhole/tests/harness/nostr.rs`, 30 runs in a loop
  with no failure; `cargo test -p pod-relay --all-features --test
  pair_code`: 12 passed (2 new); `cargo test -p pod-services
  --all-features`: `crates/pod-services/tests/nostr.rs` (1) and the
  others pass;
  `cargo test -p pod-services-watch --all-features`:
  `http_json_asks_for_the_type_of_accept` and the others pass.
  `cargo test -p pod-wormhole --all-features --test live -- --ignored
  two_ends_meet_at_each_public_nostr_relay`: both relays paired, from a
  cloud session through its HTTPS proxy. `live.yml` runs it each
  Thursday (row L44 of `docs/live-calls.md`).
- Plants: each proven by hand (`git apply`, the run of its
  `RUN`, `git apply -R`). `wormhole-nostr-tag-constant` fails
  `an_event_with_the_tag_of_another_code_is_not_delivered` (the taker of
  another code met the giver); `wormhole-nostr-no-resend` fails
  `the_taker_sends_its_first_message_again_until_the_giver_answers` ("the
  taker after its losses: Nobody"); `wormhole-nostr-refusal-ignored` fails
  `a_relay_that_refuses_is_passed_over_with_its_words` (the words of the
  `OK` false are not said); `wormhole-nostr-signature-unchecked` fails
  `an_event_that_does_not_check_is_refused_and_dropped` (the stand-in takes
  the forged event); `relay-code-nostr-after-brokers` fails
  `where_no_mailbox_answers_the_ends_meet_at_a_nostr_relay_before_a_broker`
  (the ends met at the broker). The fuzz target `nostr_message` runs
  `nostr::event::said` and `checked` (`fuzz/`, its seeds in
  fuzz/seeds/nostr_message). The patches of the plants of T-277
  (`relay-t277-brokers-not-tried`, `relay-t277-nobody-stops-the-taker`)
  are made again on the new `opened` and `joined`; each still fails its
  test.
- podssh's part (PodUtils/podssh@T-545): set `Ends::relays` of its runs
  of a code to the endpoint `relay` of each service of
  `pod_services::of_kind("nostr-relay")`, in the order of the ranks
  (`WsUrl::parse(&service.endpoint("relay")?.url())`); give a switch that
  empties it (as `PODSSH_MQTT` empties `Ends::brokers`); show the event
  `relay.code.nostr` as the line of the third party; a caller of
  `pod-wormhole` alone calls `nostr::opened` and `nostr::joined` with
  `Relays::new(points, config)`.

# T-551: `pod-wormhole`: WebTorrent's trackers as a meeting point of the pairing by a code (PodUtils/podssh#235)

**Source:** PodUtils/podssh#235 (COORD-058 of podssh; PodUtils/podssh@T-695)
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** M
**Status:** done

## Problem

The second meeting point of podssh's ruling Q77, after Nostr (T-550) and
before the MQTT brokers: WebTorrent's trackers. A tracker passes any JSON
between the peers of one `info_hash`, as offers and answers (podssh's
docs/carriers.md, the verdict of PodUtils/podssh@T-546). The `info_hash`
comes from the nameplate (20 characters), with a random `peer_id`. The
waiting end announces with no offer and announces again each interval; the
other end sends the messages as offers, and the answers carry the replies.
Each message carries the nameplate's tag, and a lost one is sent again; no
copy is kept, so a message that reaches no peer must be repeated. The ask:

- In `pod-wormhole`: the transport beside `mqtt`.
- In `pod-services`: the public trackers, of a new kind, with the facts
  that say whose they are. PodUtils/podssh@T-546 found each default
  tracker listed as "Questionable Content" in a filter list; podssh's
  manual says so, from a fact of the catalog.
- In `pod-relay`'s runs of a code: the place in Q77's order, with the
  third party's line before the first byte.
- Tests against `webtorrent/bittorrent-tracker` at a pinned release, and
  one short live pairing from GitHub Actions.

## Premise

- Read: `crates/pod-wormhole/src/mqtt.rs` and
  `crates/pod-relay/src/code/run.rs`, as T-550 says.
- Read through web.pkgforge.dev on 2026-11-14: the newest release of
  `webtorrent/bittorrent-tracker` is `v11.2.3` (2026-05-25).
- Known, not read for this entry: the WebSocket protocol of
  bittorrent-tracker. A client sends `{"action":"announce", "info_hash",
  "peer_id", "numwant", "offers":[{"offer", "offer_id"}], ...}`; the
  tracker gives each offer to other peers of the `info_hash` with the
  sender's `peer_id`; a peer's `answer` with `to_peer_id` and `offer_id`
  goes to that peer; an announce answer names the `interval`. The
  `info_hash` and the `peer_id` are 20 characters each. A giver can speak
  only in an answer to an offer.
- Not measured: a public tracker, from this sandbox.

## Approach

1. Read first: `server.js`, `lib/server/parse-websocket.js` and
   `lib/server/swarm.js` of bittorrent-tracker at `v11.2.3` (or the
   release of the start): the checks of an offer and an answer, the most
   offers of one announce, the size of a message, and the interval.
2. `pod_wormhole::tracker`, beside `mqtt`, over `pod-ws`: the
   `info_hash` is 20 characters from `mqtt::topic` of the nameplate (a
   hash, in hex), never from the words; a random `peer_id` for each
   pairing. The giver announces with no offer, again each interval. The
   taker sends each message of the mailbox as an offer, with the tag of
   the nameplate, and again each `RESEND` until it has the answer; the
   giver answers each offer with its next message or an empty one, so
   that the taker's offers carry the giver's words back. `Place` gains the
   tracker.
3. `pod-services`: the kind `webtorrent-tracker` (endpoint `tracker`,
   `wss`, 443), with each tracker's party, and the fact of the filter list
   (its name, its category, the date of the read; PodUtils/podssh@T-546)
   that podssh's manual shows; watches: `websocket-upgrade`.
4. `pod-relay`: the place after Nostr and before the brokers (Q77), with
   an event `TRACKER_WORDS` before the first tracker.
5. Tests: an offline stand-in tracker for `cargo test` (two `info_hash`
   values kept apart, a lost offer sent again), and bittorrent-tracker at
   the pinned release in CI from a hashed npm lock file. A plant: the
   giver that answers with no message, and the test of the exchange
   fails. One short live pairing through each public tracker of the
   catalog in `live.yml`.

## Prove

```sh
cargo test -p pod-wormhole --all-features
cargo test -p pod-relay --all-features --test pair_code
cargo test -p pod-services --all-features
cargo test -p pod-wormhole --all-features --test live -- --ignored
```

## Correction

2026-11-14: the premise says that a tracker reads no field of an offer or
an answer and that any JSON passes. bittorrent-tracker 11.2.3 reads none,
but tracker.openwebtorrent.com refuses an offer with no object of `type`
and `sdp`, and tracker.webtorrent.dev (Aquatic) answers "Invalid request":
the messages go as a note in the `sdp` (C556). tracker.webtorrent.dev also
passes one answer to each offer, so the giver answers a fresh offer for
each message, keeps one offer to answer, and answers the taker's offers at
its close (C558). The Approach's "the giver answers each offer with its
next message or an empty one" becomes a note with each message that the
taker lacks and the counts of each end. The premise did not measure a
public tracker: from a cloud session, tracker.openwebtorrent.com and
tracker.webtorrent.dev paired two ends; tracker.files.fm:7073 (not on 443)
got no tunnel from the proxy, and tracker.btorrent.xyz answered 502 (C557).

## Done

2026-11-14, commit `ce519c7` (the code, the tests, the services, the step
of CI) and the commit of the plants and the record after it. Calls C556 to
C561.

- `pod_wormhole::tracker` (`crates/pod-wormhole/src/tracker.rs`,
  `crates/pod-wormhole/src/tracker/link.rs`,
  `crates/pod-wormhole/src/tracker/wire.rs`): `Trackers { points, config,
  resend }`, `opened`, `joined`, `info_hash(tag)`, `RESEND` 2 s, `SDP_MOST`
  10,000, `LINGER` 10 s, `ANNOUNCE_LEAST` 1 s, `ANNOUNCE_MOST` 120 s;
  `Tracker::connect` proves the tracker (the answer to an announce of
  `started`), `add`, `message(wait)`, `close(mood)` (the giver's linger,
  the taker's last count, then `stopped`); `wire::said`, `wire::Note`
  (`to_sdp`, `indexed`), `wire::note_of`. `Place::Tracker`. The events
  `wormhole.tracker`.
- `pod-services`: the kind `webtorrent-tracker`, `openwebtorrent-tracker`
  (rank 1) and `webtorrent-dev-tracker` (rank 2), with `sdp-bytes-most`,
  `software` and the filter list of each (`filter-list` hagezi,
  `filter-list-part` anti.piracy, `filter-category` "Questionable
  Content"); `pod-services-watch`: `kind_facts("webtorrent-tracker")`. The
  8 watches of the two files hold from a cloud session
  (`cargo run -p pod-services-watch -- --service openwebtorrent-tracker
  --service webtorrent-dev-tracker`).
- `pod-relay`: `Ends::trackers` and `Ends::trackers()`; `opened` and
  `joined` try the mailboxes, the Nostr relays, the trackers, then the
  brokers, with the event `relay.code.tracker` (`TRACKER_WORDS`) before
  the first tracker.
- CI: `scripts/ci/tracker-tests.sh` (a step of `ci.yml`) runs
  `crates/pod-wormhole/tests/tracker.rs` against bittorrent-tracker 11.2.3
  from `scripts/ci/bittorrent-tracker/package-lock.json`; run here, it
  said "webtorrent/bittorrent-tracker: two ends paired". Row L47 of
  `docs/live-calls.md`; `scripts/check-workflows.py` takes
  registry.npmjs.org as a host of the downloads.
- Tests: `cargo test -p pod-wormhole --all-features`: 50 unit tests (4 new
  of `tracker` and `tracker::wire`: the `info_hash`, the bounds, the
  messages of bittorrent-tracker, a note both ways) and
  `crates/pod-wormhole/tests/tracker.rs` (9) on the stand-in tracker of
  `crates/pod-wormhole/tests/harness/tracker.rs`, 30 runs in a loop with
  no failure; `cargo test -p pod-relay --all-features --test pair_code`:
  14 passed (2 new); `cargo test -p pod-services --all-features`:
  `crates/pod-services/tests/tracker.rs` (1) and the others pass.
  `cargo test -p pod-wormhole --all-features --test live -- --ignored
  two_ends_meet_at_each_public_webtorrent_tracker`: both trackers paired,
  from a cloud session through its HTTPS proxy. `live.yml` runs it each
  Thursday (row L46 of `docs/live-calls.md`).
- Plants: each proven by hand (`git apply`, the run of its `RUN`, `git
  apply -R`). `wormhole-tracker-empty-answer` fails
  `two_ends_pair_through_the_swarm_of_a_code_and_move_messages_each_way`
  ("the taker: Nobody"); `wormhole-tracker-no-resend` fails
  `a_lost_offer_is_offered_again_until_the_giver_answers` ("the taker
  after its losses: Nobody"); `wormhole-tracker-info-hash-constant` fails
  `tracker::tests::an_info_hash_is_20_hex_digits_of_the_tag_alone`;
  `wormhole-tracker-refusal-ignored` fails
  `a_tracker_that_refuses_or_says_nothing_is_passed_over_with_its_words`
  (the words of the refusal are not said); `wormhole-tracker-stranger-heard`
  fails `a_stranger_in_the_swarm_after_the_taker_is_not_heard` (the
  stranger's phase 0 does not open); `relay-code-trackers-after-brokers`
  fails `where_no_mailbox_or_relay_answers_the_ends_meet_at_a_tracker_before_a_broker`
  (the ends met at the broker). The patches of
  `relay-code-nostr-after-brokers` and `relay-t277-brokers-not-tried` are
  made again on the new `Ends::next_places` and `opened`; each still fails
  its test, and `relay-t277-nobody-stops-the-taker` too. The fuzz target
  `tracker_message` runs `tracker::wire::said`, `note_of` and
  `Note::to_sdp` (`fuzz/`, its seeds in fuzz/seeds/tracker_message).
- podssh's part (PodUtils/podssh@T-695): set `Ends::trackers` of its runs
  of a code to the endpoint `tracker` of each service of
  `pod_services::of_kind("webtorrent-tracker")`, in the order of the ranks
  (`WsUrl::parse(&service.endpoint("tracker")?.url())`); give a switch that
  empties it (as `PODSSH_MQTT` empties `Ends::brokers`); show the event
  `relay.code.tracker` as the line of the third party; show in the manual
  the facts `filter-list`, `filter-list-part` and `filter-category` of each
  tracker (a filter of piracy refuses each domain). A caller of
  `pod-wormhole` alone calls `tracker::opened` and `tracker::joined` with
  `Trackers::new(points, config)`, and keeps the giver reading (`receive`)
  or closing while the taker waits for its messages: the giver speaks only
  in an answer to an offer.

# T-554: `pod-wormhole`: an ntfy topic as a meeting point of the pairing where a proxy refuses the WebSocket upgrade (PodUtils/podssh#237)

**Source:** PodUtils/podssh#237 (COORD-060 of podssh; PodUtils/podssh@T-496)
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** M
**Status:** done

## Problem

Each meeting point of the pairing by a code is a WebSocket: the mailboxes
of magic-wormhole, and the MQTT brokers over a WebSocket. Behind a proxy
that refuses the upgrade, a node and an operator cannot pair, though the
Piping Server road carries their sessions there afterwards. podssh read
(its docs/carriers.md, "Meeting points", 2026-11-02): ntfy.sh can be used
with no account, under rate limits, and its topics are public; its limits
are 4,096 bytes a message, 60 requests at once then one each 5 s, and 250
messages a day; a topic is read as a JSON stream by a GET (`TOPIC/json`),
polled with `poll=1` and `since=`, and written by a POST; the server is
Apache-2.0 and GPLv2, and release v2.36.0 publishes `checksums.txt`. The
ask:

- In `pod-wormhole`: a transport of the code's messages over an ntfy
  topic. Each message is one JSON body of a side, a phase and the payload,
  under 4,096 bytes. The topic derives from the nameplate alone, never
  from the words that SPAKE2 takes. The other side's messages are read as
  a JSON stream with `since=` at the code's start. One connection: on
  HTTP/2 the stream's GET and the POSTs share it; on HTTP/1.1 a poll and a
  POST take turns.
- In `pod-relay`'s runs of a code: after the mailboxes, or first where a
  probe found the upgrade refused (as the Piping Server road is), with the
  third party's line before the first byte. podssh's glue gives the switch
  (`PODSSH_PAIR_NTFY`).
- In `pod-services`: ntfy.sh, of a new kind, with its limits as facts.
- Tests against ntfy's own server (v2.36.0, checked against its
  `checksums.txt`), and one short live pairing from GitHub Actions.

## Premise

- Read: `crates/pod-wormhole/src/mqtt.rs` and
  `crates/pod-relay/src/code/run.rs`, as T-550 says;
  `crates/pod-relay/src/http_road/mod.rs` says how a run learns that the
  upgrade was refused.
- Read: the client of `pod-proxy` speaks HTTP/1.1; `h2` is in the
  workspace (`pod-link`'s `h2` carrier, `pod-dns`'s DoH).
- Read through web.pkgforge.dev on 2026-11-14: the newest release of
  `binwiederhier/ntfy` is `v2.37.2` (2026-11-04), with `checksums.txt`;
  `v2.37.0` and `v2.37.1` came on 2026-11-02.
- Not measured: ntfy.sh, from this sandbox.

## Approach

1. Read first: ntfy's documents of publishing and subscribing and its
   server's limits at the tag of the stand-in: the JSON stream and its
   `since=` (a time or a message id), `poll=1`, the size of a message,
   and the rate limits of a visitor with no account.
2. `pod_wormhole::ntfy`, beside `mqtt`, over HTTPS on 443: the topic is
   `mqtt::topic` of the nameplate with the tag of the direction, never
   from the words. Each message of the mailbox is one POST of a JSON body
   (side, phase, payload) under 4,096 bytes; a longer one fails before
   the send. The reader takes the other side's messages from
   `TOPIC/json?since=START` and drops its own. On HTTP/2 (ALPN `h2`, over
   `h2` as the DoH of `pod-dns` does) one connection holds the stream and
   the POSTs; on HTTP/1.1 one connection polls (`poll=1`, `since=` of the
   last id) and posts in turns, within the rate limit. `Place` gains the
   topic.
3. `pod-services`: the kind `ntfy-server` (endpoint `server`, `https`,
   443) with `message-bytes-most`, `requests-burst`,
   `request-refill-seconds`, `messages-per-day` and `account-required`,
   each from its source; watches: `http-status` of the root and an
   `http-text` of the document of the limits.
4. `pod-relay`: the place after the mailboxes, or first where the run
   found the upgrade refused, with an event `NTFY_WORDS` before the first
   byte, as `BROKER_WORDS`. With T-550 and T-551, the default order is
   the mailboxes, ntfy, Nostr, the trackers, the brokers; podssh's glue
   can give another.
5. Tests: an offline stand-in of the two routes for `cargo test` (a
   message of 4,097 bytes refused, the poll after `since=`, the turns on
   HTTP/1.1), and ntfy's server at v2.36.0 or newer in CI, its archive
   checked against `checksums.txt` before it runs. A plant: the topic
   from the whole code, and the test that the topic holds no word fails.
   One short live pairing through ntfy.sh in `live.yml`.

## Prove

```sh
cargo test -p pod-wormhole --all-features
cargo test -p pod-relay --all-features --test pair_code
cargo test -p pod-services --all-features
cargo test -p pod-wormhole --all-features --test live -- --ignored
```

## Correction

2026-11-14: the Problem and the Approach say that a message is at most
4,096 bytes. ntfy 2.37.2 keeps a body of the whole `message-size-limit`
as an attachment (`util.Peek` marks the limit reached at 4,096 bytes):
ntfy.sh, ntfy.envs.net and ntfy.adminforge.de gave a body of 4,095 bytes
back as the message and kept one of 4,096 as `attachment.txt`, so the
bound is 4,095 and a message of 4,096 bytes fails before the send (C570).
The Approach's "the place after the mailboxes" stands, as podssh#237 asks;
the run has no probe of the way, so the caller sets
`Ends::upgrade_refused` from its own probe (C573). The premise did not
measure ntfy.sh: from a cloud session ntfy.sh, ntfy.envs.net and
ntfy.adminforge.de paired two ends on HTTP/2 and on HTTP/1.1, and
ntfy.jae.fi got 502 from the proxy (C569). ntfy publishes a static binary
with `checksums.txt`: CI runs release 2.37.2 on the loopback (C572).

## Done

2026-11-14, commit `4e5b956` (the code, the tests, the services, the step
of CI) and the commit of the plants and the record after it. Calls C568 to
C573.

- `pod_wormhole::ntfy` (`crates/pod-wormhole/src/ntfy.rs`,
  `crates/pod-wormhole/src/ntfy/link.rs`,
  `crates/pod-wormhole/src/ntfy/conn.rs`,
  `crates/pod-wormhole/src/ntfy/wire.rs`): `Servers { points, config,
  pace, h2 }`, `Pace { poll, waiting, retry }`, `Point::parse`, `opened`,
  `joined`, `MESSAGE_MOST` 4,095, `POLL` 1 s, `WAITING` 5 s, `RETRY` 5 s,
  `SINCE_SLACK` 60 s; `Topic::connect` proves the server (the stream's
  `open` line on HTTP/2, a poll's `200` on HTTP/1.1), `add`,
  `message(wait)`, `close`, `end`; `conn::dial`, `Conn` (`Http1`, `Http2`:
  `request`, `stream`, `end`); `wire::said`, `content`, `refusal`,
  `Lines`. `Place::Ntfy`. The events `wormhole.ntfy`. The topic is
  `mqtt::topic` of the AppID and the nameplate with its direction, never
  of the words.
- `pod-services`: the kind `ntfy-server`, `ntfy-sh` (rank 1),
  `envs-ntfy` (rank 2) and `adminforge-ntfy` (rank 3) with
  `message-bytes-most`, `requests-burst`, `request-refill-seconds`,
  `messages-per-day` and `account-required`; `pod-services-watch`:
  `kind_facts("ntfy-server")`.
- `pod-relay`: `Ends::ntfy`, `Ends::upgrade_refused`, `Ends::pace` and
  `Ends::ntfy()`; `opened` and `joined` try the mailboxes, the ntfy
  servers, the Nostr relays, the trackers, then the brokers, and the ntfy
  servers first where `upgrade_refused` is set, with the event
  `relay.code.ntfy` (`NTFY_WORDS`, after `no mailbox answered` or
  `UPGRADE_REFUSED`) before the first server.
- CI: `scripts/ci/ntfy-tests.sh` (a step of `ci.yml`) fetches ntfy 2.37.2
  by its SHA-256 and runs `crates/pod-wormhole/tests/ntfy.rs` against
  `ntfy serve` on 127.0.0.1; run here, it said "binwiederhier/ntfy: two
  ends paired". Rows L48 and L49 of `docs/live-calls.md`.
- Tests: `cargo test -p pod-wormhole --all-features`: 56 unit tests (6 new
  of `ntfy` and `ntfy::wire`: a point, the topic, the lines of ntfy's
  documents, the bound of a message, a refusal, the lines of the parts)
  and `crates/pod-wormhole/tests/ntfy.rs` (10) on the stand-in server of
  `crates/pod-wormhole/tests/harness/ntfy.rs` and
  `crates/pod-wormhole/tests/harness/ntfy_tls.rs`; `cargo test -p
  pod-relay --all-features --test pair_code`: 16 passed (2 new); `cargo
  test -p pod-services --all-features`: `crates/pod-services/tests/ntfy.rs`
  (1) and the others pass; `cargo test -p pod-services-watch
  --all-features` passes. `cargo test -p pod-wormhole --all-features
  --test live -- --ignored two_ends_meet_at_each_public_ntfy_server`: the
  three servers paired on HTTP/2 and on HTTP/1.1, from a cloud session
  through its HTTPS proxy. `live.yml` runs it each Thursday (row L48).
- Plants: each proven by hand (`git apply`, the run of its `RUN`, `git
  apply -R`). `wormhole-ntfy-topic-of-code` fails
  `two_ends_pair_by_polls_that_take_turns_with_the_posts_on_http1` (a
  request names the topic of the whole code);
  `wormhole-ntfy-connection-each-request` fails the same test (16
  connections, not 2); `wormhole-ntfy-size-unchecked` fails
  `a_message_of_4096_bytes_or_more_fails_before_the_send` (the stand-in
  refused the body with `413`); `wormhole-ntfy-since-start` fails
  `a_read_takes_what_came_since_its_start_then_since_the_last_id` (each
  poll asks since the start); `wormhole-ntfy-limit-fails` fails
  `a_post_answered_429_goes_again_after_the_wait` ("the taker: Refused");
  `wormhole-ntfy-polls-on-h2` fails
  `two_ends_pair_on_one_http2_connection_each_with_a_stream` (polls on
  HTTP/2); `relay-code-ntfy-after-relays` fails
  `where_no_mailbox_answers_the_ends_meet_at_an_ntfy_server_before_a_relay`
  (the ends met at the relay); `relay-code-ntfy-not-first` fails
  `where_the_upgrade_is_refused_the_ends_meet_at_an_ntfy_server_before_a_mailbox`
  (the ends met at the mailbox). The patches of
  `relay-code-nostr-after-brokers`, `relay-code-trackers-after-brokers`,
  `relay-t277-brokers-not-tried` and `relay-t277-nobody-stops-the-taker`
  are made again on the new `next_places`, `opened` and `joined`; each
  still fails its test.
  The fuzz target `ntfy_message` runs `ntfy::wire::said`, `Lines` and
  `refusal` (`fuzz/`, its seeds in fuzz/seeds/ntfy_message).
- In place: the test target `tracker` of `pod-wormhole` needs the feature
  `standin` (its harness takes the stand-in mailbox), and its
  `[[test]]` did not say so.
- podssh's part (PodUtils/podssh@T-496): set `Ends::ntfy` of its runs of a
  code to the endpoint `server` of each service of
  `pod_services::of_kind("ntfy-server")`, in the order of the ranks
  (`ntfy::Point::parse(&service.endpoint("server")?.url())`), or to the
  server that `PODSSH_PAIR_NTFY` names, and empty it for `no`; set
  `Ends::upgrade_refused` where its probe found that the way out refuses
  the WebSocket upgrade (as its Piping Server road learns it); show the
  event `relay.code.ntfy` as the line of the third party. A caller of
  `pod-wormhole` alone calls `ntfy::opened` and `ntfy::joined` with
  `Servers::new(points, config)`. ntfy.sh takes 250 messages a day from
  one address, and a pairing posts seven.

# T-541: `pod-link`: the dns and icmp carriers stop at the wrap of the sequence of the ARQ: `dgram_tls` never calls `rekeyed` (PodUtils/podnet#540)

**Source:** PodUtils/podnet#540 (COORD-074 of podnet, PodUtils/podnet@T-277).
**Category:** defect
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

podnet: "`pod_link::arq` asks for a rekey before its sequence wraps, and holds the frames at the wrap until the caller calls `rekeyed`. [...] The `dns` and `icmp` carriers run the ARQ inside the crate, under `dgram_tls`, and `dgram_tls` never calls `rekeyed`. So a `dns` or an `icmp` link stops at the wrap of one direction, after about 4 TiB of that direction. The sequence of the ARQ is not a nonce (TLS above it has its own), so the answer can be to let the wrap pass." The ask: answer the rekey in `dgram_tls`, and test that a `dns` or `icmp` link over the lossy shim crosses the wrap, with a small sequence space for the test.

## Premise

- Read: `crates/pod-link/src/arq/send.rs` (`rekey_due`, `rekeyed`, `can_send`: a new frame waits at the wrap until `wrap_ok`), `crates/pod-link/src/arq/stream.rs` (the task of `ArqStream`; only `ArqHandle::rekeyed` answers), `crates/pod-link/src/dgram_tls.rs` (the one user of `ArqStream` in the crate: the dns and icmp carriers). The udp carrier runs KCP, not this ARQ.
- `ArqSettings::start` sets the first sequence, so a test starts 64 frames before the wrap (as `tests/arq.rs`, `the_bytes_wait_at_the_wrap_until_the_rekey`, does).
- Not measured: a link of 4 TiB.

## Approach

1. `ArqStream::wrap_freely` (in the crate): the task of the ARQ calls `Arq::rekeyed` in its send loop at each ask, before it takes the datagrams to send. A task in `dgram_tls` that waits on `rekey_wanted` would not end with the ARQ, so the answer is in the task of the ARQ.
2. `dgram_tls::stream` calls it on each ARQ that it starts, with a comment that tells why (call C290).
3. Tests: a dns link and an icmp link over the `Lossy` shim (3 % loss, reorder, duplicates) start 64 frames before the wrap and move 128 KiB each way. The plant `link-dgram-wrap-waits` removes the call: the transfer stops at the wrap.

## Prove

```sh
cargo test -p pod-link --all-features --test dns_loss --test icmp bytes_cross_the_wrap_of_the_sequence_at_loss
sh scripts/plant-each.sh link-dgram-wrap-waits
```

## Done

2026-11-14, commit `59668bb`. `ArqStream::wrap_freely` (in the crate): the task of the ARQ calls `rekeyed` at each ask; `dgram_tls::stream` sets it on each ARQ of the dns and icmp carriers (call C290). `cargo test -p pod-link --all-features`: 183 passed, 2 ignored. `bytes_cross_the_wrap_of_the_sequence_at_loss` (`tests/dns_loss.rs`, `tests/icmp.rs`): the first sequence is 64 frames before the wrap, and 128 KiB cross each way at 3 % loss with reorder and duplicates. The plant `link-dgram-wrap-waits` (the call removed) fails each test after 60 s ("the transfer in time: Elapsed"). No other in-crate user of `ArqStream` exists; the udp carrier runs KCP.

# T-543: `pod-link`: the dns carrier and the TC bit: each answer keeps to the size of its query, and a cut answer does not stall the link (PodUtils/podnet#533)

**Source:** PodUtils/podnet#533 (COORD-071 of podnet, ask 2; PodUtils/podnet@T-278).
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podnet: "A resolver can truncate an answer (the flag `TC`); the client then asks again over TCP/53. The `dns` carrier of v0.40.0 has no such rule. If a resolver on the path truncates the TXT answers of the carrier, the link stalls. Please add the TC rule, or say that the carrier keeps each answer under the size that a resolver does not truncate (and the size)."

## Premise

- Read: `crates/pod-link/src/dns/server.rs` (`drain` fills each answer to `DnsSettings::answer`, 1232, with an OPT record also when the query has none), `crates/pod-link/src/dns/client.rs` (each query sends EDNS0 with `answer`; `txt::read` of a cut answer gives the whole records before the cut; a query that fails gives no datagram, and the ARQ sends again), `crates/pod-dns/src/txt.rs` (`question` gives no EDNS0 size).
- The listener did not read the size of the query: a resolver that sends no EDNS0 toward the server takes 512 bytes, and the listener sent up to 1232. That resolver then cuts the answer at each batch down.
- Not measured: a public resolver.

## Approach

1. `pod_dns::txt::asked_size(msg)`: the EDNS0 size of a query, at least 512, or `None` with no OPT record.
2. The listener keeps each answer to `min(asked or 512, answer)`, and puts an OPT record in an answer only when the query had one (call C291). One datagram always fits in 512 bytes.
3. The dialer takes the whole records of a cut answer and goes on (`dns::client::datagrams`); no query goes again over TCP/53 (call C292).
4. Tests: a stand-in resolver on the loopback that removes the OPT record of each query (answers of 512 bytes or less, none with an OPT record), that sends it whole (answers within 1232 and over 512), and that cuts each second answer with the TC bit (the link moves 32 KiB each way). A unit test of the dialer reads a cut answer. Plants: `dns-answer-past-the-query-size`, `dns-cut-answer-dropped`, `dns-txt-asked-size-under-512`.

## Prove

```sh
cargo test -p pod-dns --all-features --lib a_server_reads_the_size_that_a_query_takes
cargo test -p pod-link --all-features --test dns_resolver
cargo test -p pod-link --all-features --lib a_cut_answer_gives_its_whole_records
sh scripts/plant-each.sh dns-answer-past-the-query-size dns-cut-answer-dropped dns-txt-asked-size-under-512
```

## Done

2026-11-14, commit `8181d12`. `pod_dns::txt::asked_size` gives the EDNS0 size of a query (at least 512; none with no OPT record). The listener of the dns carrier keeps each answer to that size (512 with none) and to `DnsSettings::answer` (1232), with an OPT record only when the query had one (call C291). The dialer takes the whole records of a cut answer and goes on; no query goes again over TCP/53 (call C292). `cargo test -p pod-dns -p pod-link --all-features`: 264 passed, 6 ignored. `tests/dns_resolver.rs` (a stand-in resolver on the loopback): `a_query_with_no_edns_gets_an_answer_of_512_bytes_or_less`, `a_query_with_edns_gets_an_answer_within_its_size` (answers over 512 and within 1232), `a_cut_answer_does_not_stall_the_link` (each third answer cut, to no record or in its second record; 32 KiB each way); 40 runs in a loop passed. Unit tests: `a_cut_answer_gives_its_whole_records`, `a_server_reads_the_size_that_a_query_takes`. Plants: `dns-answer-past-the-query-size` fails the first test ("an answer of 1206 bytes to a query with no EDNS0"); `dns-cut-answer-dropped` fails `a_cut_answer_gives_its_whole_records` (left `[]`); `dns-txt-asked-size-under-512` fails `a_server_reads_the_size_that_a_query_takes` (left `Some(100)`). The integration test of the cut passes under `dns-cut-answer-dropped` (in 16 s, not 1 s): the ARQ recovers the dropped records, so the unit test is the check of that rule. The plant `dns-down-needs-the-txt` (T-493) moved to `dns::client::datagrams` and fails as before ("the handshake ... took more than 20s").

# T-542: `pod-link`: one typed marker for "the peer refused the key of this end" on each carrier (PodUtils/podnet#533)

**Source:** PodUtils/podnet#533 (COORD-071 of podnet, ask 1; PodUtils/podnet@T-273).
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podnet: "podnet exits 77 when the peer refuses the key of this end. On `quic`, the refusal reaches podnet only as a `noq::ConnectionError` (a close of the connection), so `src/link.rs` reads that close to find it; without that, podnet exited 69. Please give each carrier one typed marker for 'the peer refused the key of this end' (for example a variant of `pod_link::Error`, or an error kind that each carrier maps to), so that the caller does not read the errors of the transport."

## Premise

- Read: `crates/pod-link/src/carrier.rs` (`CarrierErrorKind::Refused` holds each refusal: the pin of this end, another user, a refusal of the way out, and the refusal of the key of this end), `crates/pod-link/src/pin.rs` (`answer_error` maps the alert of a refused certificate to `Refused`; `tls_error` maps a `pod_tls::peer::Refusal` to `Refused` and an alert to `Io`), `crates/pod-link/src/quic/io.rs` (`connection_error` maps a close with such an alert code to `Refused`), `crates/pod-link/src/poll/lane.rs` (a `Refused` lane is fatal).
- The tests named `the_listener_refuses_a_client_of_another_key_and_stays_open` and `a_refused_key_is_named_on_each_side` check `Refused` on the dialer: a caller cannot tell the two refusals apart by the kind.

## Approach

1. `CarrierErrorKind::KeyRefused`, of the class `Refused` (77) (call C293).
2. `pin::answer_error` and `pin::tls_error` map the alert of a refused certificate to it; `quic::io::connection_error` maps the close with that alert code to it. The poll carrier treats it as `Refused`.
3. Tests: the dialer whose key the listener refused gets `KeyRefused` on tcp, ws, h2, quic, udp, poll, dns and icmp; the listener whose key the dialer refused gets `KeyRefused` where the alert reaches it. Plants: `link-key-refused-untyped`, `link-key-refused-alert-untyped`, `link-quic-key-refused-untyped`.

## Prove

```sh
cargo test -p pod-link --all-features --test tcp_refused --test ws --test h2 --test quic --test udp --test poll --test dns --test icmp refuse
sh scripts/plant-each.sh link-key-refused-untyped link-key-refused-alert-untyped link-quic-key-refused-untyped
```

## Done

2026-11-14, commit `8895ca2`. `CarrierErrorKind::KeyRefused` (class `Refused`, 77) is the typed refusal of the key of this end (call C293); `pin::answer_error`, `pin::tls_error` and `quic::io::connection_error` map to it, and the poll carrier ends a lane at it. `cargo test -p pod-dns -p pod-link --all-features`: 264 passed, 6 ignored. The dialer whose key the listener refused gets `KeyRefused` with exit code 77 in `the_listener_refuses_a_client_of_another_key_and_stays_open` (tcp, ws, h2, quic, udp, poll, dns) and `a_refused_key_is_named_on_each_side` (icmp). The listener whose key the dialer refused gets `KeyRefused` in `the_dialer_refuses_a_listener_of_another_key_and_names_it` on tcp, h2, quic and poll. Plants: `link-key-refused-untyped` fails the dialer's check on tcp, ws, h2, udp, poll, dns and icmp ("right: KeyRefused"); `link-key-refused-alert-untyped` fails the listener's check on tcp; `link-quic-key-refused-untyped` fails both checks on quic. Not proven: the listener's side on dns and icmp (the listener reads the end of the stream before the alert: always on dns, at times on icmp), on udp (the handshake of the listener times out) and on ws (the accept gives no error of a handshake); the parent has the finding.

# T-557: `pod-link`: the listener of the dns, icmp, udp and ws carriers gets no `KeyRefused` when the dialer refuses its key

**Source:** T-542 (a finding of its work; PodUtils/podnet#533).
**Category:** defect
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

T-542 gives `CarrierErrorKind::KeyRefused` to the end whose key the peer refused. When the dialer refuses the key of the listener, the accept of the listener gets it on tcp, h2, quic and poll only:

- dns: the listener always reads the end of the stream before the TLS alert (`Io(UnexpectedEof)`);
- icmp: the same, at times;
- udp: the handshake of the listener ends at its time limit (10 s);
- ws: `accept_key` gives no error for the failed handshake in 30 s.

## Premise

- Read: the report of T-542; `crates/pod-link/src/dgram_tls.rs`, `src/arq/stream.rs`, `src/udp/stream.rs`, `src/ws.rs`.
- Not measured: whether the alert of the dialer reaches `ArqStream::poll_write`, or KCP, before the stream closes.

## Approach

1. Trace whether the alert reaches `ArqStream::poll_write` when `dgram_tls::connect` fails, and whether the write can return pending; flush it before the end.
2. udp: flush KCP before the stream drops.
3. ws: give a failed handshake out through `accept_key`.
4. Then add the checks of the listener side for the four carriers, with a plant.

## Prove

```sh
cargo test -p pod-link --all-features
```

## Correction

2026-11-15: the run of `plants.yml` 43716332294 at `3ee1988` (#177), shard 0, missed the plant `link-arq-vectored-first-only` of this entry: "exit 0, and the output has no match of: right: KeyRefused". The test `the_dialer_refuses_a_listener_of_another_key_and_names_it` of `tests/dns.rs` passed with the plant applied on the runner; the sharded runs of `98017c3` and `7119770` caught it. The check depends on a timing of the runner: the entry stays open with this as its first step (the test must fail with the plant on each run).

2026-11-15, commit fc85198 (call C672): the cause is measured. tokio-rustls flushes the queued records of the dialer after each record that it reads, and makes its last write after the failed handshake once, as one vectored write. When the `ServerHello` comes in a read of its own, the change-cipher-spec (6 bytes) goes out alone before the certificate arrives, and the last write holds the alert (24 bytes) alone: the plant writes it, and the test passes. When the flight of the listener comes in one read, the last write holds both, and the plant drops the alert. With the plant, beside 4 busy loops, the test passed 28 of 30 runs on 1 CPU and 17 of 30 on 2 CPUs; with no load, it passed 0 of 30. The new test `one_vectored_write_takes_each_buffer` of `tests/arq.rs` (paused clock) and of `tests/udp_loss.rs` (KCP) writes three buffers in one call; the plants `link-arq-vectored-first-only` and `link-kcp-vectored-first-only` run it with the check of the listener (`--no-fail-fast`). `plant-each.sh link-arq-vectored-first-only` caught the plant 20 of 20 runs beside 4 busy loops (in the last log the check of the listener passed and the new test failed); `link-kcp-vectored-first-only` 5 of 5. With no plant, each new test and the check of the listener passed 30 of 30 under the same load. The entry stays done: its Problem stays solved.

## Done

2026-11-14, commit 3305394. The listener of each carrier that pins keys
now gets `KeyRefused` (77) when the dialer refused its key. dns, icmp
and udp: the alert of the dialer was lost, not late: tokio-rustls makes
one last vectored write after a failed handshake, and `ArqStream` and
`KcpStream` wrote its first buffer only (the change-cipher-spec, 6
bytes). Both now write each buffer of a vectored write (calls C391,
C392). ws: the new `pod_ws::serve::serve_reporting` gives each failed
TLS handshake to the caller, and the ws listener gives the refusal of
its key as an error of one accept (call C390).

`cargo test -p pod-link -p pod-ws --all-features`: pod-link 188 passed,
2 ignored; pod-ws passes. `the_dialer_refuses_a_listener_of_another_key_and_names_it`
of tests/dns.rs, tests/icmp.rs (new), tests/udp.rs and tests/ws.rs
checks the listener; each fails on the old code and passed 20 runs of
20. Plants `link-arq-vectored-first-only`, `link-kcp-vectored-first-only`
("right: KeyRefused"), `link-ws-key-refused-skipped` and
`ws-serve-tls-failure-unreported` ("the refusal in time"): each fails
its test.

# T-558: `pod-hold`: the rule of raw sockets leaves `AF_PACKET` open

**Source:** T-544 (a finding of its work; PodUtils/podnet#538).
**Category:** defect
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

T-544 refuses `socket(AF_INET or AF_INET6, SOCK_RAW, ...)` and ping sockets in a held child. A payload with `CAP_NET_RAW` can still send frames out of the host through `socket(AF_PACKET, SOCK_RAW or SOCK_DGRAM, ...)`.

## Premise

- Read: the report of T-544; `crates/pod-hold/src/filter.rs` (`deny_raw`).

## Approach

Add `AF_PACKET` (each type) to the rule, under the opt-in of T-544 or a separate one. Ask podnet by a COORD issue whether `--allow-raw` covers it. A case of the BPF unit test, a test of a held child (`EPERM` with no notification), and a plant.

## Prove

```sh
cargo test -p pod-hold --all-features
sh scripts/plant-each.sh hold-t558-packet-let-through hold-t558-inet-sock-packet-let-through
```

## Done

2026-11-14, commit c498ab0 (with T-574). The raw part of the socket rules
of `Calls::sockets()` has `AF_PACKET` of each type, under
`Calls::allow_raw()` (C365): the kernel answers `EPERM`, and the
supervisor gets no notification. Two findings are in the same rule
(C367): the kernel makes a packet socket of `socket(AF_INET, SOCK_PACKET,
...)` (as root on the test host it opened under the opt-in), and `AF_XDP`
sends frames to a device; both get `EPERM` (`AF_INET6` with `SOCK_PACKET`
too). No COORD issue to podnet: the opt-in gives the call to the
supervisor (C366), so podnet's handler refuses `AF_PACKET` under
`--allow-raw icmp`; the parent tells podnet in its answer to
PodUtils/podnet#550.

`cargo test -p pod-hold --all-features`: 61 unit tests passed; 5 tests of
`tests/hold.rs` and 1 of `tests/veth.rs` passed, with no `SKIP:` line.
New: `filter::tests::each_part_of_the_socket_rules_has_its_own_opt_in`
(a BPF run of `AF_PACKET` of three types, `AF_XDP`, `SOCK_PACKET` of
both families, each with each opt-in) and, in `tests/hold.rs`,
`each_part_of_the_socket_rules_has_its_own_opt_in` (a held child:
`AF_PACKET` with `SOCK_RAW` and `SOCK_DGRAM` and `AF_INET` with
`SOCK_PACKET` get `EPERM` with no notification, and reach the supervisor
with `allow_raw`). Plants, each caught by hand:
`hold-t558-packet-let-through` (the held test) and
`hold-t558-inet-sock-packet-let-through` (the BPF test).
`plant-each.sh --apply`: 0 stale. `check-windows.sh` and
`check-macos.sh`: 0 faults.

# T-559: `pod-relay`: the client of the HTTP road's one-POST form (PodUtils/podssh#239)

**Source:** PodUtils/podssh#239 (COORD-062 of podssh, for PodUtils/podssh@T-622).
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podssh's relay role serves a third form of the HTTP road
(crates/podssh-relay/src/serve/http_road/one.rs of podssh; podssh's
docs/relay.md, "The relay role's HTTP road"):
`POST /v1/http/one/HOST/PORT[?family=4|6]`, HTTP/1.1,
`Transfer-Encoding: chunked`, the forward token in `X-Relay-Token`, the
open's checks and refusals (`X-Refuse`). `200` at once, its answer in
chunks, whose first bytes are the mark `podssh-http-one 1\n`; then each
chunk up goes to the target as it comes, and the target's bytes come down
in chunks as they come, on that one connection. The body's last chunk is
TCP's half-close to the target; the target's end is the answer's last
chunk; a limit met ends the connection with no last chunk (a cut).
`/relays.json` names it in `limits.http`: `forms` adds `one`.

The ask: the client of the form in `pod-relay`'s `http_road`. (1) The
form first where the HTTP road is taken and the host's `limits.http.forms`
names `one` (or the host is not known yet), on one connection. (2) A bound
of 2 s on the mark: no mark ends that request and takes the split form,
and the result is kept for the egress for the rest of the run. (3) HTTP/2
where ALPN gives `h2`, else HTTP/1.1 with chunked bodies both ways.
(4) `Form::One`: `PODSSH_HTTP_ROAD=one` asks for it alone; `auto` tries
it first; `split` and `serial` leave it out. (5) The form's name where
podssh says the form that carried (`doctor`'s line `forward`).

## Premise

- Read: PodUtils/podssh#239; podssh's docs/relay.md (the HTTP road),
  crates/podssh-relay/src/serve/http_road/one.rs and
  crates/podssh-relay/tests/http_road_one.rs; podssh's
  crates/podssh-cli/src/http_road.rs and its `doctor`, which print
  `Opened::form.name()`; `crates/pod-relay/src/http_road/`.
- The role refuses the form on other than HTTP/1.1 (`411`), and sends the
  mark after its dial of the target, before it reads the body.
- The road's forms offer no ALPN (`Road::tls` passes none), so no form
  of the road speaks HTTP/2 now.
- Not measured: the form through a real proxy; podssh measured 20 MiB each
  way with curl at its role.

## Approach

`http_road::one`: the request's head with `Transfer-Encoding: chunked`,
then a bound of 2 s on the answer's head and mark. Each write of the
caller goes up as a chunk; the caller's end is the last chunk; the
answer's last chunk is the target's end, and an answer with none is a
failure. `Form::One`; `client::open` with no form asked takes it first
where `Road::forms` names it or is not known and `Road::learned` does not
hold the host; else the split form, then the serial one. Tests against the
stand-in role of `tests/`, with a hop in memory that streams and one that
holds a body in chunks to its end.

## Prove

```sh
cargo test -p pod-relay --test http_road_one
cargo test -p pod-relay --all-features
sh scripts/plant-each.sh relay-t559-mark-unbounded relay-t559-held-forgotten relay-t559-no-last-chunk relay-t559-cut-is-an-end
```

## Done

2026-11-14, commit 0ea3c5e (the client and its tests) and the next (the
bound of the test, the plants, the record). `http_road::one`: the request
`POST /v1/http/one/HOST/PORT[?family]` of HTTP/1.1 with the token in
`X-Relay-Token` and `Transfer-Encoding: chunked`, then `MARK_WAIT` (2 s)
on the answer's head and mark (C310). Each write of the caller goes up as
a chunk; the caller's end is the last chunk. The answer's last chunk is
the target's end, `End::Closed { 1000, "target closed" }`; an answer with
no last chunk is `End::Failed` ("the relay cut the answer"). A refusal of
the role keeps its status and words (`ConnectError::Refused`), as the
open of the split form. `Form::One` (`name` "one", the name that podssh's
`doctor` and `ssh` print from `Opened::form`), `Form::parse`, `Forms`.
`client::open`: no form asked takes the one-POST form first where
`Road::forms` names `one` or is none and `Road::learned` does not hold the
host; no mark, a host that serves no such form, or an answer with no mark
takes the split form, then the serial one, and keeps the host in
`Road::learned`; `Road::in_run(&ctx)` makes that the run's
(`Context::http_road_learned`, C312); `Road::with_forms` takes
`pool::Limits::http_forms`, which `parse_limits` now reads from
`limits.http.forms` and the pool's cache keeps. `Form::One` asked alone
fails there with `ConnectError::Http` ("the one-POST form of the HTTP
road did not go: no mark of the relay in 2 s: a hop on the way holds the
request's body to its end"); `Form::Split` and `Form::Serial` never try
it. HTTP/2: the form offers no ALPN, as the other forms, so it is
HTTP/1.1 alone (C311): podssh's role serves the form on HTTP/1.1 alone,
and an offer of `h2` would never carry it there. T-478: the one-POST form
has no such gap: a caller that is gone ends the session and drops the
connection, and the role ends the session as its body is cut (C313).
`tests/http_road_one.rs`, 5 passed, on a paused clock against the
stand-in role of `tests/http_road_stand_in/` (now with the form, `one.rs`,
and a hop in memory, `hop.rs`, that streams or holds a body in chunks to
its end): 4 MiB each way with equal digests, straight and through the
hop that streams, on one request with the token; through the holding hop
the split form after 2 s (within 3 s), 1 MiB each way, and the next open
of the run at once with one try of the form in all; a cut answer fails
after the 256 KiB before the cut; `Form::One` alone fails with words
that name the mark and the hop; a role with no such form (`404`), a
host whose forms do not name it, and the forms `split` and `serial`
asked send no request of the form. Unit tests: the forms and their
names, the route, the chunk, the memory, `limits.http.forms`. The plants,
each proved by hand: `relay-t559-mark-unbounded` (a bound of 20 s; the
fallback test fails: "took the split form after 20s"),
`relay-t559-held-forgotten` (the memory not kept; "the run keeps the
held form"), `relay-t559-no-last-chunk` (no last chunk up; the transfer
test runs out of its bound), `relay-t559-cut-is-an-end` (a cut read as
the end; the cut test fails). `cargo test -p pod-relay --all-features`:
no failure.

# T-545: `pod-wormhole`: `Wormhole::derive_key`, magic-wormhole's derived key, for the transit key of podnet's transfer (PodUtils/podnet#535)

**Source:** PodUtils/podnet#535 (COORD-072 of podnet), ask 1; it blocks PodUtils/podnet@T-103 and the file part of PodUtils/podnet@T-106.
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

magic-wormhole derives the transit key from the wormhole key:
`derive_key(purpose, length)` with the purpose `<appid>/transit-key`. The
`Wormhole` of `pod-wormhole` gives no such call, and the key is private.
podnet now sends a random transit key inside its sealed `transit` message:
this works podnet to podnet, but a Python `wormhole` client cannot read it,
so a file or a directory does not move between podnet and Python. podnet
asks for `Wormhole::derive_key(purpose: &str, len: usize) -> Vec<u8>`
(HKDF-SHA256 of the shared key, as magic-wormhole's `derive_key`).

## Premise

- Read 2026-11-14, magic-wormhole at
  `657bd5d51c67db2109219ff57b7d997afa57261f` (its default branch):
  magic-wormhole/magic-wormhole:src/wormhole/_key.py,
  `derive_key(key, purpose, length=SecretBox.KEY_SIZE)` returns
  `HKDF(key, length, CTXinfo=purpose)`;
  magic-wormhole/magic-wormhole:src/wormhole/util.py, `HKDF(skm, outlen,
  salt=None, CTXinfo=b"")` is RFC 5869 HKDF with SHA-256: no salt, the
  purpose as the info. magic-wormhole/magic-wormhole:src/wormhole/wormhole.py,
  `derive_key(self, purpose, length)` takes a `str`, raises `NoKeyError`
  with no key, and gives `derive_key(self._key, to_bytes(purpose), length)`;
  `to_bytes` is the UTF-8 of the NFC form.
  magic-wormhole/magic-wormhole:src/wormhole/test/test_keys.py,
  `test_derive_key`: the key `588ba9ee...131ef1` gives
  `835b5df8...ee1b2a` for `purpose1` (32 bytes) and `f2238e84315b47eb6279`
  for `purpose2` (10 bytes).
- The release 0.24.0 of the Python library (`scripts/ci/wormhole-servers.txt`)
  has the same `derive_key`, and gives the same two vectors here.
- `crates/pod-wormhole/src/wormhole.rs` makes the key of each phase with
  HKDF-SHA256 and no salt already (`phase_key`).

## Approach

`wormhole::derive_key(&key, purpose, len)`: HKDF-SHA256, no salt, the
purpose as the info, `len` bytes in a `Zeroizing<Vec<u8>>`; `None` past
`DERIVED_MOST` (255 blocks of 32, the bound of HKDF-SHA256).
`Wormhole::derive_key(&self, purpose: &str, len)` over the agreed key:
`Failed::Broken` before `agree`, `Failed::Mine` past the bound (call C294).
A unit test with the vectors of `test_keys.py`; the test of two ends over
each mailbox derives one key at both ends and another for another purpose;
the peer of the Python library prints its transit key
(`APPID/transit-key`, 32 bytes), and the two tests of the library compare
it with this crate's. The plant `wormhole-derived-key-salted` gives the
purpose as the salt.

## Prove

```sh
cargo test -p pod-wormhole --all-features
sh scripts/plant-each.sh wormhole-derived-key-salted
```

## Done

2026-11-14, commit 8924ae3.
`Wormhole::derive_key(purpose, len)` gives
`Result<Zeroizing<Vec<u8>>, Failed>`: `Failed::Broken("no key was agreed")`
before `agree`, `Failed::Mine` past `DERIVED_MOST` (8160); the pure
`wormhole::derive_key(&key, purpose, len)` gives `None` past it (C294).
`cargo test -p pod-wormhole --all-features` with the Python packages of
`scripts/ci/wormhole-servers.txt` and `POD_WORMHOLE_TEST_SERVER=required`:
75 passed, 3 ignored (the live ones). New: the unit test
`a_derived_key_is_magic_wormholes_by_purpose_and_length` (the two vectors
of magic-wormhole's `test_keys.py`; 0 bytes; the first 32 of 8160 bytes;
`None` at 8161); `two_ends_of_one_code_agree_a_key_and_move_a_message_each_way`
derives one transit key at both ends and another for another purpose, on
the stand-in and on magic-wormhole's mailbox server; each test that uses
`two` holds `Failed::Broken` before `agree`; the peer of magic-wormhole
0.24.0 prints its `derive_key("APPID/transit-key", 32)`, and
`a_peer_of_magic_wormholes_python_library_takes_this_crate_s_code` and
`this_crate_takes_a_code_that_magic_wormholes_python_library_gives` find
it equal to this crate's. The plant `wormhole-derived-key-salted` fails
`a_derived_key_is_magic_wormholes_by_purpose_and_length` (checked by hand:
FAILED); `sh scripts/plant-each.sh --apply`: 0 stale.

# T-547: `pod-wormhole`: the count of words of a new code in `Config`, as magic-wormhole's `--code-length` (PodUtils/podnet#535)

**Source:** PodUtils/podnet#535 (COORD-072 of podnet), ask 3.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

magic-wormhole takes `--code-length N`. `meet::Ends` and `Config` fix the
code at 3 words. podnet asks for the count in `Ends` or `Config`, so that
podnet can give `--words N`.

## Premise

- Read 2026-11-14, magic-wormhole at
  `657bd5d51c67db2109219ff57b7d997afa57261f`:
  magic-wormhole/magic-wormhole:src/wormhole/cli/cli.py, `--code-length`
  (`-c`, NUMWORDS) defaults to 2 for `send` and `receive --allocate` and
  is `int(...)` with no bound; magic-wormhole/magic-wormhole:src/wormhole/_wordlist.py,
  `choose_words(length)` draws `length` words, odd first, with no bound. So
  magic-wormhole takes any count; 0 gives a code with no secret.
- `crates/pod-wormhole/src/phrase.rs`: `Code::make` draws `WORDS` (3)
  words; `Code::parse` takes 1 to `WORDS_MOST` (8) words. `meet::opened`
  and `mqtt::opened` make the code.

## Approach

`Config::words` (default `WORDS`, 3: the default of the crate stays).
`Code::make(nameplate, words, entropy)` takes the count;
`phrase::check_words` holds it to `WORDS_LEAST` (2) to `WORDS_MOST` (8)
and refuses others with `Failed::Mine` (call C295). `meet::opened` checks
the count before it dials a mailbox, and `mqtt::opened` before a broker.
A taker reads the count from the code. Tests: a unit test of each count
and of the bounds; a code of 5 words over each mailbox, and of 6 words over
each broker, meets and confirms; a count out of bounds fails before a
dial. The plant `wormhole-code-words-fixed` makes `meet::opened` draw
`WORDS` words.

## Prove

```sh
cargo test -p pod-wormhole --all-features
sh scripts/plant-each.sh wormhole-code-words-fixed
```

## Done

2026-11-14, commit 8e70d5f. `Config::words`
(default `WORDS`, 3); `Code::make(nameplate, words, entropy)`;
`phrase::check_words` (2 to 8, else `Failed::Mine`); `meet::opened` and
`mqtt::opened` refuse a count out of bounds before a dial (C295).
`cargo test -p pod-wormhole --all-features` with magic-wormhole's mailbox
server and `POD_WORMHOLE_TEST_SERVER=required`: 78 passed, 3 ignored. New:
`a_new_code_has_the_words_asked_for_within_its_bounds` (each count from 2
to 8 is made and parsed back; 0, 1 and 9 are refused);
`a_code_of_the_words_asked_for_meets_and_a_count_out_of_bounds_reaches_no_mailbox`
(5 words over the stand-in and magic-wormhole's server; 1 and 9 fail with
`Failed::Mine` at a URL where nothing listens);
`a_code_of_the_words_asked_for_pairs_at_a_broker_and_a_count_out_of_bounds_reaches_none`
(6 words over the stand-in broker). The plant `wormhole-code-words-fixed`
fails the test of the mailboxes (checked by hand: "left: 3, right: 5").
`examples/static-probe/src/main.rs` calls `Code::make` with `WORDS` (the
commit after 8c2d5ad; 8e70d5f left it with the old signature).

# T-546: `pod-wormhole`: the stand-in mailbox behind the feature `standin`, for the tests of a consumer (PodUtils/podnet#535)

**Source:** PodUtils/podnet#535 (COORD-072 of podnet), ask 2.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The stand-in mailbox of `pod-wormhole` is private to its tests, and
`pod-testkit` has none. podnet wrote its own
(podnet's tests/transfer_harness/mod.rs). podnet asks for one behind a
feature (for example `standin`), as `pod-wg` and `pod-devtunnel` do.

## Premise

- Read: `crates/pod-devtunnel/Cargo.toml` and `crates/pod-wg/Cargo.toml`
  (a feature `standin`, a module `src/standin.rs` under
  `#[cfg(feature = "standin")]`, the tests that need it under
  `required-features`); `scripts/check-probe.py` (each feature is in a
  static probe or in ALLOWED: the probe turns on `standin` of `pod-wg` and
  `pod-devtunnel`, as it runs a client against them).
- The stand-in of the tests of this crate (tests/harness/stand_in.rs at
  `8e70d5f`) uses `pod-proxy` (the head of the upgrade), the server half
  of `pod-ws`, and tokio's `net`, `io-util`, `sync` and `rt`. Each test
  file of the crate includes the harness. `pod-relay` keeps a copy of the
  same file (`crates/pod-relay/tests/mailbox_harness/`).

## Approach

`src/standin.rs` under the feature `standin` (the optional `pod-proxy` and
tokio's features): `start()` and `scripted(script)` give an `io::Result`,
and no call panics (call C296). The harness of the tests re-exports it with
wrappers that fail the test; each test file has `required-features =
["standin"]`, and the plants and `scripts/ci/mosquitto-tests.sh` run with
`--features standin`. `pod-relay`'s harness takes it as a dev-dependency in
place of its copy. `scripts/check-probe.py`: ALLOWED, as no binary needs it.
The plant `wormhole-standin-not-crowded` lets a third side claim a
nameplate on the stand-in.

## Prove

```sh
cargo test -p pod-wormhole --features standin
cargo test -p pod-relay --all-features --test pair_code
python3 scripts/check-probe.py
sh scripts/plant-each.sh wormhole-standin-not-crowded
```

## Done

2026-11-14, commit dc122c6.
`pod_wormhole::standin` (feature `standin`): `start()`, `Stand` (`url`,
`moods`, `tamper`, `nameplates`), `scripted(script)`, `welcome()`; no
`unwrap` (C296). `tests/harness/` re-exports it; the six test files have
`required-features = ["standin"]`; the plants of the group `wormhole` and
`scripts/ci/mosquitto-tests.sh` run with `--features standin`.
`pod-relay`'s `tests/mailbox_harness/` takes it as a dev-dependency, and its
copy is gone. `python3 scripts/check-probe.py --no-tree`: 0 faults (the
feature is in ALLOWED). `cargo test -p pod-wormhole --all-features` with
magic-wormhole's mailbox server: 78 passed, 3 ignored; `cargo test -p
pod-wormhole` with no feature: the 37 unit tests;
`cargo test -p pod-relay --all-features --test pair_code`: 10 passed. The
plant `wormhole-standin-not-crowded` fails
`a_third_end_is_refused_while_two_hold_the_code` on the stand-in (checked
by hand: "a third end of the code was not refused").

# T-548: `pod-services`: magic-wormhole's TCP transit relay as a service of the kind `transit`, with the watch `transit-relay` (PodUtils/podnet#535)

**Source:** PodUtils/podnet#535 (COORD-072 of podnet), ask 4.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`pod-services` lists the WebSocket transit relay of Least Authority only.
magic-wormhole's default transit relay is
`tcp:transit.magic-wormhole.io:4001`. podnet asks for it as a `transit`
service (with its measurement), so that podnet adds no host of its own
(podnet's ruling: Services crate).

## Premise

- Read 2026-11-14: magic-wormhole/magic-wormhole:src/wormhole/cli/public_relay.py
  at `657bd5d51c67db2109219ff57b7d997afa57261f`, `TRANSIT_RELAY =
  "tcp:transit.magic-wormhole.io:4001"`;
  magic-wormhole/magic-wormhole-transit-relay:src/wormhole_transit_relay/transit_server.py
  at `6d50be62c118df74d8f0a042662fbc407dfd5849`: each end sends
  `please relay TOKEN for side SIDE\n` (TOKEN 64 hex digits, SIDE 16); the
  relay answers `ok\n` to both when the two ends of a token come, then
  carries the bytes of each to the other.
- `crates/pod-services/services/leastauthority-transit.toml`: the kind
  `transit`, the endpoint `relay` (`wss`), the watch `websocket-upgrade`;
  the kind has no table of facts. No watch kind speaks the handshake of a
  transit relay.
- Not measured: the relay from a GitHub runner (the egress of this
  session is an HTTPS proxy; the first run of `services.yml` measures it).
  The watch was run against magic-wormhole-transit-relay 0.5.0 on the
  loopback.

## Approach

`services/magic-wormhole-transit.toml`: the endpoint `relay` (`tcp`,
`transit.magic-wormhole.io`, 4001), no rank, the watches `github-file`
(`public_relay.py` names the host) and `transit-relay`. The new watch kind
`transit-relay` is both ends: two connections of one random token each get
`ok`, and a line of the run from the first comes out of the second. The
kind `transit` gets a table in `schema::kind_facts`: the endpoint `relay`,
no fact; the scheme of the endpoint tells TCP from WebSocket (call C297).
Tests against a transit relay of the test, against bad relays, and against
magic-wormhole's own relay in `scripts/ci/mailbox-tests.sh`. The plant
`services-transit-bytes-not-compared`. A row L40 in `docs/live-calls.md`.

## Prove

```sh
cargo test -p pod-services -p pod-services-watch --all-features
sh scripts/plant-each.sh services-transit-bytes-not-compared
cargo run -q -p pod-services-watch -- --service magic-wormhole-transit
```

## Done

2026-11-14, commit 8c2d5ad. The service
`magic-wormhole-transit` (kind `transit`; `relay`: `tcp`,
`transit.magic-wormhole.io`, 4001; no rank), the watch kind
`transit-relay` (`crates/pod-services-watch/src/transit.rs`), the table of
the kind `transit` (C297), the row L40 of `docs/live-calls.md`.
`cargo test -p pod-services -p pod-services-watch --all-features`: 97 and
73 passed. `tests/watches/transit.rs`:
`transit_relay_pairs_two_ends_and_carries_the_bytes_of_one_to_the_other`
(a relay of the test holds; relays that carry nothing, carry other bytes
or answer `impatient`, and an endpoint of `wss`, fail with their words);
`transit_relay_holds_against_magic_wormholes_own_relay` passed against
magic-wormhole-transit-relay 0.5.0 on the loopback
(`POD_WORMHOLE_TEST_SERVER=required`). The plant
`services-transit-bytes-not-compared` fails the first test (checked by
hand). `cargo run -q -p pod-services-watch -- --service
magic-wormhole-transit` here: `github-file` holds (`public_relay.py` holds
the host); `transit-relay` failed with a reset after the dial, as the
egress proxy of this session takes no TCP to port 4001. The first run of
`services.yml` on a GitHub runner is the live measurement.

# T-560: `pod-relay`: the one-POST form of the HTTP road over HTTP/2

**Source:** T-559 (a finding of its work; item 3 of PodUtils/podssh#239).
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

The client of the one-POST form (T-559) runs over HTTP/1.1 only. podssh asks for HTTP/2 where ALPN gives `h2`: one stream, on one connection.

## Premise

- Read: the report of T-559; calls C310 to C313; `crates/pod-relay/src/http_road/one.rs`.
- podssh's role serves the form over HTTP/2 on one stream since its commit `44bdf8a` (crates/podssh-relay/src/serve/http2/one.rs; its test tests/relay_serve_h2.rs, 4 MiB each way; PodUtils/podssh#241).

## Approach

Offer `h2,http/1.1` on the connection of the one form only. That needs a dial that reports the result of ALPN (`Dial` gives a `BoxIo` today). Then one stream of `h2` with a request body that streams, as `pod-services-watch/src/xhttp.rs` uses the crate `h2`. A test against a stand-in of TLS with `h2`.

## Prove

```sh
cargo test -p pod-relay --all-features --test http_road_one
```

## Correction

2026-11-15: `nightly.yml` run 43699451813 at `98017c3`: under qemu-user the split form comes after 4.82 s (riscv64) and 4.06 s (ppc64le), against the bound of 4 s of `with_no_mark_on_the_stream_the_client_takes_the_split_form_within_the_bound`. T-682 scales the part of the bound that is not the timer.

## Done

2026-11-14, commit bfeacab. The connection of the one-POST form offers
`h2` and `http/1.1` by ALPN: `Road::dial_alpn: Option<DialAlpn>` beside
`Road::dial`, `conn::{DialAlpn, DialingAlpn, Dialed, ALPN_OFFER, H2}`;
`Road::tls` sets it (call C358, which replaces C311). Where the host
chooses `h2`, the form is one stream of HTTP/2 (`http_road/one_h2.rs`),
framed as podssh's role serves it at `44bdf8a` (call C359): DATA each
way, the request's `END_STREAM` the caller's end, the answer's the
target's end (`1000 target closed`), a reset a cut, `MARK_WAIT` on the
head and the mark. Where the host chooses `http/1.1` or nothing, the form
goes on the same connection over HTTP/1.1; a reset with
`HTTP_1_1_REQUIRED` sends the form once more on a connection with no
ALPN. `tests/http_road_one_h2.rs`, 6 passed, from `Road::tls` to a
stand-in of TLS on the loopback whose HTTP/2 is h2's own server: 4 MiB
each way on one stream with equal digests (one connection of `h2`, one
stream, the token, no `transfer-encoding`); a reset after 256 KiB is a
cut; a host that chooses `http/1.1` carries the form on the same
connection; a role that asks for HTTP/1.1 gets the form on a second
connection; a `403` keeps its words; with no mark the split form after 2
s to 4 s, and the host learned. Looped 15 times with no failure. The
plants, each proved by hand: `relay-t560-no-h2-offer` (the stream test
fails: ALPN `None`), `relay-t560-reset-is-an-end` (the cut test fails:
`Closed`), `relay-t560-http1-not-asked-again` (the test of the role that
asks for HTTP/1.1 fails: `Split`). The plant `relay-t559-mark-unbounded`
takes the new context of `one.rs`. `cargo test -p pod-relay
--all-features`: 422 passed. `cargo clippy -p pod-relay --all-targets
--all-features -- -D warnings`, the check of Windows and of macOS:
clean.


# T-561: `pod-wg`: the preshared key of a handshake (PodUtils/podnet#547)

**Source:** PodUtils/podnet#547 (COORD-076 of podnet), ask 2; it blocks PodUtils/podnet@T-337.
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

podnet asks: "`Keys` takes an optional preshared key, and the initiation
and `Initiation::finish` mix it in (the psk step of Noise IKpsk2).
podnet's `wg:FILE` refuses `PresharedKey` until then." The initiator of
`pod-wg` always uses the zero key, so a peer with a `PresharedKey` refuses
each handshake.

## Premise

- Read: `crates/pod-wg/src/handshake.rs` (`Keys`, `initiate_with`,
  `Initiation::finish`: `kdf::<3>` over 32 zero bytes after the DH steps of
  the response), `crates/pod-wg/src/standin.rs` (the same zero key).
- The WireGuard paper, section 5.4.3: the response computes
  `(C, τ, κ) = Kdf3(C, Q)` and `H = Hash(H || τ)` after `Kdf1(C, DH(...))`
  of its ephemeral key with both keys of the initiator; `Q` is zero when
  the peers have no preshared key. The initiation (section 5.4.2) has no
  step with `Q`.
- No published WireGuard handshake with a preshared key on fixed keys was
  found: the tests of wireguard-go (`noise_test.go`) and of boringtun make
  random keys, and the selftests of the kernel module have no handshake.

## Approach

`Keys::preshared: Option<[u8; 32]>` (`None`: the zero key);
`Initiation::finish` mixes it in at the psk step. The stand-in gets
`respond_preshared` and `start_preshared`. Tests: both sides with one key;
one side only and two keys fail with `Error::ResponseNotAuthentic`; a
vector from an independent program (the Noise steps on Python's hashlib,
hmac and `cryptography`), with the zero key too; the stand-in on the
loopback. The plant `wg-psk-step-skipped`. Call C314.

## Prove

```sh
cargo test -p pod-wg --all-features
sh scripts/plant-each.sh wg-psk-step-skipped
```

## Done

2026-11-14, commit 2caef52. `Keys::preshared: Option<[u8; 32]>` (`None`:
the zero key); `Initiation::finish` mixes it in at the psk step of the
response; the stand-in's `respond_preshared` and `start_preshared` (call
C314). `cargo test -p pod-wg --all-features`: 22 passed, with
`handshake::tests::a_handshake_with_a_preshared_key` (both sides pass; one
side only, either side, and two different keys fail as
`Error::ResponseNotAuthentic`; `Debug` of `Keys` is `Keys { .. }`),
`handshake::tests::the_vector_of_a_handshake_with_a_preshared_key` (the
initiation, the response and both session keys on fixed values, as an
independent Python program of the Noise steps computed them; the empty
payload and the send key of the zero key from the same program) and
`standin::tests::the_standin_takes_a_preshared_key` (the loopback). The
plant `wg-psk-step-skipped` (the zero key in `finish`) fails both
handshake tests (checked by hand: 2 failed). `cargo clippy -p pod-wg -p
pod-warp --all-targets --all-features -- -D warnings`: clean.

# T-562: `pod-wg`: a responder sans I/O for a list of peers (PodUtils/podnet#547)

**Source:** PodUtils/podnet#547 (COORD-076 of podnet), ask 1; it blocks PodUtils/podnet@T-125.
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podnet asks for a responder with no I/O for `podnet wg serve`: "a private
key and a list of peer public keys, each with an optional preshared key;
read an initiation: check mac1, decrypt the static key and find it in the
list (an unknown key gets no answer), and refuse a timestamp that is not
newer than the last one of that peer; under load (a flag of the caller): a
cookie reply, from a secret that changes each 120 s; transport messages of
several peers at once, found by the receiver index; the timers of the
responder: refuse after 180 s, keepalive, no rekey that the responder
starts; a `Debug` that shows no key, and a fuzz target of the read of an
initiation." The only responder of `pod-wg` is the stand-in: it panics on
a bad initiation, takes one client key, has no replay check of the
timestamp, one fixed cookie and no timers.

## Premise

- Read: `crates/pod-wg/src/standin.rs` (the steps of the responder, with
  `assert!` on each check), `crates/pod-wg/src/session.rs` (`Session` and
  its window of counters, which the responder takes as it is),
  `crates/pod-wg/src/cookie.rs` (mac2 over zero reserved bytes).
- The WireGuard paper, sections 5.1 (the timestamp), 5.4.4 and 5.4.7 (mac1,
  mac2 and the cookie of the source address under a secret of 120 s), 5.4.6
  (the responder sends after the first transport message of the
  initiator), 6 (REJECT_AFTER_TIME 180 s, KEEPALIVE_TIMEOUT 10 s,
  REJECT_AFTER_MESSAGES; only the initiator rekeys). The flood rule of 50
  initiations a second of one peer is the Linux kernel's (`noise.c`,
  `INITIATIONS_PER_SECOND`), from memory of its source, not read in this
  session.
- Not measured: the responder against a WireGuard of another
  implementation (wireguard-go, the kernel); the egress of this session
  takes no UDP.

## Approach

`Responder` (`src/responder.rs`, `src/responder/peer.rs`) on the steps of
`src/respond.rs`, which the stand-in also takes. `receive(message, from,
under_load, now) -> Incoming`; `send(peer, packets, now)`; `tick(now)`.
Tests: the initiator of the crate against it (several peers, a rekey of a
`Tunnel`, a replay, an unknown key, a cookie under load and its retry, the
expiry at 180 s, the keepalive and no initiation). The fuzz target
`wg_initiation`. The plants `wg-responder-timestamp-unchecked`,
`wg-responder-unknown-key-answered`, `wg-responder-mac2-unchecked`,
`wg-responder-session-never-expires`. Calls C315 to C319.

## Prove

```sh
cargo test -p pod-wg --all-features
cargo test --manifest-path fuzz/Cargo.toml --lib
sh scripts/plant-each.sh wg-responder-timestamp-unchecked wg-responder-unknown-key-answered wg-responder-mac2-unchecked wg-responder-session-never-expires
```

## Done

2026-11-14, commit c4f04bd. `Responder`, `Peer`, `Incoming` and the
constants `REJECT_AFTER`, `KEEPALIVE_TIMEOUT`, `COOKIE_SECRET_AGE`,
`INITIATION_GAP`, `REJECT_AFTER_MESSAGES` (`crates/pod-wg/src/responder.rs`,
`crates/pod-wg/src/responder/peer.rs`), on the steps of
`crates/pod-wg/src/respond.rs`; nine new `Error` variants; calls C315 to
C319. The stand-in keeps its form and runs on the steps of the responder.
`cargo test -p pod-wg --all-features`: 28 passed, with
`responder::tests::several_peers_at_once_with_the_initiator_of_the_crate`
(three peers, one with a preshared key; no send before the confirmation; a
rekey of a `Tunnel` with the old session kept until the peer sends in the
new one), `a_replayed_initiation_is_refused` (the same bytes, an older and
an equal stamp; the flood gap), `an_unknown_key_gets_nothing` (and a bad
mac1, a bad body, the wrong lengths and types; `Debug`),
`a_cookie_under_load_and_the_retry_with_mac2` (another address and a new
secret get a cookie reply again), `a_session_expires_after_180_s` (both
ways, and a session that waits) and
`a_keepalive_when_the_responder_sent_nothing` (no initiation from 100 s to
400 s). `cargo test --manifest-path fuzz/Cargo.toml --lib`: 13 passed,
with the new target `wg_initiation` on its seeds and 2000 mutations. The
plants (each checked by hand: the test fails at its assertion):
`wg-responder-timestamp-unchecked`, `wg-responder-unknown-key-answered`,
`wg-responder-mac2-unchecked`, `wg-responder-session-never-expires`.
`cargo clippy -p pod-wg -p pod-warp --all-targets --all-features -- -D
warnings` and the clippy of `fuzz/`: clean. Not done: a test against
another WireGuard implementation (no UDP egress here).

# T-574: `pod-hold`: a filter that refuses raw sockets and lets ping sockets reach the supervisor (PodUtils/podnet#550)

**Source:** PodUtils/podnet#550 (COORD-077 of podnet, note 1; PodUtils/podnet@T-334).
**Category:** feature
**Milestone:** M6
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`Calls::sockets()` refuses `SOCK_RAW` of `AF_INET` and `AF_INET6` and the ping sockets (`SOCK_DGRAM` with `IPPROTO_ICMP` or `IPPROTO_ICMPV6`) in the kernel filter (T-544). podnet emulates ping sockets in its supervisor (a ping to the gateway of the pod), so it uses `Calls::sockets().allow_raw()` and refuses `SOCK_RAW` in the supervisor. podnet asks for a form that refuses only `SOCK_RAW` in the kernel filter and lets the ping sockets reach the supervisor, with `allow_raw()` as the opt-in of the raw part.

## Premise

- Read: PodUtils/podnet#550; `crates/pod-hold/src/filter.rs` (`deny_raw`); T-544 and call C301; T-558 (`AF_PACKET`, the same rule).

## Approach

Split the rule in two: the raw part (`SOCK_RAW` of `AF_INET` and `AF_INET6`, and `AF_PACKET` of T-558) and the ping part. A method of `Calls` lets the ping part through to the notification or to the kernel, and `allow_raw()` keeps its meaning for the raw part. Do T-558 in the same work. Cases of the BPF unit test, a test of a held child for each part, and a plant.

## Prove

```sh
cargo test -p pod-hold --all-features
sh scripts/plant-each.sh hold-t574-ping-refused-when-allowed hold-t574-raw-opt-in-opens-ping
sh scripts/agent/check-windows.sh
sh scripts/agent/check-macos.sh
```

## Done

2026-11-14, commit c498ab0 (with T-558). The socket rules have two parts,
each with its own opt-in (C364): the raw part (`SOCK_RAW` and
`SOCK_PACKET` of `AF_INET` and `AF_INET6`, `AF_PACKET` and `AF_XDP` of
each type) and the ping part (`SOCK_DGRAM` with `IPPROTO_ICMP` or
`IPPROTO_ICMPV6` of `AF_INET` and `AF_INET6`). `Calls::sockets()`
refuses both with `EPERM` in the kernel. The API: `Calls::allow_raw()`
(the raw part only), `Calls::allow_ping()` (new), `Calls::denies_raw()`,
`Calls::denies_ping()` (new). An allowed socket goes to the user
notification, never straight to the kernel (C366). podnet's ask is
`Calls::sockets().allow_ping()`, with `.allow_raw()` under `--allow-raw
icmp`; `Calls::sockets().allow_raw()` alone now refuses the ping part.

`cargo test -p pod-hold --all-features`: 61 unit tests passed; 5 tests of
`tests/hold.rs` and 1 of `tests/veth.rs` passed, with no `SKIP:` line.
New: `filter::tests::each_part_of_the_socket_rules_has_its_own_opt_in`
(a BPF run of each part, each family, the type flags and the other
sockets with `Calls::sockets()`, `allow_raw`, `allow_ping` and both) and,
in `tests/hold.rs`, `each_part_of_the_socket_rules_has_its_own_opt_in`
(a held child: a refused socket gets `EPERM` with no notification; an
allowed one reaches the supervisor, which proceeds; the kernel of this
host answered the ping socket of `AF_INET` with `EACCES` (no ping group)
and `AF_INET6` with `EAFNOSUPPORT`). Plants, each caught by hand:
`hold-t574-ping-refused-when-allowed` (the held test) and
`hold-t574-raw-opt-in-opens-ping` (the BPF test); the two plants of T-544
follow the new code and are caught. `plant-each.sh --apply`: 0 stale.
`check-windows.sh` and `check-macos.sh`: 0 faults. `cargo doc` of
`pod-hold` with `-D warnings`: clean.

# T-579: `pod-session`: a spare link that moved no byte backs off before the next (PodUtils/podssh#242)

**Source:** PodUtils/podssh#242 (COORD-065 of podssh; PodUtils/podssh@T-781).
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

A road that holds the bytes of the far end for longer than `spare_after` (localhost.run holds them for 13 s and more) starts a loop: the link is silent, a spare link opens beside it and takes the session, the node replays the same unacknowledged bytes (192 to 256 KiB) into the same third party, and the silence comes again. In podssh's live runs (42989086142, 42994885604, 43560494966) the session moved none of its 1 MiB in 120 s. podssh now sets `spare_links: false` for a race of third parties' tunnels of HTTP (its commit `77ab640`).

## Premise

- Read: PodUtils/podssh#242; `crates/pod-session/src/beside.rs`, the settings `spare_after` and `DEAD_AFTER`, and the record of the acknowledged position.

## Approach

When a spare took the session and the acknowledged position did not move before the next silence, double the wait before the next spare, up to `DEAD_AFTER`. Reset the wait at the first byte that moves the acknowledged position. A test with a stand-in link that holds each byte for 5 s: the session makes progress with spares on. A plant: the wait does not grow.

## Prove

```sh
cargo test -p pod-session --all-features
```

## Done

2026-11-14, commit 3254189. `beside::SpareWait` (`took`, `wait`) and
`beside::Position`: after a spare that took the session at the position
of the spare before, the next spare waits double, `DEAD_AFTER` at most;
the first byte that moves the session (received, or acknowledged by the
far end) sets the wait back. The wait is the floor of the question's
wait and of the heartbeat's quiet. Call C357. `cargo test -p pod-session
--all-features`: 154 passed, with
`beside::tests::the_wait_doubles_after_each_spare_that_moved_nothing_up_to_dead_after_and_a_move_sets_it_back`
and `tests/session_spare_backoff.rs` (2 tests: a road that answers the
handshake at once and holds each other record for 5 s, then 13 s; 1 MiB
each way comes in under 10 s after one spare, and in under 40 s after
spares that waited 3 s, 6 s and 12 s). On the code before the change
both tests of the road fail: a spare each 3 s, and no byte moves in
120 s. The plant `session-t579-spare-wait-fixed` (the wait does not
double) fails the test of the 5 s road (checked by hand). `cargo clippy
-p pod-session --all-targets --all-features -- -D warnings`: clean.

# T-589: `pod-netstack`: `bind_udp` with port 0 takes an ephemeral port (PodUtils/podnet#552)

**Source:** PodUtils/podnet#552 (COORD-078 of podnet; a defect of PodUtils/podnet@T-117).
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`NetStack::bind_udp` refuses port 0. podnet's UDP over a `wg:FILE` network bound port 0 and never worked; podnet now picks free ports from 49152 itself. `connect` picks an ephemeral port; `bind_udp` should do the same.

## Premise

- Read: PodUtils/podnet#552; `crates/pod-netstack/src/stack.rs` (`bind_udp`, `bind_udp_any`), `crates/pod-netstack/src/stack/udp_service.rs` (`bind_udp`, `bindable`), and the choice of an ephemeral port of `connect`.

## Approach

Port 0 in `bind_udp` (and `bind_udp_any`) takes a free port of the ephemeral range, with the rule of `connect`, and `local_addr` gives the port that it took. A second bind of port 0 takes another port. Tests and a plant.

## Prove

```sh
cargo test -p pod-netstack --all-features
```

## Done

2026-11-14, commit 5bd7246. `NetStack::bind_udp` and `bind_udp_any` with port 0 take a free port of 49152 to 65535: the first port to try is the port that `connect` takes (`ephemeral_port` of the id of the socket and `random_seed`); a port that a UDP socket (also one that drains its queue) or the gateway service holds moves the choice to the next port, with a wrap; a range with no free port is `AddrInUse` (call C403). `UdpSocket::local_addr` gives the port. podnet can drop its own choice of ports from 49152 (PodUtils/podnet#552).

- `cargo test -p pod-netstack --all-features`: 82 passed (3 new in `src/stack/tests/ephemeral.rs`: `a_bind_of_port_zero_takes_a_free_port`, `a_bind_of_any_address_and_port_zero_takes_a_free_port`, `a_bind_of_port_zero_skips_a_held_port`). `a_refused_bind_frees_its_socket` no longer binds UDP port 0.
- Plants, each caught by hand: `netstack-udp-port-zero-refused` and `netstack-udp-port-zero-takes-a-held-port` (`a bind of port 0`).
- clippy, rustfmt and `cargo doc` are clean; the README has the section "UDP", "Port 0".

# T-612: `pod-proxy`: the label of a unix proxy shows the user info of its URL (PodUtils/podnet#555)

**Source:** PodUtils/podnet#555 (COORD-079 of podnet, item 1; podnet's fuzz run 43590953876).
**Category:** defect
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

The text of the issue, with a made-up password written as PW: `Proxy::parse("socks5h+unix://user:PW@x")` succeeds, and the carrier is `Unix("//user:PW@x")`. `Proxy::label()` prints the path whole. A password that a user writes in the URL form appears in each line that names the proxy (`proxy socks5h+unix://user:PW@x`, and `could not reach the proxy unix://user:PW@x: ...`). The fuzz target `proxy_url` of podnet asserts that a label holds no `@`; its crash input is `socks5+unix://ss@proxy:2]:/`. The ask: a `+unix` (or `unix:`) proxy whose path starts with `//` or holds `@` is a usage error (the URL form of a unix proxy has no authority), or its user info is parsed as credentials and kept out of `label()`, `carrier_text()` and the error texts. A test: the label and each error of such a spec hold no secret.

## Premise

- Read: PodUtils/podnet#555; `crates/pod-proxy/src/proxy/parse.rs` (`Proxy::parse_with`: the `+unix:` and `://unix:` forms take each path that starts with `/`), `crates/pod-proxy/src/proxy.rs` (`label`, `Display`, `Debug` and `carrier_text` show the path whole, and the first word of a command), the texts of `DialError` (`crates/pod-proxy/src/error.rs`, `crates/pod-proxy/src/stream.rs`, `crates/pod-proxy/src/socks5.rs`, `crates/pod-proxy/src/connect.rs`: each names the proxy by its `Display`), and the fuzz target `proxy_url` (`fuzz/src/targets.rs`: it checks the `@` of the label of a proxy on TCP only).
- A proxy on a Unix socket never has credentials: `Proxy::elsewhere` makes it with none, and no setter adds them. No consumer gives a user to a SOCKS5 proxy on a Unix socket.
- Not measured: podnet's fuzz run itself; the crash input is in the issue.

## Approach

Refuse the user info, as the ask's first choice: a Unix path that starts with `//` (a host, but `///PATH`, an empty host) or that holds `@` is a usage error whose text names the scheme only. Grep each text that shows a proxy, a carrier or a spec, and keep each user and password out of it. Tests: the crash input, a table of specs (Unix, HTTP, SOCKS5, commands, with `]`, `%40` and `@@`) whose label, Display, Debug and errors hold no secret. The fuzz target `proxy_url` checks the `@` of each form that a proxy shows, with the crash input as a seed. Plants.

## Prove

```sh
cargo test -p pod-proxy --all-features --lib proxy::tests
cargo test --manifest-path fuzz/Cargo.toml --lib
```

## Done

2026-11-14, commit db133bd. `Proxy::parse_with` refuses a Unix path that starts with `//` (a host; `///PATH` stays a path) or holds `@`: the error is `a proxy on a Unix socket has no host, no user and no password (no // and no @ after unix:): SCHEME+unix:/PATH`, with the scheme only (call C475; C476 for `///PATH`). A control character in a Unix path is an error (C477). The label, the Display and the Debug of a proxy command show its first word that is not an assignment, and `exec:...` when that word holds `@` (C478). Each text of the crate that names a proxy (`label`, `Display`, `Debug`, `kind_text`, the errors of the parser, and the `DialError` texts, which take the `Display`) holds no user and no password. The fuzz target `proxy_url` checks that no `@` reaches the label, the Display or the Debug form, and that a Unix path has no host; its new seeds are `unix-authority` (podnet's crash input), `unix-at` and `unix-empty-host` (C479).

- `cargo test -p pod-proxy --all-features`: all passed (2 new in `crates/pod-proxy/src/proxy/tests.rs`: `a_unix_proxy_has_no_host_and_no_user`, `the_label_and_the_errors_of_a_spec_hold_no_secret`, a table of 26 forms of a spec with a user and a password, each parsed as a variable and as the user's value).
- `cargo test --manifest-path fuzz/Cargo.toml --lib`: 13 passed, 4 ignored.
- Plants, each caught by hand: `proxy-unix-host-in-label` (`a_unix_proxy_has_no_host_and_no_user`), `proxy-exec-assignment-shown` (`the_label_and_the_errors_of_a_spec_hold_no_secret`), `fuzz-proxy-unix-user-shown` (the fuzz target on the seed `unix-at`).
- clippy of `pod-proxy` and of the fuzz library, rustfmt, `cargo doc` and `scripts/agent/check-windows.sh` and `scripts/agent/check-macos.sh` are clean; the README of `pod-proxy` lists the new tests, and `fuzz/README.md` the checks of `proxy_url`.

# T-623: `pod-link`: the S3 carrier against a real S3-compatible server

**Source:** T-494 (a finding of its work).
**Category:** chore
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The S3 carrier (T-494) is proven only against a stand-in of the crate, which checks signatures with the signer of the crate. The examples of the AWS documentation prove the signer, but no test shows that a real S3-compatible server takes the requests: the path style, `If-None-Match` on a PUT, the pages of a listing, and the codes of its errors. podnet's own test runs against a local S3-compatible server.

## Premise

- Read: the report of T-494 and calls C489 to C491; `crates/pod-link/tests/s3.rs`; `scripts/ci/` (the scripts that run a server of another party pinned by digest, for example `coredns-tests.sh`); `docs/live-calls.md`.

## Approach

Pin a server of another party (MinIO or SeaweedFS) by digest, run it on the loopback of CI from a script of `scripts/ci/`, add a row to `docs/live-calls.md`, and run the tests of `tests/s3.rs` against it, under a feature or as ignored tests that the script runs.

## Prove

```sh
sh scripts/ci/s3-tests.sh
```

## Done

2026-11-14, commit `afdd022`. The server is SeaweedFS 4.53 (Apache-2.0),
pinned by the SHA-256 of `linux_amd64.tar.gz` (call C540); MinIO is
archived and dl.min.io answers 410. `scripts/ci/s3-tests.sh` starts
`weed mini` on 127.0.0.1 with the bucket `links` and the made-up key, waits
for a signed listing, and runs the new target `tests/s3_seaweedfs.rs` with
`POD_LINK_TEST_S3=required`; it must print the line of each test. Six
tests, with objects of 16 KiB and pages of one key: a link of three
streams each way, the prune, `If-None-Match: *` (412), `If-Match` (412),
`max-keys` and `start-after`, a wrong secret (403
`SignatureDoesNotMatch`) and a missing bucket (404 `NoSuchBucket`), a
listener that takes no older session, the budget. The step "pod-link
against SeaweedFS" of `ci.yml`, the row L41 and the server row of
`docs/live-calls.md`, the row of `scripts/` in `docs/design.md`.
`scripts/check-workflows.py` takes the loopback as no host of another
party. The egress of the session refuses github.com: the archive came
through fetch.web.pkgforge.dev with the same SHA-256 and went to the cache
of the script, and the script then passed here (6 passed, 3.9 s). The next
run of `ci.yml` proves the download from github.com. Plants:
`link-s3-numeric-reference-kept` (SeaweedFS writes the ETag of a listing
as `&#34;...&#34;`; the stand-in tests of `tests/s3.rs` pass, 4 tests of
the script fail) and `link-s3-seaweedfs-skip-fails`, each caught by hand.

# T-624: `pod-warp`: after a key that is not the pinned one, the client tries each door of the new record and names the mismatch

**Source:** T-621 (a finding of its work).
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

After a door shows a key other than the pinned one, `open_after_mismatch` (`crates/pod-warp/src/open.rs`) reads the record again and dials only its first door (`masque_endpoint()`, IPv4). If that dial fails for another cause, the raw error comes back: the text drops the mismatch, and the other doors (IPv6) are never tried. A user sees "TCP refused to 192.0.2.1:443" with no word of the mismatch.

## Premise

- Read: the report of T-621 and call C520; `crates/pod-warp/src/open.rs` (`open_after_mismatch`, `open_on`).

## Approach

After the new read, try the doors of the new record in order (the door that showed the key first, when the record still lists it), and fold each failure into one text that names the mismatch, as `open_on` does. A test with a dial that sends the first IPv4 dial to an edge of another key; a plant.

## Prove

```sh
cargo test -p pod-warp --all-features
```

## Done

2026-11-14, commit 3c0ebc7. After a door shows a key that is not the pinned key, `open_after_mismatch` reads the record again and tries each of its doors in the order of `door::doors`, the door that showed the key first when the record still lists it (call C530). A door that fails for another cause gives way to the next door (event `warp.door`). Each door that shows another key again is tried once more after `PIN_RETRY`. When each door fails, one failure names the mismatch and the chain of each door, and adds "does not match either" with `why_shown` when a door still shows another key; its stage, class and cause are those of a door that shows another key, else of the first door (C531). An example: `TLS handshake with the MASQUE front door 192.0.2.1:443: ... MASQUE peer key does not match the pinned key; ...; the device record was read again, and each of its doors failed: TCP to the MASQUE front door 192.0.2.1:443 ...: Connection refused (os error 111); TCP to the MASQUE front door [2001:db8::1]:443 ...: Connection refused (os error 111)`.

- `cargo test -p pod-warp --all-features`: all passed. The new test `tests/net.rs` `after_another_key_each_door_of_the_record_read_again_is_tried` sends the first IPv4 dial to a second edge (another key) and each later IPv4 dial to a refusing port (`common::Moved::with_first`): the IPv6 door opens after 3 dials, and with the IPv6 door refused too the failure names the mismatch and both doors (stage `Connect`, no try again). The binary of `--test net` passed 30 runs of 30.
- Plants, each caught by hand: `warp-fresh-record-first-door-ends` (a door that fails for another cause ends the try with its raw failure), `warp-fresh-record-mismatch-unnamed` (the failure drops the mismatch).
- clippy, rustfmt and `cargo doc` of `pod-warp` are clean; the README of `pod-warp` names the behaviour, the test and the plants.

# T-627: `pod-link`: the stand-in of the S3 API behind the feature `s3-standin` (PodUtils/podnet#564)

**Source:** PodUtils/podnet#564 (COORD-082 of podnet; PodUtils/podnet@T-279).
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

The stand-in of the S3 API that the tests of the carrier `s3` use is in `crates/pod-link/tests/common`, so a consumer cannot use it. podnet made a copy for its tests of `link_s3` (tests/link_s3_standin/mod.rs of podnet), and the two copies can drift apart.

## Premise

- Read: PodUtils/podnet#564; crates/pod-link/tests/common/s3.rs (the stand-in `StandIn` and `key`); `crates/pod-link/tests/s3.rs`; the feature `standin` of `crates/pod-wg/Cargo.toml` and `crates/pod-devtunnel/Cargo.toml` (`src/standin.rs`).

## Approach

Move the stand-in into a module `s3::standin` of the crate behind a new feature `s3-standin` (which takes `s3`), with a public API: start, the endpoint, the buckets, the keys under a prefix, and the faults that the tests use. The tests of `s3.rs` take it from the crate (`required-features`). The README of the crate lists the feature, and it says that no binary turns it on. Tell podnet on the hand-off that it can remove its copy.

## Prove

```sh
cargo test -p pod-link --all-features --test s3
```

## Done

2026-11-14, commits `08c7ddd` and `76bf345`. The stand-in of the S3 API
moved from tests/common/s3.rs to `s3::standin` behind the new feature
`s3-standin` (which takes `s3`), as `pod-wg` and `pod-devtunnel` give
theirs (call C552). The API: `key()`, `StandIn::start` (the runtime of
the caller), `StandIn::start_thread` (a thread with its own runtime, for
podnet's tests that start podnet processes), `addr`, `url`, `endpoint`,
`buckets`, `keys`, `counts` (a copy, `Counts`), `lose_puts`. A drop
closes the port, then each connection. Fixes in the stand-in: a request
with no signature gets 403 `AccessDenied` (the copy in the tests served
it); a path with a bad escape gets an answer, not a panic; `max-keys=0`
lists no key. `tests/s3.rs` (8), `tests/s3_seaweedfs.rs` (6) and the new
`tests/s3_standin.rs` (3) take it with `required-features`; 200 runs of
`tests/s3_standin.rs` pass. The README lists the feature (no binary
turns it on), the API, the tests and the row of PodUtils/podnet@T-279;
`scripts/check-probe.py` lists the feature in `ALLOWED`. Plants
`link-s3-standin-unsigned-taken`, `link-s3-standin-signature-unchecked`,
`link-s3-standin-runs-after-drop` and `link-s3-standin-escape-panics`,
each caught by hand; `sh scripts/plant-each.sh --apply`: 0 stale. podnet
can remove tests/link_s3_standin/mod.rs (the hand-off).

# T-634: `pod-relay`: the run of a code fails over the Nostr relays alone, though `pod_wormhole::nostr` pairs (PodUtils/podssh#247)

**Source:** PodUtils/podssh#247 (COORD-070 of podssh), a defect of T-550.
**Category:** defect
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

podssh with `v0.47.0`: `pod_relay::code::run::give` and `run::take` with
one mailbox that nobody serves and `Ends::relays` filled from
`pod_services::of_kind("nostr-relay")` fail, with each relay alone
(`relay.primal.net`, `offchain.pub`) and with both. The taker says "the
other end said nothing for 60 s" with `Waits::default()`. From the same
container, `nostr::opened` and `nostr::joined` pair directly at both
relays, the live test `two_ends_meet_at_each_public_nostr_relay` passes,
and the same run over `Ends::ntfy` alone or `Ends::trackers` alone
pairs. podssh leaves `Ends::relays` empty until a tag fixes the run
(PodUtils/podssh@T-545). The stand-in relay of `pair_code.rs` pairs where
the public relays fail.

## Premise

- Read: `crates/pod-relay/src/code/run.rs`,
  `crates/pod-wormhole/src/nostr.rs`, `crates/pod-wormhole/src/nostr/relay.rs`,
  `crates/pod-wormhole/src/wormhole.rs`, `crates/pod-wormhole/tests/nostr.rs`,
  `crates/pod-wormhole/tests/harness/nostr.rs`, `crates/pod-wormhole/tests/live.rs`,
  `crates/pod-relay/tests/pair_code.rs`. podssh's reproduction
  (crates/podssh-relay/tests/pair_meet_live.rs of podssh) is not on its
  `main` branch: not read.
- Measured from a cloud session through its HTTPS proxy, with each message
  of the relay connection printed: at `relay.primal.net` the key, both
  versions, the offer and the answer go through. The giver sends its last
  word (phase `1`), then at once `CLOSE` and the Close of the WebSocket.
  The relay sends no `OK` for that event, and the taker never gets it: the
  taker fails after a step. The giver's run succeeds. The live test of
  `pod-wormhole` passes because no end closes right after its last
  message.
- The stand-in takes each message of a connection before it reads the
  next, so it passes the last word of an end that closes at once. Not
  measured: whether the relay or the proxy drops the event.

## Approach

1. `nostr::Relay` keeps the ids of its events that the relay did not
   answer (`OK`). `close` reads the relay until each one has its answer,
   within the answer limit, and then sends `CLOSE` (call C595).
2. The stand-in takes each message `LAG` (20 ms) after it came, and drops
   the messages of a connection that ended before then, as a public relay
   does (call C596). The run of `pair_code.rs` over the stand-in relay then
   fails on the old code with the symptom of podssh.
3. A test in `tests/nostr.rs`: the last word of an end that closes at once
   reaches the other end. Two plants. A live test of the run at each public
   relay after a dead mailbox in `pair_code_live.rs` (call C597).

## Prove

```sh
cargo test -p pod-wormhole --features standin --test nostr
cargo test -p pod-relay --all-features --test pair_code
cargo test -p pod-relay --all-features --test pair_code_live -- --ignored --nocapture nostr
```

## Done

2026-11-14, commit `6db015a`. Calls C595 to C597.

- The cause: the giver sent its last word and closed at once (`CLOSE`, then
  the Close of the WebSocket). The public relay sent no `OK` for that event
  and did not pass it on, so the taker waited a step for it. The live test
  of `pod-wormhole` closes no end right after its last message, and the
  stand-in took each message of a connection before it read the next.
- `pod_wormhole::nostr::Relay::close` reads the relay until it answered
  each event of this end, within the answer limit, then sends `CLOSE`;
  `nostr::relay::UNANSWERED_MOST` (64). The stand-in of
  `crates/pod-wormhole/tests/harness/nostr.rs` takes each message `LAG`
  (20 ms) after it came and drops the messages of a connection that ended
  before then.
- Tests: `cargo test -p pod-wormhole --features standin --test nostr`: 8
  passed (1 new:
  `the_last_word_of_an_end_that_closes_at_once_reaches_the_other_end`), 30
  runs in a loop with no failure; on the old code the new test fails
  ("the other end said nothing for 1 s"). `cargo test -p pod-relay
  --all-features --test pair_code`: 16 passed; on the old code
  `where_no_mailbox_answers_the_ends_meet_at_a_nostr_relay_before_a_broker`
  fails ("the taker: the other end said nothing for 20 s"), the symptom of
  podssh. `cargo test -p pod-wormhole -p pod-relay --all-features`: each
  passes.
- Live, from a cloud session through its HTTPS proxy: the new
  `two_ends_pair_through_each_public_nostr_relay_after_a_dead_mailbox` of
  `crates/pod-relay/tests/pair_code_live.rs` failed before the fix
  (`relay.primal.net`: "the other end said nothing for 60 s"); after it,
  `relay.primal.net` pairs in 3.2 s and `offchain.pub` in 1.9 s. The
  mailboxes of `pair_code_live` and `two_ends_meet_at_each_public_nostr_relay`
  of `pod-wormhole` pass. The run at the one public MQTT broker
  (`public.cloud.shiftr.io`) after a dead mailbox pairs in 3.0 s, so the
  brokers do not have the defect.
- Plants `wormhole-nostr-close-unanswered` and
  `relay-code-nostr-last-word-lost`, each caught by hand (15 of 15 runs of
  the first); the patch of `wormhole-nostr-refusal-ignored` follows the
  new `heed`, caught by hand; `wormhole-nostr-no-resend`,
  `wormhole-nostr-tag-constant`, `wormhole-nostr-signature-unchecked` and
  `relay-code-nostr-after-brokers` still caught on the new stand-in.
  `sh scripts/plant-each.sh --apply`: 0 stale.
- The READMEs of `pod-wormhole` and `pod-relay` give the close, the
  stand-in, the tests and the plants; their counts of plants (25) and of
  ignored tests (21) are corrected.

# T-639: `pod-link`: a closed half of a split session can end the listener with "the carrier of the link ended" in place of the lost half

**Source:** PodUtils/podnet#572 (COORD-085 of podnet;
PodUtils/podnet@T-353), a defect of T-500.
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podnet with `v0.49.0`: its test
`link_split::a_closed_down_half_ends_the_session_with_down_carrier_lost`
(a `pod_link::Link` over each end of `split::Split`, two memory halves;
the down half closes after one stream moved 3 bytes) failed one time in a
full `cargo test --workspace` under load with `listener: the carrier of
the link ended`. Alone it passed 5 times of 5, and it passed in each CI
run. The rule of call C493 (and of podnet's decisions) is that each end
names the lost half. The session ends in each case, and the dialer names
the half; only the text at the listener differs. The ask: the byte
stream of a split end gives the `Lost` error, not an end of stream, when
a half is lost, also when the loss and the end race; a test of `pod-link`
that closes each half many times under load. podnet takes the tag with
the fix and runs its test 50 times.

## Premise

- Read: PodUtils/podnet#572; podnet's tests/link_split.rs (its `Paths`
  close a half at each end at one time); `crates/pod-link/src/split/`,
  `crates/pod-link/src/pipes.rs`, `crates/pod-link/src/link.rs`,
  `crates/pod-link/tests/split.rs`; the Done section of T-500, calls C492
  and C493.
- The probable cause of podnet: the link reads 0 bytes from the byte
  stream of the split end (`Ok(0)` gives `Error::CarrierEnded`) before
  the run of that end set the `Lost` error on its pipes.

## Approach

1. Find the path that gives the end of the bytes after a loss, and make
   the split give `Lost` on it.
2. A test in `tests/split.rs` that closes each half of a session of two
   links in many rounds (at least 200) on the multi-thread runtime with
   tasks that load it, and checks the text of each link in each round. It
   fails on the old code.
3. A plant.

## Prove

```sh
cargo test -p pod-link --all-features --test split
```

## Done

2026-11-14, commit `1bc0e9c`. Calls C610 and C611.

- The cause: a link closes its byte stream when the loss of a half ended
  it. The task of an end that sends the link's bytes on its main half
  could read that close before it saw the fault, and sent `END`. The peer
  link then read the end of its bytes and ended with "the carrier of the
  link ended" before its own end saw the loss. The task now reads the
  fault after each read of the link's bytes and sends `LOST` in place of
  `END` (call C610). The fault is set before the state turns to `Failed`
  (`pipes::Ends::failed`). A write to the byte stream of a carrier of
  tasks (poll, s3, split) after a fault gives the fault.
- Test: `a_closed_half_ends_each_link_with_the_loss_of_that_half_in_each_round_under_load`
  in `tests/split.rs`: 240 rounds of a link over each end, 3 bytes, then
  the up or the down half closed at each end (podnet's form) or at one end
  while the other end sees silence, on the multi-thread runtime (4
  workers) with 8 tasks that spin 200 µs between yields; it checks the
  text of each link in each round (call C611). On the old code: 6 of 240
  rounds and 14 of 240 rounds failed in two runs (one of them in podnet's
  form, "the listener: the carrier of the link ended"). After the fix:
  `tests/split.rs` 6 passed, 10 runs of 10; the pipes test checks the
  write after a fault. `cargo test -p pod-link --all-features`: each
  passes.
- Plant `link-split-end-after-loss`, caught by hand (14 of 240 rounds
  failed). `sh scripts/plant-each.sh --apply`: 0 stale.
- podnet proves it with the tag of the fix: its test
  `link_split::a_closed_down_half_ends_the_session_with_down_carrier_lost`
  50 times in a loop, and its full `cargo test --workspace` under load.

# T-643: `pod-quic`: a way whose reads keep failing spins a core, and the drop of the endpoint hangs (PodUtils/podssh#253)

**Source:** PodUtils/podssh#253 (COORD-075 of podssh; PodUtils/podssh@T-787).
**Category:** defect
**Milestone:** M1
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The receive loop of noq 1.4.0 (`RecvState::poll_socket` of its endpoint) answers `ECONNRESET` and `EMSGSIZE` with `continue`, before its work limiter. `pod_quic::socket::DatagramSocket::poll_recv` passes each error of the `Datagrams` through. A way that fails each read with one of those keeps one worker of the runtime busy with no yield, and the driver holds the lock of the endpoint, so the drop of the `Endpoint` hangs too. podssh measured it: `pod_quic::over` with a `Datagrams` that fails each read with `ConnectionReset` gave 19,971,985 failed reads and 2.998 s of CPU in 3 s (its test crates/podssh-relay/tests/quic_socket_fails.rs, ignored until the take).

## Premise

- Read: PodUtils/podssh#253; `crates/pod-quic/src/socket.rs`, the endpoint on the system socket (`endpoint::on_socket`); noq 1.4.0 `src/endpoint.rs`. osewa ends such an endpoint of quinn after 256 empty reads in a row.

## Approach

Count the failed reads in a row in `DatagramSocket`, and in a wrapper of the system socket of `endpoint::on_socket`; reset the count on a read that succeeds or is `Pending`. Past a bound (256), return an error kind that noq does not skip (`BrokenPipe`), so that the driver ends and the caller dials again. A test like podssh's (a `Datagrams` that fails each read: the driver ends, the CPU stays bounded, the drop returns), and a plant that leaves the bound out.

## Prove

```sh
cargo test -p pod-quic --all-features
```

## Done

2026-11-15, the commit of the fix. `crates/pod-quic/src/bounded.rs`: `Bounded` counts the failed reads in a row of the socket of an endpoint and gives `BrokenPipe` at `FAILED_READS_MOST` (256); a read that succeeds or waits starts the count again. `endpoint::on_socket` wraps the system socket of tokio in it, and `over` wraps the `DatagramSocket`. Two tests: the error at the bound and the reset by a wait; an endpoint over a way that fails each read reads at most 512 times in 300 ms, and its drop returns. With the bound removed, the first test failed, and the second hung (the spin of PodUtils/podssh#253). The plant `quic-failed-reads-unbounded`. `cargo test -p pod-quic --all-features` passed.

# T-686: `pod-relay`: a new link of the resumable layer rides the pooled HTTP/2 connection of the silent link it replaces (PodUtils/podssh#262)

**Source:** PodUtils/podssh#262 (COORD-078 of podssh; PodUtils/podssh@T-789).
**Category:** defect
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

After a cut of the client's leg of 15 s, a session of the forward road in the resumable layer comes back about 12.5 s after the link returns; OpenSSH over plain TCP in the same cell comes back in 0.4 to 0.8 s. `pod-session` opens its spare link in time, but the spare's `open` takes its WebSocket over HTTP/2 from `ctx.h2_pool()` (`h2_road::connect`, then `pod_ws::h2_road::connect`): the same TCP connection that carries the silent link. The spare cannot move until the TCP retransmission of that connection fires, and `beside.rs` then closes the spare as "the link spoke again". podssh measured it with `v0.51.0` at `3f6a9fe` (an iptables DROP of the port of the role in a private network namespace; `feel-netem.yml` runs 43709370293 and 43710388009 showed the same): with HTTP/2 the first byte came 12.7 and 12.6 s after the return; with `PODSSH_H2=no`, 0.40 and 0.19 s; `ss` and `strace` show no new TCP socket for the spare. The ask: a new link of the resumable layer (a spare, and a resume after a loss) does not take a pooled HTTP/2 connection that carries the link it replaces, for example a fresh connection, or one that spoke within the wait of the spare; a test of a session over the HTTP/2 road whose connection goes silent and whose spare takes a new TCP connection and resumes within a round trip of the return. The node roads that ride the HTTP/2 pool may have the same defect.

## Premise

- Read: PodUtils/podssh#262; `crates/pod-relay` (the HTTP/2 road, `h2_road::connect`, the pool of the context), `crates/pod-session` (the spare, `beside.rs`), `crates/pod-ws` (`h2_road::connect`).
- Not measured here: the cut. podssh's numbers above.

## Approach

The open of a new link of the resumable layer asks the pool for a connection other than the one of the link that it replaces, or a fresh one: the pool takes a key or a mark of the silent connection, and a connection that has not spoken within the wait is not reused for a spare. The same for each road that rides the pool. A test over the HTTP/2 road on the loopback with a connection that stops answering (a stand-in proxy that holds the bytes): the spare opens a second TCP connection and the session resumes. A plant that gives the spare the pooled connection again.

## Prove

```sh
cargo test -p pod-relay --all-features
cargo test -p pod-session --all-features
```

## Done

2026-11-15, the commit of the fix. The cause: `forward_layer::carry` opens each next link with `Dialer::link`, which goes through `open::open` and `h2_road::connect` to `pod_ws::h2_road::connect` on `ctx.h2_pool()`. `Pool::place` gave the first connection of the host with room, so the spare's CONNECT went on the TCP connection of the silent link. The fix (call C666): each connection of the pool has a serial number; `pod_ws::h2_road::Links` records the newest one that a link of a session rode, and `connect_link` takes only a newer one, or dials one. `Dialer::new` gives its context its own `Links` (`Context::for_one_session`): the first link shares a kept connection, each spare, resume or move takes another. `tests/layer_h2_spare.rs`: a session of the layer over HTTP/2 to a stand-in of TLS with h2's server and a far end that keeps its sessions, through a stand-in proxy that holds each byte of its first connection. With the fix: a second TCP connection, and the whole test ran in 3.2 s (bound of the echo after the cut: `SPARE_AFTER` and 4 s of scaled work). Without the filter of the pool: no echo within 7 s, notes `[Spare { quiet: 3.0 s }]`, one connection. The plant `relay-t686-spare-on-pooled-connection`. The node roads (`reverse::wire::open`) take the upgrade of HTTP/1.1 (`pod_ws::connect`) on a new connection for each socket, and the QUIC road dials a connection for each session: neither rides the pool. A silent connection that still takes the first link of a new session is T-687. `cargo test -p pod-relay --all-features`, `cargo test -p pod-session --all-features` and `cargo test -p pod-ws --all-features` passed.

# T-687: `pod-ws`: a silent connection of the HTTP/2 pool takes the first link of each new session until its TCP fails

**Source:** T-686 (a finding of its work).
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`Pool::place` gives a new session the first kept connection of its host with room. A connection whose TCP went silent stays in the pool, not retired, until its TCP fails or it idles out. A CONNECT on it gets no answer: `open_on` ends with `ConnectError::Timeout` after `WsClientConfig::timeout` and returns it as the session's end (`Carry::Answer`), with no retire of the connection and no try on a new one. Each new session of the process to that host thus waits for the whole timeout and fails, while a new TCP connection would pass. T-686 keeps the replacing links of the resumable layer off such a connection, not the first link of another session.

## Premise

- Read: `crates/pod-ws/src/h2_road/mod.rs` (`connect_link`, `open_on`, `Carry`), `crates/pod-ws/src/h2_road/pool.rs` (`Pool::place`, `Conn::retire`).
- Not measured: the wait of a new session on a silent connection; the read of the code above gives it.

## Approach

A CONNECT whose answer did not come within a bound well under the timeout (for example the larger of 2 s and three round trips of the connection's PING) retires the connection and goes once to a new one, as a connection that ended under the CONNECT does (`Carry::Connection`). Or the pool sends a PING on a connection that heard nothing for some seconds before it gives a place on it, and retires it when no answer comes within the bound. A test: a stand-in proxy that holds the bytes of the first connection; a second session opens on a new connection within the bound.

## Prove

```sh
cargo test -p pod-ws --all-features
```

## Done

2026-11-15, the commit of the fix. The cause: `open_on` waited for the answer of a CONNECT for the whole `WsClientConfig::timeout` and gave `Carry::Answer(Timeout)`, which `connect_link` returned with no retire, so the next new session got the same silent connection from `Pool::place`. The fix (call C667): a CONNECT with no answer within `h2_road::ANSWER_WAIT` (2 s) sends a PING on its connection (`Conn::answers`, the `h2::PingPong` of `dial::settled` kept in `Conn` with the round trip of its first PING). No answer within the larger of `PING_WAIT` (2 s) and four such round trips gives `Carry::Connection`: `connect_link` retires the connection and goes once to a new one. A connection that answers its PING keeps the CONNECT to the timeout and stays shared (podssh T-321); a CONNECT that then times out retires its connection. `crates/pod-relay/tests/layer_h2_spare.rs`, `a_new_session_beside_a_silent_connection_of_http2_takes_a_new_one`: a first session over HTTP/2 through the stand-in proxy, whose connection the proxy then holds; a second `Dialer` of the same context opens a session on a second TCP connection and echoes within `ANSWER_WAIT + PING_WAIT` and 4 s of scaled work. With the fix: about 4.2 s. Without the PING (the CONNECT waits the whole timeout): `no second session within 8.0 s`. The plant `relay-t687-silent-connection-kept` (no retire on `Carry::Connection`): the second try takes the silent connection again, then HTTP/1.1, which the stand-in does not take; caught. `cargo test -p pod-ws --all-features` and `cargo test -p pod-relay --all-features` passed.

# T-731: `pod-transit`: `two_ends_meet_and_carry_one_mib_each_way` failed once in CI: the client paired with a node connection that the relay stub closed

**Source:** `ci.yml` run 43771483683 at `0d6421c` (PodUtils/podcrates#196), the job `test`.
**Category:** defect
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

In CI the test failed at `crates/pod-transit/tests/transit.rs:95` with `ConnectionReset` on the link. The log shows the node's first meeting at the loopback transit relay fail ("the relay ended the connection before the meeting: the server closed the connection without a WebSocket Close") and a retry in 842 ms, while the client already met a connection: the client took the node's dead connection for its pair. `pod-transit` did not change since `v0.56.0`, whose `ci` passed.

## Premise

- Not reproduced here: 40 runs of the test alone, 25 runs of the file, and 40 runs on one loaded CPU passed.

## Approach

Read the relay stub of the tests and the meeting of the client and the node: find why the stub closes the node's first connection with no Close, and whether a client can pair with a connection that closes before the meeting. Fix the stub or the client (a pair that ends before the first byte is a failed meeting, and the client meets again), with a test that forces the close (a stub that drops the first connection of the node).

## Prove

```sh
cargo test -p pod-transit --test transit
```

## Correction

2026-11-15. The runs of the Premise prove nothing about the relay: with no Python that imports `wormhole_transit_relay` (this host has none, and `POD_WORMHOLE_TEST_PYTHON` was not set), each test of the relay says `did not run` and passes. Only `scripts/ci/mailbox-tests.sh` runs them, with `POD_WORMHOLE_TEST_SERVER=required`.

## Done

2026-11-15, the commit of the fix. The cause is in the harness of the tests, not in the crate. The lines `node: waiting at the transit relay ... for the pair's operators`, `a meeting ... failed` and `waiting ... again in 842 ms` come from `far::serve_kept`, which only `a_session_moves_to_another_road_when_the_transit_relay_stops` runs (`--nocapture` puts the lines of each test in one log): that test stops its relay on purpose, and its node's waiting connection then ends with no Close. `two_ends_meet_and_carry_one_mib_each_way` met through `node_meets` and `Dialer::link`: both ends proved the pair, so the link was a true meeting. `Relay::start` chose its two ports with `free_port` (a bind to port 0, then the release of the port) and took the relay as up when a connection to the WebSocket port passed. Two tests that run at a time can choose one port: the second relay cannot bind it and ends, while the first relay's listener takes the connections of the second test. The test of 1 MiB then ran through the relay of the other test, and its link got `ConnectionReset` when that test killed its relay. The client cannot take a connection that ends before the meeting for its pair: `meet` gives a link only after `ok`, the hello and a proof of the pair from the peer, and an end before the proof is `Dropped` or `Broken` (the client tries again). The fix (call C717): `transit_server::own_listener` takes the relay only when its own child holds the listening socket of the port (its inode in `/proc/net/tcp` among the files of `/proc/PID/fd`; on a system with no `/proc`, a connection while the child lives); a child that ends or does not listen within `START_LIMIT` is killed, and `Relay::start` tries again with new ports, 5 times at most. The test `the_harness_takes_only_its_own_child_s_listener_for_a_relay` (Linux, python3 alone, no relay): a listener of the test holds a port; a child that cannot bind that port ends and is refused, a child that does not listen is refused within 2 s, and a child that listens on its own port is taken. The plant `transit-harness-any-listener` (a connection to the port as the proof, as before) fails it at each run. No change of the crate: no line for the release notes. `cargo test -p pod-transit --all-features` passed; 100 runs of `--test transit` passed as root, 100 as uid 65534, 100 on one loaded CPU (`taskset -c 0`, with two busy loops on that CPU) and 20 more there as uid 65534. The tests of the relay itself said `did not run` here (see the Correction); `scripts/ci/mailbox-tests.sh` runs them. The same choice of a port is in five more harnesses: T-732.

# T-742: `pod-link`: the unix carrier fails under Landlock: the move of its staged socket gives `EXDEV` (PodUtils/podnet#592)

**Source:** PodUtils/podnet#592 (COORD-095 of podnet: its lane in the labs of `pod-lab`; PodUtils/podnet@T-380).
**Category:** defect
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`bind_private` of `crates/pod-link/src/unix.rs` binds a socket at a staged path and moves it into place with `renameat2`. Landlock with no `LANDLOCK_ACCESS_FS_REFER` refuses that move with `EXDEV`, so `podnet link listen PATH` exits "bind PATH: Cross-device link (os error 18)" in each lab with Landlock (`root-unreadable`, `no-kvm`, `no-tun`, `no-net-admin`, `proc-masked`). The `bind-denied` host of PodUtils/podrunner#170 runs under Landlock, and the unix carrier is the way of `link` there.

## Premise

- Read: PodUtils/podnet#592. podnet fixed the same move in its own listener (PodUtils/podnet@T-379, `PodUtils/podnet:crates/podnet-netns/src/listen.rs` at `d788b51`): on `EXDEV` it binds at the path with `fchmod 0600` before the bind.

## Approach

On `EXDEV`, bind at the path with the mode set before the bind (no window where the socket is open to others), and a test under a Landlock domain with no `REFER` (a lab of `pod-lab`, or a ruleset in the test), with a plant.

## Prove

```sh
cargo test -p pod-link --all-features
```

## Done

2026-11-16, the commit of the fix. The fix (call C724): `bind_private` of `crates/pod-link/src/unix.rs` binds at the path when the move of the staged socket gives `EXDEV`, and also when the staged name does not fit in `sun_path` (before, that path bound with the mode of the umask and set 0600 after the bind: a window). On Linux each bind of the carrier (`bind_at`, `bind_socket`) makes the socket with `socket(2)`, sets its mode with `fchmod` 0600, then binds and listens: Linux makes the file of the socket with the mode of the inode of the socket less the umask (`unix_bind_bsd`), so the file never has a wider mode; `bind_with_mode` then sets 0600 again. On another unix the bind and the chmod stay as before. A refused `listen` now gives `bind PATH: listen: ...`, not a bare refused bind (a finding of podnet's `listen-denied` lane, fixed in place). Tests: `tests/unix_hosts.rs`, `a_unix_listener_binds_under_landlock_with_no_refer` (a thread under a Landlock domain that handles no `REFER`, checked by a refused move of a file: the listener binds with the mode 0600, leaves no staging directory and answers a dial); before the fix it fails with podnet's error, `bind /tmp/...: Invalid cross-device link (os error 18)`. `a_refused_listen_names_listen` (a seccomp filter of one thread refuses `listen` with `EACCES`): before, `bind ...: Permission denied`. `src/unix/tests.rs`, `the_mode_of_the_socket_is_the_mode_of_its_file_at_the_bind` (the modes 0600, 0640 and 0700 against the umask of `/proc/self/status`) and `a_bind_with_the_mode_refuses_an_existing_path`. The plants `link-unix-exdev-fatal` (no `EXDEV` branch), `link-unix-mode-after-bind` (no `fchmod`) and `link-unix-listen-named-bind` (the bind of tokio) are caught. The tests pass as root and as uid 65534. podnet can drop its wait on PodUtils/podnet@T-380 and take the tag; its own `bind_with_mode` in `podnet-netns` stays its own. Release notes: `pod-link`: the unix carrier listens under Landlock with no `REFER` (the labs `no-tun`, `root-unreadable`, `proc-masked`, the `bind-denied` host of podrunner); each listener of the unix carrier on Linux has the mode 0600 from its bind on; a refused `listen` names `listen`.

# T-743: `pod-hold`: the netns backend needs a UDP socket for its interface ioctls, which `udp-denied` refuses (PodUtils/podnet#592)

**Source:** PodUtils/podnet#592 (PodUtils/podnet@T-381).
**Category:** defect
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

`control` of `crates/pod-hold/src/netns/tun.rs` opens `socket(AF_INET, SOCK_DGRAM)` only for the interface ioctls. In the lab `udp-denied`, `podnet pod --backend netns -- /bin/true` exits 3: "socket() for the interfaces: Operation not permitted".

## Premise

- Read: PodUtils/podnet#592: podnet measured that `SIOCGIFFLAGS` on an `AF_UNIX` datagram socket works (each interface ioctl of a socket goes to `dev_ioctl`).

## Approach

Use an `AF_UNIX` datagram socket for the interface ioctls (or fall back to it on `EPERM`/`EACCES`), with a test in the lab `udp-denied` and a plant.

## Prove

```sh
cargo test -p pod-hold --all-features
```

## Done

2026-11-16, the commit of the fix. The premise is not complete: an `AF_UNIX` socket takes only the ioctls of `dev_ioctl` (its family has no ioctl, so `sock_do_ioctl` gives the rest to `dev_ioctl`). Measured in a user and a network namespace with a TUN on kernel 6.18: `AF_UNIX` (datagram and stream) and `AF_NETLINK` give `SIOCSIFFLAGS` and `SIOCSIFMTU`, and `ENOTTY` for `SIOCSIFADDR`, `SIOCSIFNETMASK` and `SIOCADDRT`; `AF_INET` of the stream and of the datagram type, and `AF_PACKET`, give each of the five. The fix (call C723): `control` of `crates/pod-hold/src/netns/tun.rs` opens `AF_INET` of the stream type, else of the datagram type, and a failure of both names both. Test: `tests/hold.rs`, `a_namespace_makes_its_wire_where_udp_sockets_are_refused`: a child installs a seccomp filter that refuses `socket(AF_INET or AF_INET6, SOCK_DGRAM)` with `EPERM` (as the lab `udp-denied`), checks the refusal, enters a network namespace, brings up `lo` and opens the wire (`WireChoice::Auto`); exit 20 when the interfaces fail. Before the fix it fails with podnet's error, `socket() for the interfaces: Operation not permitted (os error 1)`. It passes as root (the TUN) and as uid 65534 (the veth wire); `scripts/root-netns-tests.sh` runs it as root in CI, where a skip fails. The plant `hold-interfaces-need-udp` (the datagram type alone) is caught. podnet can drop its wait on PodUtils/podnet@T-381 and take the tag. Release notes: `pod-hold`: the netns hold works where the host refuses UDP sockets (the lab `udp-denied`): the interface ioctls use a TCP socket.

2026-11-16: `ci.yml` run 43788174436 at `5792f06` failed in the job `test`: as a normal user on the runner, the namespace of the test held no `CAP_NET_ADMIN`, so `SIOCSIFFLAGS` on `lo` gave `EPERM`, and the test took that for the defect. The test now takes only "socket() for the interfaces" (the refused socket) for the defect, and an ioctl that gives `EPERM` for the host, as a skip; `scripts/root-netns-tests.sh` runs it as root and fails on a skip. The plant `hold-interfaces-need-udp` is still caught.

# T-767: `pod-session`: a resume that the client calls done, after which the session never moves (PodUtils/podssh#283)

**Source:** PodUtils/podssh#283 (COORD-091 of podssh; PodUtils/podssh@T-780).
**Category:** defect
**Milestone:** M2
**Priority:** P1
**Effort:** M
**Status:** partial

## Problem

podssh: the m6 step of its gate (`cargo test -p podssh-cli --features
podssh-cli/iroh-test --test m6_exit -- --ignored --test-threads 1`) failed
six times since 2026-11-13; each time one test hangs to its limit of 600 s,
a different test each time, and its rerun passes. In the newest failure (run
43823925587 of podssh, at podcrates `v0.60.0`, test
`the_layer_survives_a_stopped_relay_host`) the client said "the new link
ended before the far end greeted; trying again in 1.1 s", then "the session
resumed after 1.2 s, 65536 bytes sent again"; after it, nothing for 600 s:
no loss of the new link (its heartbeat was answered), no session byte
moved, and the node's own link to the relay came back. In another failure
the client said "a spare link answered; resuming the session", then "the
session resumed after 0.0 s, 688128 bytes sent again". The replay of the
sixth failure was 64 KiB, below a pair's window of 768 KiB. podssh read
`pump/up.rs`, `pump/down.rs`, `pump/outbox.rs`, `link.rs`, `keep.rs`
(`run_resumed`, `take`) and `resume.rs` (`resume_loop`) and found no wait
that holds with no `ACK` due.

The asks: (1) a line of the far end at each resume, at the node's default
level or behind a setting, with its number, the offsets of both ends, the
bytes sent again, and whether an old link was stopped or an older resume
gave way; (2) a look at the case of the sixth failure: a new link that the
relay takes and ends before the far end greets, then a second link that
resumes 1.1 s later, at the same resume deadline.

The consumer amended the issue (2026-11-16 09:00 UTC): "It fails here too:
the test fails off the gate now, on a cloud host. `cargo test -p
podssh-cli --features podssh-cli/iroh-test --test m6_exit -- --ignored
--exact the_layer_survives_a_stopped_relay_host` at `f5b4f43`, 15 runs one
after another. Run 8 failed (600 s); each other run took 15 s. The client:
it resumed twice in a row, 'after 0.0 s, 98304 bytes sent again', then
'after 0.0 s, 130126 bytes sent again', then said nothing. The client's
proxy (after the faults' clock started): one CONNECT before the RST of the
open tunnels, and one CONNECT after it. The node's proxy: one CONNECT after
the RST and no other. That one is the node's link to the relay ('online
again'). So the node opened no new connection for the resumed session, and
yet the client heard a greeting and the resume's proof. The question: which
way carried the node's half of that second resume? If it is a way that no
longer reaches the node's live session, the client would hear a resume as
done and then nothing." The changed asks: (1) the far end's line also names
which link (connection) carries the resume; (2) "a look at the way a resume
reaches the node when the relay's open tunnels are cut: in the failure
above, two resumes came quickly, and the node opened no new connection for
either."

## Premise

- Read: PodUtils/podssh#283.
- Not measured: the failure here; podssh saw none in about 40 runs on a
  cloud host.

## Approach

(1) The far end writes one event at each resume (`session.resume` of the
caller's sink): its number, the offset that each end acknowledged, the
bytes that it sends again, and whether it stopped an old link or an older
resume gave way. (2) A test of `pod-session` that reproduces the sixth
case on the loopback: a first resume whose link ends before the greeting
of the far end, a second resume 1.1 s later, then bytes both ways; and the
same with the old link still alive. Fix the cause that the test shows.

Progress (2026-11-16):

- Ask 1 is done. `Keeper::run_resumed` sends one event `session.resume`
  (`keep::RESUME`, level `Info`) to the sink of `Keeper::with_tell` at each
  resume: its number, the link's name (`Accepted::by`,
  `far::serve_admitted_by`; pod-relay's node names its relay session),
  the client's offset from its `PROOF`, the far end's offset now and in its
  `ACCEPT`, the bytes not yet written to the target, the bytes sent again,
  and what held the session (no link, the first link, the link of resume
  M). A resume that does not take the session says why (an older resume
  gives way to resume M; the session ended; the old link did not stop; the
  offsets not kept). `crates/pod-session/README.md` gives the line. The
  plant `session-t767-stop-unsaid` (no record of the stopped link) is
  caught by `tests/session_resume_again.rs`.
- Ask 2: `tests/session_resume_again.rs` runs the sixth case (a first new
  link ends before the greeting, a second resumes after the backoff, bytes
  both ways on the pair's window) with the old link ended at both ends, and
  with it still alive at the far end. `tests/session_resume_twice.rs` runs
  the amended case: the relay host stops (its link silent, open at both
  ends), a spare resumes, the relay resets each open link at once, and the
  client resumes again in less than 1 ms; the two resend counts are 0.0 s
  apart and the second is not a multiple of 32 KiB, as in run 8. With the
  final tests, 100 runs of the three tests (50 on an idle host, 50 with each
  of the 4 cores busy) moved the bytes whole each time; in 4 of them the
  spare came only after 20 s (T-768). In an earlier loop, one run showed a
  resume that the client called
  done and that the far end did not take: the far end's resume 1 still
  waited for the old link to stop when the reset ended its link, and the
  client's resume 2 came first, so resume 1 gave way; resume 2 took the
  session and the bytes moved. That is the rule of the newest resume, not
  a hang.
- Ruled out by reading: a lost wake of `room` (the reader's `notify_one`
  keeps a permit), of the writer (`Outbox::wake`) and of the stop of a link
  (`notify_one` each 50 ms in `take`); a lost `ACK` (each change of the
  bytes delivered is followed by the reader's check, and the writer's `ACK`
  takes the newest offset); a gap between the two ends' offsets (the
  client's offset is exact; the far end's `ACCEPT` can be lower than what
  it has at the take, which only resends bytes that it drops, and its
  `PONG` acknowledges them within one heartbeat); a deadlock of the locks
  (one order: slots or sessions, then state); an old link that does not
  stop (each wait of the pump is bounded or watches the stop); a resume of
  a newer number before an older one at the client (the far end numbers
  each resume before its `ACCEPT`); a target of pod-relay that keeps the
  waker of an old task (each is tokio's `DuplexStream` or `TcpStream`). The
  first new link that ended before the greeting touches no state of the
  session at either end.
- Open: no test here hangs. At the next failure, the node's line names the
  relay session of each resume. If the newest resume's line says that it
  took the session, the far end carries it on that relay session: compare
  its `ACCEPT` offset with the client's "N bytes sent again" (the client's
  sent offset less it) and the relay's log of that session. If no line of
  the newest resume comes, its handshake answered on a link whose task did
  not reach `run_resumed`. If its line says that it gave way, a newer
  handshake came that the client did not make.
- The seventh failure (podssh at `v0.63.0`, `5340fbb`, gate run
  43870922795, test `the_layer_survives_a_stall_of_3_minutes`): the node's
  line says resume 1, no link carried the session, the client has
  5,040,486 bytes of the node and the node sends 0 again; the node wrote
  7,814,432 bytes of the client to the target (an echo). So the node's
  reader of the target had no byte in flight at the resume: with a window
  of 768 KiB it would hold up to that much, and it held none. The reader
  stopped before the stall, with room, and the resume did not move it.
  Ask 3 of podssh: a line of that reader's wait. Done (2026-11-16): the
  far end sends `session.stall` (`pump::STALL`, level `Info`, sink of
  `Keeper::with_tell`) once for each wait of the reader of the target
  that holds 20 s (`pump::STALL_AFTER`): for room (the replay buffer or
  the window), for the writer of the link, for room of the records sent
  again, or on a target that sent no byte while it took bytes of the
  client; with the offsets sent and acknowledged, the bytes kept, the room
  and the window. The resume line ends with the room of the replay buffer
  and the window. `Watch` moved to `pump/watch.rs` (the limit of 500
  lines). Test: `pump::stall::tests` on a paused clock; plant
  `session-t767-stall-twice`. The next failure names the wait, or says
  by no line that the reader waits on the target with no byte taken.

## Prove

```sh
cargo test -p pod-session --all-features
```

- 2026-11-19: podssh at `bcd2b2f` (podcrates `v0.73.0`: the guard of T-939 and the line of T-945) ran its loop of `m6_exit` on the iroh road 6 times, and each passed (PodUtils/podssh#320). The eighth failure did not come again; the entry stays partial until a run of the gate shows the cause or the gate holds over more runs.
- 2026-11-19: the eleventh failure of podssh's gate (run 44506565889, `iroh::the_iroh_road_survives_a_new_address`, podcrates `v0.76.0`) names the reader that stopped: podssh's watch of its pipe said "SSH read nothing for 20 s while the channel waits to give it bytes", and the layer said that the application took no byte for 21 s while 961099 bytes waited. So podssh's own SSH client held the session; the layer (T-945) and the channel of `pod-e2e` (T-959) waited on it. podssh takes the rest (PodUtils/podssh@T-780, a watch of SSH's own write to the channel) and writes on PodUtils/podssh#283 if the next lines point back at a crate. Asks 2 to 4 of #283 stay open there with no new work for podcrates; this entry stays partial until podssh closes them or a run names a crate.


# T-774: `pod-link`: a normative document of the wire, with the order of the checks and the rule of the timers, held to the code by a test

**Source:** The study of zmux (`docs/research/zmux.md`, lesson 1; the ruling "zmux", 2026-11-16).
**Category:** docs
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The rules of the wire of `pod-link` are in the README of the crate
(sections "The wire" and "Settings") and in the code. Two kinds of rule
are only in the code. (1) The order of the checks: when a frame breaks
two rules, the code decides which error the link gives. `shape` in
`crates/pod-link/src/frame.rs` checks the type, then id 0 on a stream
frame, then an id on a link frame, then the length; `on_data` in
`crates/pod-link/src/inbound.rs` checks the window of the link, then a
stream that never existed, then `DATA` after `CLOSE`, then the window of
the stream. (2) The rule of the timers: the keepalive fires at the first
moment where `now >= last_heard + dead_after`
(`crates/pod-link/src/link.rs:254` says "if now >= dead {"). No text says
which rules are normative, and no test holds the README to the
constants. The study found that a reader of the README takes the wrong
model: it looked for a varint and a "bad reset code", and `pod-link` has
neither (fixed widths, big endian; each reset code is valid).

## Premise

- Read: `crates/pod-link/README.md`, `crates/pod-link/src/frame.rs`,
  `state.rs`, `inbound.rs`, `link.rs`, `settings.rs`, `error.rs`; the
  report of the study (`docs/research/zmux.md`, section 1, "The method of
  test").
- Read: `crates/pod-job/src/schemas.rs` writes its schemas when a
  variable is set and else fails on a difference: the pattern of a drift
  test in this workspace.
- Not measured: a second implementation from the README alone.

## Approach

Move the link part of "The wire" and the table "Settings" of the README
into one document of the crate, crates/pod-link/WIRE.md, in ASD-STE100
(call C750). The README keeps one paragraph and a link to it.

- A status line: the document is normative for the link of `pod-link`,
  and names the version of the crate that it describes.
- Each rule once, with an id (W-01, W-02, ...), and each other text
  names the id. The carriers (tcp, ws, h2, s3, split) and the ARQ stay
  in the README: they are the wire of a carrier, not of the link.
- A section "The order of the checks": the decoder's order, then the
  link's order for each frame type, as the code does it now. A change of
  the order changes the document in the same commit.
- A section "Time": each duration T fires at the first moment where
  `now >= t0 + T`; t0 of each timer (the last byte heard for `dead_after`;
  the later of the last byte heard and the last `PING` for `keepalive`).
- A section "Constants": the type bytes, the reset and `GOAWAY` codes
  with names, `INITIAL_WINDOW`, the limits of each setting, the limit of
  8192 frames that wait (`CONTROL_LIMIT`) and the queue of `DATA`
  (`queue_limit`).
- A test of the crate (wire_doc) reads the tables of the document and
  compares each value with the constant of the crate (`frame::kind`,
  `ResetCode`, `GoAwayCode`, `settings`). A difference fails with the
  row and the constant. Make `CONTROL_LIMIT` and the queue limit
  readable by the test (a `pub` constant or a function).

## Prove

```sh
cargo test -p pod-link --all-features --test wire_doc
cargo todo check
```

A changed type byte in the document, and a changed limit in
`settings.rs`, each make the test fail with the row named.

## Done

2026-11-16, the commit of the change. `crates/pod-link/WIRE.md` is the
normative wire of the link: 35 rules (W-01 to W-35; a rule marked "(this
crate)" is the behaviour of this implementation, which a peer does not
check), the order of the checks of the decoder and of the link for each
frame type, the section "Time" (`now >= t0 + T`; the t0 of each timer;
`dead_after` before the `PING` at the same moment), and the tables of the
frame types, the codes, the limits and the settings. The README keeps
the carriers and points to it. The status line names the protocol
version (the last byte of the preface), not the version of the crate: a
version of the crate in the text would change at each release with no
change of the wire. `CONTROL_LIMIT`, `MAX_CREDIT`, `MIN_QUEUE` and
`Settings::queue_limit` are public now. The test `tests/wire_doc.rs`
checks each row of the frame types against the decoder (the type byte,
the id 0 and an id, the smallest and largest body and one past each),
each code and each limit against its constant (both ways: no row with no
constant, no constant with no row), each setting against
`Settings::default` and `Settings::check` at each limit and one past it,
the order of the rule ids, each reference to a rule, the range in the
README and the protocol version. A type byte of `RESET` changed to 9 in
the document fails with "row RESET: the type 9, the crate has 5";
`MAX_STREAMS` changed to 4000 in `settings.rs` fails with "row
MAX_STREAMS: 4096, the crate has 4000" and "row max_streams: check of
4096 gives an error". Release notes: `pod-link`: `WIRE.md`, the normative
wire of the link; `CONTROL_LIMIT`, `MAX_CREDIT`, `MIN_QUEUE` and
`Settings::queue_limit` are public.

# T-775: `pod-link`: a table of frames in bytes with the result of each, read by one test

**Source:** The study of zmux (`docs/research/zmux.md`, lesson 2; the ruling "zmux", 2026-11-16).
**Category:** chore
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The tests of the frame codec are Rust code with literal bytes
(`crates/pod-link/tests/frames.rs`). The fuzz seeds of `link_frame`
(`fuzz/seeds/link_frame/`, 6 files) are bytes with no expected result:
`fuzz/src/targets/link.rs` checks invariants (a decoded frame encodes to
bytes that decode to the same frame), not the frames or the error of
each input. No data file gives, for each input, the frames that the
decoder must give or the error that it must give by name. Such a file
can be read by a second reader of the wire and by T-776, and it is the
evidence that each row of T-774 has a case.

## Premise

- Read: `crates/pod-link/src/frame.rs` (`FrameError` has 8 variants:
  `Preface`, `UnknownKind`, `TooLong`, `BadLength`, `LinkId`, `StreamId`,
  `EmptyData`, `ZeroCredit`); `crates/pod-link/tests/frames.rs`;
  `fuzz/src/targets/link.rs`; `fuzz/src/arq_seeds.rs` (a test that the
  seeds on disk are the ones that a writer makes).
- Read: the decoder accepts each stream id and each reset code. A wrong
  parity, an id used again and a frame of a stream that never existed
  are errors of the link (`crates/pod-link/src/inbound.rs`), not of the
  decoder: they belong to T-776.
- Not measured: the number of rows that cover each rule of T-774.

## Approach

One text file, crates/pod-link/tests/wire/frames.txt, in podcrates' own
form (call C750): a line of the count, then one case on each line:

```text
ID  PREFACE  SPLIT  HEX  =>  RESULT
```

`PREFACE` is `yes` or `no` (`Decoder::with_preface` or `new`). `SPLIT`
is the size of each push (1 for one byte at a time, `all` for one push).
`RESULT` is `ok` and the frames in the `Debug` form of `Frame`, or `err`
and the variant of `FrameError` with its fields. A comment line starts
with `#` and names the rule of T-774. Start at 20 valid and 20 invalid
cases: each frame type at its smallest and largest body, a `DATA` of
1 MiB, a header split at each byte, two frames in one push, the
preface cut and wrong, each variant of `FrameError`, a type byte that
fails before its header is whole, a body over 1 MiB that fails at its
header.

A test of the crate (wire_frames) reads the file, runs each case, and
gives the id of a case that fails. It fails when the count line and the
cases differ. Each valid case also encodes: the bytes of
`Frame::encode` are the bytes of the case (the fixed widths make the
encoding unique). An ignored writer test puts the bytes of each case
into `fuzz/seeds/link_frame/`, and a test checks that the seeds on disk
are the ones it writes, as `fuzz/src/arq_seeds.rs` does.

## Prove

```sh
cargo test -p pod-link --all-features --test wire_frames
cargo test --manifest-path fuzz/Cargo.toml --lib
```

A case with a wrong result, and a count line one too high, each make
the test fail with the id or the count named.

## Done

2026-11-16, the commit of the change. `crates/pod-link/tests/wire/frames.txt`
holds 55 cases (26 valid, 29 invalid) in the form of the entry, with one
addition: a HEX field is pieces joined by a dot, and a piece `bb*N` is N
copies, so that a body of 1 MiB fits on one line; the body of a `Data`
in a result has the same form (a run of 4 or more equal bytes is one
piece). A result `err` lists the frames that came before the error after
the word "after". The cases: each type at its smallest and largest body,
a `DATA` of 1 MiB, each code that the protocol does not name, the preface
whole, one byte at a time and cut, a header split at each byte and at 3,
two frames in one push, the life of a stream in one push, a header, a
body and a frame cut, the preface wrong at its first and its last byte,
each variant of `FrameError`, a type byte that fails alone and after a
frame, a body over 1 MiB and the largest length at the header, the order
type, id, length of the decoder, and an error that stays. The test
`tests/wire_frames.rs` runs each case, checks W-08 (the same error again,
no byte taken after it), encodes the frames of each valid case to the
bytes that the decoder took, and fails on a count line that differs. A
result `Close(5)` for the bytes of `Close(4)` fails with "close: the
decoder gives ok Close(4)"; a count of 55 for 54 cases fails with "the
count line says Some(55), the file has 54 cases". `fuzz/src/frame_seeds.rs`
writes one seed of `link_frame` for each case with at most 4 KiB of input
(54 seeds; the case of 1 MiB stays out of the tree), and its test checks
that the directory holds these seeds and no other; the 6 seeds written by
hand went, as the cases hold each of them.

# T-776: `pod-link`: scripted cases of a link on a paused clock, and the table of the classes of defects

**Source:** The study of zmux (`docs/research/zmux.md`, lesson 3; the ruling "zmux", 2026-11-16).
**Category:** chore
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

The rules of the link (credit, parity, half close, reset, `GOAWAY`, the
keepalive, the limit of the frames that wait) are tested by Rust tests
of the crate, each written by hand (`crates/pod-link/tests/limits.rs`,
`streams.rs`, `windows.rs`, `keepalive.rs`). No list says which rule of
the wire each test holds, and no list says, for each kind of defect that
a multiplexer can have (a credit that is lost, a stream that never
ends, a frame of a gone stream that ends the link), whether a test
catches it or the design makes it impossible. The study showed a form
for both: a script of steps that a runner plays against one end, with a
peer whose bytes are fully scripted and a clock that moves only at a
step, and a table of classes with a disposition for each.

## Premise

- Read: `crates/pod-link/tests/common/mod.rs` has `RawPeer`, a peer that
  sends and reads raw frames against one `Link`;
  `crates/pod-link/tests/keepalive.rs` runs on a paused clock
  (`start_paused`). The link has no source of chance: the value of a
  `PING` is a counter.
- Read: the report of the study, the six obligations of a runner
  (`docs/research/zmux.md`, section 1).
- Not measured: how many of the present tests a case can replace.

## Approach

Cases as text files under crates/pod-link/tests/cases/, one case for
each file, in podcrates' own form (call C750). A header names the role
of the end under test, its settings when not the default, the rules of
T-774 that the case holds, and the class of defect. Steps:
`send FRAME` (the peer writes a frame), `expect FRAME` (the next frame
from the end), `expect-quiet` (no frame), `open`, `accept`,
`write STREAM BYTES`, `read STREAM COUNT`, `close STREAM`,
`reset STREAM CODE`, `advance DURATION`, `expect-end ERROR`.

The runner (a test cases, over `RawPeer`):

- applies the settings before the first step;
- runs the steps in order and gives the file and the step at a failure;
- moves the clock only at `advance`;
- lets the end run until it is idle before each `expect`;
- takes the end of the link as an event; only an `expect` decides;
- fails when the link ended and no `expect-end` covered it;
- asserts the count of the cases (a line in a file of the directory).

Start with 20 cases: open and data both ways, half close, reset by each
end, a window that runs out and comes back, the doubling of a window,
the limit of streams (`REFUSED`), `GOAWAY` with streams open, the dead
time and the `PING`s before it, a frame of a gone stream that is
dropped and gives its credit back, a frame of a stream that never
existed, a wrong parity, an id used again, `DATA` after `CLOSE`, `DATA`
past each window, the order of two errors in one frame (T-774), and a
peer that sends `PING`s and reads nothing (8192 frames).

The table of the classes goes into crates/pod-link/WIRE.md (T-774): the
classes (credit, streams, close, time, memory), each class with a case,
or with the invariant of the code that makes it impossible. A fix of a
defect of the link adds a case and its row in the same commit.

## Prove

```sh
cargo test -p pod-link --all-features --test cases
```

A case with a wrong `expect`, and a count one too high, each make the
test fail with the file and the step named. Each class of the table
names a case that exists or an invariant.

## Start condition

T-774 gives the ids of the rules.

## Done

2026-11-16, the commit of the change. `crates/pod-link/tests/cases/`
holds 24 cases (`NAME.case`, the form in its `README.md`, the count in
`count`), and `tests/cases.rs` runs them: each case on a fresh
current-thread runtime with a paused clock, against a peer of raw bytes
over the memory carrier. The runner polls each operation once and never
waits, so the clock moves only at `advance`; before each check it gives
the bytes of the peer and the writes that wait for credit, and lets the
end run until nothing moves for 64 rounds. After the end of the link only
`expect` steps can follow until an `expect-end`; an end that no
`expect-end` covers fails. Two steps more than the entry: `expect-eof`
(the end ended its carrier) and `drop` (the drop of the handle of a
stream), and the header key `ring` (the size of the carrier, for the case
of a peer that does not read). The cases: the 20 of the entry, with the
order of two errors in two forms (the window of the link before a stream
that never existed; a `CLOSE` before the window of the stream), and the
timers moved by a byte of the peer and an overflow of credit. The
helpers of the text forms are in `tests/common/wire.rs`, shared with
`wire_frames`. `WIRE.md` has the section "The classes of defects": 22
rows in 5 classes, each with its cases, or with the invariant of the
code (the queue of `DATA`, the length at the header); a test checks that
each case of a row exists. A wrong `expect` fails with
"window-doubles.case: line 13 (expect Window(1, 16384)): the end sent
Window(1, 24576)"; a count of 24 for 23 cases fails with
"tests/cases/count says 24, the directory has 23 cases". No case found a
defect of the link.

# T-777: `pod-link`: a committed measurement of the link: throughput, round trips, allocations for each frame and memory

**Source:** The study of zmux (`docs/research/zmux.md`, lesson 4 and section 2; the ruling "zmux", 2026-11-16).
**Category:** measurement
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podcrates has no benchmark: no `benches/` directory, no `[[bench]]` and
no measurement of the multiplexer of `pod-link` in the tree. A change of
the data path (T-778) or of the writer's queue (T-442) has no number to
compare with. The study measured the link once, with a harness that is
not in this repository, on one host: bulk on one stream 1.52 to
2.19 GB/s (median 1.75), on eight streams 1.72 to 2.63 GB/s (median
2.51), a round trip of 64 bytes p50 18 to 26 us idle and 18 to 23 us
under bulk, all on the memory carrier. It did not measure allocations,
memory or the tcp carrier. The zmux benchmark reported allocations for
each frame and the live heap of a session, which do not change with the
speed of the host.

## Premise

- Measured by the study (2026-11-16, one host, 8 vCPU): the numbers
  above; the bulk samples vary by up to about 50 % between runs.
- Read: T-525 recorded the throughput of the poll carrier on the
  loopback; nothing else records a number of the link.
- Not measured: allocations, memory, the tcp carrier, an emulated
  target.

## Approach

podcrates' own harness (call C751): one ignored test, measure, in
`pod-link`, with no new dependency.

- One line for each result: `MEASURE scen=NAME key=value ...`, with the
  carrier, the settings that differ from the default, and the run.
- Scenarios: bulk on 1 stream and on 8 streams (a byte counts when the
  reader took it and acknowledged the end); a round trip of 64 bytes,
  idle and under a bulk stream on the same link (each stream says its
  role in its first bytes, not by the order of accept); each on the
  memory carrier, and on the tcp carrier over the loopback when the
  feature `tcp` is on.
- A counting allocator in the test binary alone (`#[global_allocator]`
  of the test file; design rule 7 keeps it out of the crate): the
  allocations and the bytes for each `DATA` frame of a bulk run, and the
  peak of the live bytes of the two ends.
- The wake latency of the host (p50, p99 of a task that a `Notify`
  wakes), so that a later gate on latency can be relative to the host.
- Three runs of each bulk scenario; the median and the range.
- The README of the crate gets a section "Measurement": the command,
  the meaning of each key, and one baseline (the host class, the date,
  the commit). No gate on throughput. The gate on allocations comes with
  T-778.

## Prove

```sh
cargo test --release -p pod-link --all-features --test measure -- --ignored --nocapture
```

The output has one `MEASURE` line for each scenario and run, each with
the allocations for each frame and the peak of the live bytes.

## Done

2026-11-16, the commit of the change. `crates/pod-link/tests/measure.rs`
(one ignored test, `measure`, no new dependency) writes `MEASURE` lines
for the scenarios of the entry: `wake`, `bulk-1` and `bulk-8` (256 MiB,
three runs and a summary with the median and the range), `rtt-idle` and
`rtt-bulk` (2000 round trips of 64 bytes), on the memory carrier and on
the tcp carrier over the loopback. A stream gives its role in its first
byte; a bulk byte counts when the far reader sent back the count at the
end of the stream. The counting allocator is the `#[global_allocator]`
of the test binary. The README of the crate has the section
"Measurement": the command, the keys and the baseline of a cloud session
of 4 threads (release mode, the code of `996f55d`): memory bulk-1 2.29
GB/s (2.24 to 2.38), bulk-8 2.17 GB/s, 2.58 and 2.81 allocations and 33
KiB of allocations for each `DATA` frame of 16 KiB, a peak of 4.5 and 9.4
MiB of live heap, round trips p50 36.5 us idle and 60.8 us under bulk;
tcp bulk-1 1.00 GB/s, bulk-8 1.19 GB/s, 5.8 and 6.2 allocations for each
frame, round trips p50 61.3 us idle and 232.0 us under bulk; the wake
latency p50 19.4 us, p99 59.5 us. The frames count is the frames of
`frame_size` that the bytes need, not a count of the link: the link has
no counter of frames. T-778 takes the allocations for each frame as its
gate.

# T-778: `pod-link`: no allocation for each `DATA` frame, and fewer copies of each byte

**Source:** The study of zmux (`docs/research/zmux.md`, lesson 5; the ruling "zmux", 2026-11-16).
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** M
**Status:** done

## Problem

The data path of a link allocates a `Vec` for each `DATA` frame at each
end, and copies each byte six times inside the crate.
`crates/pod-link/src/stream.rs:135` says
"self.push_out(Frame::Data(id, bytes[..n].to_vec()));"; the writer then
encodes the frame into its batch. At the reader, the bytes of the
carrier go into the decoder's buffer (`Decoder::push`), the decoder
copies the body into a new `Vec` (`crates/pod-link/src/frame.rs:265`
says "kind::DATA => Frame::Data(id, body.to_vec()),"), `on_data` copies
it into the buffer of the stream (`crates/pod-link/src/inbound.rs:121`),
and a read copies it into the caller's buffer. The Rust core of zmux
measured 0.0000 allocations for each frame on its bulk path, and
0.0005 to 0.0007 over TCP.

## Premise

- Read: the files above at `567a56c`.
- Not measured: the cost of the allocations and the copies; T-777
  measures it.

## Approach

After T-777 gives the baseline:

1. The decoder gives a `DATA` body as a slice of its own buffer (a
   borrowed frame), and `on_data` copies it straight into the buffer of
   the stream: no `Vec` for each body.
2. The read loop reads from the carrier into the free end of the
   decoder's buffer: no second buffer.
3. A write of a stream puts the header and the bytes straight into the
   queue of the link (a byte queue that keeps the bounds of each frame
   and the count of `DATA` bytes): no `Vec` for each frame, one copy
   less.
4. `Frame` and `Decoder::next` stay for the fuzz target and for the
   tests, or change with them (design rule 3).

Keep each change only when T-777 shows a gain. A test of the crate then
asserts the allocations for each `DATA` frame of a bulk run of 64 MiB
with the counting allocator: 0 after the first frames, with a margin of
16 allocations for the whole run.

## Prove

```sh
cargo test --release -p pod-link --all-features --test measure -- --ignored --nocapture
cargo test -p pod-link --all-features
```

The allocations for each `DATA` frame are 0 at steady state; the median
of bulk on 1 stream is not lower than the baseline of T-777 on the same
host. A planted `to_vec` in `on_data` makes the test of allocations fail.

## Start condition

T-777 is done.

## Done

2026-11-16, the commit of the change. Three changes of the data path,
each measured with `tests/measure.rs` (release mode, the host of the
baseline of T-777; memory carrier, allocations for each `DATA` frame of
16 KiB on 1 stream):

1. `Decoder::next_borrowed` gives a `DATA` body as a slice of the buffer
   of the decoder (`frame::Borrowed`), and the read loop gives it to
   `State::on_data`, which copies it once into the buffer of the stream.
   `Decoder::next` stays, with a copy, for the fuzz target and the tests.
   2.58 to 1.61 allocations for each frame; tcp 5.8 to 4.9.
2. The writer keeps the bodies of the `DATA` frames that it sent (at most
   8), and a write of a stream takes one (`State::data`): 1.61 to 0.62.
3. A probe of the sizes found the rest: two allocations for each read of
   the carrier, not for each frame. `Shared::with` took the list of the
   wakers out of the lock with `mem::take`, and `unblock` the list of the
   writers that wait; each next push allocated again. The wakers now go
   out through an array of 8 (the list keeps its capacity), and `unblock`
   gives its list back: 0.62 to 0.00 (about 70 allocations for a run of
   256 MiB, the harness included).

The result: memory 0.00 allocations and 516 bytes allocated for each
frame (from 2.58 and 33 KiB), with 8 streams 0.01; tcp 3.2 (from 5.8),
the rest in the TLS of the carrier (T-798). The throughput did not move
past the noise of this host: the old code gives a median of bulk-1 of
2.29, 2.18 and 2.49 GB/s in three sessions of three runs, the new code
2.20, 2.25 and 1.98 GB/s; bulk-8 2.17, 2.29, 2.15 against 2.18, 2.08,
2.30. The Prove asks for a median not lower than the baseline: with a
spread of 12 % between runs of one code, this host cannot show a change
of 4 %, so the result is "no change that the host can show", not a gain.
The test `tests/allocations.rs` (one test in its binary, the counting
allocator of the binary): a bulk run of 64 MiB over the memory carrier on
a current-thread runtime makes at most 16 allocations after its first 1
MiB; it makes 0. The plant `link-data-body-copied` (a `to_vec` of the
body in `on_data`) fails it with "4096 allocations for 4096 DATA frames
of 16 KiB: over 16". Steps 2 and 3 of the Approach (a read of the carrier
into the free end of the buffer of the decoder; a byte queue of the
writer with the header and the bytes of each frame) are not taken: the
allocations are 0 without them, and each removes a copy whose gain this
host cannot measure; T-799 holds them. Release notes: `pod-link`: the
data path allocates nothing for each `DATA` frame at steady state;
`Decoder::next_borrowed` and `frame::Borrowed` give a `DATA` body with no
copy.

# T-768: `pod-session`: a spare waits 20 s, not 1 s to 3 s, when the client's writer is held by a full link

**Source:** T-767 (a run of `tests/session_resume_twice.rs`).
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** open

## Problem

A spare link opens when a question of the client (`DATA` that the far end
acknowledges, a `PING`) waited `spare_after` (1 s to 3 s) with nothing from
the far end, or when nothing came for `SPARE_QUIET` (20 s). The writer of
the pump (`pump/outbox.rs`, `write`) marks a question with
`Hearing::asked` only after the write of the record ended. A relay host that
stopped holds the link open and reads nothing: the link's buffers fill, the
writer waits in its write of a `DATA` batch, and no question is marked; the
heartbeat's `PING` waits behind it too. The spare then opens only at
`SPARE_QUIET`. In one run of `two_resumes_in_quick_succession_after_the_open_links_are_cut_move_bytes_both_ways`
the spare came after 20.03 s; in the other runs, after 1.0 s.

## Premise

- Read: `pump/outbox.rs` (`write`, `put`), `hearing.rs`, `beside.rs`
  (`silence`).
- Measured: the client's notes of one run of
  `tests/session_resume_twice.rs` (`Spare { quiet: 20.03 s }`).
- Not measured: how often a stalled relay fills the link before the first
  question is marked.

## Approach

Mark the question when the writer starts the write of a `DATA` batch that
the far end acknowledges, or of a `PING`, not when the write ends: a write
that a full link holds is a question that waits. A far end that only reads
slowly still sends its `ACK`s and `PING`s, and each of them answers the
question. A test: a link whose relay stops reading while the client sends
must get a spare within `SPARE_AFTER`, with a plant that marks the question
after the write.

## Prove

```sh
cargo test -p pod-session --all-features --test session_spare
```

# T-798: `pod-link`: the tcp carrier allocates about 3 times for each `DATA` frame

**Source:** T-778 (the measurement of `tests/measure.rs`).
**Category:** measurement
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

After T-778 the link allocates nothing for each `DATA` frame over the
memory carrier, but a bulk run over the tcp carrier (TLS 1.3 over the
loopback) makes 3.2 allocations and about 40 KiB of allocations for each
frame of 16 KiB (`tests/measure.rs`, bulk-1, tcp). The link is the same,
so the allocations are in the carrier: tokio-rustls, rustls (a record of
TLS is at most 16 KiB; each encryption and decryption can allocate) or
the split halves of tokio.

## Premise

- Measured: `tests/measure.rs` in release mode on a cloud session of 4
  threads: tcp bulk-1 3.18 to 3.44 allocations for each frame, bulk-8 2.90
  to 3.14.
- Not measured: which layer allocates.

## Approach

A probe of the sizes and of the call sites of the allocations of a tcp
run (a counting allocator with a histogram of the sizes, as T-778 did;
then a backtrace of each size class). Where rustls allocates for each
record, read whether a setting of rustls (a buffer limit, the
`unbuffered` API) or a change of the carrier (a write of whole records,
a reuse of the plain buffer) removes it. Keep a change only when the
measurement shows fewer allocations and no lower throughput.

## Prove

```sh
cargo test --release -p pod-link --all-features --test measure -- --ignored --nocapture
```

# T-799: `pod-link`: two copies less of each byte: a read into the buffer of the decoder, and a writer queue of bytes

**Source:** T-778 (steps 2 and 3 of its Approach, not taken).
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

After T-778 each byte of a stream is copied five times inside the
crate: the writer copies the bytes of a write into a body of a `DATA`
frame (`State::data`), then the body into its batch (`take_batch`); the
reader reads the carrier into a buffer of 64 KiB (`read_loop`), pushes it
into the buffer of the decoder (`Decoder::push`), then copies the body
into the buffer of the stream (`on_data`), and a read copies it to the
caller.

## Premise

- Read: `crates/pod-link/src/link.rs` (`read_loop`, `write_loop`),
  `state.rs` (`data`, `take_batch`), `frame.rs` (`Decoder::push`).
- Measured (T-778): the bulk throughput of one code varies by 12 % between
  runs on a cloud session of 4 threads; a change of one copy is under
  that.

## Approach

(1) The read loop reads from the carrier into the free end of the buffer
of the decoder (a `Decoder::spare(n) -> &mut [u8]` and a `filled(n)`): no
second buffer. (2) A write of a stream puts the header and the bytes
straight into a byte queue of the link, which keeps the bounds of each
frame and the count of `DATA` bytes; the writer takes whole frames from
it. Measure on a host with a stable clock (the bulk spread under 3 %, for
example a runner with fixed CPUs, or `perf stat` cycles for each byte), and
keep each change only when it shows a gain.

## Prove

```sh
cargo test --release -p pod-link --all-features --test measure -- --ignored --nocapture
cargo test -p pod-link --all-features
```


# T-801: `pod-relay`: a link of the resumable layer over the HTTP road, where the way refuses the upgrade of a link (PodUtils/podssh#287)

**Source:** PodUtils/podssh#287 (COORD-095 of podssh, PodUtils/podssh@T-798).
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

podssh: "In `ws-upgrade-refused` the session itself reaches podssh's relay
role over the HTTP road (PodUtils/podssh@T-319), but each link of the layer is a WebSocket
upgrade, so the first link is refused and the session goes on with no
resume: the cut ends it. That class is a cloud container's, where a lost
link is most common." The ask: in the client of `pod-relay`, a link of the
layer (`forward_layer::client`) over the HTTP road where the way refused
the upgrade of the link: the road opened to the endpoint of the layer of a
session (a target that names the layer, the id of the session and its
resume, beside `http_road::client::Target` of a host and a port), each new
link a new session of the road, in the forms of the road (one-POST, split,
serial), in a form of the request that the role can tell from a forward
session. podssh's role then gives such a session to its layer as one link.

## Premise

- Read: PodUtils/podssh#287; `crates/pod-relay/src/forward_layer/client.rs`
  (`Dialer::link`: each link is a WebSocket upgrade to `/v1/layer/HOST/PORT`,
  and a `pod_session::ws_link::Link` over it); `crates/pod-relay/src/http_road/client.rs`
  (`Target`, the open and the forms).
- Not measured: whether `pod_session::ws_link::Link` takes a byte stream
  of the road (it starts from a WebSocket session).

## Approach

A target of the road for the layer: `http_road::client::Target` gets the
form of a layer (`/v1/layer/HOST/PORT` as the target of the open, so the
role tells it from a forward session by its path, as it does for the
upgrade), and `forward_layer::client::Dialer::link` opens it in the form of
the road that the run learned, when the upgrade of a link is refused (an
answer of the way that is not 101, as the class `ws-upgrade-refused`
gives). The link of the layer runs over the byte stream of the road
session: where `ws_link::Link` needs a WebSocket, a link over a byte stream
in `pod-session` with the same records. A test: a link of the layer over
each form of the road, behind a stand-in that refuses each upgrade, and a
resume after a cut of the road's connections; the stand-in role of the
tests serves the layer on the road. Name the tag on the hand-off; podssh's
role then takes the session as one link.

## Prove

```sh
cargo test -p pod-relay --all-features --test http_road_layer
```

## Done

2026-11-16. The client of a link of the layer on the HTTP road, with no
break of an API that a consumer uses (C771): `http_road::client::open_layer`
opens `OPEN_LAYER` (`/v1/http/open-layer/HOST/PORT`) or `ONE_LAYER` in the
forms of the road; `layer_route` names them, and `route` gives `None`, so
that a role with no layer on the road answers `404`.
`forward_layer::Dialer::link_or_road` takes the road for a link once the
way refused the upgrade of a link (`way_refusal::upgrade_refused`, moved
from `pod-piping`, which takes it from here), with the token as the
WebSocket road gets one; each later link of the session goes on it, a new
session of the road each. `forward_layer::Link` is the WebSocket link or
the road's stream, whose failed end gives its words on the read after the
last byte. `carry_or_road` carries a session with such links; `link` and
`carry` stay the upgrade alone. Tests: `tests/http_road_layer.rs` (a
session of the layer over the split, serial and one-POST forms against a
stand-in role in memory whose far end keeps the sessions, a large write,
a cut of each session of the road, and the resume on a new one; a refused
token is final with the role's words); unit tests of the cue and of the end
of a road link. Plants `relay-layer-road-forward-path`,
`relay-layer-way-refusal-ignored`, `relay-layer-road-end-silent`. podssh's
role serves the two opens at its take; its `relay_layer.rs` then calls
`link_or_road` and `carry_or_road`.

# T-802: `pod-turn`: a NAT probe (the mapping, the filtering and the port of RFC 5780) on the caller's datagrams (GitHub #212)

**Source:** GitHub #212 (an issue from outside the team, verified and confirmed).
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** open

## Problem

No crate of podcrates classifies a NAT. `pod-turn` has the codec of STUN
(`crates/pod-turn/src/stun.rs`) and the TURN client, which reads its
mapped address; `crates/pod-services-watch/src/stun.rs` asks one server for
a Binding and gives no class. podssh's doctor has the only NAT probe
(the file `nat.rs` of `podssh-cli` in PodUtils/podssh): it binds one IPv4 socket,
stops at two answers, calls two different mapped addresses "hole punching
will not work here" and never compares the mapped port with the local port.
T-499 (NAT traversal) needs the class of each end.

## Premise

- Read: GitHub #212 (its claims checked against the tree; its "only probe"
  claim is wrong: `pod-services-watch` has a Binding client too);
  `crates/pod-turn/src/stun.rs`, `settings.rs` (`UdpSource`, `SystemUdp`),
  `client.rs`; podssh's `nat.rs` at `0370b0b`.
- Read: the issue proposes a probe that binds its own `0.0.0.0:0` socket
  (against design rule 5: the caller gives the way out) and a test
  `mapped == local` that cannot match on a socket bound to the
  unspecified address.
- Not measured: a host with two ways out (multi-WAN).

## Approach

`pod_turn::nat::probe` (feature `nat`) on the caller's datagrams (a
`UdpSource`, the system's by default): RFC 5780's tests with
OTHER-ADDRESS, RESPONSE-ORIGIN and CHANGE-REQUEST where a server gives
them, else two servers; the mapping (endpoint-independent, address- or
address-and-port-dependent), the filtering where the server can change
its address, the preservation of the port, and `Unknown` with its reason.
The local address comes from a connect of the socket, as podssh's
`own_address` does. podcrates writes its own code from the mechanism; it
takes no code of the project that the issue names. A test against a
stand-in STUN server with two addresses on the loopback, before T-499.

## Prove

```sh
cargo test -p pod-turn --all-features --test nat
```

# T-803: `pod-link`: an adaptive parity datagram for the ARQ, off on a clean link (GitHub #213)

**Source:** GitHub #213 (an issue from outside the team, verified and confirmed).
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** M
**Status:** open

## Problem

The ARQ of `pod-link` (`crates/pod-link/src/arq/`) has no forward erasure
code. A lost fragment with three or more acknowledged frames after it goes
again after a fast retransmit (`arq/send.rs`, a threshold of 3), about one
round trip; a lost fragment at the tail of a burst waits for its RTO (at
least 200 ms, and 1 s before the first sample), and the ordered stream
stalls behind it.

## Premise

- Read: GitHub #213 (its claims checked against the tree). Two claims are
  wrong: each loss does not wait for the RTO (the fast retransmit exists),
  and the ARQ is one for each link, not for each stream, so its ring is
  about 32 KiB for each link, not 8 MiB.
- Read: `arq/wire.rs` (four kinds; an unknown kind fails `decode`, and
  `on_datagram` counts it as dropped), `arq/machine.rs`, `arq/send.rs`,
  `arq/recv.rs`, `arq/settings.rs`.
- Not measured: the stall of a tail loss on a lossy carrier.

## Approach

`ArqSettings::fec` (off by default): a datagram of kind 5, `PARITY`, the
XOR of a group of DATA datagrams, sent where a moving mean of the loss is
over a bound; the receiver rebuilds one lost datagram of a group. Its
bytes count in the congestion window of T-515. Only the sender needs the
setting. podcrates writes its own code from the mechanism; it takes no
code of the project that the issue names. Tests on the lossy carrier of
the tests at 2 % and 10 % loss (the stall of a tail loss before and
after), a fuzz case of kind 5 (T-516), and a plant.

## Prove

```sh
cargo test -p pod-link --all-features --test arq_machine
```

## Start condition

T-515 is done.

# T-805: `pod-relay`: a node that gets `409` for its own lost socket gives up on that route sooner than `rejoin`, and says so (PodUtils/podnet#604)

**Source:** PodUtils/podnet#604 (COORD-098 of podnet, PodUtils/podnet@T-406, ask 2).
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podnet's live run 43912914825: the link of a node of
`pod_relay::reverse::node` ended with a reset and no Close; from then on,
each new connect of the node got `409` ("the relay still holds the node's
lost socket") for at least 150 s, and an operator's SSH reached the relay
and stopped in the key exchange, sent to the lost socket. The node
connects again until `Settings::rejoin` (600 s, `RESUME_DEADLINE`) after
the loss (`crates/pod-relay/src/reverse/node.rs`, the arm of `409`), with
one line for each answer. podnet asks: after some time of `409` for its
lost socket, shorter than `rejoin`, ask for a new pair, or say so in one
line, so that an operator does not wait on a dead route.

## Premise

- Read: PodUtils/podnet#604; `reverse/node.rs` (the loop of the connects,
  the `409` at the first registration and after a loss), `reverse/serve.rs`
  (`Settings::rejoin`, the bound of a first `409`).
- Read: ask 1 of the issue (the relay server drops the socket of a node at
  once after a reset or a failed write) is for the relay role, which is
  podssh's until `pod-relay-serve` (T-036): its premise has it.
- Not measured: how long the relay of the pool holds a socket after a
  reset.

## Approach

A setting `Settings::lost_hold` (default 60 s): when the relay answers
`409` for the node's own lost socket for longer than it, the node says one
line (the route is held by a socket that the relay did not drop; the
operator's connections reach that socket) and ends its run with a typed
failure that the caller can act on (a new pair, another relay of the
pool); `rejoin` stays the bound of the session's resume. A test: a
stand-in relay that answers `409` after a reset; the node ends at
`lost_hold` with its line, not at `rejoin`.

## Prove

```sh
cargo test -p pod-relay --all-features --lib reverse::node
```

## Done

2026-11-16. `Settings::lost_hold` (`LOST_HOLD`, 60 s): after a loss, a node
that the relay answers `409` for its own socket for longer than
`lost_hold`, counted from the first such `409`, says one line
(`held_line`: the time of the `409`, the operators' connections reach the
held socket, take a new pair or another relay) and ends with
`Exit::NameInUse` (no new variant: a `match` of a consumer stays whole); `rejoin` stays the bound of the whole wait (C770).
Test: `tests/blocking_held.rs`, a stand-in that drops the socket and then
answers `409`: the node ends at its hold of 2 s with one line, not at the
10 minutes of `rejoin`. The four tests of `409` in `blocking_plain.rs`
pass as before. Plants `relay-lost-socket-held-to-rejoin` and
`relay-lost-socket-held-silent`.

# T-812: `pod-relay`: a measurement by hand of the throughput of the hosted relays (the default host and its seeds)

**Source:** the dig study of 2026-11-16 (an archive of the owner), `poc/`.
**Category:** measurement
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** partial

## Problem

Nothing in podcrates measures the bytes each second through a relay. The rank (`rank.rs`, `rank_auto.rs`) reads the share of failed probes and the time to open a session, never a rate. The default host `tcp.ssh.relay.ajam.dev` and its six seeds have no throughput number in the record. The dig study measured only relays on one machine over the loopback (a local `podssh relay serve`: 328.6 MiB/s at the median).

## Premise

- Read: `crates/pod-relay/src/rank.rs`, `rank_auto.rs`; `crates/pod-link/tests/measure.rs` (the `MEASURE` lines of T-777); `ajam-relay.toml` (the caps).
- Not measured: the rate of any hosted relay, a rate limit of the hosted relays.

## Approach

An ignored test of `pod-relay` against a stand-in on the loopback, and an input `measure` of `live.yml` that runs against the default host and each seed, one connection at a time: 16 MiB each way (under the budget of 60 MiB), the MiB/s, the time to open, the round trip p50 and p99, in the `MEASURE` form of T-777. Keep each run as a dated reading under docs/research.

## Prove

```sh
cargo test -p pod-relay --all-features --test measure -- --ignored --nocapture
```

## Correction

2026-11-17: the tool is in: `crates/pod-relay/tests/live_measure.rs`
(ignored; the input `relay: live_measure` of `live.yml`) makes a pair at the
default host and at each seed of `pool::seed()`, one at a time, runs a node
that echoes on the runner, and opens one operator session through the
relay: the time to the first echo, 100 round trips of 64 bytes (p50, p99),
then 16 MiB each way at once; one `MEASURE scen=relay relay=HOST ...` line
for each host, or its failure. Both ends are the test's, so each byte
crosses the relay two times. A stand-in run on the loopback is not taken
(the rate of a stand-in says nothing of a hosted relay).

2026-11-17: the first run (`live.yml` 44001248788, a GitHub runner;
`docs/research/relay-rates.md`): the default host opened in 294 ms, round
trips of 49.7 ms p50 and 58.3 ms p99, 17.33 MiB/s each way at once. Each of
the six seeds refused the pair with `503 reverse: unavailable`: only the
default host makes pairs. Open: the seeds, by a forward session to a target
that the test holds (a runner takes no inbound connection: a second relay
session as the target, or a public echo of the allow list); runs from other
egresses and over time.

# T-813: `pod-ws`: the speed of a relay path in two modes (75 and 330 MiB/s) that the dig study saw

**Source:** the dig study of 2026-11-16 (an archive of the owner), `poc/results.jsonl`.
**Category:** research
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

Five runs of 16 MiB through a local `podssh relay serve` on the loopback gave 74.7 MiB/s in some runs and about 330 MiB/s in the others; the study gives no cause. If the relay and its client share the bridge of `pod-ws`, the cause can be here.

## Premise

- Read: `poc/RESULTS.md` and `results.jsonl` of the archive; `crates/pod-ws/tests/bridge.rs` (`throughput`, ignored).
- Not read: whether `podssh relay serve` uses `pod_ws::bridge`.

## Approach

Run the ignored throughput test of `pod-ws` 20 times; where it is bimodal, find the cause (the size of a read, the scheduling of the two pumps) and fix it with a test; where it is not, ask podssh by a COORD issue.

## Prove

```sh
cargo test -p pod-ws --all-features --test bridge throughput -- --ignored --nocapture
```

## Correction

2026-11-17: podssh measured the study's shape against its relay role
(PodUtils/podssh#299, its T-803; `relay_serve_rate.rs`, plain `ws://`, 16
MiB in frames of 49,152 bytes, a new session for each of 20 runs): with 4
worker threads, 11 runs near 240 MiB/s and 9 near 650; with 2 the same two
modes; with one thread (`current_thread`) one mode, 381 to 430 (median
406.5). The role carries a session in one task with no queue between its
legs, so the two speeds are the placement of the tasks of each socket by
the multi-thread runtime of tokio, not the role and not the bridge of
`pod-ws`. podssh makes no change: the slow mode is far above each link that
the role serves.

## Done

2026-11-17. Measured: the bridge of `pod-ws` on both ends (`bridge::bridge`,
`Limits::TUNGSTENITE`), 16 MiB in chunks of 49,152 bytes on the loopback, 20
runs in a release build: 521, 578, then 793 to 1165 MiB/s; a warm-up and no
split near 75 and 330 MiB/s. The cause of the study's two speeds is not in
the bridge of `pod-ws`: COORD-127 (#217) asks podssh to run the study's
matrix on its relay role again.

# T-814: `pod-relay`: the bytes and the time of each session for each host, and an order by measured rate in shadow

**Source:** the dig study of 2026-11-16 (an archive of the owner), report 04.
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** M
**Status:** partial

## Problem

The rank orders hosts by failed probes and the time to open; no session counts its bytes for its host (`stream.rs` gives `NoTally`), so a slow host with a fast open stays first. A failed check of identity counts as an ordinary failure, which decays.

## Premise

- Read: report 04 of the archive (an estimator whose smoothing follows volatility, a relayed-path penalty, an integrity tier that does not decay); `crates/pod-relay/src/rank_list.rs`, `failures.rs`, `stream.rs`.

## Approach

Tally the bytes and the time of each finished session for each host and egress (no new connection). Write the order by measured rate beside the current order, as a line, for one release before any change of the order. Keep a failed check of identity in a tier of its own that does not decay.

## Prove

```sh
cargo test -p pod-relay --all-features rank
```

## Correction

2026-11-17: the tally and the shadow order are in. `rates` (a file
`relay-rates.json` of the cache beside the rank): the bytes and the busy
time of each finished session (the gaps under 1 s between transfers, so an
SSH session that waits then copies keeps its rate), the last 16 of each
host, by egress; a session of less than 1 MiB keeps none; `Rates::mib_s` is
the median of the day; `rates::order` orders names by it. `stream::of_measured`
counts a session (`stream::of` stays as it was); `rank_list::placed_now`
writes a debug line where the order by rate differs from the order it gives.
Tests: five unit tests and `a_measured_session_leaves_its_rate`; plants
`relay-rate-counts-idle`, `relay-rate-not-kept`. Open: the consumers take
`of_measured` in place of `of` (a hand-off); the tier of a failed check of
identity, which does not decay; the change of the order after a release in
shadow.

# T-815: `pod-relay`: the budget of a session under a relay whose caps are larger than the default relay's

**Source:** the ingest study of 2026-11-16 (an archive of the owner), its gap list.
**Category:** research
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`byte_budget_under` lowers the budget of 60 MiB for a relay with smaller caps and never raises it (`relay.rs`); `MOVE_BYTES` of `pod-session` is 48 MiB. A relay that publishes a cap of 256 MiB still gets a new session each 48 MiB.

## Premise

- Read: `crates/pod-relay/src/relay.rs` (`byte_budget_under`), `crates/pod-session/src/resume.rs` (`MOVE_BYTES`, `MOVE_AGE`), the calls of PodUtils/podssh@T-342.
- Not measured: the cost of a move of the session on a hosted relay.

## Approach

Decide whether the budget and `MOVE_BYTES` follow the cap in use with the same margins; record the call; where they do, a test with a cap of 256 MiB.

## Prove

```sh
cargo test -p pod-relay --all-features budget
```

## Done

2026-11-17. Call C773: the budget does not follow a larger cap; a smaller
one lowers it, as before. The finding of the measurement beside it (a move
each 3.5 s of a copy at the measured rate) is T-835.

# T-818: `pod-link`: forward error correction and a NACK backstop (TR-1, TR-2 of the ingest study)

**Source:** the ingest study of 2026-11-16 (an archive of the owner), TR-1, TR-2.
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** L
**Status:** open

## Problem

The ARQ is the only recovery of `pod-link`. On a lossy path each lost frame costs a round trip; the study's model gives 21.6 % residual loss of an object of 28 symbols with one repair at 3 % loss, 1.3 % with three.

## Premise

- Read: TR-1 and TR-2 of the ingest study (a systematic Reed-Solomon code over GF(2^8), a repair count for each class, a NACK with a grace of 5 ms and at most 64 missing counters).
- Not measured: FEC on a real lossy path; the residual numbers are a binomial model.

## Approach

A module `fec` of `pod-link`, off by default: the code, the cap of an object checked before an allocation, a repair count for each class, then a NACK for the missing frames with a bounded retransmit buffer. Measure it under netem (T-777 form) before it is on.

## Prove

```sh
cargo test -p pod-link --all-features fec
```

# T-819: `pod-netstack`: a ladder of the path MTU, a clamp of the MSS, and a class for each flow (TR-3, TR-4 of the ingest study)

**Source:** the ingest study of 2026-11-16 (an archive of the owner), TR-3, TR-4.
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** M
**Status:** partial

## Problem

The MTU of `pod-netstack` is fixed at 1280 and the size of a fragment is taken once; no flow has a class.

## Premise

- Read: TR-3 and TR-4 of the ingest study (a ladder of 1120, 1320, 1400 and 1500 with sealed probes; the clamp with the checksum update of RFC 1624; a table of 4096 flows with an idle eviction of 30 s).

## Approach

The ladder with sealed probes, the clamp of the MSS of a SYN, then the class of a flow (a policy, DSCP, then a heuristic). Each with a unit test of its bytes.

## Prove

```sh
cargo test -p pod-netstack --all-features
```

## Correction

2026-11-17, part 1 (the clamp of the MSS, TR-3): `crates/pod-netstack/src/mss.rs`, `mss::clamp_mss(packet, mtu)`. A SYN or a SYN-ACK of IPv4 or IPv6 (no fragment; no extension header of IPv6) whose MSS option is over `mtu - 40` (IPv4) or `mtu - 60` (IPv6) gets that value in place, and its TCP checksum is updated by the incremental form of RFC 1624 over each 16-bit word that the value touches: the value can start at an odd offset after other options, and an update of one aligned word gave a wrong checksum there (the first test run found it). Each other packet stays as it is. Tests: 5 in `src/mss_tests.rs`, each against a checksum computed in full over the pseudo-header (IPv4 SYN, IPv6 SYN-ACK, the option after a timestamp and a window scale at an odd offset, nine packets that stay, the incremental form against a full sum). The plant `netstack-mss-checksum-word-aligned` is caught.

2026-11-17, part 2 (the class of a flow, TR-4): `crates/pod-netstack/src/class.rs`. `Classes::class(key, dscp, len, now)` gives `Realtime`, `Bulk` or `Default`: a rule of the caller (a port) first, then the DSCP (46, 40, 48, 56 `Realtime`; 8, 10, 12, 14 `Bulk`), then the heuristic after 8 packets (more than half over 1000 bytes: `Bulk`; more than half under 256 bytes at 20 or more each second: `Realtime`). `Key::of` orders the two ends (address and port), so both directions have one key. The table holds at most 4096 flows: a new flow into a full table drops the flows idle for 30 s, and gets no history when each is live. Tests: 4 in `src/class_tests.rs`. The plant `netstack-flow-key-without-ports` (the study's planted defect) is caught.

The clamp has no call in a crate yet, by design: the stack's own TCP (smoltcp) takes its MSS from its MTU, and the TUN of `pod-warp` carries the host's own traffic, whose MSS the kernel takes from the MTU of the interface (1280). A caller that forwards the packets of other hosts through a smaller path calls `mss::clamp_mss`.

2026-11-17, part 3 (the ladder of the path MTU, TR-3), 011ce65: `crates/pod-netstack/src/pmtu.rs`, sans-IO in the manner of RFC 8899. `Ladder` takes the first rung of `RUNGS` (1120, 1320, 1400, 1500) to work; `poll(now)` gives a `Probe { id, size }` of the next rung, again after `PROBE_WAIT` (5 s) with no answer, and after `MOST_PROBES` (3) unanswered probes rests below that rung; `acked(id, now)` confirms the rung of the probe that is out (an old or unknown id changes nothing); a settled search tries the next rung again after `RAISE_AFTER` (600 s); `black_hole()` goes back to the first rung and searches again. Tests: 5 in `src/pmtu_tests.rs`. The plant `netstack-pmtu-probes-for-ever` is caught.

Open: the call of the class on the outbound path of the stack and the settings of a carrier for each class (`pod-link`'s ARQ); the carrier of `pod-link` sends each probe of the ladder as a padded packet, sealed as data, and tells the ladder of its answers and of a black hole (TR-3).


# T-820: The datapath: GSO with a fate rule, rings for each worker, a pool of source ports, the offload of TUN, a table of MAC addresses in O(1) (DP-1 to DP-5 of the ingest study)

**Source:** the ingest study of 2026-11-16 (an archive of the owner), DP-1 to DP-5.
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** L
**Status:** open

## Problem

`pod-core` emulates GSO by chunks with no rule for the fate of a batch; the queue of `pod-netstack` is one mutex; each endpoint has one socket; TUN has no offload; podnet's table of MAC addresses evicts in O(n).

## Premise

- Read: DP-1 to DP-5 of the ingest study; `crates/pod-netstack/src/device/queue.rs`.
- Not measured: each gain; DP-2 waits for DP-1 and DP-3.

## Approach

In order, each measured before and after (T-777 form): GSO with `UDP_SEGMENT` and a capability probe; rings for each worker pair with `SO_REUSEPORT` and a keyed hash; a pool of up to 64 source sockets; then `IFF_VNET_HDR`. The table of MAC addresses goes to podnet by a COORD issue.

## Prove

```sh
cargo test -p pod-netstack --all-features
```

# T-821: The wire: a keyed mask of the header of `pod-e2e`, cover traffic, a JA4 audit of the hello of `pod-tls`, an ML-KEM hybrid (WR-1 to WR-5 of the ingest study)

**Source:** the ingest study of 2026-11-16 (an archive of the owner), WR-1 to WR-5.
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** L
**Status:** open

## Problem

The header of a record of `pod-e2e` goes in clear after a constant magic; an idle tunnel is silent; a bump of a dependency can change the fingerprint of the hello of `pod-tls` unseen; a recorded session of `pod-e2e` has no post-quantum secrecy. A camouflage front (WR-4) conflicts with the refusal of REALITY in `pod-egress`.

## Premise

- Read: WR-1 to WR-5 of the ingest study; T-490 (the browser-shaped hello); `crates/pod-e2e/src/lib.rs` (the pattern of Noise).

## Approach

First the JA4 pin test of the default hello of `pod-tls` (with T-490). Then the mask of the header with a key from the exporter for each direction and epoch (a bump of the version of the record). Then the hybrid of ML-KEM-768 in the handshake of `pod-e2e`. Cover traffic, off by default, last. WR-4 waits for a ruling of the owner: write the question.

## Prove

```sh
cargo test -p pod-e2e -p pod-tls --all-features
```

# T-829: A COORD issue to podssh: the ideas of the dig study for the relay server (a ban after strikes, the PROXY header, a STUN responder, a sweep of liveness)

**Source:** the dig study of 2026-11-16 (an archive of the owner), report 01.
**Category:** chore
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

The relay server is podssh's until T-036; report 01 of the dig study has ideas for it (a ban after 20 strikes in 60 s for 300 s, trusted ranges of the PROXY protocol, a STUN responder with 5 each second for each source, deadlines of liveness of 30, 90, 120 and 10 s, a check of an advertised address against the observed source).

## Premise

- Read: report 01 of the dig study (checked against the code of PodUtils by its authors).

## Approach

Open a COORD issue `(to podssh)` with the ideas and their numbers; T-036 takes what podssh takes.

## Prove

```sh
cargo todo check
```

## Done

2026-11-17. COORD-127 (#217) to podssh: the five ideas of report 01 for the
relay server with their numbers, beside the two speeds of T-813.

# T-834: `pod-relay`: a renewal reads the default relay's `400` for an unknown reverse path as an error, not as no renewal (PodUtils/podssh#294)

**Source:** PodUtils/podssh#294 (COORD-098 of podssh, PodUtils/podssh@T-713).
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`pair::renew::renew` reads `404` and `405` as `Renewed::NotServed` and each
other status as an error. The default relay (`tcp.ssh.relay.ajam.dev`,
version `2026-10-03-r2`) answers the path of a renewal with `400 reverse:
use /v1/node/<name>, /v1/connect/<name>, /v1/status/<name> or
/v1/stop/<name>`, so each renewal against it ends in an error where it means
that the relay renews no pair.

## Premise

- Read: PodUtils/podssh#294 (podssh's conform check saw it live on
  2026-11-17); `crates/pod-relay/src/pair/renew.rs`.

## Approach

`not_served(status, body)`: `404`, `405`, or a `400` whose words start with
`reverse:` and name `/v1/stop/` (the list of the contract's reverse paths);
a unit test with the exact body; a plant.

## Prove

```sh
cargo test -p pod-relay --all-features --lib renew
```

## Done

2026-11-17. `pair::renew::not_served` reads the default relay's `400` with
the list of the reverse paths as `Renewed::NotServed`; another `400`, and
a `403`, stay errors. The unit test
`the_default_relay_s_400_with_the_reverse_paths_renews_no_pair` and the
plant `relay-renew-400-is-error`.

# T-835: `pod-relay`: the cost of a move of a session for a copy on the default relay

**Source:** T-812 (the first reading of `docs/research/relay-rates.md`).
**Category:** measurement
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

The default relay carried 17.33 MiB/s each way, and opened a session in
294 ms. A copy that keeps the budget of 60 MiB then opens a new session
about each 3.5 s of full use; if the copy waits for each open, the moves
cost up to about 8 % of its time. No run measures what a move costs a copy.

## Premise

- Read: `docs/research/relay-rates.md`; `crates/pod-relay/src/budget.rs`
  (the copy that opens a new session first); `MOVE_BYTES` of `pod-session`.
- Not measured: the time that a copy loses at each new session.

## Approach

Extend `tests/live_measure.rs` with a copy of 256 MiB through the default
relay with the budget of a session, and the same bytes in one session where
the relay allows it (it does not: so against the rate of one session):
the MiB/s of each, and the time of each move. Write the reading under
docs/research; where a move costs more than a few percent, open the next
session before the budget ends.

## Prove

```sh
cargo test -p pod-relay --features pair --test live_measure -- --ignored --nocapture
```

## Done

2026-11-17. `crates/pod-relay/tests/live_measure.rs` copies 5 sessions of 56 MiB one way through the default relay (a first byte `SINK`, the node's answer `READY`, then the bytes; the echo has a queue of 64 MiB) and writes the seconds of each session and of its open. Three runs failed first and each named a cause: the operator streamed before the node was ready (`NeverReady`, more than 1 MiB of input), then the node's bound of 1 MiB of waiting bytes ended a session whose local side still read (T-837, fixed). `live.yml` run 44009291585: 280 MiB in 11.80 s (23.72 MiB/s); 33 to 60 MiB/s for each session after its open; an open of 0.36 to 1.96 s (a mean of 1.06 s), 45 % of the time of the copy. The readings of five runs and the spread between them are in `docs/research/relay-rates.md`. A session opened ahead of the move (a spare) is the way to take the cost back; podssh has spares (its T-650).


# T-836: `pod-relay`: a route of a node to a Unix socket (PodUtils/podssh#295)

**Source:** PodUtils/podssh#295 (COORD-099 of podssh, PodUtils/podssh@T-714; cfb's ask A-S-08).
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

A node of podssh offers a route to a Unix socket (`ROUTE=unix-connect:PATH`). `pod_relay::reverse::routes::Route` holds a host and a port, and `dial_target` dials TCP alone, so podssh serves a Unix route in an opener of its own beside `Routes::open_with`. `pod-e2e`'s `move_out` takes the target as a `TcpStream`, so a session to a Unix socket must also be one to move.

## Premise

- Read: PodUtils/podssh#295; `crates/pod-relay/src/reverse/routes.rs` and `tcp.rs`; `crates/pod-e2e/src/hold/mod.rs` (`move_out`).
- podssh builds `Route` with struct literals (`node.rs`, `node_udp.rs`, `share/file.rs`, `node_temp.rs`): a new field breaks it.

## Approach

Keep the type of `Route`: a Unix target is the host `unix:PATH`, or `unix:@NAME` for an abstract name on Linux, with port 0. `dial_target` connects to it and gives the socket as a `TcpStream` of the same descriptor, so `Routes`, the roost's check and `move_out` take it with no change.

## Prove

```sh
cargo test -p pod-relay --all-features --test routes_unix
```

## Done

2026-11-17. `crates/pod-relay/src/reverse/unix_target.rs`: `UNIX` (`unix:`), `Route::unix(name, path)`, `Route::unix_path`, `unix_path(host)` (a host with a path after the prefix) and `connect(path, timeout)`: a blocking connect in `spawn_blocking` under the timeout (`@NAME`: `SocketAddr::from_abstract_name` on Linux and Android), then the descriptor as a `std::net::TcpStream`, non-blocking, as a tokio `TcpStream`; with no Unix socket on the system, a refusal. `reverse::tcp::dial_target` dials a Unix host that way, and the words of the roost's check name the socket. So `Routes::open` and `open_with` (a route of the command line or a share), `TcpHandler` and the move of a session take a Unix route with no change of a type; `pod-e2e`'s `move_out` gets a `TcpStream` of the socket. The address and the TCP options of such a stream are not there. Only the node's owner names a target (a route or a share); an operator names a route, and an exit dials through the node's own `ExitDial`, which must refuse a host `unix:` (podssh's exit dials TCP alone). Tests: `tests/routes_unix.rs`, 6 (a path both ways, an abstract name on Linux, a socket that is not there refused with its path, the roost's check that passes and that refuses, the stream taken as a std `TcpStream` and back, the prefix). The plant `relay-unix-route-as-tcp` (the Unix branch removed) is caught. podssh's `--caller-line` stays podssh's: it writes the first line on the stream that `open_with` gives.

# T-837: `pod-relay`: a node ends a session whose local side still reads, at the first MiB that waits for it

**Source:** T-835 (the live copy of `live.yml` run 44007816989).
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`reverse::route::offer` ends a session (`close` with "the node's local side does not read") when the bytes that wait for its local side pass `Handler::queue_bytes`, at once. A handler with no layer has `QUEUE_BYTES`, 1 MiB. The relay has no flow control for one session, so its frames come in bursts: at 20 MiB/s a pause of 50 ms of the local copy is 1 MiB. The live copy through the default relay, with a local side that only sinks the bytes, was ended so after 19 MiB of its second session. A plain node of podnet (a TCP or Unix target, `TcpHandler`) has the same bound for an upload.

## Premise

- Read: `crates/pod-relay/src/reverse/route.rs` (`offer`, `to_local`); `src/reverse/node.rs` (`QUEUE_BYTES`); the test `a_local_side_that_does_not_read_ends_its_session_and_not_the_others` of `tests/reverse_node.rs` (a flood of 2.5 MiB to a local side that never reads).
- The layered handler of podssh has `2 * replay_capacity`.
- Not measured: the bursts of the relay; podnet's plain mode under an upload.

## Approach

A local side "does not read" when a write to it has waited for a grace time (`STALL_GRACE`), not when the queue passes the limit once. Over `queue_bytes`, a frame waits while the local copy made progress within the grace; a hard bound (a multiple of `queue_bytes`) still ends a session at once, so the memory of a stuck session stays bounded. The flood test keeps its result with a flood above the hard bound, and a new test: a local side that reads slowly with bursts above `queue_bytes` keeps its session.

## Prove

```sh
cargo test -p pod-relay --all-features --test reverse_node
```

## Done

2026-11-17. `crates/pod-relay/src/reverse/route.rs`: each route keeps when its copy to the local side last moved (a write ended, or it waited for bytes: `Moved`, milliseconds since the route was made). `offer` ends a session past `queue_bytes` only when the copy did not move for `STALL_GRACE` (2 s), or when the bytes that wait pass `HARD_LIMIT` (4) times `queue_bytes`; a local side that reads gets the bursts of the relay. The memory of a stuck session stays at most 4 MiB for a handler with no layer. Tests: `a_local_side_that_reads_slower_than_a_burst_keeps_its_session` (a queue of 256 KiB, a burst of 768 KiB, a reader of 16 KiB each 5 ms: each byte comes, no close of a stall), and the flood of `a_local_side_that_does_not_read_ends_its_session_and_not_the_others` now passes the hard bound (5 MiB). `cargo test --all-features` of `pod-relay` passed. The plant `relay-stall-at-first-mib` (the old rule) is caught by the new test. The live copy gives its echo a queue of 64 MiB still, so that it measures the relay alone.


# T-844: `pod-relay`: a route to a Unix socket on Windows (PodUtils/podssh#298)

**Source:** PodUtils/podssh#298 (COORD-102 of podssh).
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`reverse::unix_target::connect` had no form for Windows ("this system has no Unix socket for a route"), so podssh keeps its own opener of a Unix route, which reaches an AF_UNIX socket on Windows through WinSock.

## Premise

- Read: PodUtils/podssh#298; `PodUtils/podssh:crates/podssh-cli/src/pipe/unix.rs` at `a4b1436` (`af_unix`, `sockaddr`, `wsa_start`; 0BSD).
- Not measured: a run on Windows here; the Windows tests of podssh run it when it takes the tag.

## Approach

On Windows, a socket of the family AF_UNIX by WinSock (Windows 10 1803 and later), connected to the path, handed on as a `TcpStream` of the same socket, as on Unix.

## Prove

```sh
cargo clippy -p pod-relay --lib --all-features --target x86_64-pc-windows-gnu -- -D warnings
cargo test -p pod-relay --all-features --test routes_unix
```

## Done

2026-11-17. `crates/pod-relay/src/reverse/unix_target.rs`: one `connect` for Unix and Windows over a blocking `open_std` that gives a std `TcpStream`: on Unix the `UnixStream` (with the abstract name of Linux) by its descriptor; on Windows a WinSock socket of AF_UNIX (`WSAStartup`, `socket`, `connect` to a `SOCKADDR_UN`, `closesocket` on a failure, `from_raw_socket`), with a refusal of a path that does not fit `sun_path` with its NUL. `pod-relay` takes `windows-sys` 0.61 (`Win32_Networking_WinSock`) on Windows. The clippy of `pod-relay` for `x86_64-pc-windows-gnu` and for Linux passes; `tests/routes_unix.rs` (6) passes on Linux. It ran on no Windows host here: podssh's Windows tests of its route run it.

# T-845: `pod-relay` and `pod-session`: a relay's liveness offer, so a relay that sleeps between frames can sleep (PodUtils/podssh#300)

**Source:** PodUtils/podssh#300 (COORD-104 of podssh, PodUtils/podssh@T-712; cfb's ask A-S-05 of COORD-014).
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

A relay that sleeps between frames (a Durable Object with WebSocket hibernation) cannot sleep while each end sends a Ping each 10 s, and while the layer of `pod-session` sends its heartbeat as payload on an idle link.

## Premise

- Read: PodUtils/podssh#300; podssh's reference `PodUtils/podssh:crates/podssh-relay/src/liveness.rs` and `PodUtils/podssh:docs/relay.md` ("A relay that sleeps between frames"). The rule: the member `liveness` `{"pingMs": N, "frame": "ws-ping"}` of the relay's `hello` (to a node), `ready` (to an operator) and `forward` of `/relays.json` is a hint. A pace from 30000 to 60000 ms is taken; over 60000 is held to 60000; under 30000, or a member of a wrong type, is not taken; no member, or `null`, keeps 10 s. `frame` is required, and only `"ws-ping"`. An end allows 3 silent checks at an offered pace. (The issue wrote `every_s`; the reference and its vectors are the wire.)
- Read: `crates/pod-relay/src/reverse/serve.rs` and `operator.rs` (the watch starts before either frame); `forward_layer::offer`; the heartbeat of `pod-session`'s layer.

## Approach

1. One reader of the member by the rule, in `pod-relay`; the node reads it from `hello` and the operator from `ready`, and each starts its watch again at the new pace.
2. `forward_layer::offer::of` keeps `forward.liveness` in `Offer`.
3. The layer of `pod-session` sends no frame of its own on an idle link while the offer holds.
4. The vectors of `PodUtils/podssh:docs/relay.md` as tests.

## Prove

```sh
cargo test -p pod-relay --all-features
cargo test -p pod-session --all-features
```

## Correction

2026-11-17, items 1 and 2: `pod_relay::liveness` (also `reverse::liveness`): `pace(member)` and `pace_in(frame)` read the member by the rule (30 to 60 s taken, over 60 s held to 60 s, under 30 s and a member that is not `{"frame":"ws-ping","every_s":N}` not taken, no member: the end's own pace). The node (`reverse::serve`) starts its watch again at the pace of `hello`'s member (from `Hello.extra`, no new field); the operator (`reverse::operator`) at the pace of `ready`'s (from the JSON of the frame: `NodeOutbound::Ready` keeps its one field, which podssh builds). `forward_layer::offer::Offer` has `liveness` (`forward.liveness`); podssh builds `Offer` only with `..Offer::default()`, so no literal of it breaks. Tests: 3 unit tests of the rule, `the_offer_keeps_the_pace_of_liveness_of_the_host`, and `tests/liveness_offer.rs` (paused time: a node told 30 s sends its first Ping at 30 s, told 600 s at 60 s, with no offer or with 5 s at 10 s). 462 tests of `pod-relay` pass.

2026-11-17, item 4: the reader takes podssh's wire, `pingMs` in milliseconds (it first read `every_s` of the issue's text), and `pod_relay::liveness::VECTORS` holds the 12 vectors of podssh's document; `each_vector_of_the_document_gives_its_pace` checks each one. The tests of the node and of the offer moved to `pingMs`.

Item 3: the far end decides, as one side that leaves `heartbeat.v1` out ends the heartbeat of both. A node's handler hears the pace of each `hello` (`Handler::liveness`), and `Layered` opens each new session with `Settings::quiet()` while an offer holds; a session that is open keeps what it agreed. With no heartbeat, the layer has no `DEAD_AFTER`, and the node's watch of the WebSocket's Pongs at the offered pace finds a dead link. The operator needs no change: its node names no heartbeat. On the forward road, the consumer sets `Settings::quiet()` when `Offer.liveness` holds.

## Done

2026-11-17, b06f11d: `pod_session::Settings.heartbeat` (default `true`), `Settings::quiet()`, `Settings::names()` and `pod_session::HEARTBEAT`; `once::asked`, `deadline::offered` and the resume's `OPEN` take `names()`. `pod_relay::reverse::Handler::liveness` (default: nothing), forwarded by `E2e`; `serve` calls it at each `hello`. Tests: `a_quiet_far_end_ends_the_heartbeat_of_both_sides` (200 s idle, no `PING` either way, the session goes on) and `the_handler_hears_the_pace_taken_at_each_hello`. Plants: `session-quiet-still-names-the-heartbeat` and `relay-node-keeps-the-heartbeat-under-an-offer`, each caught. The tests of `pod-session` and `pod-relay` pass. No test of the operator's side runs a whole session.

# T-847: `pod-ws`, `pod-relay` and `pod-egress`: headers of the caller for a host on each WebSocket road, for Cloudflare Access (PodUtils/podssh#302)

**Source:** PodUtils/podssh#302 (COORD-105 of podssh, podssh T-718, cfb's ask A-C-04).
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

A relay behind Cloudflare Access refuses each socket that has no service-token headers (`CF-Access-Client-Id`, `CF-Access-Client-Secret`). No road of `pod-relay` and no forwarder of `pod-egress` takes a header of the caller.

## Premise

- Read: PodUtils/podssh#302. podssh reads the credentials from files, keeps them in `Zeroizing`, and sends them only to a host that the user named.
- Read: `crates/pod-ws/src/handshake.rs` (`build_request`), `crates/pod-ws/src/client.rs` (`WsClientConfig`, `ConnectError::Refused`), the roads of `pod-relay` (forward, `/v1/node/`, `/v1/connect/`; HTTP/1.1, HTTP/2, cleartext, QUIC) and the forwarder of `pod-egress`.

## Approach

1. A set of extra headers for a host, given by the caller (a function of the host, values in `Zeroizing`), in `WsClientConfig` and in the settings of each road of `pod-relay` and of the forwarder of `pod-egress`.
2. `build_request` and the request of each other road write them after `X-Relay-Token`.
3. A name or a value with a control character is refused; the refusal names the header, never its value.
4. A value of a header is masked in the body of `ConnectError::Refused`; a refusal of 401 or 403 keeps its status.
5. Tests of each road, with a server that reads the headers; a plant.

## Prove

```sh
cargo test -p pod-ws --all-features
cargo test -p pod-relay --all-features
cargo test -p pod-egress --all-features
```

## Done

2026-11-17, c71f14eb6 and cf9326c86: `pod_ws::Headers` (a function of the host that gives `(&'static str, Zeroizing<String>)`; `Headers::new`, `from_arc`, `in_cleartext`, `for_host`, `for_cleartext`) is the field `pod_ws::Way::headers` (`Way::with_headers`, `Way::extra`, `Way::extra_in_cleartext`; none by default, so no literal of a consumer breaks; call C775). `pod_ws::Extra` is the checked set of one host: a name that is no HTTP token, a name that a road writes itself or a name twice, and a value with a control character (CR, LF, NUL, another C0, DEL) are refused before any byte, by the name alone; `Extra::masked` replaces each value in the words of `Refused`, `Upgrade`, `Http` and `Config` (a quote cut in a value too) and keeps the status. New: `handshake::build_request_with`, `client::upgrade_carrying`, `plain::connect_loopback_carrying`, `url::asking_carrying`. The roads that write them after the token: `connect` (HTTP/1.1), `h2_road` (CONNECT of RFC 8441, never indexed by HPACK, and its fallback), QUIC (`quic_road::dial`), the plain wire (forward, `/v1/node/`, `/v1/connect/`), the HTTP road (the open, each request of a session after `X-Session-Key`, the one-POST form on HTTP/1.1 and HTTP/2), the URL road (the forwarder and Wisp of `pod-egress`), `https_post_json` and `https_with_token` (the mint, the pairing calls) and `/health`. A road in cleartext (`cleartext::connect`, its mint, `ws://`) writes them only with `Headers::in_cleartext(true)`: podssh sets it to send them there. Tests: `pod-ws` `headers::tests` (5), `handshake::tests::the_caller_s_headers_follow_the_token`, `tests/caller_headers.rs` (HTTP/1.1 and HTTP/2 over TLS, 401 and 403 masked with their status, cleartext and `ws://` opt-in, `wss://`, the POST, a control character before any connection, plain `ws://`); `pod-relay` `tests/caller_headers.rs` (the HTTP road, the cleartext road, the forward opener on the plain wire), `reverse::wire::tests`, `quic_road::dial::tests` (the upgrade on a stream), `http_road::one_h2::tests`; `pod-egress` `tests/forwarder.rs` `the_caller_s_headers_go_to_the_forwarder_where_allowed`. Plant `ws-caller-headers-not-sent`, caught. The fetches of `/relays.json` send none: T-848.

# T-848: `pod-relay`: the headers of the caller on the fetches of `/relays.json`

**Source:** T-847.
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`pool::body_of` (the pool) and `forward_layer::offer::over` (the offer of the layer) ask a relay host for `/relays.json` with no header of the caller (`pod_ws::Headers`). A relay behind Cloudflare Access refuses them, so the pool and the offer of such a host are empty.

## Premise

- Read: `crates/pod-relay/src/pool.rs` (`body_of`), `crates/pod-relay/src/forward_layer/offer.rs` (`over`), `crates/pod-relay/src/health.rs` (`fetch`, which sends them).

## Approach

1. Give each fetch the `Extra` of its host (`Way::extra`), as `health::fetch` does; the functions that take a stream take it as an argument.
2. Mask each value in the words of a refusal.
3. A test with a stand-in that reads the head of each fetch.

## Prove

```sh
cargo test -p pod-relay --all-features
```

## Done

2026-11-17, 5384ec0: `pool::body_with` and `forward_layer::offer::over_with` take the `Extra` of the host and write it after `Accept`; `body_of` and `over` stay, with no header. `pool::refresh`, the fetch of a member (`pool::members`) and `forward_layer::offer::ask` give `Way::extra` of the host, as `health::fetch` does. A refusal of the document names only its status, so no value can show. Test: `the_fetches_of_the_relay_document_carry_the_headers` in `tests/caller_headers.rs` (both fetches, over an in-memory stream). Plant `relay-document-fetch-without-headers`, caught. The plants `relay-layer-road-forward-path`, `relay-t234-plain-any-host` and `ws-chosen-protocol-unchecked`, stale after T-847, are made again on its code and caught.

# T-856: The wire vectors of `tokens.json` through the relay role's tokens

**Source:** T-120 (the import of the wire page and its vectors of PodUtils/podssh#304)
**Category:** feature
**Milestone:** M4
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

`crates/pod-relay/tests/wire_vectors.rs` reads 109 of the 117 vectors of
`docs/wire.md` through `pod-session`, `pod-relay` and `pod-ws`. It skips the
8 vectors of `crates/pod-relay/tests/vectors/tokens.json` (3 `mint`, 5
`check`): the relay role mints and checks a forward token
(`PodUtils/podssh:crates/podssh-relay/src/serve/tokens.rs`), and podcrates
has no relay role. Only `scripts/check-wire-vectors.py` reads them here.

## Premise

- Read: `PodUtils/podssh:crates/podssh-relay/tests/wire/tokens.rs` at `04f141b`
  (it calls `serve::tokens::Tokens::new`, `mint` and `check`); no crate of
  podcrates holds `Tokens` or the MAC of `ephm1.` tokens.
- Measured: `cargo test -p pod-relay --all-features --test wire_vectors --
  --nocapture` prints `117 in all, 109 run, 8 skipped`.

## Approach

At the import of the relay role (T-036), port podssh's
`tests/wire/tokens.rs` (its blob is in `crates/pod-relay/SOURCES`): read
each `mint` and `check` vector through the role's `Tokens`. Set the kinds
`mint` and `check` of `crates/pod-relay/tests/wire/kinds.rs` to run, and
remove the skip.

## Prove

```sh
cargo test -p pod-relay --all-features --test wire_vectors -- --nocapture
```

## Start condition

T-036 is done.

# T-860: `pod-piping`: the probe of a node's upgrade sends no header of the caller (PodUtils/podssh#306)

**Source:** PodUtils/podssh#306 (COORD-109 of podssh, PodUtils/podssh@T-718); a road that T-847 left out.
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

`pod_piping::probe::upgrade` writes its own request with no header of `Way::extra`. A relay behind a gate such as Cloudflare Access answers it 403, `upgrade_refused` reads that as a refusal on the way, and a node of podssh leaves the pair's road, which carries the headers and would pass.

## Premise

- Read: PodUtils/podssh#306; `crates/pod-piping/src/probe.rs`; `pod_ws::headers` (`Extra::write`, `Extra::masked`).

## Approach

The probe takes `way.extra(&relay.host)` (a bad value is refused by name), writes it after `User-Agent`, and masks the values in the words of a refusal. A test of the request and of the mask; a plant.

## Prove

```sh
cargo test -p pod-piping --all-features --lib probe
```

## Done

2026-11-17, fdfcba699: `probe::upgrade` takes `Way::extra` of the relay's host (an error of the headers is `Probe::Unknown` with its words), `request` writes it after `User-Agent`, and the answer's error goes through `Extra::masked`. Test: `the_probe_carries_the_headers_of_the_caller` (the order after `User-Agent`; a refusal that quotes the secret shows none of it). The tests of the library of `pod-piping` pass. Plant `piping-probe-without-headers`, caught. podssh drops its skip of the probe (`process_names`) with the take.

# T-865: `pod-relay`: keep a relay member's `meet` from its `/relays.json` (PodUtils/podssh#307)

**Source:** PodUtils/podssh#307 (COORD-110 of podssh, PodUtils/podssh@T-720).
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

podssh's relay role advertises its meeting points in `/relays.json` as `meet` (`{"iroh": "https://HOST/", "derp": "HOST", "probe": "/generate_204"}`, each key `null` when its road is off). A member of a net lists its net's relay host first, as its iroh relay and its DERP home, when that host's `meet` names them. `pod_relay::pool::members` reads each relay member's `/relays.json` once a day and keeps no `meet`.

## Premise

- Read: PodUtils/podssh#307; `crates/pod-relay/src/pool/members.rs` (`take_from`, `ask_due_with`) and its cache.

## Approach

`members` keeps a member's `meet` beside what it keeps now, in the same cached file, on the same daily read, with no new connection; `members::meet(cache, &Member) -> Option<Meet { iroh: Option<String>, derp: Option<String> }>` gives it. A value that is not an `https://` URL (iroh) or a name (DERP) of the member's own host is dropped, so a member cannot send the net's traffic to another party. Tests; a plant.

## Prove

```sh
cargo test -p pod-relay --all-features
```

## Done

2026-11-17, 3385a6494, 09758eaf6: `pool::members::Meet { iroh: Option<String>, derp: Option<String> }`; `Meet::of_body(body, &Member)` reads `meet` of the `/relays.json` body that the daily ask already fetches; `Meet::checked(iroh, derp, &Member)` keeps a value only when it names the member's own host; `members::meet(cache, &Member)` (and `meet_in(dirs, &Member)`) gives the kept points, checked again on each read, `None` when no value holds. `Ask` keeps `meet` in `relay-members.json` (`serde(default)`: a file of the form before reads, with no `meet`). The rule of the host: the authority must be, the case of the letters aside, `Relay::name` of the member (`HOST`, or `HOST:PORT` when the port is not 443) or `HOST:PORT` with the member's own port; iroh is `https://` + that authority + one `/` or none (no path, user, query, other scheme); DERP is that authority alone. A kept value is the member's own form (`https://NAME/`, `NAME`), never the peer's text. `probe` is not kept. The points need no signed list: each names the host that the net names already; an ask that fails keeps none until the next answer. A cache from before the change gets the points on the member's next daily ask. Tests in `pool/members_tests.rs`: `the_meeting_points_of_the_member_s_own_host_are_read`, `a_meeting_point_off_the_member_s_own_host_is_dropped`, `the_cache_keeps_the_meeting_points_and_reads_a_file_of_the_form_before`, `the_daily_ask_keeps_the_member_s_meeting_points` (through `ask_due_with` with a stand-in `/relays.json`), `a_kept_meeting_point_is_checked_again_when_read`. Plant `relay-member-meet-off-its-host` (the peer's text kept unchecked), caught.

# T-869: `pod-relay`: `OpenError::lines` says the cause of a refusal twice (PodUtils/podssh#309)

**Source:** PodUtils/podssh#309 (COORD-112 of podssh).
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

For a relay's refusal of the policy (`400` or `403`) and for a `502`, `pod_relay::open::OpenError::lines` gives the words of `ConnectError::Refused` and then a hint that says the same cause again. The first line of a `502` names its target three times: `the relay refused the session: HTTP 502: relay: cannot reach T: T: Connection refused`, then `the relay could not reach T`.

## Premise

- Read: PodUtils/podssh#309; `crates/pod-relay/src/open.rs` (`OpenError::lines`); the `Display` of `ConnectError::Refused` in `pod-ws`.

## Approach

A refusal of the policy is one line: `the relay's policy does not allow T (HTTP S)`. A `502` is one line: `the relay could not reach T: CAUSE`, where CAUSE is the relay's words with each part that names the relay or the target again taken out (`(HTTP 502)` when nothing is left). A refusal whose hint adds a fact (a token of the environment that the relay rejected, a proxy's refusal) keeps two lines. A test holds the line count of each refusal; a plant.

## Prove

```sh
cargo test -p pod-relay --all-features --lib open::tests
```

## Done

2026-11-17, 9e286138e: `OpenError::lines` (now in `crates/pod-relay/src/open_lines.rs`) gives one line for a refusal of the policy, `the relay's policy does not allow T (HTTP S)`, and one line for a `502`, `the relay could not reach T: CAUSE`, where CAUSE is the relay's words (one line, by `pod_core::text::one_line`) with each leading `relay:`, `cannot reach`, T and `:` taken out; `(HTTP 502)` when nothing is left. A token of the environment that the relay rejected and a proxy's refusal keep their second line, as each adds a fact. Test: `a_refusal_says_its_cause_once` in `crates/pod-relay/src/open_tests.rs` (the line of each refusal, and two lines for the token). The tests of `pod-relay` pass. Plant `relay-refusal-says-its-cause-twice` (the relay's whole words after the target), caught.

# T-904: `pod-relay`: a relay that issues no token names no way to give one (PodUtils/podssh#313)

**Source:** PodUtils/podssh#313 (COORD-116 of podssh, item 2).
**Category:** defect
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

A relay that mints for no one gives `this relay does not issue tokens (HTTP 503); ask its operator for one and give it as the relay token`: the line names no way to give it.

## Premise

- Read: PodUtils/podssh#313; `crates/pod-relay/src/token.rs`, `src/settings.rs` (`Names::token`).

## Approach

The line names `Names::token`, the words of the consumer for its token (podssh: `PODSSH_RELAY_TOKEN`).

## Prove

```sh
cargo test -p pod-relay --all-features --lib token
```

## Done

2026-11-17, 9e16f1886, 6c39d9102: `TokenError::NotIssued { give }`: the mint fills `give` from `Settings::names.token`, and the line ends `give it in GIVE` (podssh sets `PODSSH_RELAY_TOKEN`; the default reads "the relay token setting"). Test `a_relay_that_issues_no_token_names_where_to_give_one`; 230 tests of the library of `pod-relay` pass, and workspace clippy is clean. Plant `relay-token-way-unnamed`, caught. Breaking: `TokenError::NotIssued` has a field.

# T-908: `pod-session`: the Close of the relay in the drain wins over the error of a write (PodUtils/podssh#314, ask 1)

**Source:** PodUtils/podssh#314 (COORD-117 of podssh, PodUtils/podssh@T-826), ask 1.
**Category:** defect
**Milestone:** M0
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

On `Written::Failed(e)` the pump drains `down` for `DRAIN`, and keeps only `End::Closed`, `Retired`, `Broken` and `LocalEnd`. A link that the relay ended with a WebSocket Close comes from `down` as `End::Lost(Some(why))`, with the code and the reason of the relay, and the drain drops it: the client says `broken pipe` in place of `the relay closed the link: 1009 session byte cap`.

## Premise

- Read: PodUtils/podssh#314; `crates/pod-session/src/pump.rs` (the drain after a failed write).

## Approach

The drain keeps an `End::Lost(Some(_))` that `down` gave, over the error of the write. A test: a peer that sends a Close, then closes the socket while the client writes; the end names the code of the Close.

## Prove

```sh
cargo test -p pod-session --all-features
```

## Done

2026-11-18, 81b979b52. In the drain after `Written::Failed`, an `End::Lost(Some(_))` that `down` gives (the cause of the link: a relay's Close with its code and reason) wins over the error of the write; a drain that times out or gives `Lost(None)` keeps the error of the write. Test `tests/session_close_over_write.rs` `the_relays_close_wins_over_the_failed_write`: a link that the relay cuts while the application writes, the write fails at once with `broken pipe`, the relay's reason comes to the read 300 ms later; the end names `1009 session byte cap`. Plant `session-drain-drops-the-close`, caught.


# T-909: `pod-relay`: the link budget of the layer under a small cap of the relay; the cap in `forward_layer::Offer` (PodUtils/podssh#314, ask 2)

**Source:** PodUtils/podssh#314 (COORD-117 of podssh, PodUtils/podssh@T-826), ask 2.
**Category:** defect
**Milestone:** M0
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

The client moves a session to a new link at `MOVE_BYTES` (48 MiB) whatever the relay publishes. `forward_layer::Offer` does not carry `limits.max_session_bytes` of the same `/relays.json`, and a kept offer comes from the cache with no read. `relay::byte_budget_under` keeps the default budget for a cap under the margin plus 1 MiB: a relay of 1 MiB to 8 MiB gets 56 MiB, and each link ends at the cap of the relay.

## Premise

- Read: PodUtils/podssh#314; `crates/pod-relay/src/forward_layer/offer.rs`, `crates/pod-relay/src/relay.rs` (`byte_budget_under`), `crates/pod-session/src/resume.rs` (`MOVE_BYTES`).

## Approach

`Offer` carries the cap of its document, kept with it in the cache. `byte_budget_under` gives a budget under each cap (three quarters of it), with a floor of 1 MiB only where the cap is smaller. Tests for a cap of 1 MiB, 8 MiB and a large one, and an offer from the cache.

## Decision

- Under a cap, `byte_budget_under` gives the larger of the cap less the margin (4 MiB) and three quarters of the cap. A larger cap never gets a smaller budget. The floor of 1 MiB stays on the variable alone: under a cap of less than 4/3 MiB, a floor leaves no room for the bytes in flight.
- `Offer::move_bytes` gives the move bytes of the layer: `MOVE_BYTES`, or three quarters of a smaller cap. This is the rule of `MOVE_BYTES` under the default cap of 64 MiB (room for the bytes in flight and a second try). The margin of 4 MiB of the plain road is not sufficient for that.

## Prove

```sh
cargo test -p pod-relay --all-features
```

## Correction

Measured on the code: the margin is 4 MiB (64 MiB less 60 MiB), not 8 MiB. Before the fix, a cap under 5 MiB got the default budget of 60 MiB, and a cap of 5 MiB to 8 MiB got 1 MiB to 4 MiB. Part 1 of `wip/t909` gave three quarters only where the margin did not fit: a cap of 4 MiB got 3 MiB and a cap of 5 MiB got 1 MiB (a larger cap, a smaller budget).

## Done

2026-11-18, e401e11b. `relay::byte_budget_under`: the larger of the cap less the margin and `relay::three_quarters(cap)`, never above `SESSION_BYTE_BUDGET` (a cap of 1 MiB gets 768 KiB, 4 MiB gets 3 MiB, 8 MiB gets 6 MiB, 16 MiB gets 12 MiB, 32 MiB gets 28 MiB). `forward_layer::Offer` has `max_session_bytes` (from `limits.max_session_bytes` of the same document, by `pool::parse_limits`) and `Offer::move_bytes()`; `forward_layer::kept` keeps the cap with the offer (a file of before it reads as no cap). Tests: `pool::tests::the_budgets_go_under_the_relays_limits_and_never_above_their_own` (each cap from 512 KiB to 80 MiB gets a budget under it and not under a smaller cap's), `forward_layer::offer::tests::the_offer_moves_the_session_under_the_cap_of_its_document` (1 MiB, 8 MiB, 1 TiB, no cap), `forward_layer::kept::tests::the_cap_of_a_session_is_kept_with_the_offer`, and `tests/layer_cap.rs` `a_session_moves_before_each_cap_of_the_relay` (a stand-in relay with a cap of 2 MiB, the offer from the cache, 8 MiB through an echo with equal digests, 4 moves or more, no link at its cap, no loss; 30 runs green). On the code before the fix, the budget test fails (a cap of 4 MiB gets 60 MiB) and the end-to-end test fails (4 links met the cap). Plant `relay-layer-budget-ignores-the-cap`, caught. podssh sets `Settings::move_bytes` from `Offer::move_bytes()` of the offer that it takes (read or kept).

# T-912: `pod-session`: a far end that lost a session by a restart is said as one that timed it out (PodUtils/podssh#315, ask 2)

**Source:** PodUtils/podssh#315 (COORD-118 of podssh, PodUtils/podssh@T-828), ask 2.
**Category:** defect
**Milestone:** M0
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

After a restart of the far end, a resume 3 s after the loss gets `REFUSE` (an unknown session), and the client says that the far end forgot the session as it keeps one 10 min after a loss. The far end did not time the session out: it lost it.

## Premise

- Read: PodUtils/podssh#315; `crates/pod-session/src/deadline.rs` (`away`, `forgotten`), `src/resume.rs`.

## Approach

`away` gives the duration with its words. `forgotten` says "the far end no longer knows the session, though this side was away for A of the K that it keeps a lost session: it was likely restarted" when the time away is less than half of the deadline, and keeps today's words else. A test of both words and of a wall clock that went back.

## Prove

```sh
cargo test -p pod-session --all-features --test session_deadline
```

## Done

2026-11-18, 70b205ed7. `deadline::away` gives `(Duration, String)`; `deadline::forgotten(why, kept, &away)` says "the far end no longer knows the session, though this side was away for A of the K that it keeps a lost session: it was likely restarted" when the time away is less than half of `kept`, and keeps the words of a timeout else. Test `a_far_end_that_lost_the_session_early_is_said_to_be_restarted` (3 s of 10 min, 4 h of 3 h, a wall clock that went back). Plant `session-restart-said-as-timeout`, caught.

# T-913: `pod-relay`: the cleartext road does not say why the TLS of the relay failed (PodUtils/podssh#315, ask 3)

**Source:** PodUtils/podssh#315 (COORD-118 of podssh, PodUtils/podssh@T-828), ask 3.
**Category:** defect
**Milestone:** M0
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Where the TLS of a relay fails (a certificate for another name, an untrusted CA) and the cleartext road is tried, the line says only "the TLS of HOST failed on the way". A user behind a proxy that intercepts TLS, or with a mistyped name, cannot see which.

## Premise

- Read: PodUtils/podssh#315; `crates/pod-relay/src/cleartext.rs` (`over`), the caller that gives the failed hosts.

## Approach

The note of the cleartext road names the TLS failure of the host, in the words of the TLS road's own error ("a certificate for listed.test, not other.test", "a certificate that no trusted CA signed"). A test with a certificate for another name.

## Prove

```sh
cargo test -p pod-relay --all-features
```

## Done

2026-11-18, 8605f8649. `pod_tls::handshake_cause(text)`: the cause of a failed handshake in plain words from rustls's text, also inside a wrapper: "a certificate for listed.test, not other.test" (with each name that rustls lists), "a certificate that is not for NAME", "a certificate that no trusted CA signed", "an expired certificate", "a certificate that is not valid yet", "a revoked certificate"; `None` else. `cleartext::tls_cause(host, attempts)` gives it, else the text of the error; `last` passes it, and `over` takes `(Relay, String)`: the note says "the TLS of HOST failed on the way (CAUSE)". Tests `hint::tests::the_cause_of_a_handshake_in_plain_words` (from rustls's own errors), `cleartext::tests::a_host_of_each_deployment_whose_tls_failed_is_tried` (the cause of each host). Plant `relay-cleartext-tls-cause-unnamed`, caught.

# T-926: `pod-relay`: a refusal of the relay's policy no longer says the relay's cause (PodUtils/podssh#318)

**Source:** PodUtils/podssh#318 (COORD-119 of podssh; PodUtils/podssh@T-843).
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** done

## Problem

Since `v0.70.0` (T-869), `OpenError::lines` gives `the relay's policy does not allow HOST:PORT (HTTP 403)` for each refusal of the policy, and drops the relay's words. The line said the cause twice before (PodUtils/podssh#309); now it says it zero times. The relay's words name the rule that refused (`port 25 is refused`, `this relay dials no address of it`), and so what the user can change. podssh asks for the cause after the target, with no part that names the relay or the target again, as the `502` branch does: `the relay's policy does not allow 140.82.112.3:25: port 25 is refused (HTTP 403)`.

## Premise

- Read: PodUtils/podssh#318; `crates/pod-relay/src/open_lines.rs`, `src/open_tests.rs`.

## Approach

A `policy_cause` beside `unreachable_cause`: the one line of the body after each leading echo (`relay:`, `forward:`, the target, the words of the refusal: `not in the ALLOW list`, `not allowed`, `is blocked`, `:`); the line keeps the cause where one is left, else the short line.

## Prove

```sh
cargo test -p pod-relay --all-features --lib open
```

## Done

2026-11-18: `policy_cause` and `cause_after` (`crates/pod-relay/src/open_lines.rs`; `unreachable_cause` takes `cause_after` too). `a_refusal_says_its_cause_once`: `forward: T not in the ALLOW list: this relay dials no address of it` gives `the relay's policy does not allow T: this relay dials no address of it (HTTP 403)`; `forward: 140.82.112.3:25 is blocked: port 25 is refused` gives `...: port 25 is refused (HTTP 403)`; a body with no cause past the echo keeps the short line. `cargo test -p pod-relay --all-features --lib open`: 16 passed. Plant `relay-policy-cause-dropped` (the cause dropped, as in `v0.70.0`), caught by that test.

# T-939: `pod-iroh` with `pod-session`: after a new address of the client on the relay-only path, no end resumes, and the bytes in flight are never sent again (PodUtils/podssh#283, ask 4)

**Source:** PodUtils/podssh#283 (COORD-091 of podssh), ask 4: the eighth failure of podssh's gate (podssh `5c254fb`, podcrates `v0.70.0`, run 44287863366, job `gate (m6)`, `iroh::the_iroh_road_survives_a_new_address` of `tests/m6_exit.rs`).
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

On the iroh road with no UDP (the iroh relay carries each byte), the client's address changes. The session does not end within 600 s. The node says one line: `session b80089bd: the target sent no byte for 23 s, while it took 201 bytes of the client; this side sent 13758313 bytes, the client acknowledged 13247259; the replay buffer keeps 511054 bytes, room 3683250; no window`. No `session.resume` line comes. The client says nothing after its authentication, though its proxy logs 14 CONNECTs to the iroh relay from the new address. The 511,054 bytes in flight on the old path are lost between the iroh relay and the client, and the client's session never decides that its link is gone (the QUIC connection looks alive), so nothing asks for the replay.

## Premise

- Read: PodUtils/podssh#283, the section of the eighth failure.
- Not read: how `pod-iroh` gives a path change of its endpoint to the link of `pod-session`; whether the client's session has a check of progress while the far end holds unacknowledged bytes.

## Approach

Two parts, each with a test on the loopback relay of `pod-iroh` that moves the client's address: (1) the client's session resumes when it gets no acknowledged progress for a bound while the far end holds bytes that it has not acknowledged (the node's stall line names that state); (2) a line at the client when its relay connection of iroh is dialled again. podssh's local loop of `m6_exit` checks the fix before its gate.

## Prove

```sh
cargo test -p pod-iroh --all-features
cargo test -p pod-session --all-features
```

## Done

2026-11-18, 16927112 and fe98382f.

- Read: `pump.rs` (`beat`), `pump/down.rs`, `pump/outbox.rs`, `beside.rs`, `hearing.rs`, `resume.rs`, `pod-iroh/src/client.rs`, iroh 1.4.1's relay actor (a `PING` each 15 s, reset by each frame of the relay) and its `home_relay_status`, podssh's `fake-proxy.py --move`, `m6_exit.rs` and `layered.rs` (the iroh road has no spare link). The client cannot know the bytes that the far end holds: no record carries the far end's sent offset. A link of the layer is ordered, so each record that the client hears from the far end comes after each byte that the far end wrote before it; with the heartbeat on both ends, a link that carries nothing for 30 s is lost. The client said no line in 600 s, so it heard the far end in each 30 s: the 511,054 bytes of the node's line were with the client's layer (held for its application, whose acknowledgement waits), or the client's session did not run. The node's line alone does not tell the two apart (T-945).
- Measured on the loopback relay of `pod-iroh` with no UDP: 14 runs of a new address (the old tunnels silent, as `--move`), 6 with spare links and 8 without, each 20 MiB whole, with no resume; QUIC carried the bytes again in about 20 s. 3 runs with no network for 45 s after the move: each resumed the session and lost no byte. No run hung.
- (1) `pod-session`: the heartbeat also ends a link when a `PING` waits `pump::PING_ANSWER` (30 s) for its `PONG` and the position of the session (bytes received, bytes acknowledged) does not move in that time, also while the far end's own records come (`pump/progress.rs`; call C784). Each road takes it. The wire does not change. Test `tests/session_heartbeat.rs` `a_link_whose_far_end_answers_no_ping_while_nothing_moves_is_lost` (a far end that sends its own `PING` each 10 s and answers none): on the code before the fix it fails ("never found lost: Elapsed" after 300 s of the paused clock); after it, the link is lost in 40 s. `pump::progress::tests` on a manual clock.
- (2) `pod-iroh`: `home_relay::say_changes(endpoint, say)` says when the connection to a home relay ends and iroh dials it again, each new cause of a failed dial, and its return with the time of the gap. `tests/new_address.rs`: the client's tunnels go silent in an echo of 8 MiB, and the new address has no network for 45 s; the session resumes, the 8 MiB come back whole, and the lines come ("ended; dialling it again", "a new dial ... failed", "is back after 30.6 s").
- `cargo test -p pod-session -p pod-iroh -p pod-relay --all-features`: 165, 72 and 493 passed (22 ignored), 0 failed. Plants `session-t939-ping-unanswered` and `iroh-t939-relay-back-unsaid`, each caught.
- podssh, at its take: call `pod_iroh::home_relay::say_changes` on the client's endpoint with its verbose line (`ssh/iroh/dial.rs`), and run its local loop of `m6_exit`; a hang after it with no line of the relay and no loss of the layer points to the client's application (T-945 gives its line).

# T-945: `pod-session`: the client says no wait of its own, so a hang with the node's `session.stall` cannot tell a loss on the link from bytes held for the client's application (PodUtils/podssh#283, ask 4)

**Source:** the work of T-939 (PodUtils/podssh#283, the eighth failure).
**Category:** feature
**Milestone:** M2
**Priority:** P2
**Effort:** M
**Status:** done

## Problem

The far end says each wait of its reader of the target that holds 20 s (`session.stall`, T-767), with its offsets. The client says none. In the eighth failure of podssh's gate the node said that the client acknowledged 511,054 bytes less than the node sent, and the client said nothing for 600 s. A link of the layer is ordered, and the client heard the far end in each 30 s (else its heartbeat would end the link), so those bytes were with the client's layer, held for its application, or its session did not run. An acknowledgement covers the bytes written to the application, so bytes held for a slow application look like bytes lost on the link at the node.

## Premise

- Read: `pump/down.rs` (the reader holds bytes for the application up to `HOLD_MAX` and acknowledges the bytes written), `pump/stall.rs` (the far end's waits), `resume.rs` (`Note`), the node's line of the eighth failure.
- Not measured: the client's state in the eighth failure.

## Approach

The client's pump says its waits as the far end does, as a `resume::Note` (a new variant, which podssh shows at `-v`): the application took no byte for 20 s while N bytes wait for it; the reader of the application waits for room or for the writer of the link; with the offsets each way. One line for each wait. A test on the paused clock: an application that stops reading gives one line after 20 s, with the bytes held.

## Prove

```sh
cargo test -p pod-session --all-features --lib pump::stall
```

## Done

2026-11-18, this branch.

- `resume::Note::Stall { words }` (new, in `resume/note.rs`): the client's pump says each wait of its link that holds 20 s (`pump::STALL_AFTER`), once for each wait, through `Watch::notes` (`Watch::noting`), which `resume::run` sets and gives to its `note`. The waits: the application took no byte while bytes wait for it (`pump/down.rs` starts the wait when bytes are held, and each byte taken starts a new one), the reader of the application waits for room or for the writer of the link, and the records sent again wait for room. A quiet application of the client gives no note. The words, for an application that took 1 KiB of 32 KiB and then read no more: `the application took no byte for 20 s, while 31744 bytes wait for it; this side sent 0 bytes, the far end acknowledged 0; the replay buffer keeps 0 bytes, room 1048576; no window; this side received 32768 bytes, 31744 of them not yet written to the application`.
- The far end's `session.stall` takes the same code (`pump/stall.rs`, `Sink`): its line now also gives `this side received R bytes, H of them not yet written to the target`, and a target that takes no byte for 20 s while bytes wait for it gives its own line. `Link::held` is new. The README of `pod-session` gives each line.
- Test `pump::stall::tests::an_application_that_stops_reading_gives_one_note_with_the_bytes_held` (paused clock, the whole pump): no note in 19 s, then exactly one note in 90 s more, with the words above. `cargo test -p pod-session --all-features`: 166 passed, 0 failed. Plant `session-t945-wait-unsaid` (the note not sent): caught. The plants `session-t767-stall-twice` and `session-t155-never-moves` follow the new code; each is caught.
- `pod-relay` `tests/layer_cap.rs` counts a `Note::Stall` as no loss.
- podssh, at its take: show `Note::Stall` at `-v`; a match on `Note` with no wildcard arm needs the new arm.

# T-942: `pod-relay`: a kept offer of the layer loses `forward.liveness`

**Source:** the review of T-909 (`crates/pod-relay/src/forward_layer/kept.rs`).
**Category:** defect
**Milestone:** M0
**Priority:** P3
**Effort:** S
**Status:** open

## Problem

`forward_layer::kept` keeps `layer`, `at`, `quic` and the cap of an offer, but not `liveness` (`forward.liveness`, T-845). An offer from the cache has `liveness: None`. A run that takes a kept offer keeps its own pace of Ping and the `PING` of the layer, so a relay that sleeps between frames stays awake on each idle link of that run.

## Premise

- Read: `crates/pod-relay/src/forward_layer/kept.rs` (`KeptOffer`, `keep`, `of_text`), `crates/pod-relay/src/forward_layer/offer.rs` (`Offer::liveness`).
- Not read: whether podssh takes `liveness` from a kept offer in its glue.

## Approach

`KeptOffer` keeps the pace in milliseconds (`#[serde(default)]`; a file of before it reads as none), and `of_text` gives it back by the rule of `crate::liveness`. A unit test: an offer with a pace of 45 s, kept and read again, has the same pace.

## Prove

```sh
cargo test -p pod-relay --all-features --lib forward_layer::kept
```

# T-953: `pod-relay`: under a small cap of the relay, a new link sends again more bytes than the cap, so the relay still ends links with 1009 (PodUtils/podssh#320)

**Source:** PodUtils/podssh#320 (COORD-122; podssh T-826)
**Category:** defect
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

Under `podssh relay serve --max-session-mib 1`, the client moved the session at 768 KiB (T-909), and the new link first sent again 1,179,648, 589,920, 1,049,836 and 903,181 bytes: the bytes that the old link did not see acknowledged. On the road of the layer, `pod_session::Settings::window` is `None`, so only the replay buffer (4 MiB) bounds them; the relay ended some new links with `1009 session byte cap`, and the client resumed. The move threshold of T-909 bounds what a link carries before the move, not what a new link sends again. The far end that the role of the relay runs has the same window.

## Premise

- Read: PodUtils/podssh#320; `crates/pod-relay/src/forward_layer/offer.rs` (`Offer::move_bytes`); `crates/pod-session/src/lib.rs` (`Settings::window`); `crates/pod-relay/tests/layer_cap.rs`.
- Measured: `tests/layer_cap.rs` with a cap of 1 MiB and a way back at a quarter of the pace of the client: with the window of the client alone, 2 links met the cap in each of 2 runs (the far end sent again past it); with the window at each end, none in 5 runs.

## Approach

`Offer::window()` is a quarter of the cap (`forward_layer::offer::window_under`); `Offer::fit(settings)` sets the move threshold and the smaller window. The role of the relay takes `window_under` of its own cap for its far end.

## Prove

```sh
cargo test -p pod-relay --all-features --test layer_cap
cargo test -p pod-relay --all-features --lib forward_layer::offer
```

## Done

2026-11-19, commit "pod-relay: the window of each end of the layer is a quarter of a small cap of the relay (T-953, PodUtils/podssh#320)". `Offer::window`, `Offer::fit` and `forward_layer::offer::window_under`; the comment of `Offer::move_bytes` no longer says that the bytes in flight fit. `tests/layer_cap.rs` takes the cap and a slow way back: `a_new_link_sends_again_less_than_a_small_cap` (1 MiB, the way back at a quarter of the pace) and the test of 2 MiB of T-909; each end takes the window of the cap; no link meets the cap in 5 runs. The plant `relay-layer-window-ignores-the-cap`; the plant of T-909 takes the new comment. podssh: `offer.fit(settings)` in `relay_layer.rs` in place of `move_bytes` alone, and `window_under` of `--max-session-mib` for the far end of its role.

# T-958: `pod-relay` with `pod-session`: a move at three quarters of a small cap of the relay still passes the cap, so the relay ends most links with 1009 (PodUtils/podssh#325)

**Source:** PodUtils/podssh#325 (COORD-124; podssh T-826)
**Category:** defect
**Milestone:** M2
**Priority:** P1
**Effort:** M
**Status:** done

## Problem

With `v0.74.0` (T-953) under `podssh relay serve --max-session-mib 1` and 3 MiB each way on podssh's loopback, a new link sends again 262,144 bytes at most, but each run has one move and then five links that the role ends with `1009 session byte cap` (3 runs in 3). `Offer::move_bytes` is three quarters of the cap, and `Watch::move_at` counts the bytes of both ways, the records included, as the relay does. After the move is due, the old link carries on until the new link is up: the bytes past the threshold are the rate times the time of the move, which at the rate of a loopback is more than a quarter of the cap. The windows of T-953 bound what a new link sends again, not what the old link carries after the threshold. `tests/layer_cap.rs` of podcrates paces its writer at some 16 MiB/s and did not show it.

## Premise

- Read: PodUtils/podssh#325; `crates/pod-session/src/pump/watch.rs` (`bytes`, `move_at`, `due`); `crates/pod-relay/src/forward_layer/offer.rs` (`move_bytes`, `window`, `fit`).
- Not read: the pump of `pod-session` from `due` to the retire of the old link (what it sends on the old link while the new one opens).
- Not measured: `tests/layer_cap.rs` of podcrates at the pace of podssh's loopback.

## Approach

Reproduce first: `tests/layer_cap.rs` with a cap of 1 MiB, 3 MiB each way, and no pace of the writer. Then bound the old link: when the move is due, the old link takes no new bytes of the application (acknowledgements and the records of the move go on), and the new link carries them, so the old link ends at `move_at` and the bytes in flight. With that, the threshold of three quarters and windows of a quarter hold the cap: the bytes in flight were counted when sent. If the pump cannot hold the bytes, make the threshold `cap - 2 * window - margin` in `Offer::fit` (a test pins the sum under the cap). A test: no link of the run meets the cap at the pace of the loopback, 5 runs in 5.

## Prove

```sh
cargo test -p pod-relay --all-features --test layer_cap
```

## Done

2026-11-19, this commit. The client holds the old link of a move (`pod_session::pump::Hold`, the field `hold` of `pump::Watch`, call C790): `beside::next` starts the hold when it decides `Beside::Move`, and a failed move (`Note::MoveFailed`) or a session that closes on the old link lifts it; a spare holds nothing. While the hold is on, the reader of the application reads nothing, the writer sends no `DATA` and no `ACK`, and a `PONG` carries the last offset acknowledged (`Link::pong_held`). The wire does not change. `forward_layer::offer::window_under` is an eighth of the cap: the old link carries the threshold (three quarters), the far end's window and one record at most. `tests/layer_cap.rs`: `a_move_at_the_rate_of_a_loopback_stays_under_a_small_cap` (1 MiB, no pace, a handshake of 30 ms) and the two tests before it pass in 5 runs of 5; the most bytes on one link were 918,361 at the rate of a loopback and 951,090 with the slow way back (of 1,048,576). On the code before the fix, the new test fails in 3 runs of 3 (8 links met the cap); with the hold and a window of a quarter, 19 links met it in 3 runs. The tests of the cache of the offer take a directory each (two tests of 1 MiB ran at once and removed the directory of the other). New tests: `pump::hold::tests::a_held_link_takes_no_bytes_and_acknowledges_none_until_the_hold_lifts`, `beside::tests::a_move_holds_the_old_link_and_a_failed_move_lifts_the_hold`, `session_move::a_failed_move_does_not_stall_the_session`; `cargo test -p pod-session --all-features`: 169 passed; `cargo test -p pod-relay --all-features`: 496 passed. Plant `session-t958-move-not-held`, caught; the plant of T-953 takes the new comment of `Offer::fit` and the words of the new window.

podssh at `ba19367` (`v0.75.0`): its `tests/layer_cap.rs` through `podssh relay serve --max-session-mib 1`, 3 MiB each way, has no link that ends with `1009 session byte cap` in 8 runs of 8 (PodUtils/podssh#327); its T-826 closes.


# T-959: `pod-e2e`: the channel says no wait of its own, so a hang where the layer holds bytes for the channel cannot tell the channel from its reader (PodUtils/podssh#283, ask 5)

**Source:** PodUtils/podssh#283 (COORD-091, ask 5; the ninth failure of podssh's gate, run 44452674747 at podssh `9989ae6`)
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

In the ninth failure of podssh's gate (`iroh::the_iroh_road_survives_a_new_address`), the client's `Note::Stall` of T-945 said: "the application took no byte for 21 s, while 288823 bytes wait for it; this side sent 20680508 bytes, the far end acknowledged 20680508; ...", and the node said no stall. The application above the layer is the channel of `pod-e2e` (`ends::operator_told`, through podssh's `channel::around`), which writes into a pipe of 256 KiB that podssh's SSH client reads. Either the reader of that pipe stopped, or the channel held for a cause of its own; the channel says no wait, so the two cannot be told apart. podssh now says each write of the channel to SSH that waits 20 s (`09ab98b`).

## Premise

- Read: PodUtils/podssh#283 (the ninth failure, ask 5); the list of `crates/pod-e2e/src/` (`channel.rs`, `channel/`, `ends.rs`).
- Not read: the loops of the channel that read the layer and write to the application, and where each can wait.

## Approach

The channel says each wait of its own that holds 20 s, as `pod-session` does (`pump/stall.rs`; reuse its rule and its words, do not copy them): the reader of the layer waits for room of the channel; the writer to the application waits; the record or the key step waits for the far end. One line for each wait, with the bytes on each side, through the channel's way to say a step (a note or a log line that podssh shows at `-v`). A test on the paused clock: an application that stops reading gives one line after 20 s with the bytes held.

## Prove

```sh
cargo test -p pod-e2e --all-features stall
```

## Done

2026-11-19, this commit.

- The carry of `Channel` says each write that takes no byte for `pod_session::pump::STALL_AFTER` (20 s, the constant of the layer's pump, taken as a dependency: `pod-e2e` depends on `pod-session` already), once for each wait, as an event of the new kind `ends::EVENT_WAIT` (`e2e.wait`, info) (`crates/pod-e2e/src/channel/stall.rs`). Each byte taken ends the wait; the next write starts a new one. The waits: the write to the application (a node's target) and the write to the layer. A read of the layer or of the application can wait with no fault, so it gives no line; the steps of the handshake have their own limits, and the records are sans-IO. The words, for an application that took 1 KiB of 32 KiB and then read no more: `the application took no byte of the channel for 20 s, while 31744 bytes wait for it; the channel read 32787 bytes from the layer, 0 of them not yet opened, and wrote 1024 bytes to the application`; to the layer: `the layer took no byte of the channel for 20 s, while L bytes wait for it; the channel read R bytes from the application and wrote W bytes to the layer`.
- Where the lines go: `Channel::with_events(events)` (new); a node gives its `Node::events`, each text after `the session of KEY: ` (`serve_asked` and `hold::resume`); the operator gives `told` of `operator_told`, which is now `FnMut(Event)` and hears `e2e.no-net` once and each `e2e.wait`. The README of `pod-e2e` gives the lines (section "Waits").
- Test `channel::stall::tests::an_application_that_stops_reading_gives_one_stall_line_each_wait` (paused clock, both ends of the channel): no line in 19 s, exactly the line above in 90 s more; after the application takes 1 KiB more, no new line in 19 s, then a second line (`30720 bytes wait`, `wrote 2048`) after 2 s more. `cargo test -p pod-e2e --all-features`: 82 passed, 0 failed; `cargo test -p pod-relay --all-features`: 496 passed, 0 failed. Plant `e2e-t959-wait-unsaid` (the event not sent): caught. The plant `e2e-t615-seed-unread` follows the new code; it is caught.
- podssh, at its take: `around_told`'s `tell` is a `oneshot` sender, which an `FnMut` does not take; show each `e2e.wait` at `-v`, as `Note::Stall`.

# T-960: `pod-term`: a cooked session always draws its prompt, so an exec'd command on podssh's server shows `$ ` between and after its lines (PodUtils/podssh#326)

**Source:** PodUtils/podssh#326 (COORD-125; podssh T-848)
**Category:** feature
**Milestone:** M2
**Priority:** P3
**Effort:** S
**Status:** done

## Problem

`podssh serve` runs an exec request with a pty, where no pty opens, on the cooked discipline of `pod-term` (`PodUtils/podssh:crates/podssh-ssh/src/server/line.rs`). `pod-term` draws its prompt `$ ` (`echo::PROMPT`) after the output of the program, and draws it again (`\r`, `$ `, `EL`) around each later piece of output. A shell on the discipline needs the prompt; an exec'd command does not, and a real pty shows none. podssh's gate at `9989ae6` (run 44452674747, `gate (ssh)`) gave `\r\x1b[KPIPES\r\n$ \r\x1b[KTERM=vt100 SSH_TTY=unset\r\n$ ` for two lines of output.

## Premise

- Read: PodUtils/podssh#326; `crates/pod-term/src/echo.rs` (`PROMPT`, the redraw).
- Not read: `Facts` and the constructors of a cooked `Session`.

## Approach

A cooked `Session` with no prompt: echo, the editing and the signals as now, and nothing drawn before a line or around the output (a field of `Facts`, or a constructor for an exec). A test: the output of a program in two reads, with no prompt, gives the bytes of the program and no `$ `.

## Prove

```sh
cargo test -p pod-term --all-features prompt
```

## Done

2026-11-19, this commit. `crates/pod-term/src/echo.rs`: the prompt is a field of `Discipline` (`PROMPT` by default); `Discipline::without_prompt` and `Session::without_prompt` give a cooked session with no prompt, for a command that a server runs with no pty: echo, the editing and the signals stay, a submit, a signal and a clear draw no prompt, and output with no line under edit is written with `\r\n` and nothing around it (no `\r`, no `EL`, no prompt). A line under edit is still hidden and drawn again around the output. `Discipline::prompt` gives the prompt for the caller to draw first (empty with no prompt). Tests (`tests/output.rs`): `a_session_with_no_prompt_draws_nothing_around_the_output` (the two lines of podssh's gate give `PIPES\r\n` and `TERM=vt100 SSH_TTY=unset\r\n`), `a_session_with_the_prompt_draws_it_as_before`. `sh scripts/agent/check.sh`: each step passed.

# T-962: `pod-session`: the hold of a move starts when the task beside the link wakes, not when the bytes pass the threshold, so on Windows the old link still meets a small cap (PodUtils/podssh#327)

**Source:** PodUtils/podssh#327 (COORD-126, amended; podssh T-826; the gate at podssh `ba19367`, run 44476622620, job `windows`, `windows-2025`)
**Category:** defect
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** done

## Problem

With `v0.75.0` (T-958), podssh's `tests/layer_cap.rs` under `relay serve --max-session-mib 1` passed 8 runs of 8 on Linux, and failed once on windows-2025: the first link ended with `1009 session byte cap`, and the client said "the session moved to a new link before the relay's limits, after 899 KiB in 0 s (32768 bytes sent again)". The move began above seven eighths of the cap (896 KiB), not at the threshold of three quarters (768 KiB). `Watch::count` tells `due` when the bytes pass `move_at`, and the hold starts in `beside::next` when its task wakes on `due`; until then the pump sends at its rate. A slow scheduler (the timer of Windows ticks each 15.625 ms) gave about 130 KiB more.

## Premise

- Read: PodUtils/podssh#327; `crates/pod-session/src/pump/watch.rs` (`count`, `due`); `crates/pod-session/src/pump/hold.rs` and `beside.rs` (where the hold starts and lifts, T-958).
- Not measured: the lag on windows-2025 in podcrates' own `tests/layer_cap.rs`.

## Approach

`Watch::count` starts the hold in the same call that passes `move_at` (when the session moves), so the pump takes no new byte of the application from that byte on. `beside::next` lifts it where no move follows: a session that closes, an application that ended, a failed move (T-958 lifts there already). A test on the paused clock: a pump whose `beside` task does not run for 50 ms after `due` sends no byte of the application past `move_at` plus one record; the plant moves the start back to `beside`.

## Prove

```sh
cargo test -p pod-session --all-features hold
cargo test -p pod-relay --all-features --test layer_cap
```

## Done

2026-11-19, this commit. `Watch::count` starts the hold in the same call that passes `move_at` (when the session moves, `move_at > 0`), and tells `due`; a move at the link's age starts the hold in `beside::next`, as before. `Watch::moves_end` sets `move_at` to 0 and lifts the hold, so that no later byte starts a hold that no task lifts. It runs where no move follows: in `beside::next` for a session that closes or whose application ended (before its wait with no end), in `beside::failed` for a failed move, and in `resume::run` when the application ended while the new link greeted. A failed spare keeps the hold of the mark: the move that is due comes next, with the hold on. A spare holds nothing. The tests of `beside` moved to `crates/pod-session/src/beside/tests.rs` (the size of `beside.rs`). New tests: `pump::hold::tests::the_hold_starts_at_the_mark_while_the_task_beside_the_link_sleeps` (no task beside the link for 50 ms after the mark: the link carries the mark and one record at most, then all of the 1 MiB once the moves end; on the code before the fix it carried 1,048,992 bytes), `pump::hold::tests::a_session_whose_application_ends_at_the_mark_closes_on_the_old_link`, `beside::tests::a_failed_move_ends_the_hold_and_the_mark_of_the_link`, `beside::tests::a_closing_session_lifts_the_hold_of_the_mark_and_does_not_move`, `beside::tests::a_failed_spare_keeps_the_hold_of_the_mark_and_the_move_follows`; each lift test fails when its lift is removed; 30 runs of 30. `cargo test -p pod-session --all-features`: 174 passed; `cargo test -p pod-relay --all-features`: 496 passed; `cargo test -p pod-relay --all-features --test layer_cap`: 5 runs of 5 (the most bytes on one link 918,361 of 1,048,576); `cargo test -p pod-e2e --all-features`: 82 passed. `tests/layer_cap.rs` does not show the lag of Windows on Linux: `resume::run` polls the pump and `beside::next` in one task, and the `connect` closure runs only after `beside` decided the move, so a wait in it delays the new link, not the start of the hold; a delay of `beside` needs a hook in the product code. The unit test with no task beside the link covers that window. Plant `session-t962-hold-late` (the start in `Watch::count` removed), caught; plant `session-t958-move-not-held` removes both starts of the hold now, caught.

# T-963: `live.yml`: public services of other parties fail from a runner of GitHub and pass from a cloud session (the Nostr relay `offchain.pub`, the WebTorrent tracker `tracker.openwebtorrent.com`)

**Source:** the scheduled run 44497255102 of `live.yml` at `fee6f8a`, attempts 1 and 2; the run 44505410827 at `e475b23`; the run 44507180792 at `b74ffbd`
**Category:** defect
**Milestone:** M2
**Priority:** P2
**Effort:** S
**Status:** partial

## Problem

`two_ends_pair_through_each_public_nostr_relay_after_a_dead_mailbox` (`crates/pod-relay/tests/pair_code_live.rs`) failed twice on the runner: "wss://offchain.pub/: the giver: the other end said nothing for 60 s". Both ends passed the greeting (the relay accepted the probe of each end and sent it back), and no `OK false`, `CLOSED` or `NOTICE` came. `relay.primal.net` paired in 2.6 s and 4.3 s in the same runs. From a cloud session, through its HTTPS proxy, the same test paired at `offchain.pub` in 2.2 s. So the relay carries the events of one end to its own subscription and not to the other end, from the network of the runners only. The relay has rank 2 in `pod-services` (`offchain-nostr.toml`), and a client on such a network waits for the answer limit of the pairing before the next meeting point.

The run 44505410827 at `e475b23` failed in `pod-wormhole`'s live test: `wss://tracker.openwebtorrent.com/` refused the upgrade with HTTP 503, while the same upgrade from a cloud session gets 101. The step ran its commands with `set -e`, so the pairing test of `pod-relay` after it did not run: one service that failed hid the others.

## Premise

- Read: the job logs of both attempts and of run 44505410827; the NIP-11 document of `offchain.pub` (strfry 1.1.3, the same limits as the catalog).
- Not measured: whether the relay drops the events or holds them past 60 s; whether it refuses each host of the runners or the two connections of one host.

## Approach

1. Done (step 1, below): the failure text names the counts at the relay and the result of both ends.
2. The cause, from the run 44507180792 at `b74ffbd`: `offchain.pub` refused the taker's event with "Policy violated and pubkey is not in our web of trust.", after it accepted the first events of both ends. The relay runs a write policy that refuses a key that it does not know, and each pairing draws a new key for each end (`pod_wormhole::nostr`). The policy does not act on each event or each connection: the same test passed from a cloud session, and `pod-wormhole`'s live test passed at the relay in the same run. The WebTorrent tracker paired in that run: its 503 was a short outage.
3. Take `offchain-nostr` out of the relays of the pairing (a fact of its write policy, with the run as its source, and out of the kind `nostr-relay` that the pairing reads), and add a public relay that takes ephemeral events of new keys from the runner and from a cloud session: the candidates `nos.lol` and `relay.damus.io` (strfry; NIP-11 read 2026-11-19), each measured by the live tests of `pod-wormhole` and `pod-relay` before it gets a rank. The live test keeps at least two relays.
4. A pairing that meets a refusal of the relay after the greeting fails with the relay's words at the taker; the giver waits its whole limit. Look whether the giver can learn the end of the pairing earlier (the taker's last word at the mailbox is not there on a relay).

## Prove

```sh
timeout 300 cargo test -p pod-relay --all-features --test pair_code_live -- --ignored --nocapture
```

## Done

2026-11-19, this commit, step 1. `pod-wormhole`: a Nostr relay counts the events of its end (`Tally`: sent, accepted with `OK` true, passed of another key, taken as messages of the other end), and `Wormhole::receive` and `Wormhole::confirm` give them in the words of an other end that said nothing: "the other end said nothing for 60 s (at the Nostr relay URL this end sent S events and the relay accepted A; it passed P events of the other key, T of them messages of the other end)". `Place::tally` gives them for a relay, `None` for the other points. The live test `pair_code_live` gives the result of both ends when one fails. Test: `tests/nostr.rs` `an_end_that_hears_nothing_names_what_the_relay_passed` (the stand-in loses the taker's message; the giver names 2 sent, 2 accepted, 2 passed). The giver of run 44497255102 failed in `receive`, after the key and the versions crossed `offchain.pub`: the relay passed the first events of both ends. Steps 2 and 3 wait for the next run of `live.yml` with this commit. The step of `pod-wormhole` and `pod-relay` in `live.yml` runs both tests and fails at its end, so a service that fails in one test does not hide the next.

2026-11-19, step 3 (call C796): `offchain-nostr` leaves the catalog of `pod-services`, and `noslol-nostr` (`wss://nos.lol/`, strfry) takes its rank 2. From a cloud session, `nos.lol` paired in the live test of `pod-wormhole` and in `pair_code_live` after a dead mailbox (3.4 s); `relay.damus.io` answered the upgrade with HTTP 503 and is not taken; `offchain.pub` reset TLS in a later run. The catalog test names `primal-nostr` and `noslol-nostr`. The entry stays partial until a run of `live.yml` passes from a runner, and for step 4.
