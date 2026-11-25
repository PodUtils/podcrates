# pod-cloudflare

The Cloudflare tunnel, spoken directly: the edge client over QUIC and over
HTTP/2 (TLS on TCP 7844), the Cap'n Proto RPC of `registerConnection`, the
legs of the edge and their race, the reconnect of each leg, the ingress
rules of cloudflared, the dial of the origins, the quick tunnels of
`trycloudflare.com`, and the check of the public URL. There is no
`cloudflared` process.

The base is podnet's tunnel (`PodUtils/podnet:src/cloudflare`,
`src/tunnel.rs`, `src/edge_h2.rs`, `src/quicktunnel.rs`, `src/config.rs`,
`src/proxy.rs` and the edge side of `src/ws.rs`; `SOURCES` names each
file). podssh's quick tunnels (`podssh-relay` `quick/`) move onto it with
T-030. This is T-029: the import, the edition, the adaptation, the tests,
the fuzz targets, the plants, this README and the static probe. The
adoption trial in podnet is open.

The crate reads no environment variable and no file that the caller does
not name, prints nothing and keeps no process state. Each failure is a
`pod_core::Failure` with its stage (`EdgeRegister`, `EdgeStream`, `Quic`,
`Tls`, `Dns`, `Origin`, `Relay`, `Warp`, `Host`) and its class. Its text
says its own step; `Failure::chain` (or a join of each text of the chain,
anyhow's `{:#}`) says the causes, each once. A line for
the user is a `pod_core::Event` of the caller's `Report`; a decision goes to
its `Observer`. The TLS provider, the way out, the resolver, the relay and
WARP are the caller's: the `Host`.

## API

| Item | Contents |
| --- | --- |
| `Host` | What the consumer gives: `tls: Arc<pod_tls::Context>` (the provider, the roots; the edge roots of Cloudflare are added as extra roots), `dial: Arc<dyn Dial>` (the way out: direct or through the proxy), `resolver: Arc<pod_dns::Resolver>` (the SRV record of the edge by DNS over HTTPS, the step that answers a name), `system_srv: Option<Arc<dyn SystemSrv>>` (the SRV record of the system resolver), `origin_no_verify: Option<Arc<rustls::ClientConfig>>` (the TLS of an origin with `noTLSVerify`), `services: Arc<dyn ServiceDispatch>`, `report: Report` (events and decisions), `relay: Option<EdgeRelay>`, `warp: Option<Arc<dyn WarpSessions>>`, `settings: Settings`. `Host::new(tls, dial)`: the resolver of `pod-dns` over the same way, no services, no relay, no WARP. `event`, `client`, `send` (a request of `pod_proxy::Client`; the failure names the request), `answering_step`, `proxy_for`, `connect`. |
| `Settings` | See [Settings](#settings). |
| `Words` | The words of the consumer in the texts for the user: `name`, `protocol`, `udp_off`, `strict`, `resolve`, `tcp_origin`. The default names no consumer. |
| `ServiceDispatch` | The services of the consumer (`Service::Static`, `Service::Custom`): `prepare(index, service)` before the start (a bad service stops the start), `serve(index, request, host, reader, writer)` for each request of the rule. `NoServices` refuses each. |
| `WarpSessions`, `WarpSession` | The WARP sessions of the leg `quic+warp`: `open()` gives a QUIC endpoint inside a session, its `closed()` check and what keeps it open. A closed session gives a new one at the next reconnect. |
| `SystemSrv` | `srv(name)`: the targets and the ports of an SRV record from the system resolver. |
| `EdgeRelay` | `dial` (TCP through the relay) and `context` (`pod_relay::Context`: the budget of a connection, `relay::byte_budget` and `time_budget`). Only the leg `http2+relay` takes it. |
| `tunnel` | `Tunnel::quick(host)`, `Tunnel::token(host, token)`, `Tunnel::builder(host)`; `TunnelBuilder` (`credentials_file`, `credentials`, `config`, `service`, `ingress`, `ha_connections` 1 to 8, `protocol`, `transport`, `reconnect`, `user_agent`, `service_url`, `verify`, `build`); `Tunnel::start() -> TunnelHandle`. `TunnelHandle`: `url`, `tunnel_id`, `account_tag`, `locations`, `leg` (`quic`, `http2`, `quic+warp`, `http2+relay`), `metrics`, `forward`, `forward_path`, `verify`, `verify_url`, `verify_accepting` (`Verified { unverified, dns }`), `wants_verify`, `stopped` (the cause when each leg stopped), `shutdown`. `LegKind`, `leg_plan`, `plan_with_proxy`, `plan_with_udp`, `NO_INGRESS`. |
| `config` | `Config { service_url, user_agent, credentials, protocol, transport, ha_connections, reconnect, ingress, verify, failover, secrets }`; `Credentials` (`Quick`, `Token`, `File`, `Explicit`; `Debug` hides the secrets), `parse_token`, `parse_credentials_file`, `Protocol`, `Transport` (`Auto`, `Direct`, `Warp`, `Relay`), `ReconnectPolicy` (`next(attempt, &Failure) -> Next`; a refusal with no retry stops the leg), `MAX_RETRY_AFTER`; `load_file`, `load_file_with(path, &Report)` (cloudflared's `config.yml`: its keys, the types of its values, a near key for a typo, the ingress rules), `CLOUDFLARED_KEYS`, `ORIGIN_REQUEST_KEYS`, `RULE_KEYS`. |
| `quicktunnel` | `provision(host, endpoint, auth_mode, user_agent, timeout, budget)` (429 and 5xx tried again after `Retry-After` or a backoff, at most `PROVISION_ATTEMPTS` in `PROVISION_BUDGET`; a name that does not resolve once, then only on a new answering step), `request_quick_tunnel`, `QuickTunnel` (`secret_bytes`), `AuthMode` (`Public`, `Otp`), `ProvisionError` (the cause of the failure), `retry_of(&Failure) -> Option<Refusal>`, `parse_retry_after`, `validate_quick_hostname`, `default_quick_service`. |
| `ingress` | `Ingress { rules, auth }` (`rule`, `rule_with`, `catch_all`, `resolve`, `resolve_index`), `IngressRule`, `Service` (`Http`, `Unix`, `UnixTls`, `Tcp`, `Forward`, `HelloWorld`, `Status`, `Static`, `Metrics`, `Custom`; `parse` of cloudflared's forms), `Custom`, `ForwardTarget`, `OriginOptions` (cloudflared's `originRequest`), `PathPattern` (a regular expression on the decoded path, as cloudflared), `credentials(user, file)` (the credential in front of the tunnel), `ingress_from_raw`. |
| `origin` | `Origins` (`new(tls)`, `with_report`, `with_words`, `with_no_verify`, `dial(service, options)`, `words`): HTTP, HTTPS, unix, TLS over unix, TCP; `noTLSVerify` with no configuration of the consumer is a failure of the class `Policy`. `connect_origin` (`localhost` with no resolver), `parse_http_url`, `split_host_port`, `local_name`, `origin_label`. |
| `proxy` | `Runtime` (`build(host, &ingress, metrics)`, `host`, `origins`, `metrics`), `serve_stream` (a QUIC stream), `serve_decoded` (a stream of the HTTP/2 leg), `EdgeReader`, `EdgeWriter`, `forward_http`, `forward_tcp`, `build_request_head`, `write_response_meta`, `write_simple_response`, `write_simple_response_with`, `header_value`, `content_length`, `is_websocket`, `auth_body`, `tcp_http_body`, `RPC_STREAM_REFUSED`. |
| `ws` | `serve_websocket` (the edge side of a forward: 101 only for a target that answers, else 502), `forward_path`, `forward_path_with`, `forward_url`, `accept_key`, `DEFAULT_PATH`. |
| `metrics` | `Metrics` (`new`, `with_prefix(name)`, the counters, `snapshot`, `prometheus`), `MetricsSnapshot`. |
| `edge_h2` | The HTTP/2 leg: `connect`, `reactor`, `serve_connection`, `body_reader`, `body_writer`, `stream_type`, `connect_request`, `edge_response`, `H2Leg`, `Keepalive`, `Budget`, `server_builder`, the windows and the limits. |
| `edge_discovery` | `discover(host, filter)` (DNS over HTTPS in `DOH_LIMIT` 8 s, the system resolver in the rest of `DISCOVERY_LIMIT` 12 s, the static list; one question to one resolver host in `DOH_EACH` 3 s; a source that takes more than `SLOW_SOURCE` 3 s gives one event `edge.discovery` with its time, its way (the proxy) and its cause; each failed DoH query goes to `tracing` at debug with each resolver and its cause; the legs of one start share one discovery), `discover_doh` (the A and AAAA questions at the same time), `discover_system`, `checked_targets`, `checked_targets_with(targets, &Report)`, `fixed_targets`, `edge_srv`, `parse_srv`. |
| `cloudflare` | The protocol: `capnp` (`Message`, `Builder`, `StructReader`, with a bound check of each list), `schema`, `rpc` (`register_connection`, `register_connection_on`, `ControlSession`, `Refused`), `stream`, `http2`, `quic` (`build_endpoint(tls, protocol)`, `dial`, `dial_any`), `Protocol`, `EdgeAddr`, `for_connection`, `quic_alpn`, `quic_sni`, `h2_sni`, `edge_tls_config`, `edge_roots_pem`, `speaks`, `MAX_EDGE_STREAMS`. |
| `race` | `race(attempts, failover, close)`: the staggered race of the legs (`LEG_STAGGER` 2 s) or a serial one; `Failover`, `Won`. |
| `udp` | `Choice` (`Off`, `On`, `Probe`), `Egress`, `probe(target, deadline)`, `packet`, `default_probe_target`, `START_DEADLINE`, `DOCTOR_DEADLINE`. |
| `fail` | `Result`, `Context`, `OrFail`, `IoFail`, `wrap`, `caused`, `find`, `stage_of`, `class_of`, `Refused`, `tls_text`, `tls_stage`. |
| `yaml`, `services`, `text` | The reader of the YAML of a configuration file; the facts of the edge and of the quick tunnels of `pod-services`; `clean` (text of a peer for a log line). |

## Features

| Feature | Contents |
| --- | --- |
| `fuzzing` | `fuzzing`: the entry points of the fuzz targets of `fuzz/` (`capnp_message`, `rpc_message`, `register_response`, `connect_request`, `h2_header_block`, `srv_record`, `request_head`, `ingress_rule`, `config_yaml`) and their seeds. No consumer turns it on. |

## Settings

Each variable of a consumer is a field (`docs/design.md`, section 1, rule
6):

| Field | Default | podnet |
| --- | --- | --- |
| `edge: Option<Vec<SocketAddr>>` | none: the discovery | `PODNET_EDGE` |
| `udp: udp::Choice` | `Probe` | `PODNET_UDP` |
| `provision_budget: Duration` | `PROVISION_BUDGET` (300 s) | `PODNET_READY_TIMEOUT` makes it shorter |
| `client_version: String` | `pod-cloudflare/VERSION` | `CLIENT_VERSION` (`podnet/VERSION`) |
| `verify_strict: bool` | false | `PODNET_VERIFY_STRICT` |
| `forward_path: String` | `/__podnet/ws` (podnet's wire) | `ws::DEFAULT_PATH` |
| `words: Words` | words that name no consumer | podnet's words: `podnet`, `--protocol`, `PODNET_UDP=off`, `Set PODNET_VERIFY_STRICT=1 to make this an error`, `give the address with --resolve HOST=IP, or use --no-verify`, `Use --forward with "podnet connect", or --port for an HTTP origin.` |

## The tests

* The unit tests of podnet's files moved with them, each with its name
  (`src/**/tests.rs`).
* `tests/quick_tunnel_standin.rs`: podnet's quick tunnel against the
  stand-in of the API and of the edge (HTTP/2 leg), over the library in
  place of podnet's binary: the visitor, the 429, the budget, the 403, the
  credential, a CONNECT proxy, an exec proxy, a refused registration, the
  fall back to HTTP/2, the CA of the edge, the counters of the consumer.
  The feature `standin` (`src/standin.rs`) gives the stand-in to the tests
  of a consumer; a consumer runs its binary against it.
  The edge of the stand-in answers each registration with `Reply::Accept`,
  `Reply::Refuse(cause)` or `Reply::RefuseRetry { cause, after }`
  (`Edge::answer`), or with a script of replies, one for each registration
  (`Edge::script`). `Standin::start_here` and `Standin::start_tunnel` run
  the stand-in and the tunnel on the runtime of the test, for a test on a
  paused clock.
* A refusal of the registration with retry: yes gets new tries for 30 s
  from the first refusal; when each leg failed so, the start fails with
  the class TempFail and the typed `fail::Refused` in its chain.
* The plants of the group `cloudflare` (`scripts/plants/cloudflare-*`):
  the bound check of a list of Cap'n Proto, `noTLSVerify` with no
  configuration, the relay on a leg other than `http2+relay`.
* The fuzz targets of `fuzz/` (`fuzz/README.md`).

## The parity list

| Capability | podnet | `pod-cloudflare` |
| --- | --- | --- |
| The QUIC leg (ALPN `argotunnel`, SNI `quic.cftunnel.com`, 256 streams) | yes | [x] |
| The HTTP/2 leg (no ALPN, the edge as the HTTP/2 client, GOAWAY, PING, the drain) | yes | [x] |
| `registerConnection` and `unregisterConnection` over Cap'n Proto; a refusal and its retry | yes | [x] |
| The bound check of each list of a message (PodUtils/podnet#555) | yes | [x] |
| The legs `quic`, `http2`, `quic+warp`, `http2+relay`; the plan with a proxy and with UDP | yes | [x] |
| The staggered race of the legs and the serial one | yes | [x] |
| The HA connections, the reconnect with backoff, `stopped` | yes | [x] |
| The leg `quic+warp` reconnects when its WARP session closes (PodUtils/podnet#603) | yes | [x] (through `WarpSessions`) |
| The relay budget of the leg `http2+relay` (PodUtils/podnet#482) | the constants | [x] (`pod_relay::relay` limits) |
| The relay on the leg `http2+relay` alone | `--transport relay` | [x] |
| The discovery: DNS over HTTPS, the system resolver, the static list, the check of the SRV shape | yes | [x] (the system resolver is `SystemSrv`) |
| The limits of the discovery and the event of a slow source (PodUtils/podnet#755) | no | [x] |
| The UDP probe | yes | [x] |
| The quick tunnels: provision, 429, `Retry-After`, the DNS retry, the hostname check | yes | [x] |
| Named tunnels: a token, a credentials file | yes | [x] |
| cloudflared's `config.yml` (keys, types, near keys, ingress, `originRequest`) | yes | [x] |
| Ingress: hostname, path as a regular expression on the decoded path, catch-all | yes | [x] |
| The origins: HTTP, HTTPS, unix, TLS over unix, TCP, timeouts | yes | [x] |
| `noTLSVerify` | a verifier that accepts each certificate | [x] (the configuration of the consumer, `Host.origin_no_verify`) |
| The request head to the origin, hop-by-hop headers, chunked bodies, trailers with a cap | yes | [x] |
| The credential in front of the tunnel (`--auth-user`) | yes | [x] (`Ingress::auth`) |
| The edge side of a forward (WebSocket to TCP or unix) | yes | [x] |
| Hello world, a status, the metrics | yes | [x] |
| podnet's services (files, git, shares, the PIN gate) | yes | [ ] stays in podnet, behind `ServiceDispatch` |
| `tunnel --inspect` (the capture of each exchange) | no | [ ] a hook on `forward_http` (open) |
| The check of the public URL | yes | [x] |
| The header oracle of cloudflared | yes | [x] (`tools/`) |
| The pin of cloudflared and its watch | yes | [ ] the pin is here; its watch (podnet `tools/check-upstream.py`, `upstream.yml`, removed by the adoption) is open |
| The fuzz targets | yes | [x] (`fuzz/`) |
| podssh's quick tunnels | | [ ] T-030 |

## The adoption map

| podnet | `pod-cloudflare` |
| --- | --- |
| `PODNET_EDGE` | `Settings::edge` |
| `PODNET_UDP` (`udp::Choice::from_env`) | `Settings::udp` (`udp::Choice::parse`) |
| `PODNET_VERIFY_STRICT` | `Settings::verify_strict` |
| `PODNET_READY_TIMEOUT` (`detach::provision_budget`) | `Settings::provision_budget` |
| `tunnel::CLIENT_VERSION` | `Settings::client_version` (`podnet/VERSION`) |
| `trust::context()` | `Host::tls` |
| `egress::proxy_for`, `egress::dial`, `egress::dial_addrs` | `Host::dial` (`Host::proxy_for`, `Host::connect`) |
| `resolve::doh_query`, `resolve::answering_step` | `Host::resolver` (`Host::answering_step`) |
| `resolve::refresh_pins` before each check of the URL | podnet puts its `PODNET_RESOLVE` pins into `Host::resolver` (`pod_dns::Resolver::set_pins`) |
| `fetch::send` | `Host::send` |
| `decisions::count("leg", ..)` | `Host::report` (`Observer::decision("leg", ..)`) |
| `decisions::prometheus()` in the metrics text | `Metrics::with_extra` (podnet's counters of the decisions), given to the tunnel with `TunnelBuilder::metrics` |
| `ws_relay::active()` | `Host::relay` (`EdgeRelay`), set for `--transport relay` |
| `ws_relay::SESSION_BYTES`, `SESSION_AGE` | `pod_relay::relay::byte_budget`, `time_budget` of `EdgeRelay::context` |
| `warp::WarpClient` (`register_or_load`, `with_masque`, `connect`), `warp_identity`, `masque_endpoint` | `Host::warp` (`WarpSessions` over `pod-warp`, in podnet) |
| hickory's SRV lookup | `Host::system_srv` (`SystemSrv`, in podnet) |
| `service::ServiceRuntime`, `Built` | `proxy::Runtime`; `ServiceDispatch` for the static files, the files, git, the shares |
| `origin::dial::dial` | `origin::Origins::dial` |
| `origin::dial::NoVerify` (`noTLSVerify`) | podnet keeps its verifier and gives its configuration in `Host::origin_no_verify` |
| `report::hide(token)` | podnet hides `Config::secrets` |
| `report::Refusal`, `report::read_refusals_with(retry_of)` | `quicktunnel::Refusal`; podnet's `quicktunnel::retry_of` finds the `Failure` in its error and calls `retry_of(&Failure)` |
| `hop::Hop`, `hop::at`, `hop::of` | `pod_core::Stage`, `failure.stage` |
| `exit::Fault`, `exit::code_of` | `pod_core::Class`, `failure.class.exit_code()`; podnet's `exit::code_of` reads the class (a code that podnet does not give exits 1) |
| `config::load_file` | `config::load_file_with(path, &report)` |
| `edge_discovery::checked_targets` | `checked_targets_with(targets, &report)` (the discovery passes `Host::report`) |
| The texts that name podnet and its flags | podnet's `Words` in `Settings::words` |
| The metrics `podnet_*` | `Metrics::with_prefix("podnet")` (the tunnel takes `Words::name`) |
| `rpc` exception "not implemented by podnet" | "not implemented by the client" |
| `ws::DEFAULT_PATH` | `Settings::forward_path` (default `/__podnet/ws`) |
| `Tunnel::quick()`, `Tunnel::token(token)` | `Tunnel::quick(host)`, `Tunnel::token(host, token)` (`podnet::tunnel::host()`) |
| `ws::bridge_session` counted by `Metrics` | `Metrics` is a `pod_ws::Tally` |
| `tests/standin/` | the feature `standin` (`standin::Standin`, `standin::quic::local_pair`) |
| `TunnelHandle::share_controls` | podnet's `ServiceDispatch` keeps them (`podnet::tunnel::start` gives its `ServiceRuntime`) |

What stays in podnet: its `ServiceDispatch` (the static files, the files,
git, the shares, the PIN gate, `share_proxy`, the axum dispatch and its
tests), its host (`src/tunnel.rs`: the TLS context, the egress, the
resolver, the system SRV records, the relay, the WARP device, the log,
the counters, the settings from its variables), its `ws::LocalSpec` and
the client of a forward, its verifier of `noTLSVerify`, the fuzz target
`local_spec`, and these tests of its binary and its own glue:
`quick_tunnel_standin_prints_json_lines`,
`quick_tunnel_standin_stops_clean_on_sigterm_after_ready`,
`quick_tunnel_standin_detach_ps_logs_and_stop` (the command line),
`tests/tunnel_config.rs` (the flags), `tests/api_surface.rs` (podnet's
API), `tests/tls.rs` and `tests/egress.rs` (podnet's trust and egress over
`pod-tls` and `pod-proxy`), `tests/signals.rs`; and the unit tests of the
code that stays: `large_share_uploads_on_one_quic_connection`,
`a_built_in_body_of_no_known_size_is_streamed`,
`the_origin_of_a_proxy_share`, `form_parsing`,
`a_refused_upgrade_is_tried_again`, `parses_local_specs`,
`fuzz_a_unix_proxy_label_holds_no_user_info`.
