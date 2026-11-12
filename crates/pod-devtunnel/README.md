# pod-devtunnel

The client side of Microsoft Dev Tunnels. A host that permits only TCP
443 reaches the relay of the service: this crate gets the connect
information of a tunnel from the management API with the user's own
token, opens the relay WebSocket through the caller's TLS, proxy and dial,
runs SSH over it to the host of the tunnel with no user authentication,
and opens a `direct-tcpip` channel to a port of the host as a byte stream.
A bounded reconnect takes a fresh connect token.

The protocol has no specification. The source of each value is the
vendor's open-source client, microsoft/dev-tunnels (MIT licence), read at
`37956c63675e3339b42c7f852a5af5da1566c5b9` on 2026-11-12, mostly its Rust
client under `rs/`. No code of it is copied. Each value is a fact of the
service `microsoft-devtunnel` of `pod-services`, with its source, and the
watches of that file check the vendor's client each day.

The asks: GitHub #94 (a contributor) and PodUtils/podssh#216 (COORD-041,
for PodUtils/podssh@T-755). This crate is the first part of T-378 (the
client side, podssh's ask); the second part is the creation, the list and
the delete of tunnels, the device-code logins of GitHub and AAD and the
token store (T-378, partial).

## API

| Item | What |
| --- | --- |
| `Relay::connect(way, settings, locator, user, report)` | the connect information, then one relay session; one try, its failure named by its step |
| `Relay::open(port)` | a `Stream` to `127.0.0.1:PORT` of the host (`direct-tcpip`); a session that is gone connects again first |
| `Relay::reconnect()`, `Relay::close()` | a new session with a fresh connect token; the end of the session. Each open stream ends |
| `Relay::tunnel()`, `Relay::sessions()` | the ids of the tunnel; the sessions opened so far |
| `Stream` | `AsyncRead + AsyncWrite + Send + Unpin`: the far end's EOF or close is `Ok(0)`; the end of the session under it is the error `ConnectionAborted` (`stream::SESSION_ENDED`); `shutdown` sends EOF; the drop closes the channel |
| `Management::new(client, &settings)`, `connect_info(&locator, &user)`, `url(&locator)` | the read of the management API |
| `ConnectInfo::parse(body, scope)`, `ConnectInfo { tunnel, endpoints }`, `token()` | the answer of the API: `Tunnel` (cluster, id, name), each relay `Endpoint` (host id, client relay URI, host keys), the `ConnectToken` |
| `Locator::parse` | `NAME`, or `ID.CLUSTER` as the vendor's tools show a tunnel |
| `UserToken::new(scheme, secret)`, `Scheme::{Github, Aad, Bearer}`, `ConnectToken` | the tokens; no `Debug` shows one |
| `Step`, `step_of`, `steps_of`, `Failed` | the named step of a failure: `Management`, `WebSocket`, `Ssh`, `Channel`, `Reconnect` |
| `Settings`, `Base`, `Way` | the settings; the base of the management API; the caller's way out (`pod_proxy::Client`, `pod_ws::Way`, `pod_tls::Trust`) |
| `service` | the facts of `microsoft-devtunnel` |
| `ssh::key_text` | the text of a host key as `hostPublicKeys` holds it |
| `standin` (feature) | `Standin::start(Plan)`: the management API, the relay and the host (russh) on the loopback; `Port::{Echo, EchoOnce, Forward}`, `Keys`, `Seen`, `drop_relays`, `refuse_upgrades`, `refuse_user` |

Each failure is a `pod_core::Failure`. Its stage and class:

| Step | Stage | Class |
| --- | --- | --- |
| `Management` | the stage of the HTTP failure, else `relay` | 401, 403: `refused` (ask the user for a new token; not tried again); 404: `not-found`; 429: `temp-fail`; other 4xx: `protocol`; 5xx: `unavailable`; no connect token in the answer: `refused`; no relay endpoint (no host connected): `unavailable`; a body that is not a tunnel: `protocol` |
| `WebSocket` | the stage of `pod_ws::ConnectError` (`proxy`, `connect`, `tls`, `websocket`) | its class (401, 403: `refused`; 429, 503: `temp-fail`) |
| `Ssh` | `relay` | a host key that is not the tunnel's, a refused user: `refused`; a timeout: `timeout` |
| `Channel` | `origin` | the reason of the host (RFC 4254): `refused`, `unavailable`, `unsupported`, `temp-fail`; no answer: `timeout` |
| `Reconnect` | the stage and class of the last try | the chain holds the failure of the last try |

## The protocol, with its sources

Each source is a file of microsoft/dev-tunnels at
`37956c63675e3339b42c7f852a5af5da1566c5b9`, read on 2026-11-12. The fact
of `microsoft-devtunnel` is in the last column.

| Value | Source | Fact |
| --- | --- | --- |
| The management API `https://global.rel.tunnels.api.visualstudio.com` | `rs/src/contracts/tunnel_service_properties.rs` (`PROD_DNS_NAME`), `rs/src/contracts/tunnel_environments.rs` | endpoint `management` |
| `api-version=2023-09-27-preview` in each query | `rs/src/management/http_client.rs` (`API_VERSIONS`, `add_query`) | `api-version` |
| `GET /tunnels/NAME` on the global host; `GET /tunnels/ID` on `CLUSTER.rel.tunnels.api.visualstudio.com` (`global.` goes out) | `rs/src/management/http_client.rs` (`TUNNELS_API_PATH`, `build_tunnel_uri`, `build_uri`) | `tunnels-path`, `cluster-host-rule` |
| `tokenScopes=connect`, and `accessTokens.connect` in the answer | `rs/src/management/http_client.rs` (`add_query`), `rs/src/contracts/tunnel_access_scopes.rs`, `rs/src/contracts/tunnel.rs` | `token-scope` |
| `Authorization: github TOKEN`, `aad TOKEN` or `bearer TOKEN` to the API | `rs/src/management/authorization.rs` | `user-schemes` |
| The JSON members `clusterId`, `tunnelId`, `name`, `accessTokens`, `endpoints`, and of each endpoint `connectionMode` (`TunnelRelay`), `hostId`, `hostPublicKeys`, `clientRelayUri` | `rs/src/contracts/tunnel.rs`, `tunnel_endpoint.rs`, `tunnel_relay_tunnel_endpoint.rs`, `tunnel_connection_mode.rs` (camelCase) | `tunnel-fields`, `endpoint-fields`, `relay-endpoint-fields` |
| The upgrade to `clientRelayUri` with `Sec-WebSocket-Protocol: tunnel-relay-client` and `Authorization: tunnel TOKEN`; binary messages carry the SSH bytes | `rs/src/connections/relay_tunnel_client.rs` (`connect`), `rs/src/connections/ws.rs` | `subprotocol`, `relay-scheme` |
| SSH: the host key is one of `hostPublicKeys` (the base64 of its wire form); the user `tunnel` with `none`; no compression | `rs/src/connections/relay_tunnel_client.rs` (`make_ssh_session`, `is_server_key_allowed`, `authenticate_none("tunnel")`) | `ssh-user`, `ssh-auth` |
| A `direct-tcpip` channel to `127.0.0.1:PORT`, from `127.0.0.1:0` | `rs/src/connections/relay_tunnel_client.rs` (`connect_to_port`) | `channel-type`, `connect-host` |

The facts `relay-calls`, `management-calls` and `user-scheme-calls` hold
the words of the vendor's code that carry these values, for the watches:
a bare `none` or `tunnel` is in the file for another reason. The
TypeScript client offers `tunnel-relay-client-v2-dev` first, and then
`tunnel-relay-client` (`ts/src/connections/tunnelRelayTunnelClient.ts`);
the Rust client offers the first protocol alone, and this crate does the
same (call C166).

## Features

* `standin`: the stand-in of the tests (`src/standin.rs`), for the tests
  of this crate and of a consumer. Its tokens are made-up values.

## Settings

The crate reads no variable of the environment and keeps no process
state.

| Setting | Default |
| --- | --- |
| `management` | the endpoint `management` of `microsoft-devtunnel` |
| `api_version` | the fact `api-version` |
| `timeout` (each step) | `pod_core::backoff::CONNECT_TIMEOUT`, 20 s |
| `reconnect_tries`, `backoff` | 4; `pod_core::backoff::backoff` |
| `keepalive`, `keepalive_max` (SSH) | 30 s, 3 |
| `limits` (the relay WebSocket) | `pod_ws::Limits::RELAY` |
| `ssh_id` | `SSH-2.0-pod-devtunnel`; a consumer gives its own name |
| The proxy, the resolver, the roots, the user agent | the caller's `Way` |
| The user's token | the caller's `UserToken`, from its own store |

## Parity

Against PodUtils/podssh#216:

| Ask of podssh | Here |
| --- | --- |
| [x] The management API: a tunnel by name or by id; its connect information and connect token | `Locator`, `Management::connect_info`, `ConnectInfo` |
| [x] The relay WebSocket with its subprotocol through the caller's TLS, proxy and WebSocket client | `pod_ws::url::dial_asking` with the caller's `pod_ws::Way` and `pod_tls::Trust` |
| [x] The client's SSH session with `none` | `ssh::start` (russh 0.65 on ring) |
| [x] A `direct-tcpip` channel as `AsyncRead + AsyncWrite` | `Relay::open`, `Stream` |
| [x] Named failures: management, WebSocket, SSH, channel, reconnect | `Step`, `step_of` |
| [x] A bounded reconnect with a fresh connect token; it ends each open stream; a new stream after it; a stream that the far end closed is not retried | `Relay::open`, `Relay::reconnect`, `Settings::reconnect_tries`; `tests/reconnect.rs`, `tests/relay.rs` |
| [x] No token in a `Debug` or a line | `tests/secrets.rs` (each line of `tracing` at each level, each event, each failure, each `Debug`) |
| [x] A stand-in relay for the tests | feature `standin`; a consumer's test of an SSH session forwards a port to its own server (`Port::Forward`) |

Against GitHub #94:

| Ask | Here |
| --- | --- |
| [x] The host and the API version as data, with a source | `microsoft-devtunnel`; the crate has no host and no relay URI in its code |
| [x] Get a tunnel, its `endpoints` and `access_tokens["connect"]` | `Management::connect_info` |
| [ ] Create, list and delete a tunnel | T-378, the second part |
| [ ] The device-code flows of GitHub and AAD (the identities as settings), the refresh of an AAD token | T-378, the second part |
| [ ] A token store in a private file (`pod_core::files`) | T-378, the second part; podssh keeps its own token file (PodUtils/podssh#216) |
| [x] The WebSocket with `tunnel-relay-client`, SSH with `tunnel` and `none`, `direct-tcpip` as `AsyncRead + AsyncWrite` | `Relay`, `Stream` |
| [x] A reconnect with a bounded backoff and a fresh connect token; a reconnect ends each stream; a stream that the far end closed is not retried | `Relay` |
| [x] A rejected token is not tried again in a loop | a 401 or 403 of the API ends a reconnect at once |
| [x] The caller gives the HTTP client, the WebSocket client, the TLS provider and an observer; no variable, no process state, no output | `Way`, `pod_core::Report` |
| [x] Two data files `devtunnel-management` and `devtunnel-relay` | one file, `microsoft-devtunnel` (kind `devtunnel`), with each value of both (call C166) |
| [x] Mock tests: the paths, `Authorization`, the scope, a get, a refusal | `tests/management.rs` (a create and a list: the second part) |
| [x] A stand-in relay: the subprotocol, `none`, `direct-tcpip`, an echo, a reconnect | `tests/relay.rs`, `tests/reconnect.rs` |
| [x] A live test with the user's own token, skipped with none | `tests/live.rs` (ignored; T-386) |
| [x] The static probe links the crate | `examples/static-probe/src/devtunnel.rs` |
| [ ] The name in the reservation list of GitHub #1 | the owner reserves the name (T-387) |

## Adoption map

podssh's road (PodUtils/podssh@T-755) takes a tag with this crate:

| podssh | Here |
| --- | --- |
| `devtunnel://TUNNEL` | `Locator::parse(TUNNEL)` |
| the token from its private file | `UserToken::new(scheme, text)`; podssh keeps the file and its doctor |
| its proxy, roots, ECH and user agent | `Way { http: pod_proxy::Client, ws: pod_ws::Way, trust }` |
| the forward of a stream | `Relay::connect` once, then `Relay::open(port)` for each stream |
| its lines | the events `devtunnel.connected`, `devtunnel.reconnect`; the decisions `devtunnel.reconnect` (`connected`, `failed`) |
| its exit codes | `Failure::class`; `step_of` for the words of its doctor |
| the test of a session to a stand-in SSH server | `standin::Plan` with `Port::Forward(ADDRESS)` of its server |

## Tests

`cargo test -p pod-devtunnel --all-features`: the unit tests; the
stand-in tests `tests/management.rs`, `tests/relay.rs`,
`tests/reconnect.rs` and `tests/secrets.rs` on the loopback
(`need_loopback!`); `tests/live.rs` is ignored (`POD_DEVTUNNEL_TOKEN`,
`POD_DEVTUNNEL_SCHEME`, `POD_DEVTUNNEL_TUNNEL`, `POD_DEVTUNNEL_PORT`; the
user's own account, never a shared one). The fuzz target
`devtunnel_connect_info` reads the connect information. The plants
`devtunnel-*` of `scripts/plants/` prove the checks of the session end,
the host key, the subprotocol, the refused user token and the hidden
token.
