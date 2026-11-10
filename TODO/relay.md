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
**Status:** open

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

# T-021: `pod-e2e`: the end-to-end channel (Noise XX)

**Source:** `docs/design.md`, section 4 (M2).
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** M
**Status:** open

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

# T-022: `pod-wormhole`: the mailbox client, SPAKE2 and the code words

**Source:** `docs/design.md`, section 4 (M2).
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** S
**Status:** partial

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

2026-11-10, partial: the release `v0.15.0` and the take of podssh remain
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
  (`adoption/podssh/pod-wormhole.needs`; 15 files, 60 lines in, 250 out;
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

# T-023: `pod-netstack`: the userspace TCP and UDP stack on smoltcp 0.14

**Source:** `docs/design.md`, section 4 (M2).
**Category:** feature
**Milestone:** M2
**Priority:** P1
**Effort:** M
**Status:** open

## Problem

WARP and the virtual networks of podnet carry TCP and UDP in user space,
on smoltcp 0.12 now.

## Premise

- Read: podnet `PodUtils/podnet:src/warp/ip.rs` (1,680),
  `PodUtils/podnet:src/vnet/device.rs` (639; it calls the recorder of
  `podnet-vnet`), `PodUtils/podnet:src/vnet/framing.rs` (253); the TCP of
  podssh-warp on smoltcp 0.14.
- smoltcp 0.14 has each feature that podnet uses.

## Approach

The procedure of `docs/design.md`, section 8; the move from smoltcp 0.12
to 0.14; a `PacketTap` trait for the recorder.

## Prove

```sh
cargo test -p pod-netstack
sh scripts/agent/check.sh
```

## Start condition

T-005 is done.

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
the blocking facade behind the feature `blocking`.

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
