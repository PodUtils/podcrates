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
podnet's relay client (`src/ws_relay.rs`, `src/relay_pool.rs`,
`src/relay_reverse.rs` at `7c5f87e`) adds the capabilities that podssh's
did not have (T-026): the budget of a stream, a session as a byte stream,
the tokens of a run, and the plain mode of the reverse road.

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
| (shared) | the gates of each origin, the limits of the relay in use, the races of the rank, the pool of HTTP/2 connections, what the QUIC road learned of each host, the tokens that the run minted or read: one run's, shared by each clone |

The crate reads no variable of the environment, keeps no process state,
and prints nothing.

## API

| Module | What |
| --- | --- |
| (root) | `Context`, `Settings`, `Names`, `open`, `Request`, `Opened`, `OpenError`, `Failure`, `Relay`, `default_relay_host`, `RelayBudget` |
| `budget` | `RelayBudget`: `meter` (bytes and an age), `meter_for` (the budgets of the run's relay), `spent` |
| `stream` | `of`: a forward session as a byte stream (`pod_core::BoxedIo`) |
| `relay` | `Relay` (`name`, `url`: an IPv6 address in brackets), `RelayList`, `parse_relay` (`[ADDR]:PORT`), `select_relays`, `parse_relay_list`, `url_host`, `forward_path`, `node_path`, `operator_path`, the budgets (`byte_budget`, `byte_budget_under`, `time_budget`, `idle_secs`), the limits of the default deployment |
| `pool` | `alternates`, `refresh`, `parse_pool`, `parse_limits`, `Limits`, `same_deployment`; `signed` (the signed lists, `Keys`), `kept` (the lists kept), `members` (the relay members of a net) |
| `token`, `cache` | `obtain`, `kept` (the token of the settings, the run or the cache, with no mint), `MintContext`, `Token`, `TokenError`, `token_key`, `remember`, `forget` (the tokens of a run); `Cache`, `Cached`, `valid_token` |
| `open`, `failover`, `failures`, `gate` | the opener (`open`; `open_on_loopback` with the feature `plain-ws`), the modes of a failover, the record of each host's faults, the gate of each origin |
| `rank`, `rank_list`, `rank_auto` | the rank of the hosts, the order of the default list, the rank in a session's background (`Races`) |
| `h2_road`, `quic_road`, `http_road`, `cleartext`, `forward_layer` | the roads to a relay |
| `health`, `addresses`, `keyed` | `/health` of a host; the addresses that carried a session; a request that the relay asks this client's key to sign |
| `pair` (feature `pair`) | `PairContext`, `create`, `status`, `stop`, `revoke`, `halt`, `rotate`, the stores of a pair and of an operator's part, `newest` (the stored pair with the most time left, or the next one) |
| `reverse` (feature `pair`) | `run` (the node), `NodeConfig`, `NodeState`, `Handler`, `TcpHandler`, `UdpHandler`, `local::{Target, LocalHandler}`, `channel` and `ChannelHandler` (each session to a service of the process, T-150), `Layered`, `E2e`, `routes::Routes`, `operator` (`OperatorLimits::eof_idle`, `EOF_IDLE`), `serve`, `Settings` (`half_close`, `drain`), `HALF_CLOSE_IDLE`, `DRAIN`, `wire::{Wire, Door, Carrier}`, the codecs |
| `grant` (feature `grant`) | `Cert`, `Door`, `Mint`, `Spent`, `parse_public` |
| `code` (feature `code`) | `exchange` (what the ends say), `run` (`give`, `take`, `give_with`, `take_with`, `Ends`, `Waits`; where no mailbox answers, the MQTT brokers of `Ends::brokers`, after the event `relay.code.broker`, T-277), `LIFETIME` |
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

### The plain mode of the reverse road

podnet's reverse mode is the same wire as podssh's (`/v1/node/NAME`, ids
of 32 lowercase hex characters, the JSON control frames), with no
resumable layer and no end-to-end channel. It is a mode of the same API,
set by the caller, with podssh's behaviour by default:

| Part | podssh (the default) | podnet's plain mode |
| --- | --- | --- |
| a node's handler | `TcpHandler`, `Layered`, `E2e` | `LocalHandler` of a `local::Target` (`host:port`, `tcp://host:port`, `[IPv6]:port`, `unix:/path`); `ChannelHandler` of `reverse::channel(depth)`: a pipe for each session, its other end with the session's id to a service of the process (PodUtils/podnet@T-083) |
| the end of a local side's writes | `Settings::half_close: None`: the session ends at once | `Some(HALF_CLOSE_IDLE)`: the operator's last bytes still go; the session ends after 2 s with none |
| the end of an operator's input | `OperatorLimits::eof_idle: None`: a Close at once (or as `node_ends` says) | `Some(EOF_IDLE)`: no Close; the node's bytes still come; the Close after 15 s with none |
| the next pair at the expiry | the caller's `RepairHook` | a `RepairHook` over `pair::newest(cache, Some(&ending))` |

For each caller: the relay's close of a session gives the bytes that wait
for its local side `Settings::drain` (`DRAIN`, 10 s), then the node drops
them and the local side; a second `open` of a live id is no new session;
a `401` to a node's upgrade ends the node as a `403` does.

## Features

| Feature | What |
| --- | --- |
| `pair` | the pairs and the reverse road |
| `blocking` | the blocking facade (with `pair`) |
| `code` | the exchange of a pairing by code (with `pair`; `pod-wormhole`) |
| `grant` | the grants used once |
| `quic` | the QUIC road (`pod-quic`, noq) |
| `plain-ws` | plain `ws://` to a stand-in relay on the loopback, for the tests of an embedder: `http://` and `ws://` in `parse_relay`, the forward opener's `open_on_loopback`, the reverse road's `Wire::PlainLoopback`, the facade's `forward_on_loopback`; a binary does not turn it on |
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
| `PODSSH_RELAY`, `PODSSH_SESSION_BUDGET`, `PODSSH_RELAY_FAILOVER` | the caller's own: `select_relays(flag, env, pool)`, `byte_budget(ctx, env)` and `failover::Mode::from_setting(value, &names.failover)` take their text |
| `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY` | the caller's dialer in `Context::way`, and `Context::proxy_of` |
| `PODSSH_RELAY_ADDR` | the pins of the caller's resolver |

## Parity

### podssh

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

### podnet

Against podnet `src/ws_relay.rs`, `src/relay_pool.rs`, `src/relay_reverse.rs`
(with `relay_reverse/pairfile.rs` and `relay_reverse/session.rs`) and
`tests/relay_reverse.rs` at `7c5f87e`, which podnet holds to fixes
(PodUtils/podnet#449); its `main` has no change of them after it. A test
in italics has podnet's name.

| podnet | Here | Test |
| --- | --- | --- |
| `RELAY_VARIABLE`, `TOKEN_VARIABLE`, `CONNECT_TOKEN_VARIABLE`, `Relay::from_env` | podnet's own: `Settings::token`, `select_relays(flag, env, pool)` | `the_flag_beats_the_environment_and_lists_are_kept_as_given` |
| `RELAY`, `set`, `active` (a process global) | none: the caller holds its `RelayList` and its `Context` | |
| `SESSION_BYTES`, `SESSION_AGE`, `MINT_MARGIN`, `MAX_MESSAGE` | `relay::SESSION_BYTE_BUDGET`, `SESSION_TIME_BUDGET`, `cache::MIN_REMAINING_MS`, `pod_ws::Limits::RELAY` | `each_budget_is_under_its_cap` |
| `RelayBudget::{meter, meter_with, spent}` | [x] `RelayBudget::{meter_for, meter, spent}` | *`a_relay_budget_is_spent_at_its_bytes_or_its_age`*, `the_budget_of_a_run_goes_under_the_limits_of_its_relay` |
| `Host::parse`, `Relay::new` of a list, `Relay::single`, `urls`, `host`, `port`, `url` | `relay::parse_relay`, `parse_relay_list`, `RelayList` | `relay_spellings_from_the_relays_documents_are_accepted` |
| `Host::parse` of `https://[IPv6]:port` (podnet issue 273), `Host::url`, `authority_host` | [x] `relay::parse_relay`: `[ADDR]` and `[ADDR]:PORT`, the address kept in its canonical form; `Relay::name`, `Relay::url` and the cache write it in brackets; a bare address stays refused (call C80) | *`relay_urls`*, *`an_ipv6_relay_host_keeps_its_port`*, `an_ipv6_relay_has_one_name_in_brackets` |
| `Host::parse` of `http://` or `ws://` to a loopback relay, and `Relay::connect` over it (podnet's tests) | [x] with the feature `plain-ws`: `parse_relay` takes it for a host on the loopback, and `open::open_on_loopback` opens the session, with the failover and the token of the settings, and no mint (calls C81, C82) | *`bytes_go_through_a_local_relay`*, `a_relay_off_the_loopback_is_refused_before_any_connection`, `the_plain_wire_takes_a_given_token_and_mints_none` |
| a list of hosts of one deployment only, as one token goes to each | a token for each deployment (`token::token_key`): a list may name more than one (call C76) | `token_key_is_one_for_each_deployment` |
| the alternates of one host (`relay_pool::alternates_of`): the cached pool, else the seed | `pool::alternates`; the default host with them is `select_relays(None, ..)`; a named host of another deployment takes none (call C76) | *`the_alternates_come_from_the_cache_else_the_seed`*, `the_default_relay_comes_first_then_pool_alternates` |
| `Relay::token`: the fixed token, the minted one in memory, the cache file, a new mint; a refused one minted again once | [x] `token::obtain` with the tokens of the run (`remember`, `forget`), and `open`'s mint after a `403` | `a_token_of_the_run_is_taken_again_with_no_file_of_the_cache`, `a_refused_token_of_the_run_is_forgotten`, `a_token_of_the_run_with_little_time_left_is_not_taken` |
| `Relay::mint`, `mint_at`: each host in turn; `429` and `503` stop | `token::mint` at each host's attempt, one deployment at a time (`MintGate`); a `429` moves to the next host (call C76) | `one_mint_at_a_time_for_each_deployment` |
| `Relay::refresh_pool` after a mint | `open`'s refresh of the pool after a mint (`pool::refresh`) | `a_relays_json_pool_is_read_in_order_and_filtered` |
| `Relay::connect`: the race of the hosts (`HOST_STAGGER`), the stop of the hosts after a refusal of the deployment | `open::open` with `failover::Mode` (`STAGGER`) and `OpenError::another_host_may_help` | `tests/failover.rs`, `errors_every_host_would_repeat_stop_the_failover` |
| `Relay::connect` of an IPv6 target: refused (the relay has no IPv6 egress) | podnet's own; the crate gives the relay's words (`relay::ipv6_note`) | `ipv6_note_only_for_an_ipv6_target_that_the_relay_closed_at_once` |
| `Relay::connect` as a byte stream (`ws::bridge` over a pipe, `PIPE_BUFFER`) | [x] `stream::of(opened.session)`, `stream::PIPE` | `a_session_carries_a_byte_stream_both_ways` |
| `refusal`, `Refusal`, `HostError`, `MintError` | `OpenError::{another_host_may_help, lines}`, `is_policy_refusal`, `TokenError` | `errors_every_host_would_repeat_stop_the_failover` |
| `websocket`, `post_json`, `websocket_at` | `pod_ws::client::{connect, https_with_token}`, `keyed::post`, `reverse::wire` | |
| `same_deployment`, `valid_host` | [x] `pool::same_deployment`: an IP address of the primary, and a host with an empty label, are no host of its deployment now | *`hosts_of_one_deployment`* |
| `parse_pool`, `seed`, `default_host`, `MAX_ALTERNATES`, `STALE_AFTER`, `MAX_BODY` | `pool::parse_pool`, `seed`, `relay::default_relay_host`, `MAX_ALTERNATES`, `STALE_AFTER_MS`, `MAX_BODY` | `a_relays_json_pool_is_read_in_order_and_filtered` |
| `read_private_checked`, `read_private`, `write_private`, `safe_name`, `MAX_FILE`, the files of the pool and of a token in the state directory | `pod_core::files` and the caller's `Cache` (`Cache::none()` keeps no file); the forms of the files are pod-relay's | `tests/cache.rs` |
| `Pair`, `check_name`, `pair`, `stop`, `pairs_dir` | `pair::Pair`, `relay::check_node_name`, `pair::create` (a `409` asked again once), `pair::stop`, the caller's `Cache` | `tests/pair.rs`, `node_names_cannot_bend_the_path` |
| `Target`, its `parse`, `connect` and `Display` | [x] `reverse::local::{Target, LocalHandler, connect}`; a refusal names the target | *`targets_and_names`*, *`a_session_to_a_closed_port_is_refused_with_the_reason`* |
| `node`: a lost socket again with a backoff; `node_close`; `401` and `403` stop | `reverse::run`, `after_close`; [x] a `401` ends the node as a `403` | `a_socket_that_served_is_taken_again_at_once_once_each_10_s`, `a_node_whose_token_is_refused_with_401_stops` |
| `node_once`: the gate of each session (`Gate`), an `open` of an open id ignored | `serve` and `sessions::Sessions`; [x] a second `open` of a live id is ignored | `a_second_open_of_a_live_session_is_no_new_session` |
| `session`: a local side that does not read ends its own session | `serve`'s queue of each session (`Handler::queue_bytes`) | `a_local_side_that_does_not_read_ends_its_session_and_not_the_others` |
| `session` after the relay's `close`: the task stops, also in a write (podnet T-194) | [x] `Settings::drain` | *`a_relay_close_ends_a_session_whose_target_does_not_read`* |
| `session`'s half close (`HALF_CLOSE_IDLE`, podnet T-196) | [x] `Settings::half_close`, `HALF_CLOSE_IDLE` | *`a_half_closing_target_gets_the_whole_input`* |
| `connect` (the operator), `EOF_IDLE` (podnet issue 109); a close other than 1000 after the input is a failure | [x] `operator::run` with `OperatorLimits::eof_idle`, `EOF_IDLE`; `Outcome::is_success` | *`the_answer_comes_back_after_the_input_ends`*, *`a_close_with_an_error_code_after_the_input_ends_is_an_error`* |
| `load` with no name (the pair with the most time left), `next_pair`, `run_node`'s next stored pair | [x] `pair::newest`, the caller's `RepairHook` over it | `the_newest_pair_and_the_next_one` |
| `save`, `load`, `read` of `NAME.json` in a private directory, with the reason of a refused file | `pair::store`, `load` (`pair-LABEL.json`), `read_file` (the reason) | `tests/pair.rs` |
| `WARN_BEFORE`, `left_words`, `expiry_line`, `warn_expiry`, `node_ready`, `instances_of_pair` | stay in podnet: its lines and its instances, as podssh's `node_expiry.rs` stays in podssh-cli (call C74) | |
| `close_hint` | `pod_ws::close_hint` | |
| `tests/relay_reverse/fake.rs`, a fake relay of the reverse road (tokio-tungstenite) | the relays that the tests here script over a pipe, and podssh's `tests/stand_in/` over the loopback; podnet's own tests keep its fake until the trial | `tests/reverse_plain.rs` |
| *`two_sessions_at_the_same_time_through_one_node`*, *`a_dropped_node_socket_is_opened_again`*, *`a_local_side_that_does_not_read_stops_no_other_session`*, *`a_session_ends_after_the_idle_time_when_the_input_ended`* | the tests of `serve`, of `run` and of the operator here | `two_sessions_carry_their_own_bytes_both_ways`, `a_local_side_that_does_not_read_ends_its_session_and_not_the_others`, `the_answer_comes_back_after_the_input_ends` |
| *`a_wrong_connect_token_is_a_config_error`*, *`a_named_node_ends_at_the_expiry`*, *`a_node_takes_the_next_pair_at_the_expiry`*, `relay_stop_checks_the_pair_before_it_stops_an_instance`, `serve_via_relay_answers_through_the_relay`, `git_serve_via_relay_gives_the_ssh_server` | stay in podnet: its exit codes, its CLI and its glue (the trial) | |

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
| `relay::RELAY_ENV`, `open::OFFLINE_ENV`, `token::TOKEN_ENV`, `cleartext::CLEARTEXT_ENV`, `quic_road::QUIC_ENV`, `h2_road::H2_ENV`, `rank_list::ENV`, `rank_auto::PACE_ENV`, `pool::signed::KEYS_ENV`, `failover::MODE_ENV` | podssh's own constants; `Settings` holds their values, `Names::podssh()` their names |
| `http_road::USER_AGENT` | the way's user agent (`Road::tls(&way, ..)`) |
| `tls_groups.rs`, `cache/`, `code/mod.rs`, `session/`, `e2e/`, `identity.rs` | stay in podssh as its glue |
| the tests of the files above | `tests/` here |
| the plants of these files | scripts/plants/relay-* here, where their test is here |

podssh took the crate at `0872c2c6` (T-025, PodUtils/podssh#206), from the
patch of its trial on podssh `75a5031`. podssh-relay keeps each module name of the cut as glue over this
crate: one `Context` of the process (`podssh_relay::context::{process, of}`),
each variable read at each call, and a wrapper of the old signature for each
free function. A struct literal of the CLI takes `ctx` in place of `proxy`;
a node takes `podssh_relay::reverse::settings()`, whose `NodeState` is the
process's, so that its upgrade, its rotations and its lines reach podssh: a
node with `Settings::default()` says no line.

### podnet

podnet's relay client moves onto this crate with T-026; its trial
(adoption/podnet/pod-relay.*) remains (T-026, partial).

| Old | New |
| --- | --- |
| `ws_relay::{set, active}` (a process global) | a `RelayList` and a `Context` that podnet holds and gives each call |
| `ws_relay::Relay::new(url, token)`, `from_env`, `single` | `relay::select_relays(Some(url), env, &[])` (the default host alone: `select_relays(None, None, &pool::alternates(&ctx.cache, host))`), `Settings::token` from `PODNET_RELAY_TOKEN`; `https://[ADDR]:PORT` too, and `http://127.0.0.1:PORT` of a test with the feature `plain-ws` |
| `Relay::connect(target, timeout)` | `open(&Request { ctx, relays, path: &relay::forward_path(host, port)?, .. }, notes)`, then `stream::of(opened.session)`; for an `http://` relay of a test, `open::open_on_loopback` with the same `Request`; an IPv6 target stays refused in podnet |
| `ws_relay::RelayBudget::{meter, meter_with}`, `SESSION_BYTES`, `SESSION_AGE` | `RelayBudget::{meter_for, meter}`, `relay::SESSION_BYTE_BUDGET`, `SESSION_TIME_BUDGET` |
| `relay_pool::{load_token, store_token, load_pool, store_pool}` and the state directory (`PODNET_STATE_DIR`, `none`) | the caller's `Cache` (`Cache::in_dirs`, `Cache::none()`): the token and the pool files of this crate; podnet's old files are not read (one more mint and one read of the pool) |
| `relay_pool::{same_deployment, parse_pool, alternates_of, seed, default_host}` | `pool::{same_deployment, parse_pool, alternates, seed}`, `relay::default_relay_host` |
| `relay_pool::{read_private_checked, read_private, write_private}` | `pod_core::files` |
| `relay_reverse::Pair` | `pair::Pair` (its tokens are read by `node_token()`, `connect_token()`, `stop_token()`; `expires_ms`; `relay` is a `Relay`) |
| `relay_reverse::{pair, stop}` | `pair::{create, stop}` with a `PairContext` |
| `relay_reverse::{save, load}`, `pairs_dir` | `pair::{store, load, newest}` over a `Cache` of podnet's `relay` directory; `pair::read_file` gives the reason of a refused file |
| `relay_reverse::Target`, `node(relay, pair, target)` | `reverse::local::Target`, `reverse::run(&mut NodeConfig { .. settings: Settings { half_close: Some(HALF_CLOSE_IDLE), ..Settings::default() } }, Arc::new(LocalHandler { .. }), stop)` |
| `relay_reverse::run_node(follow, pair, target)` | `reverse::run` with a `RepairHook` over `pair::newest(cache, Some(&ending))`, and a `stop` at the pair's end; the expiry lines stay in podnet |
| `relay_reverse::connect(relay, name, token, input, output)`, `EOF_IDLE` | `reverse::operator::run(&OperatorConfig { limits: OperatorLimits { eof_idle: Some(EOF_IDLE), .. }, .. }, io)`; `Outcome::is_success` |
| `relay_reverse::{node_ready, instances_of_pair, WARN_BEFORE, expiry_line}`, `CONNECT_TOKEN_VARIABLE` | stay in podnet |

### podvm

The blocking facade: `pod_relay::blocking::Client::new(Config::new(ctx))`
with podvm's `Context` (its way out, its cache, its settings).

## Tests

`cargo test -p pod-relay --all-features`: the unit tests of each module
(the hosts and their order, the pool and its signed lists, the gates on a
paused clock, the record of failures, the rank, the tokens and their keys,
the roads' choices, the codecs and the close table of the reverse road, the
grants, the exchange of a code, the classes of the failures, the budget of
a stream, a session as a byte stream, the tokens of a run, podnet's
targets, the newest pair of the store), and the
integration tests over loopback stand-ins: the blocking facade with a
stand-in relay, the reverse road (its node, its operator, its framing, its
fair queue, its layer, its spares, its window, its dialect, podnet's plain
mode), the
end-to-end channel over the layered road, a node's routes and shares, the
grants, the pairing by code at a stand-in mailbox, the limits of a pool's
body, the failover, the forward opener over plain `ws://` to a stand-in
relay (podnet's `bytes_go_through_a_local_relay`). The tests that reach the live relay or the public
mailboxes are `#[ignore]` (19). `live.yml` runs `pair_code_live` each
Thursday, and one of the others by hand (its input `relay`; `until` for
`reverse_drops_live`). It runs `pair_code` each day against
magic-wormhole's own mailbox server (`POD_WORMHOLE_TEST_SERVER=required`).

## Sources

`SOURCES` lists each imported file with its blob.
