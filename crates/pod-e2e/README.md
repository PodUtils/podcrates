# pod-e2e

The end-to-end channel of podssh for the crates of podcrates and their
binaries: Noise XX (`Noise_XX_25519_ChaChaPoly_SHA256`, on snow 0.10,
pure Rust) over the bytes of a session, so that a relay carries
ciphertext only. Each end proves its key of `pod-identity`: the second
and the third message of the handshake carry the sender's Ed25519 key and
its signature of the Noise static key, encrypted. The operator learns the
node's key before it proves its own. The node then sends its verdict: let
in, or refused with the reason. Each message after it is data, or the
clean end of one direction (the half-close of TCP). A message that fails
its check (changed, dropped, repeated or out of order) ends the session,
and a stream that ends with no clean end is a cut.

The channel runs above the resumable layer of `pod-session`: one
handshake for each session, which a resume carries on. With it: the word
of the operator's proof (a route of the node, or the target of an exit),
a grant used once that the node checks before it dials, the gate of a
node's person after its door, the net's document that two ends give each
other, the tokens of one leg of a session, and the handover of a node's
channel to the binary that takes its place, from the same keys and
nonces. In place of the carry, the two ends can agree on a suite and take
records from the exporter of the handshake: keys for a label that only the
two ends hold, so that each hop between them (a relay, a peer of a mesh)
carries ciphertext only ([Records](#records)).

The source is podssh's `crates/podssh-relay/src/e2e/` at `74760e8`.
podssh `docs/design.md` and `SECURITY.md` give the design and what the
channel defends against.

## API

| Module | What |
| --- | --- |
| (root) | `MAGIC`, `PATTERN`, `PROLOGUE`, `MAX_MESSAGE`, `TAG`, `MAX_DATA`, `MAX_GRANT`, `MAX_WORD`, `HANDSHAKE_LIMIT`, `VERDICT_LIMIT`, `VERDICT_STEP`; `Ask` (`of_route`, `read`, `word`, `route`, `exit`), `exit_word`; `Refusal`; `Error`; `initiate`, `initiate_with`, `Offered` (`peer`, `net_unanswered`, `accept`, `accept_granted`, `reject`); `respond`, `respond_with`, `Asked` (`peer`, `route`, `ask`, `grant`, `admit`, `admit_with`, `refuse`); `Channel` (`carry`, `carry_held`, `leg_seed`, `handshake_hash`, `exporter`, `agree`, `into_transport`), `Carried`, `Held`, `Start`; `Exporter` |
| `exporter` | `Exporter` (`export`, `handshake_hash`, `is_initiator`, `records`), `RESERVED`, `MAX_EXPORT` |
| `record` | `Suite` (`ALL`, `name`, `id`, `of_id`), `agree`; `Settings` (`DEFAULT`); `Records` (`seal`, `open`, `resume`, `replayed`, `epochs`, `counters`), `Opened`, `Record`, `Fault`; `Header` (`encode`, `parse`), `record_len`, `bucket`; `HEADER`, `VERSION`, `MAX_LENGTH`, `BUCKETS`, `PHASE`, `WINDOW`, `RESUME` |
| `ends` | `Node` (`identity`, `admit`, `events`, `redeem`, `leases`, `gate`, `docs`, `holds`; `open_to_all`), `Admit`, `Redeem`; `serve_node`, `serve_routed`, `serve_asked`, `granted`, `TARGET_WAIT`, `NO_EXIT`, `GRANT_REFUSED`; `operator`, `operator_to`, `operator_granted`, `operator_told`, `NO_NET`; the kinds of the events `EVENT_*` |
| `person` | `Gate` (`new`, `with_times`, `times`, `standing`, `cross`), `Times` (`DEFAULT`), `Question`, `Answer`, `Asker`, `Pass`, `By` |
| `net` | `Docs` (`held`, `shares_with`, `take`), `Held` (`newer_than`), `OFFER` |
| `legs` | `token`, `digest`, `hex`, `unhex`, `offered`, `frame`, `Frames`, `WINDOW`, `LIFE_MS`, `OFFER`; `Leases` (`attach`, `issue`, `restore`, `spent`, `held`), `Lease` (`seed`, `hand_over`); `during`, `current`, `Taker` (`give`, `holds`, `pick`, `upgraded`, `refused`, `token_at`), `Pick` |
| `hold` | `Holds` (`holding`, `move_out`, `len`, `is_empty`), `Hold` (`is_asked`), `held`, `current`, `resume`, `HOLD_LIMIT`; `ChannelState` (`encode`, `decode`), `state::{split, transport_of, VERSION}` |
| `fuzzing` | feature `fuzzing`: `take_frames`, `verdict`, `verdict_body`, `net_part`, `net_bytes`, `proof`, `Proof`, `Answer`, `records` (the records of one end of an exporter of fixed keys) |

Each `Error` converts to a `pod_core::Failure` of the stage `Session`,
with the error as its cause:

| Error | Class |
| --- | --- |
| `Io` | the class of its `io::ErrorKind` (`Class::of_io`); the `io::Error` is its source |
| `NotChannel` (the peer does not speak the channel) | `Unsupported` |
| `Handshake`, `Tampered`, `Malformed` | `Protocol` |
| `Cut` (the stream ended before the clean end) | `Unavailable` |
| `Refused(NotAllowed)`, `Refused(Other)`, `NodeKey` (this end refused the node's key) | `Refused` |
| `Refused(NoTarget)` | `Unavailable` |
| `Timeout` | `Timeout` |
| `NoSuite` (the two ends share no suite of records) | `Config` |

A `record::Fault` converts to a `Failure` of the stage `Session`:
`ResumeExpired` and `Exhausted` (the layer is ended) are `Unavailable`,
each other is `Protocol`.

## Records

`Channel::agree(offer)` sends one message `SUITES` (type 5: one byte for
each suite, 1 `chacha20-poly1305`, 2 `aes-256-gcm`, in the sender's order)
and reads the peer's; the suite is the first of the initiator's offer that
the responder offers too, and with none `Error::NoSuite`.
`Channel::exporter()` gives the `Exporter`; `Exporter::records(suite,
settings, now)` gives the `Records` of this end, and
`Channel::into_transport()` gives the stream back for them. The layer is
sans-IO: the caller carries the bytes and gives the time.

A record is a header of 24 bytes (big-endian: `version` u8 (1), `type`
u8, `flags` u16, `stream` u32, `counter` u64, `length` u32, `pad` u16,
`reserved` u16 (0)), a body of `length` bytes (the payload, then `pad`
zero bytes) and a tag of 16 bytes. The header is the associated data. The
nonce is the 4-byte prefix of the sender's direction, then the counter.
The format is the one of PodUtils/podnet#472.

| Rule | Value |
| --- | --- |
| The keys | from the exporter, with the suite's name and the direction in the context; the key of each epoch and the secret of the next come from the secret of the epoch |
| The counter | each direction from 0, never back; `Fault::Exhausted` at its end |
| The rekey | after `rekey_records` (2^31), `rekey_bytes` (1 GiB) or `rekey_age` (1 h); bit 0 of the flags (`PHASE`) is the parity of the epoch; the receiver keeps the key before and the key after |
| The types and the flags | the type `0xFF` (`RESUME`) and bit 0 of the flags are the layer's; the rest are the caller's |
| The padding | with `pad` on, to 0, 128, 512, 1024 or 2048 bytes, and above them to the next multiple of 2048; a body is at most 16 MiB |
| The replay window | 1024 counters behind the highest: below it `Fault::Stale`; a repeat in it `Fault::Replayed`, counted (`replayed`) |
| RESUME | `Records::resume` after a lost carrier: sealed under the resume key of its direction, with its number as the counter; the body is the token of its number (32 bytes), the counter that the sender waits for next (u64), a fresh prefix (4 bytes) for its records after it, and its epoch (u64). It opens once (a number at or above the lowest not used), within `resume_window` (10 min) after the last record that opened. A late one, or the third that fails, ends the layer (`Fault::ResumeExpired`). |

`Records::open` gives `Opened::Record` or `Opened::Resume { next }`: the
peer did not get this end's records from `next` on, and the caller sends
their payloads again in new records. A record that fails leaves the layer
as it was; the caller drops it or ends the link. A record sealed before a
RESUME that comes after it does not open. Compression (podnet's) runs
before the seal, and sets a flag of the caller.

## Events

A node's lines go to its `Node::events` (a `pod_core::Events`), one kind
for each step; the text is the line of podssh.

| Kind | Level | When |
| --- | --- | --- |
| `e2e.handshake` | warn | a session ended in the channel's handshake |
| `e2e.refused` | warn | the node refused an operator: a no of its person that stands, its admission, a grant that failed, its person |
| `e2e.grant` | info | a grant let a key in, or the node set a grant aside |
| `e2e.target` | warn | the target refused the operator, or did not answer within `TARGET_WAIT` |
| `e2e.in` | info | an operator came in, and by what (the door, a grant, a yes of the person, its cover) |
| `e2e.ended` | warn | a session ended with an error after the handshake |
| `e2e.no-net` | info | at the operator's end (`operator_told`): the node gave no document of the net |

## Features

`fuzzing`: the readers of the frames, the verdict, the net's part and the
proof are public for the fuzz targets of `fuzz/` (`e2e_frame`,
`e2e_proof`), and so are the records of the two ends of an exporter of
fixed keys (`e2e_record`). No consumer turns it on.

## Settings

The crate reads no variable of the environment and keeps no process
state, but the task-local values of a session (the hold of a channel's
task, the mark of a granted target, the taker of an operator's tokens).

| Value | Where |
| --- | --- |
| The node's key, who can come in, a grant's check | `Node::identity`, `Node::admit`, `Node::redeem` |
| Where a node's lines go | `Node::events` |
| The node's person: who answers, and the times | `Node::gate`: `Gate::new(asker)` or `Gate::with_times(asker, Times)` |
| The net's documents | `Node::docs`; the operator: the argument `docs` of `operator_told` and of `initiate_with` (`respond_with` for a node of the caller's own) |
| The holds of a node's channels for a handover | `Node::holds` (one `Holds` for each node, which its roads share); `Holds::holding(task)` where the channel's task starts; `Holds::move_out` to `Keeper::hand_over_with` of `pod-session` |
| The tokens of one leg | `Node::leases`: `Leases::attach(frames)` with the node's socket to its role |
| The operator's taker of the tokens | `legs::during(session)` around the session |
| The records: padding, the rekey, the resume | `record::Settings` (`Settings::DEFAULT`), to `Exporter::records` |

The limits are constants: `HANDSHAKE_LIMIT` (30 s for each step),
`VERDICT_LIMIT` (130 s: a step and the question of a person),
`TARGET_WAIT` (20 s), `HOLD_LIMIT` (10 s), `MAX_GRANT` (8 KiB),
`Times::DEFAULT` (a question of 100 s, a cover of 120 s, a no that
stands 15 s).

## Parity

Against podssh `crates/podssh-relay` at `74760e8`:

| Capability of podssh | Here |
| --- | --- |
| `e2e/mod.rs`, `ask.rs`, `frame.rs`, `handshake.rs`, `channel.rs`, `channel/carry.rs` | [x] the root, `Ask`, the handshake, `Channel` |
| `e2e/ends.rs`: the node's `say` | [x] `Node::events`, the events `e2e.*` |
| `e2e/net.rs`: `install`, `installed` (a process global) | [x] `Node::docs`, and the argument `docs` of the operator |
| `e2e/hold/`: the registry of the holds (a process global), `impl Movable for DuplexStream` | [x] `hold::Holds` in `Node::holds`; `Holds::holding`, `Holds::move_out` |
| `e2e/legs.rs`, `legs_tests.rs`, `person.rs`, `person_tests.rs`, `hold/state.rs`, `hold/resume.rs` | [x] |
| `tests/e2e.rs`, `e2e_faults.rs`, `e2e_net.rs`, `e2e_harness/`, `routes.rs`, `person.rs` | [x] the same names |
| `tests/session_handover.rs`: the channel across a handover | [x] the same name |
| `tests/e2e_layered.rs` | in `pod-relay`: it runs `reverse::{Layered, E2e, Handler}` |
| the plants of `e2e/` (`t226-route-unmarked`, `t429-net-gossip`, `t615-seed-unread`, `t615-window-unmoved`, six of `t638`, `t643-no-net-unsaid`, `t668-nonce-reset`) | [x] `scripts/plants/e2e-*`; the three whose tests stay in podssh (`grant.rs`, `relay_serve_once.rs`) run `tests/door.rs` here |

Differences, with the choice:

* `initiate` and `respond` take no document of the net (podssh: the
  documents that the process installed); `initiate_with` and
  `respond_with` take the caller's.
* `operator_told` takes `docs`, and `told` hears an event
  (`e2e.no-net`), not a line.
* The words name no binary and no flag of a consumer: `NO_EXIT` has no
  `(podssh node --exit)`; `NO_NET` says "of an older release";
  `Error::NotChannel` says "an end with the channel off" (podssh:
  `--no-e2e`); a proof that an end cannot read is "not one that this end
  reads". `Error::NotChannel` keeps "podssh's end-to-end channel": it is
  the name of the protocol, as `MAGIC` is.
* The wire keeps `podssh-e2e/1` (the magic and the prologue),
  `podssh-net/1` (the offer of the net) and the label of a token of one
  leg: they name the protocol, so that the ends of each consumer speak to
  each other (call C47 for `pod-session`).
* `Error::Io` gives its `io::Error` as its source, so that
  `Failure::io_cause` finds it.

## Adoption map

### podssh

| Old | New |
| --- | --- |
| `podssh-relay/src/e2e/` | `src/e2e/mod.rs`: `pub use pod_e2e::*` and `pod_core::{Event, Events, NoEvents}`; `events(say)` (a node's events as podssh's lines) and `say(&node)` (podssh's own lines of a node through its events); `src/e2e/net.rs` (podssh's process documents: `install`, `installed`, and `process()`, a `Docs` that reads them at each call); `src/e2e/hold.rs` (podssh's process `Holds`: `holds()`, `holding(task)`, and podssh's `Movable` of the pipe, `holds().move_out`) |
| each `ends::Node { .., say, .. }` | `events: e2e::events(say)`, `docs: e2e::net::process()`, `holds: e2e::hold::holds()` |
| `(channel.say)(line)`, `channel.say.clone()` of a node | `e2e::say(&channel)` |
| `ends::operator_told(app, identity, check, route, grant, tell)` | `.., grant, e2e::net::installed(), tell)`, with `tell` of an `Event` |
| `ends::operator(..)` of the blocking facade | `ends::operator_told(.., None, None, e2e::net::installed(), \|_\| {})` |
| `Node::open_to_all(key, Arc::new(\|line\| ..))` | `Node::open_to_all(key, e2e::events(Arc::new(\|line\| ..)))`, or `Arc::new(e2e::NoEvents)` |
| `e2e::hold::holding(task)`, `hold::resume(..)`, `keeper.hand_over()` | the same calls of the glue |
| `e2e/legs_tests.rs`'s `crate::cache::valid_token` | the test here holds the rule |
| the tests `e2e.rs`, `e2e_faults.rs`, `e2e_net.rs`, `routes.rs`, `person.rs`, `session_handover.rs` | `tests/` here; podssh keeps `e2e_harness/` for `grant.rs`, `quick_exit.rs` and `e2e_layered.rs` |
| the thirteen plants of `e2e/` | `scripts/plants/e2e-*` here |
| the citations of the record of the removed files | `PodUtils/podssh:PATH`; the other documents name `PodUtils/podcrates:crates/pod-e2e/...` |
| `snow` in `podssh-relay/Cargo.toml` and in the workspace | `pod-e2e` (tag `v0.22.0`) |

`adoption/podssh/pod-e2e.patch` (at podssh `74760e8`) is the hand-off.

### podnet

podnet has no end-to-end channel now. No file of podnet is replaced, and
no file of podnet is frozen. The take adds `pod-e2e` above `pod-session`
on its relay road: a `Node` with podnet's key of `pod-identity`, its
events to podnet's lines, and the operator's end of `ends::operator`.

The record layer of podnet's link (PodUtils/podnet@T-298,
PodUtils/podnet@T-300, PodUtils/podnet@T-301) is `record` here: the
handshake between the two ends of the link, `Channel::agree` with the
suites of podnet's settings, `Exporter::records`, then the records over
the carrier. A suite that does not match is `Error::NoSuite` (exit 78);
`Fault::ResumeExpired` is podnet's `ResumeExpired` (exit 69). The
compression (PodUtils/podnet@T-299) runs before `Records::seal` and sets a
bit of the flags above bit 0.

## Tests

`cargo test -p pod-e2e`: the unit tests of each module (the words of a
proof, the frames, the verdict and its seed, the state of a handover and
the transport from its keys, the tokens of one leg, the net's part of a
proof, the gate of a person on tokio's paused clock, the classes of the
failures); and over duplex pipes and a stand-in relay that changes, drops,
repeats, moves and cuts frames: the cacophony vector of the pattern, no
plain text at the relay, each fault of a relay an end of the session, the
half-close, a peer with no channel, a replayed proof, the net's
documents, a node's routes and an exit's target, a node's person, a grant
at the door, the operator's seed of its tokens, and a session across a
handover with a TCP target on the loopback; the exporter (the same key
at both ends, another for another label, another hash or other keys) and
the records (`tests/record.rs` and `record::tests`: each suite, a changed
byte, a repeat and a stale counter, a rekey by count, by bytes and by age,
the buckets, a RESUME used once, three failures and a late one, the epoch
of a RESUME, the agreement of the suites, a document of the net before
them). The plants are
`scripts/plants/e2e-*` (group `e2e`). The fuzz targets `e2e_frame`,
`e2e_proof`, `e2e_handover` and `e2e_record` read the frames, the verdict,
the net's part of a proof, a proof, the state of a handover, and the
records that a peer sends (T-510).

## Sources

`SOURCES` lists each imported file with its blob.
