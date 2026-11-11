# pod-session

The resumable layer of podssh for the crates of podcrates and their
binaries: a byte stream between two ends that outlives its link to a relay.
Each side numbers the bytes that it sends with 64-bit offsets from 0; the
first link sets the session's id and a 256-bit secret (X25519, HKDF-SHA-256);
after a loss the client proves the secret (HMAC-SHA-256) on a new link
through any road, and each side sends again from the other's received
offset. The layer has a replay buffer, a heartbeat, the move to a new link
before the limits of a relay, a spare link beside a link that went silent,
the end of one direction, a session that ends with its link, the far end's
sessions with their budget, and the handover of a far end's sessions to the
binary that takes its place. With it: UDP in a byte stream, the link over a
WebSocket of `pod-ws`, and the rule that reads a peer's JSON answer.

The source is podssh's `crates/podssh-relay`: `src/session/` (with
`handover/`), `src/udp/`, `src/answer.rs` and `src/transit/link.rs`, at
`c5636c9`. podssh `docs/design.md`, section 5, gives the records, the
handshake and what the layer defends against.

## API

| Module | What |
| --- | --- |
| (root) | `Settings` (what one end offers and keeps), `FEATURES`, `KEEP`, `SHELL_FEATURES`; re-exports of the main types of the modules below |
| `Settings` | `features`, `replay_capacity`, `window`, `pair_window`, `resume_deadline`, `client_deadline`, `wall_clock`, `move_bytes`, `move_age`, `spare_links`, `resumes`, `half_close`; `on_the_pair()`, `off_the_pair()`, `link(&Established)` |
| `record`, `decode` | the records (`Record`, `Hello`, `Opening`, `Acceptance`, `Role`, `RefuseCode`), `Record::encode`, `Decoder` (whole records out of the bytes of a link), `decode_body`, the limits (`HEADER`, `MAX_BODY`, `MAX_DATA`, `MAX_REASON`, `MAGIC`, `VERSION`) |
| `offset`, `replay` | `Inbound`, `Outbound`; `Replay`, `capacity(value)`, `DEFAULT_CAPACITY`, `MAX_CAPACITY`, `PAIR_WINDOW`, `NotKept` |
| `secret` | `SessionId`, `Nonce`, `Proof`, `Secret`, `derive`, `prove`, `verify`; `Entropy`, `OsEntropy`, `NoRandom` of `pod-core` |
| `handshake` | `ClientHandshake`, `FarHandshake`, `Established`, `Ask`, `Step`, `HandshakeError` (sans-IO) |
| `link` | `Link` (a session past its handshake, record by record), `Event`, `LinkError`, `LinkParts` |
| `client`, `far` | the two ends over tokio streams: `client::start`, `Client`, `Outcome`, `Rewound` (`Rewound::new(first, link)`: the link with bytes that a router read given back first); `far::accept`, `accept_with_room`, `serve`, `serve_admitted`, `Accepted`, `FarError` |
| `pump` | one link of a session: `run`, `Carry`, `Watch`, `End`, `Ended`, `PING_EVERY`, `DEAD_AFTER` |
| `resume`, `beside`, `race` | the client's session across links (`resume::run`, `Next`, `Note`, `RESUME_DEADLINE`, `MOVE_BYTES`, `MOVE_AGE`); the spare link; the race of roads to one far end (`race`, `in_turn`, `each_timed`) |
| `keep`, `budget`, `sessions` | `Keeper` (the far end's sessions across links: `new`, `with_tell`, `census`, `admit`, `run_new`, `run_resumed`, `hand_over`, `hand_over_with`, `take_over`); `Census`, `Told`, `Tell`, `refusal`, `filling`, `holders` |
| `deadline`, `hearing`, `round_trip`, `once`, `half_close` | the deadline of each end, what the client heard, the round trip of a link, `once.v1`, `end.v1` |
| `handover` | `LayerState::{encode, decode}`, `Handed`, `Moved`, `Movable` (for `tokio::net::TcpStream`), `Moving`, `scope`, `current` |
| `udp` | `codec` (`framed`, `encode`, `Decoder`, `Frames`, `MAX_DATAGRAM`), `flow` (`connect`, `probe`, `carry`, `NoFlow`, `FlowEnd`, `IDLE`) |
| `ws_link` | `Link::start(RelaySession, first)`, `Link::start_by(.., who)`: a byte stream over a WebSocket session of `pod-ws` |
| `answer` | `says_true`, `offers`, `kept`, `hint`, `dialect` |

Each error converts to a `pod_core::Failure` of the stage `Session` with a
class: a busy far end is `TempFail`, a bad proof `Refused`, a forgotten
session `Unavailable`, a wrong version or role `Unsupported`, bytes that are
not records `Protocol`, a wait at its limit `Timeout`. `NoFlow` keeps the
failure of the resolver, and a host that refuses UDP is `Policy` at the stage
`Host`.

## Features

None.

## Settings

The crate reads no variable of the environment and keeps no process
state, except the task-local id of `handover::scope`.

| Value | Where |
| --- | --- |
| The features, the replay buffer, the window, the deadlines, the move, the spare link, `--no-resume`, the end of one direction | `Settings` |
| The limits of the relay: the window of the pair's road (1 MiB held for a slow receiver), the bytes (64 MiB) and the age (12 h) of a session | `Settings::pair_window`, `move_bytes`, `move_age` |
| The value of the replay buffer of the consumer (podssh's `PODSSH_REPLAY_BUFFER`) | `replay::capacity(Option<&str>)` |
| Where the budget's lines go, and the words of what changes the budget | `Keeper::with_tell(Tell::new(events, change))`: the events `session.budget.refused` and `session.budget.filling` |
| The resolver of a UDP flow | `udp::flow::connect(&dyn Resolve, ..)`, `probe(&dyn Resolve, ..)` |
| The move of a target at a handover | `Movable` for a TCP stream, or `Keeper::hand_over_with(move_out)` |
| The random bytes | an `Entropy` to each handshake |

## Parity

Against podssh `crates/podssh-relay` at `c5636c9`:

| Capability of podssh | Here |
| --- | --- |
| `session/`: records, offsets, secrets, handshakes, link, replay, pump, resume, move, spare, race, keep, budget, deadline, hearing, round trip, once, half close, sessions | [x] each module, with its tests |
| `session/handover/`, `keep/handover.rs` | [x] `handover`, `Keeper::hand_over`, `take_over`; `hand_over_with` is new |
| `session/replay.rs`: `CAPACITY_ENV` | podssh keeps the variable; `capacity` parses its value |
| `session/budget.rs`: `tell`, `say` (a process global) | [x] `Tell` of each `Keeper` |
| `udp/` | [x] `udp`; the resolver is the caller's |
| `answer.rs` | [x] `answer` |
| `transit/link.rs` | [x] `ws_link` |
| `tests/session_*.rs` (19 files), `tests/udp_framing.rs` | [x] the same names; the test of the handover with the channel is in `pod-e2e` |
| `tests/session_policy.rs`, `tests/session_relay_resume.rs` | `session_policy.rs` (the close table of the reverse road and the forward layer) is in `pod-relay`; `session_relay_resume.rs` runs the role and waits for `pod-relay-serve` |

Differences, with the choice:

* `Role` shows "a client", "a node", "a relay role" (podssh: "a podssh
  client", "a podssh node", "podssh serve").
* The refusal of the budget names the consumer's words (`Tell::change`);
  the default is "the resume deadline and the replay buffer of the node".
* The wire keeps `podssh-session` as its magic and in the labels of its
  keys: it is the name of the protocol, so that the ends of each consumer
  speak to each other.

## Adoption map

### podssh

| Old | New |
| --- | --- |
| `podssh-relay/src/session/` | `src/session.rs`: `pub use pod_session::*`, with podssh's `budget` (the sink of `node_flags.rs` and its words) and `handover` (podssh's own `Movable`, which the pipe of `e2e/hold` implements) |
| `budget::tell(..)`, `budget::say(..)` | the same functions of podssh's glue; each `Keeper::new` takes `.with_tell(session::budget::told())` |
| `Keeper::hand_over()` of a `DuplexStream` keeper | `keeper.hand_over_with(<A as session::handover::Movable>::move_out)` |
| `replay::CAPACITY_ENV` | podssh's constant `PODSSH_REPLAY_BUFFER` in `podssh-cli/src/layered.rs` |
| `podssh-relay/src/udp/` | `pub use pod_session::udp`; `flow::connect` and `flow::probe` take `&*podssh_ws::resolve::resolver()` |
| `podssh-relay/src/answer.rs` | `pub use pod_session::answer` |
| `podssh-relay/src/transit/link.rs` | `pub use pod_session::ws_link::*` |
| the tests `session_*.rs` and `udp_framing.rs` but `session_policy.rs` and `session_relay_resume.rs` | `tests/` here; `session_handover.rs` keeps the test with the channel |
| the plants `t151-*`, `t152-*`, `t154-*`, `t155-*`, `t168-*`, `t435-*`, `t586-*`, `t612-*`, `t632-*`, `t633-*`, `t668-replay-dropped` | `scripts/plants/session-*` here |
| the citations of the record of the removed files | `PodUtils/podssh:PATH`; each done entry whose Prove names a moved test target has a `## Correction` that names it |
| `podssh-relay/Cargo.toml` | `pod-session` (tag `v0.15.0`) |

`adoption/podssh/pod-session.patch` (at podssh `9975fe92`, after its
take of `pod-quic`) is the hand-off; it applies after the patch of
`pod-wormhole` (`pod-session.needs`).

### podnet

podnet has no resumable layer now (PodUtils/podnet T-086 takes it). No
file of podnet is replaced, and no file of podnet is frozen. The take adds
`pod-session` (and later `pod-e2e`) under its relay road: a session over
`ws_relay.rs`, `ws_link::Link` over a `pod_ws::RelaySession`, the variables
of the layer as rows of podnet's `src/manual/facts.rs`, and the budget's
lines through `Keeper::with_tell` to podnet's events.

## Tests

`cargo test -p pod-session`: the unit tests of each module, the 20 test
files of podssh on duplex pipes and the loopback (a stand-in relay,
targets on `127.0.0.1:0`, the paused clock of tokio for the heartbeat and
the deadlines), and `tests/session_budget.rs` (what a closing session and
a closed one hold of the budget). The plants of podssh's layer are
`scripts/plants/session-*` (group `session`). The fuzz targets `session_record`, `session_handover` and
`udp_frame` read the records, the state of a handover and the UDP framing.

## Sources

`SOURCES` lists each imported file with its blob.
