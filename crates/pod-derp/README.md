# pod-derp

A carrier of the resumable layer (`pod-session`) through DERP servers: the
servers that Tailscale runs for tailcat, which need no account, Tailscale's
main fleet, or a derper of the user's own. DERP runs over a WebSocket at
`wss://HOST/derp` on port 443, through the caller's way out, so it passes
where an HTTPS proxy lets TLS pass. A server forwards packets between
Curve25519 keys, keeps their order, and drops the oldest of a receiver's 32
when they wait; under the layer an adapter numbers the bytes of each link,
acknowledges them and sends again what was lost, with a window under the
server's queue. A node serves the links that clients open at its home; a
client opens them by the node's ticket (`derp:` and base32), at the home
or at another server of the fleet. The end-to-end channel above the layer
(`pod-e2e`, the caller's) makes the server trusted for availability only.

The source is podssh's `crates/podssh-relay/src/derp/` with its tests
`derp_link.rs`, `derp_live.rs`, `derp_main_live.rs` and the stand-in
`derp_stand_in/`, at `b9c04657` (podssh froze them there for the move:
PodUtils/podssh@T-684, Q101). It is pure Rust: NaCl's `crypto_box` of RustCrypto.

## API

| Module | What |
| --- | --- |
| (root) | `FEATURES` (the features of the layer over DERP: `replay.v1`, `heartbeat.v1`); `Names`; re-exports `Dialer`, `carry`, `Conn`, `Node`, `Key`, `Secret`, `Link`, `Server`, `Ticket` |
| `Names` | `app` (the `AppName` of the client's info), `user_agent` (the `User-Agent` of the upgrade), `main` (the setting of the main fleet in a refusal and a line); `Default` (neutral), `Names::podssh(user_agent)` |
| `dial` | `Dial` (`Arc<dyn Fn(&Server) -> Dialing>`: a byte stream to a server before its upgrade; a test gives a pipe), `Dialing`, `BoxIo`, `Io`; `tls(Way, Trust)` (TLS through the caller's `pod_ws::Way`); `request(server, key, user_agent)`, `upgrade(io, server, user_agent)`; `PATH`, `SUBPROTOCOL`, `CONNECT_LIMIT`, `IDLE_LIMIT`, `WRITE_LIMIT`, `RECEIVE_CAP` |
| `servers` | `Server` (`new`, `parse`, `authority`, `in_fleet`, `fleet_name`, `fleet_note(&Names)`), `tailcat`, `tailcat_fleet`, `main_fleet`, `fleet_with(main)`, `main_wanted(value, &Names)`, `chosen(named, main, &Names)`, `for_client(home, main)`, `for_client_in(home, list)`; derper's probe with the caller's `pod_proxy::Client`: `probe`, `probe_kept` (with `pod_proxy::Missed`), `answers`, `home`; `probe_path`, `PROBE_LIMIT`, `PORT`, `TAILCAT_FLEET`, `MAIN_FLEET` |
| `ticket` | `Ticket` (`node`: the Ed25519 key of `pod-identity`, `derp`: the node's key on the server, `server`), its `Display` and `parse`; `SCHEME` |
| `key` | `Key` (`hex`, `short`), `Secret` (`of_node(&Identity)`, `random(&mut dyn Entropy)`, `from_bytes`, `public`, `seal`, `open`), `client_info_json(app)`, `server_info(json)`, `ServerInfo` |
| `frame` | DERP's frames, sans-IO: `Decoder` (`push`, `decode`, `buffered`), `Frame`, `FrameError`, `encode`, `client_info`, `send_packet`, `ping`, `pong`; `kind`, `gone`, `MAGIC`, `HEADER`, `KEY`, `NONCE`, `MAX_PACKET`, `MAX_FRAME`, `VERSION` |
| `packet` | the packets of a link, sans-IO: `Packet` (`encode`, `decode`), `Body`, `Ack`, `PacketError`, `data`, `LinkId`; `MAGIC` (`pdl1`), `ID`, `HEAD`, `DATA_HEAD`, `MAX_RANGES`, `MAX_REASON` |
| `send`, `recv`, `rtt` | the adapter, sans-IO: `Sender`, `Outgoing`, `Receiver`, `Rtt`; `SEGMENT`, `MAX_WINDOW`, `SPAN`, `SEND_BUFFER`, `DEAD_AFTER`, `RECV_BUFFER`, `ACK_DELAY`, `rtt::{INITIAL, MIN, MAX}` |
| `conn` | `Conn` (`start(server, io, Setup)`, `server`, `key`, `server_key`, `info`, `why_ended`, `ended`, `close`, `links`, `open_link`, `accept`), `Setup` (`secret`, `serves`, `events`, `names`), `HEALTH`; `HANDSHAKE_LIMIT`, `PING_EVERY`, `MAX_LINKS`, `MAX_PER_PEER` |
| `link` | `Link` (`AsyncRead`, `AsyncWrite`; `opened`, `peer`, `round_trip`); `OPEN_LIMIT`, `LINGER` |
| `client` | `Dialer` (`new(ticket, fleet, dial, events, names)`, `with_servers(ticket, servers, dial, events, names)`, `servers`, `key`, `server`, `link`, `close`), `carry`, `carry_heard` |
| `far` | `Node` (`secret`, `server`, `dial`, `events`, `names`), `connect`, `connect_to`, `home(secret, dial, events, names, servers, probe)`, `serve(node, settings, budget, open, tell)`, `serve_kept`, `serve_from`, `serve_roaming`, `Roam` (`servers`, `moved`; `stay`), `Moved`; `MOVE_AFTER`, `MAIN_SEARCH`; the kinds of the node's events `ONLINE`, `ENDED`, `UNREACHABLE`, `RETRY`, `MOVED` |
| `standin` | (feature `standin`) `StandIn`: a DERP server in memory (`new(drop_percent)`, `server`, `connect`, `dial`, `cut`, `health`, `connected`; the counters `forwarded`, `dropped`, `overflowed`, `logins`; `refuses`) |

A failure of a connection is a `pod_core::Failure`: a dial keeps the stage
and the class of the caller's dial and TLS; the upgrade is `WebSocket`
(`Timeout` at its limit; a refusal by its status); DERP's handshake is
`Relay` (a close there `Refused`, a wait at its limit `Timeout`, bytes
that are not DERP or a box that does not open `Protocol`, an ended
connection by the class of the session's error); a node that does not
answer is `Relay`, `Unavailable`; `carry` gives the layer's failures of the
stage `Session`. A parse of the user's text (`Server::parse`,
`Ticket::parse`, `chosen`, `main_wanted`) gives its words (call C606).

## Features

| Feature | What |
| --- | --- |
| `standin` | the stand-in DERP server in memory (`standin`), for the tests of this crate and of a consumer: a consumer turns it on in `[dev-dependencies]` only, and no binary carries it |

## Settings

The crate reads no variable of the environment and keeps no process
state.

| Value | Where |
| --- | --- |
| The way out (the proxy, the TLS context, the trust) | `dial::tls(Way, Trust)` or a `Dial` of the caller's; the client of derper's probe |
| The servers that the user names (podssh `--derp-server`, `PODSSH_DERP_SERVER`) | `servers::chosen(named, ..)`; the consumer gives the flag, else the variable when it is not empty |
| The main fleet after tailcat's (podssh `PODSSH_DERP_MAIN`) | `servers::chosen(.., main, ..)`, `main_wanted`, `fleet_with(main)`, `for_client(home, main)` |
| The program's names on the wire and in the words | `Names` (`Names::podssh`) |
| The lines for the user | `Arc<dyn pod_core::Events>` of `Setup`, `Node`, `Dialer`, `far::home` |
| The lines of the keeper's budget | the `pod_session::budget::Tell` of `far::serve` |
| The layer's settings, the budget of the replay buffers | `pod_session::Settings`, `budget` of `far::serve*` |

## Parity

- [x] DERP's frames, handshake and keys as derper v1.104.4 writes them; the
  client's info with the program's name; a server's rate kept.
- [x] The adapter: numbered segments, acknowledgements with ranges, the
  sends again at a loss and at the timeout, the window under the server's
  queue, a closed window probed.
- [x] The links of a client and of a node, the `OPEN` sent again, the
  limits of links on a connection.
- [x] The client across the servers of the fleet; the node's home, its
  backoff, its move when its home goes away; the main fleet after tailcat's
  for 60 s at most.
- [x] The ticket, its base32 and its version.
- [x] Each unit test, `derp_link`, `derp_live` and `derp_main_live` with
  their names; the plant of podssh T-449.
- [ ] The interop of `derp_link`'s ignored tests against Tailscale's derper
  (`PodUtils/podssh:scripts/interop-derp.sh`, by hand on a host with
  Podman, with `PODCRATES` set to a checkout of podcrates): no workflow of
  podcrates runs it (call C609).

## Adoption map

### podssh

| Old | New |
| --- | --- |
| `podssh-relay/src/derp/` | `src/derp.rs`: `pub use pod_derp::*`, with podssh's `Say` (a callback of text) and the glue: `say_events(Say) -> Arc<dyn Events>`, `names()` (`Names::podssh(podssh_ws::dial::user_agent())`), `dial::tls(trust, proxy)` over `podssh_ws::client::way(&proxy)`, the probes over `podssh_ws::client::getter(&proxy)`, `servers::{ENV, MAIN_ENV, from_environment(flag), fleet(), for_client(home)}` that read the variables |
| `derp::Say` in `Setup`, `Node`, `Dialer::{new, with_servers}`, `far::home` | `events: derp::say_events(say)`, `names: derp::names()` |
| `far::serve(node, settings, budget, open)` | `far::serve(.., session::budget::told())` |
| `servers::probe_kept(server, trust, proxy)` (`doctor/derp.rs`) | the glue of the same name |
| `crate::open::backoff` (the node's retry) | `pod_core::backoff::backoff` |
| a `String` error of `Conn::start`, `Dialer::link`, `far::connect`, `carry` | `pod_core::Failure` (its `Display` is the same words) |
| `tests/derp_stand_in/mod.rs` (`#[path]` of podssh-cli `tests/derp_support` and podssh-iroh `tests/record_roads.rs`) | `pod_derp::standin` (feature `standin` in `[dev-dependencies]`) |
| `tests/derp_link.rs`, `derp_live.rs`, `derp_main_live.rs`; `.github/workflows/derp-main-live.yml` | `tests/` here; the measures `derp-tailcat` and `derp-main` of `live.yml`, by hand |
| the plant `t449-derp-registered-late` | `scripts/plants/derp-t449-registered-late` here |
| `src/serve/derp.rs`, `src/serve/derp_hub.rs` (the relay role's DERP server) | stay; they take `pod_derp::{frame, Key, Secret}` |
| `podssh-relay/Cargo.toml`: the feature `derp = ["dep:crypto_box"]` | `derp = ["dep:pod-derp"]`; `crypto_box` stays for `tox` |

`adoption/podssh/pod-derp.patch` (at podssh `b9c04657`,
`pod-derp.commit`) is the hand-off, with its steps in `pod-derp.run`.

### podnet

podnet takes the crate for its DERP road (`src/derp.rs`: `podnet derp node`,
`podnet derp connect TICKET`, `podnet send --derp`), with its own `Names`
(app `podnet`) and the default fleet of the crate. No file of podnet is
replaced.

## Tests

`cargo test -p pod-derp --all-features` (`derp_link` needs the feature
`standin`). The unit tests of each module, with the two halves of a link
against each other through a server of the test's that drops packets
(`arq_tests`); `tests/derp_link.rs` on the stand-in in memory: 8 MiB each
way with no loss and with 5 % dropped, a session of the layer across a cut
connection, a client that gives up, a node whose home went away and moves,
a server that closes in the handshake, the node's events of each kind
(`a_node_s_lines_are_events_of_their_kinds`), and the stage and the class
of each failure (`a_failure_of_the_road_carries_its_stage_and_class`). Its
three `through_derper_*` tests are ignored: they need a derper
(`POD_DERP_TEST_SERVER`, `POD_DERP_TEST_CA`; the leaf certificate of a
derper with `-certmode manual` needs `basicConstraints=critical,CA:FALSE`,
as rustls refuses a self-signed certificate with `CA:TRUE`). `derp_live` (tailcat's
fleet) and `derp_main_live` (the first server of each region of the main
fleet) are live and ignored; `live.yml` runs each by hand (its input
`measure`: `derp-tailcat`, `derp-main`).

The plants are `scripts/plants/derp-*` (group `derp`).

## Sources

`SOURCES` lists each imported file with its blob.
