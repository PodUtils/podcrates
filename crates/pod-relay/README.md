# pod-relay

The client of a WebSocket-to-TCP relay of podssh's contract, for the
crates of podcrates and their binaries: which relay hosts to use and in
what order (an explicit list, or the default host and its pool, ranked
from this host), a forward session with failover from host to host, the
tokens (given, cached or minted), the roads to a relay (TLS, HTTP/2 by
RFC 8441, QUIC where UDP passes, plain HTTP/1.1 requests, cleartext last),
the forward layer of `pod-session`, the pairs of the reverse road, its node
and its operator with the end-to-end channel of `pod-e2e`, the grants used
once, the exchange of a pairing by a short code over `pod-wormhole`, and a
blocking facade for a caller with no async runtime (podvm).

The source is podssh's `crates/podssh-relay/src/` at `75a5031`
(`SOURCES`). podssh's `docs/relay.md`, `docs/reverse.md` and its
`SECURITY.md` give the contract and what the client defends against.

## The context of the caller

Each call that reaches a relay takes a `Context`:

| Field | What |
| --- | --- |
| `way` | the `pod_ws::Way`: the TLS context of the binary, its dialer, its user agent |
| `proxy_of` | the proxy that the dialer takes for a host (a health check names it; QUIC goes only where there is none) |
| `resolver` | the `pod_dns::Resolver` under the dialer, where the caller has one: the addresses that carried a session go to its memory |
| `cache` | the places of the private files (`Cache`: `pod_core::files::Places` and what the variable of the cache says) |
| `settings` | the `Settings` that the consumer read from its environment and its settings file |
| `events` | where the lines for the user go |
| (shared) | the gates of each origin, the limits of the relay in use, the races of the rank, the pool of HTTP/2 connections, what the QUIC road learned of each host: one run's, shared by each clone |

The crate reads no variable of the environment, keeps no process state,
and prints nothing.

## API

| Module | What |
| --- | --- |
| (root) | `Context`, `Settings`, `Names`, `open`, `Request`, `Opened`, `OpenError`, `Failure`, `Relay`, `default_relay_host` |
| `relay` | `Relay`, `RelayList`, `select_relays`, `parse_relay_list`, `forward_path`, `node_path`, `operator_path`, the budgets (`byte_budget`, `byte_budget_under`, `time_budget`, `idle_secs`), the limits of the default deployment |
| `pool` | `alternates`, `refresh`, `parse_pool`, `parse_limits`, `Limits`, `same_deployment`; `signed` (the signed lists, `Keys`), `kept` (the lists kept), `members` (the relay members of a net) |
| `token`, `cache` | `obtain`, `MintContext`, `Token`, `TokenError`, `token_key`; `Cache`, `Cached`, `valid_token` |
| `open`, `failover`, `failures`, `gate` | the opener, the modes of a failover, the record of each host's faults, the gate of each origin |
| `rank`, `rank_list`, `rank_auto` | the rank of the hosts, the order of the default list, the rank in a session's background (`Races`) |
| `h2_road`, `quic_road`, `http_road`, `cleartext`, `forward_layer` | the roads to a relay |
| `health`, `addresses`, `keyed` | `/health` of a host; the addresses that carried a session; a request that the relay asks this client's key to sign |
| `pair` (feature `pair`) | `PairContext`, `create`, `status`, `stop`, `revoke`, `halt`, `rotate`, the stores of a pair and of an operator's part |
| `reverse` (feature `pair`) | `run` (the node), `NodeConfig`, `NodeState`, `Handler`, `TcpHandler`, `UdpHandler`, `Layered`, `E2e`, `routes::Routes`, `operator`, `serve`, `Settings`, `wire::{Wire, Door, Carrier}`, the codecs |
| `grant` (feature `grant`) | `Cert`, `Door`, `Mint`, `Spent`, `parse_public` |
| `code` (feature `code`) | `exchange` (what the ends say), `run` (`give`, `take`, `give_with`, `take_with`, `Ends`, `Waits`), `LIFETIME` |
| `blocking` (feature `blocking`) | `Client`, `Config`, `NodeOptions`, `Operator`, `Forward`, `Local`, `BlockingHandler`, `NodeChannel`, `OperatorChannel`, `Stopper` |
| `failure` | the stage and the class of each error |
| `fuzzing` (feature `fuzzing`) | the readers of the wire for `fuzz/` |

### Failures

Each error converts to a `pod_core::Failure`. A failed connection keeps the
stage and the class of `pod-ws`; an answer of the relay is at the stage
`Relay`:

| Error | Class |
| --- | --- |
| `OpenError::Offline` | `Policy` |
| `OpenError::Deadline` | `Timeout` |
| `OpenError::Held` | `TempFail` |
| `TokenError::BadEnvironment` (a given token that is no token) | `Usage` |
| `TokenError::RateLimited` | `TempFail` |
| `TokenError::NotIssued` | `Unsupported` |
| `TokenError::Failed` | `Unavailable` |
| `PairError::Forbidden` | `Refused` |
| `PairError::ShortLived` | `Revoked` |
| `PairError::BadAnswer` | `Protocol` |
| `PairError::BadLabel` | `Usage` |
| `PairError::Store` | `Config` (stage `Host`) |
| `HealthError::Answer` | `Protocol` |

`open::Failure` is classed by its last attempt.

### Events

| Kind | Level | When |
| --- | --- | --- |
| `reverse.spares` | info | a node's relay keeps sessions readied ahead, or not (once for each change) |
| `reverse.closed` | warn | the relay closed a node's session with a reason (one each `SAY_EVERY`) |

The opener's notes (`open`'s `notes`), a node's lines (`NodeConfig::say`)
and a UDP handler's lines (`UdpHandler::say`) are callbacks of the caller,
as in podssh.

## Features

| Feature | What |
| --- | --- |
| `pair` | the pairs and the reverse road |
| `blocking` | the blocking facade (with `pair`) |
| `code` | the exchange of a pairing by code (with `pair`; `pod-wormhole`) |
| `grant` | the grants used once |
| `quic` | the QUIC road (`pod-quic`, noq) |
| `plain-ws` | plain `ws://` to a stand-in relay on the loopback, for the tests of an embedder; a binary does not turn it on |
| `fuzzing` | the readers of the wire for `fuzz/`; no consumer turns it on |

## Settings

Each variable of podssh is a field of `Settings`; the consumer keeps the
variable, reads it once, and gives its text. `Names` holds the names that
the words of the crate cite (`Names::podssh()` gives podssh's; the
default names are neutral).

| podssh | `Settings` |
| --- | --- |
| `PODSSH_RELAY_TOKEN` | `token` |
| `PODSSH_OFFLINE` | `offline` |
| `PODSSH_RELAY_KEYS`; the settings file's `RelayKeys` and the net's relay members' keys | `keys`; `keys_by_settings` |
| the net's relay members (`pool::members::set_by_net`) | `members` |
| `PODSSH_RELAY_RANK`; the settings file's `RelayRank no` and `RelayRank auto` | `rank`; `rank_by_file`, `rank_auto_by_file` |
| `PODSSH_RELAY_RANK_PACE` | `rank_pace` |
| `PODSSH_H2`, `PODSSH_QUIC`, `PODSSH_CLEARTEXT` | `h2`, `quic`, `cleartext` |
| `PODSSH_CACHE_DIR` | `Context::cache` (`Cache::new(places, chosen)`) |
| `PODSSH_RELAY`, `PODSSH_SESSION_BUDGET` | the caller's own: `select_relays(flag, env, pool)` and `byte_budget(ctx, env)` take their text |
| `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY` | the caller's dialer in `Context::way`, and `Context::proxy_of` |
| `PODSSH_RELAY_ADDR` | the pins of the caller's resolver |

## Parity

Against podssh `crates/podssh-relay` at `75a5031`:

| Capability of podssh | Here |
| --- | --- |
| `relay.rs`, `open.rs`, `failover.rs`, `failures.rs`, `rank*.rs`, `token.rs`, `health.rs`, `keyed.rs` | [x] |
| `pool.rs`, `pool/` | [x] `pool`; the keys and the members of the process are `Settings` |
| `gate.rs` (`gate::process()`, a process global) | [x] `Context::gates` |
| `pool::set_in_use`, `pool::in_use` (a process global) | [x] `Context::set_in_use`, `Context::in_use` |
| `rank_auto::racing` (process atomics) | [x] `rank_auto::Races` in the `Context` |
| `addresses.rs` (`install` into podssh's process resolver) | [x] the memory of `Context::resolver` |
| `cache.rs`: the token entries | [x] `cache`; its files are `pod_core::files` |
| `h2_road.rs` over podssh-ws's process `POOL` | [x] `Context::h2_pool` |
| `quic_road/` (a process memory of each host's UDP) | [x] in the `Context` |
| `cleartext.rs`, `http_road/`, `forward_layer/` | [x] |
| `pair.rs`, `pair/` | [x] |
| `code/exchange.rs`, `code/run.rs` | [x] `code`; the AppID, the name and the way are the caller's `pod_wormhole::Config` |
| `grant/` | [x] |
| `reverse/`: `node::set_upgrading`, `legs` (`ROTATES`), `spares` (`SAY`, `SAID`) | [x] `reverse::NodeState` in `Settings::node` |
| `reverse/wire.rs`: `serve::reverse::carrier::Door` | [x] the trait `reverse::wire::Door` (`Carrier::of`) |
| `reverse/tcp.rs`: the dial of the environment's proxy | [x] the caller's `pod_proxy::Dialer` (`TcpHandler::dialer`, `Routes::dialer`) |
| `reverse/udp.rs`: podssh's process resolver | [x] `UdpHandler::resolve` |
| `reverse/layered.rs`: the budget's lines to podssh's sink | [x] `Layered::telling` |
| `blocking/` | [x] `blocking`; `Config::new(ctx)` |
| `tls_groups.rs` | stays in podssh: it gives `pod-tls` the file of its cache |
| the tests of these files | [x] 370 of 380 with their names; 7 of the cache's places are `pod-core`'s, 3 of magic-wormhole's Python peer `pod-wormhole`'s |
| `tests/session_relay_resume.rs`, `relay_serve_*.rs`, `reverse_carrier.rs`, `reverse_restart.rs`, `http_road.rs` | wait for `pod-relay-serve`: they run the relay role |
| podnet's `src/ws_relay.rs`, `src/relay_pool.rs`, `src/relay_reverse.rs` | T-026 |

## Adoption map

### podssh

| Old | New |
| --- | --- |
| `podssh-relay/src/{relay,open,pool,failover,failures,gate,rank,rank_auto,rank_list,token,health,addresses,keyed,h2_road,cleartext}.rs` and `pool/`, `forward_layer/`, `pair/`, `grant/`, `reverse/`, `http_road/`, `quic_road/`, `blocking/` | `pod_relay::*`, the same module names |
| `podssh_ws::ProxyChoice` and `podssh_ws::WsClientConfig { proxy }` | a `pod_relay::Context` (with podssh's way of `podssh_ws::client::way`) and `pod_ws::WsClientConfig { way, limits }` |
| `token::MintContext { relay, trust, proxy, timeout }` | `MintContext { ctx, relay, trust, timeout }` |
| `open::Request { relays, .. }` | `Request { ctx, relays, .. }` |
| `pair::PairContext { relay, trust, proxy, timeout }` | `PairContext { ctx, relay, trust, timeout }` |
| `reverse::NodeConfig { proxy, .. }`, `OperatorConfig { proxy, .. }` | `{ ctx, .. }` |
| `reverse::node::set_upgrading(on)`, `stopping_words()` | `settings.node.set_upgrading(on)`, `stopping_words(&node)` |
| `reverse::legs::{heard, rotates, offers}()` | the same with `&NodeState` |
| `reverse::spares::tell(say)` | `NodeState::new(events)`: the event `reverse.spares` |
| `pod_session::budget::say` of a close's line | the event `reverse.closed` of `NodeState::events` |
| `reverse::Layered::new` with podssh's `budget::told()` | `Layered::telling(.., budget::told())` |
| `reverse::TcpHandler { .. }`, `routes::Routes { .. }` | with `dialer: podssh_ws::dial::dialer(&proxy, false)` |
| `reverse::UdpHandler::new(host, port, timeout)` | `UdpHandler::new(host, port, timeout, resolve::resolver())` |
| `reverse::wire::Wire::Carrier(serve::reverse::carrier::Door)` | `Wire::Carrier(Carrier::of(door))`, with podssh's role's `impl Door` |
| `code::run::Ends { mailboxes, trust, proxy, say }` | `Ends { mailboxes, config: code::config(&trust, &proxy, say) }` (podssh's `code/mod.rs` stays) |
| `gate::process()` | `ctx.gates()` |
| `pool::{set_in_use, in_use}` | `ctx.set_in_use`, `ctx.in_use` |
| `pool::signed::set_by_settings(keys)` | `Settings::keys_by_settings` |
| `pool::members::set_by_net(members)` | `Settings::members` |
| `rank_list::set_by_settings(on)`, `rank_auto::set_by_settings(auto)` | `Settings::rank_by_file`, `Settings::rank_auto_by_file` |
| `rank_auto::{racing, calm, unless_a_race, a_race_runs}` | `rank_auto::racing(ctx.races())`, `ctx.races().calm()` and the others |
| `addresses::install()`, `carried(host, peer)`, `failed(host)` | the same with `&ctx` (the resolver of `podssh_ws::resolve::resolver()` in `ctx.resolver`) |
| `cache::{load, store_noted, remove, chosen}` of a token | `ctx.cache.{load, store_noted, remove, chosen}` |
| `cache::{load_file, store_file, candidate_dirs, read_dirs}` of the other files | `Cache::{load_file, store_file, dirs, read_dirs}`, or `pod_core::files` |
| `relay::byte_budget(env)` | `relay::byte_budget(&ctx, env)` |
| `relay::RELAY_ENV`, `open::OFFLINE_ENV`, `token::TOKEN_ENV`, `cleartext::CLEARTEXT_ENV`, `quic_road::QUIC_ENV`, `h2_road::H2_ENV`, `rank_list::ENV`, `rank_auto::PACE_ENV`, `pool::signed::KEYS_ENV` | podssh's own constants; `Settings` holds their values, `Names::podssh()` their names |
| `http_road::USER_AGENT` | the way's user agent (`Road::tls(&way, ..)`) |
| `tls_groups.rs`, `cache/`, `code/mod.rs`, `session/`, `e2e/`, `identity.rs` | stay in podssh as its glue |
| the tests of the files above | `tests/` here |
| the plants of these files | scripts/plants/relay-* here, where their test is here |

The trial of podssh (adoption/podssh/pod-relay.patch) is not written: its
glue replaces each of podssh's call sites of the old API (T-025, Done and
its next steps).

### podnet

podnet's relay client (`src/ws_relay.rs`, `src/relay_pool.rs`,
`src/relay_reverse.rs`) moves onto this crate with T-026: its plain reverse
mode becomes a mode of the API of the reverse road.

### podvm

The blocking facade: `pod_relay::blocking::Client::new(Config::new(ctx))`
with podvm's `Context` (its way out, its cache, its settings).

## Tests

`cargo test -p pod-relay --all-features`: the unit tests of each module
(the hosts and their order, the pool and its signed lists, the gates on a
paused clock, the record of failures, the rank, the tokens and their keys,
the roads' choices, the codecs and the close table of the reverse road, the
grants, the exchange of a code, the classes of the failures), and the
integration tests over loopback stand-ins: the blocking facade with a
stand-in relay, the reverse road (its node, its operator, its framing, its
fair queue, its layer, its spares, its window, its dialect), the
end-to-end channel over the layered road, a node's routes and shares, the
grants, the pairing by code at a stand-in mailbox, the limits of a pool's
body, the failover. The tests that reach the live relay or the public
mailboxes are `#[ignore]` (19).

## Sources

`SOURCES` lists each imported file with its blob.
