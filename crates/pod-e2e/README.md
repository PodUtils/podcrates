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
nonces.

The source is podssh's `crates/podssh-relay/src/e2e/` at `74760e8`.
podssh `docs/design.md` and `SECURITY.md` give the design and what the
channel defends against.

## API

| Module | What |
| --- | --- |
| (root) | `MAGIC`, `PATTERN`, `PROLOGUE`, `MAX_MESSAGE`, `TAG`, `MAX_DATA`, `MAX_GRANT`, `MAX_WORD`, `HANDSHAKE_LIMIT`, `VERDICT_LIMIT`, `VERDICT_STEP`; `Ask` (`of_route`, `read`, `word`, `route`, `exit`), `exit_word`; `Refusal`; `Error`; `initiate`, `initiate_with`, `Offered` (`peer`, `net_unanswered`, `accept`, `accept_granted`, `reject`); `respond`, `respond_with`, `Asked` (`peer`, `route`, `ask`, `grant`, `admit`, `admit_with`, `refuse`); `Channel` (`carry`, `carry_held`, `leg_seed`), `Carried`, `Held`, `Start` |
| `ends` | `Node` (`identity`, `admit`, `events`, `redeem`, `leases`, `gate`, `docs`, `holds`; `open_to_all`), `Admit`, `Redeem`; `serve_node`, `serve_routed`, `serve_asked`, `granted`, `TARGET_WAIT`, `NO_EXIT`, `GRANT_REFUSED`; `operator`, `operator_to`, `operator_granted`, `operator_told`, `NO_NET`; the kinds of the events `EVENT_*` |
| `person` | `Gate` (`new`, `with_times`, `times`, `standing`, `cross`), `Times` (`DEFAULT`), `Question`, `Answer`, `Asker`, `Pass`, `By` |
| `net` | `Docs` (`held`, `shares_with`, `take`), `Held` (`newer_than`), `OFFER` |
| `legs` | `token`, `digest`, `hex`, `unhex`, `offered`, `frame`, `Frames`, `WINDOW`, `LIFE_MS`, `OFFER`; `Leases` (`attach`, `issue`, `restore`, `spent`, `held`), `Lease` (`seed`, `hand_over`); `during`, `current`, `Taker` (`give`, `holds`, `pick`, `upgraded`, `refused`, `token_at`), `Pick` |
| `hold` | `Holds` (`holding`, `move_out`, `len`, `is_empty`), `Hold` (`is_asked`), `held`, `current`, `resume`, `HOLD_LIMIT`; `ChannelState` (`encode`, `decode`), `state::{split, transport_of, VERSION}` |
| `fuzzing` | feature `fuzzing`: `take_frames`, `verdict`, `verdict_body`, `net_part`, `net_bytes`, `proof`, `Proof`, `Answer` |

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
`e2e_proof`). No consumer turns it on.

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
| `tests/e2e_layered.rs` | stays in podssh until `pod-relay`: it runs podssh's `reverse::{Layered, E2e, Handler}` |
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
handover with a TCP target on the loopback. The plants are
`scripts/plants/e2e-*` (group `e2e`). The fuzz targets `e2e_frame`,
`e2e_proof` and `e2e_handover` read the frames, the verdict, the net's
part of a proof, a proof, and the state of a handover.

## Sources

`SOURCES` lists each imported file with its blob.
