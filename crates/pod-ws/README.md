# pod-ws

WebSockets (RFC 6455) for the crates of podcrates and their binaries: the
frames, a session that two tasks use at once, the client of the relay over
TLS, over HTTP/2's extended CONNECT (RFC 8441) and in cleartext, a
WebSocket that a URL names, the server's side of the handshake, and the
bridge of a session and a raw stream.

The base is podssh's `crates/podssh-ws` (its WebSocket parts); podnet's
`src/ws.rs` adds the bridge, its client dial, the close codes and
`close_hint`. The crate replaces tokio-tungstenite in podnet.

## API

| Module | What |
| --- | --- |
| `way` | `Way`: the TLS context of the binary (`Arc<pod_tls::Context>`), its dialer (`Arc<dyn pod_core::Dial>`), the reader of HTTPS records for ECH (feature `ech`, `Way::with_records`) and its user agent. `Way::{tcp, open_tls, tls_over}`; `dial_error`, `connect_error` (a `Failure` as a `DialError` or a `ConnectError`) |
| `frame` | `Frame`, `Role`, `encode`, `decode`, `decode_capped`, `apply_mask`, the opcodes, `FORWARD_MAX_FRAME`, `NODE_MAX_FRAME`, `MAX_RECEIVE_CAP`, `receive_cap`, `FORWARD_SEND_PAYLOAD`, `MAX_CONTROL_PAYLOAD` |
| `handshake` | `generate_key`, `generate_key_from(&mut dyn Entropy)`, `build_request`, `accept_key`, `sha1`, `check_response`, `read_response`, `handshake`, `masking_key`, `masking_key_from`, `TOKEN_HEADER`, `PROTO_HEADER`, `REVERSE_PROTO`, `reverse_socket` |
| `session` | `RelaySession<S>`: `new`, `over_tls`, `over_cleartext`, `over_h2`, `with_limits`, `as_server`, `naming`, `with_zero_mask`, `with_max_message`, `holding_close_answer`, `without_pings`, `with_road`, `set_receive_cap`, `read_frame`, `send_binary`, `send_text`, `send_ping`, `send_pong`, `send_close`, `watch_liveness`, `take_pongs`, `pong_arrived`, `peer`, `road`, `last_heard`; `RelayStream`, `BoxedIo`, `PONGS_KEPT` |
| `limits` | `Limits { message, frame }`: `RELAY` (the default: 16 MiB and 262144 bytes), `TUNGSTENITE` (64 MiB and 16 MiB); `MAX_MESSAGE` |
| `close` | `close_payload`, `close_code_and_reason`, `code::{NORMAL, GOING_AWAY, PROTOCOL, TOO_BIG, ERROR, TRY_LATER}`, `close_hint`, `ForwardClose`, `forward_close`, `ForwardClose::what(target, app)` |
| `client` | `WsClientConfig { endpoint, trust, server_name, timeout, idle_timeout, way, limits }`, `Endpoint`, `check_path`, `relay_knobs_only`, `connect`, `open_tls`, `upgrade`, `upgrade_with`, `https_post_json`, `https_with_token`, `read_frame_over`, `next_event`, `next_event_capped`, `Event`, `doctor`, `is_policy_refusal`, `request_error`, `ConnectError` (and `cut`, `stage_and_class`), the bounds `DEFAULT_TIMEOUT`, `DEFAULT_IDLE_TIMEOUT`, `LIVENESS_EVERY`, `LIVENESS_ALLOWED`, `WRITE_TIMEOUT`, `UPGRADE_HEAD` |
| `h2_road` | `Pool` (`const fn new`), `connect(&pool, &config, token)`, `probe(&way, ..)`, `Road`, `Fallback`, `Offered`, `Stream`, `duplex`, the windows and bounds |
| `cleartext` | `connect`, `tls_failed`, `probe(&way, host, port, wait)`, `Heard`, `PROBE_PATH`, `PLAIN_TO_TLS` |
| `url` | `WsUrl` (`parse`, `shown`, `host_header`), `request_target`, `request`, `offering`, `asking`, `Asking { protocol, authorization, alpn, limits }`, `Stream`, `Carried`, `dial`, `dial_offering`, `dial_asking`, `failure` |
| `server` | `check_upgrade`, `check_upgrade_or_raw`, `Upgrade`, `Refused`, `asks_upgrade`, `switching`, `switching_raw`, `session` |
| `bridge` | `bridge(session, io, &dyn Tally)`, `dial(url, &way, &trust, timeout, authorization)`, `connect_bridge`, `Tally`, `NoTally`, `DIAL_TIMEOUT` |
| `wake` | `Wake`, `Reading`, `slept`, `TICK`, `LATE` |
| `plain` | feature `plain-ws`: `connect_loopback` |
| `error` | `WsError`, `SessionError`, `Verdict`; each error is also a `pod_core::Failure` (`From`) at its stage with its class |

## Features

* `ech`: Encrypted Client Hello on each TLS road, with the reader that
  `Way::with_records` gives (`pod-dns` has one, its feature `ech`). It
  turns on the feature `ech` of `pod-tls`.
* `plain-ws`: a session over plain TCP to the loopback, for the tests of
  an embedder. No binary turns it on.

## Settings

The crate reads no variable of the environment and keeps no process
state.

| Value | Where |
| --- | --- |
| The proxy, `NO_PROXY`, the resolver and its pins | the dialer of the `Way` |
| The roots, `SSL_CERT_FILE`, the bundle next to the binary, the mode of ECH | the `pod_tls::Context` of the `Way` |
| The user agent (`podssh/VERSION`, `podnet/VERSION`) | `Way::user_agent` |
| The frame and the message that a session takes in | `Limits` (`WsClientConfig::limits`, `Asking::limits`, `RelaySession::with_limits`) |
| The HTTP/2 connections of the relay, and what was found of each host | `h2_road::Pool`, a value of the caller |
| The name of the client in the words of a Close | `ForwardClose::what(target, app)` |

## Parity

Against podssh `crates/podssh-ws` at `8cd0f2b`:

| Capability of podssh | Here |
| --- | --- |
| [x] `frame`, `wire`, `handshake`, `session`, `close`, `error`, `server`, `url`, `plain`, `h2_road`, `cleartext`, `wake` | the same modules |
| [x] `ProxyChoice`, `tls::context()`, `resolve::resolver()` of each road | the `Way` of the caller |
| [x] `crypto::random::OsRandom` of the keys | `pod_core::entropy::OsEntropy`; a source that fails is an error |
| [x] `h2_road`'s process static `KEPT` | `h2_road::Pool`, keyed by the label of the way in place of the `ProxyChoice` |
| [x] `url::USER_AGENT` (`podssh/VERSION`), the user agent of the probe and of the CONNECT | `Way::user_agent` |
| [x] `session::MAX_MESSAGE`, the forward cap | `Limits::RELAY`, the default |
| [x] `via::http2::Stream` under `h2_road::Stream` | `h2_road::duplex` (the stream alone) |
| [x] `client::{https_post_json, https_with_token}` | the same, with the `Way` first |
| [x] `client::{getter, https_get, https_get_accepting}` | podssh keeps them over `pod_proxy::Client`; their failure is `client::request_error` |
| [x] `doctor` | the trust store and the handshake; the line of the crypto provider is podssh's |
| [x] `ForwardClose::{what, remedy}` | `what(target, app)`; `remedy` (podssh's flags) stays in podssh |
| [x] The test of the raw form in `server.rs` (podssh's WebTunnel client) | the bytes of its request in the test, and its check of the answer |
| [x] The tests | each test of the imported files with its name; the asserts of `remedy` go with it to podssh |
| [x] The fuzz targets `ws_frame`, `ws_upgrade`, `close_reason` | in `fuzz/` of podcrates |

Against podnet `src/ws.rs` at `76ffb5e`:

| Capability of podnet | Here |
| --- | --- |
| [x] `bridge(ws, io, &metrics)` over tokio-tungstenite | `bridge(session, io, &dyn Tally)`: each message to one write, each read to one binary message, Pings answered, a Close of the far end in `tracing` with `close_hint`, the end of the WebSocket way ends the bridge |
| [x] `dial(url, timeout, authorization)`: the egress of podnet, TLS with ALPN `http/1.1`, `Authorization`, the time of each step at `-v` | `bridge::dial(url, &way, &trust, timeout, authorization)`: the dialer and the context of the `Way`; `connect_ms`, `upgrade_ms` in `tracing` |
| [x] A 401 with podnet's hint | `ConnectError::Refused { status: 401, .. }`; the hint is podnet's |
| [x] The limits of tokio-tungstenite | `Limits::TUNGSTENITE` for the bridge's dial |
| [x] `connect_bridge`, `DIAL_TIMEOUT` | the same, with the `Way`, the trust and a `Tally` |
| [x] `accept_key` (the `sha1` crate) | podssh's `handshake::accept_key` |
| [x] `relay_reverse::close_hint` | `close::close_hint`, with the codes in `close::code` |
| [x] The tests `bridge_moves_bytes_both_ways`, `accept_key_matches_rfc_example`, the measure `throughput` | `tests/bridge.rs` and `bridge.rs`, with their names |
| [ ] `serve_websocket`, `forward_path`, `forward_url`, `DEFAULT_PATH` | `pod-cloudflare` (T-029): they serve the edge |
| [ ] `LocalSpec` | stays in podnet (its command line) |

## Adoption map

| Consumer | Old path | New path |
| --- | --- | --- |
| podssh | `podssh_ws::{frame, handshake, session, close, server, wake}` | `pod_ws::*` (podssh-ws re-exports them) |
| podssh | `podssh_ws::client::WsClientConfig { .., proxy }` | podssh keeps its config, and builds `pod_ws::WsClientConfig { .., way, limits: Limits::RELAY }` with `way` from its `ProxyChoice` (`dial::dialer(proxy)`, `tls::context()`, `ech::Records`, `dial::user_agent()`) |
| podssh | `podssh_ws::{connect, client::{open_tls, open_tls_offering, https_post_json, https_with_token}}` | the same functions of `pod_ws::client`, with the `Way` |
| podssh | `podssh_ws::h2_road::{connect, probe}` | `pod_ws::h2_road::connect(&POOL, ..)`, one `static POOL: Pool = Pool::new()` in podssh; `probe(&way, ..)` |
| podssh | `podssh_ws::cleartext::{connect, probe(host, port, &proxy, wait)}` | `pod_ws::cleartext::{connect, probe(&way, host, port, wait)}` |
| podssh | `podssh_ws::url::{dial, dial_offering}(url, .., &trust, &proxy, ..)`, `url::{request, offering}(url, key, ..)`, `url::USER_AGENT` | `pod_ws::url::{dial, dial_offering}(url, .., &way, &trust, ..)`; `request(url, key, user_agent)`, `offering(url, key, user_agent, protocol)`; podssh keeps `USER_AGENT` |
| podssh | `url::Stream::Plain(TcpStream)` (`wstunnel`, `pt`, `vless/ws`, `upterm`, `chisel`, `podssh-cli` `doctor/stream/http.rs`) | `Stream::Plain(Box::new(tcp))` |
| podssh | `RelaySession::{over_tls, over_cleartext, over_h2}` | the same, with `WRITE_TIMEOUT` as the last argument; `over_cleartext` takes a `pod_core::Connected` |
| podssh | `handshake::{generate_key_from, masking_key_from}(&dyn SecureRandom)` | `(&mut dyn pod_core::entropy::Entropy)` |
| podssh | `ForwardClose::what(target)`, `ForwardClose::remedy(reason)` (`pipe/relay.rs`, `http_road.rs`, `podssh-ssh` `run.rs`) | `what(target, "podssh")`; `remedy` is podssh's trait `close::Remedy` (re-exported by `podssh_ws::session`), with its asserts |
| podssh | `client::{getter, https_get, https_get_accepting}` | podssh keeps them; `get_error` is `pod_ws::client::request_error(failure, limit, host, port)` |
| podssh | `doctor(config)` | podssh's line of the crypto provider, then `pod_ws::doctor(&config.pod())` |
| podssh | the tests `rfc6455`, `control_frames`, `frame_lengths`, `handshake_tail`, `limits`, `session`, `plain_loopback`, `h2_road`, `cleartext_road`, `properties`, `random_source` and the unit tests of the moved files | leave podssh-ws; podcrates runs them with their names. The record cites the old files as `PodUtils/podssh:PATH`, and each entry whose Prove names a moved target gets a Correction |
| podssh | `impl From<pod_tls::Error> for WsError` of `tls.rs` | goes: `pod-ws` has it; `tls::connect_error` and `tls_over` stay podssh's |
| podssh | the fuzz targets `ws_frame`, `ws_upgrade`, `close_reason` | the targets of the same names in `fuzz/` of podcrates; podssh's own still build over the re-exports, and podssh removes them |
| podnet | `ws::dial(url, timeout, authorization)` (`cli/cmd/connect.rs`), a `WebSocketStream` | `ws::dial` keeps podnet's words and hops over `pod_ws::bridge::dial(url, &ws::way(), &Trust::Default, timeout, authorization)`, a `RelaySession`; `connect` closes it with `send_close(1000, "")` |
| podnet | `ws::connect_bridge(url, io, authorization)` | `pod_ws::bridge::connect_bridge(url, &way, &Trust::Default, io, authorization, &metrics)` |
| podnet | `ws::bridge(ws, io, &metrics)` (`ws.rs` `serve_websocket`) | `pod_ws::bridge(pod_ws::server::session(socket, Vec::new(), None).with_limits(Limits::TUNGSTENITE), io, &Counted(metrics))`; `Counted` (podnet's) implements `pod_ws::Tally` over `Metrics` |
| podnet | `ws::bridge` of `ws_relay.rs` (a tungstenite socket of the relay) | `ws::bridge` stays, over tokio-tungstenite, for the relay client alone until `pod-relay` |
| podnet | `ws::accept_key`, `ws::DIAL_TIMEOUT`, the dependency `sha1` | `pod_ws::handshake::accept_key`, `pod_ws::bridge::DIAL_TIMEOUT`; `sha1` leaves |
| podnet | `relay_reverse::close_hint` | `pod_ws::close_hint` (`relay_reverse` re-exports it); the row `ws -> relay_reverse` of `scripts/layers-allow.txt` goes |
| podnet | `trust::context().client_config_with(&[], &[b"http/1.1"])` of `ws::dial` | the `pod_tls::Context` of the `Way`; the bridge's dial offers `http/1.1` |
| podnet | `crate::egress::dial` of `ws::dial` | the dialer of the `Way` (podnet's `Egress`) |

The adoption trials are `adoption/podssh/pod-ws.patch` and
`adoption/podnet/pod-ws.patch` with their `.run` files.

## Tests

`cargo test -p pod-ws --all-features`. The tests run on the loopback
(`need_loopback!`) with TLS on ring and certificates minted in pure Rust
(`pod-tls`'s `tests/common/cert_for_test.rs`). No test reads the network
or sets a variable.

## Sources

`SOURCES` lists each imported file with its blob: podssh
`crates/podssh-ws/src` and its tests at `8cd0f2b`, the fuzz targets, and
podnet `src/ws.rs` and `src/relay_reverse.rs` (`close_hint`) at `76ffb5e`.
