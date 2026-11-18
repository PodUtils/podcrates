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
| `client`, `far` | the two ends over tokio streams: `client::start`, `Client`, `Outcome`, `Rewound` (`Rewound::new(first, link)`: the link with bytes that a router read given back first); `far::accept`, `accept_with_room`, `serve`, `serve_admitted`, `serve_admitted_by` (the link named for the line of a resume), `Accepted` (`by(name)`, `link_name`), `FarError` |
| `pump` | one link of a session: `run`, `Carry`, `Watch`, `End`, `Ended`, `PING_EVERY`, `DEAD_AFTER`, `PING_ANSWER` (a link whose far end answers no `PING` for 30 s while the session does not move is lost, also while the far end's own records come: PodUtils/podssh#283, ask 4) |
| `resume`, `beside`, `race` | the client's session across links (`resume::run`, `Next`, `Note`, `RESUME_DEADLINE`, `MOVE_BYTES`, `MOVE_AGE`); the spare link (`spare_after`, `SpareWait`, `Position`, `SPARE_AFTER`, `SPARE_SOONEST`, `SPARE_QUIET`, `SPARE_EVERY`); the race of roads to one far end (`race`, `in_turn`, `each_timed`) |
| `keep`, `budget`, `sessions` | `Keeper` (the far end's sessions across links: `new`, `with_tell`, `census`, `admit`, `run_new`, `run_resumed`, `hand_over`, `hand_over_with`, `take_over`), `keep::RESUME` (the kind of the line of each resume); `Census`, `Told`, `Tell`, `refusal`, `filling`, `holders` |
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
| Where the line of each resume goes | the same sink: the event `session.resume` (level `Info`), below |
| The resolver of a UDP flow | `udp::flow::connect(&dyn Resolve, ..)`, `probe(&dyn Resolve, ..)` |
| The move of a target at a handover | `Movable` for a TCP stream, or `Keeper::hand_over_with(move_out)` |
| The random bytes | an `Entropy` to each handshake |

## The line of each resume

The far end sends one event of the kind `session.resume` (`keep::RESUME`,
level `Info`) to the sink of `Keeper::with_tell` at each resume that
`Keeper::run_resumed` runs (PodUtils/podssh#283). A resume that takes its
session says:

```text
session 0187db18: resume 2 on relay session ID: the client has 1441792 bytes of this side, and this side sends 622592 bytes again; this side has 1671168 bytes of the client (1671168 in its ACCEPT), 0 of them not yet written to the target; it stopped the link of resume 1; room 1048576 in the replay buffer, a window of 786432 bytes
```

* `resume N`: the number of the resume, in the order of the handshakes of
  the far end (`Sessions`).
* `on NAME`: the caller's name of the link (`Accepted::by`,
  `far::serve_admitted_by`); pod-relay's node names the relay session.
  Absent when the caller gives none.
* `the client has X bytes of this side`: the client's received offset in
  its `PROOF`; `this side sends Y bytes again` from it.
* `this side has R bytes of the client (A in its ACCEPT)`: the far end's
  received offset when the resume took the session, and the offset that its
  `ACCEPT` gave the client. The client sends again from `A`, and its line
  "N bytes sent again" is its sent offset less `A`. `R` is more than `A`
  when the old link read on after the `ACCEPT`.
* `H of them not yet written to the target`: the bytes that wait for the
  target.
* What held the session: `no link carried the session`, `it stopped the
  first link`, or `it stopped the link of resume M`.
* `room R in the replay buffer, a window of W bytes`: what the reader of
  the target may send on the new link, once the resume took the session;
  `no window` where the road sets none.

A resume that does not take its session says `resume N on NAME did not take
it:` and why: `a newer link resumed the session (resume M); this resume gave
way`, `the session ended on its old link`, `the old link of the session did
not stop`, or the offsets that the far end no longer keeps.

The far end also sends one event of the kind `session.stall`
(`pump::STALL`, level `Info`) to the same sink when a wait of a link holds
for 20 s (`pump::STALL_AFTER`), once for each wait:

```text
session 0187db18: the reader of the target waits for room for 20 s; this side sent 5040486 bytes, the client acknowledged 4253952; the replay buffer keeps 786534 bytes, room 262042; a window of 786432 bytes; this side received 81920 bytes, 0 of them not yet written to the target
```

* `the reader of the target waits for room`: the replay buffer or the
  window is full, and no `ACK` came.
* `the reader of the target waits N s for the writer of the link`: the
  writer did not take the last batch.
* `the records sent again wait for room`: the bytes of a resume wait for
  the window.
* `the target sent no byte for N s, while it took B bytes of the client`:
  the reader of the target waits on the target. A target that took no byte
  in the wait gives no line.
* `the target took no byte for N s, while H bytes wait for it`: the writer
  of the target waits on the target, and the client's bytes stay held. An
  acknowledgement covers only the bytes written, so the client sees no
  acknowledgement of them.
* `this side received R bytes, H of them not yet written to the target`:
  the received offset, and the bytes held for the target.

The client says the same waits of its own link as `resume::Note::Stall`,
one note for each wait, with the words alone (`resume::run` gives each
note to its caller):

```text
the application took no byte for 20 s, while 31744 bytes wait for it; this side sent 0 bytes, the far end acknowledged 0; the replay buffer keeps 0 bytes, room 1048576; no window; this side received 32768 bytes, 31744 of them not yet written to the application
```

* `the application took no byte for N s, while H bytes wait for it`: the
  application of the client is slow, and the far end sees no
  acknowledgement of the bytes held. A link that lost the bytes gives no
  such note.
* `the reader of the application waits for room` and `the reader of the
  application waits N s for the writer of the link`: as at the far end.
* A quiet application of the client gives no note: a user who types
  nothing is not a wait.

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
a closed one hold of the budget), `tests/session_resume_again.rs` and
`tests/session_resume_twice.rs` (the cases of PodUtils/podssh#283: a first
new link that ends before the greeting, then a resume about 1.1 s later,
with the old link ended or still alive at the far end; a spare beside a
stopped relay host, then a reset of each open link and a second resume at
once; bytes both ways, whole, and the far end's line of each resume),
`tests/session_heartbeat.rs` (with a far end whose own `PING`s come, but
which answers none of this side's), and `tests/session_spare_backoff.rs` (a
spare on a road that answers the handshake at once and holds each other
byte for 5 s or 13 s: the wait of the next spare doubles until a link
carries the bytes). The plants of podssh's layer are
`scripts/plants/session-*` (group `session`). The fuzz targets `session_record`, `session_handover` and
`udp_frame` read the records, the state of a handover and the UDP framing.

## Sources

`SOURCES` lists each imported file with its blob.
