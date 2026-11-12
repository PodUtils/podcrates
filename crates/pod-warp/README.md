# pod-warp

Cloudflare WARP, spoken directly: anonymous registration, the P-256
identity of the device and its file, MASQUE CONNECT-IP over HTTP/2 on TCP
443 through the caller's dial with the key of the front door pinned, and a
userspace stack over the session, given as a network of the caller: a
`pod_core::Dial` (TCP streams, names resolved inside the tunnel) and
`pod_core::Datagrams` (UDP).

The base is podnet's `src/warp/` (`PodUtils/podnet:src/warp`); podssh's
`crates/podssh-warp` retires onto it (`docs/design.md`, section 4). This
is the first part of T-028 (partial): the registration, the identity, the
doors, the session over HTTP/2, the network and the rules. The rest is
T-335 to T-339.

The crate reads no environment variable and no file that the caller does
not name, prints nothing and keeps no global state. Each failure is a
`pod_core::Failure` (stage `Warp`, or the stage of the dial); a line for
the user is a `pod_core::Event` of the caller's `Report`; a decision goes
to its `Observer`. The TLS provider and the way out are the caller's.

**Build setting.** The stack is `pod-netstack`'s: each binary sets
`SMOLTCP_IFACE_MAX_ADDR_COUNT = "8"` in the `[env]` table of its
`.cargo/config.toml`.

## API

| Item | Contents |
| --- | --- |
| `Settings` | `api: Api`, `way: Way`, `door: Door`, `routes: Routes`, `hints: DeviceHints`, `report: Report`, `mtu` (1280); `Settings::new(api, way)`. |
| `Warp` | `register(settings)`, `register_or_load(settings, &IdentityFile)`, `from_parts`, `from_stored`, `save(path)`; `device`, `identity`, `settings`; `refresh`, `account`, `set_license`, `rotate_key`, `delete`; `open_session() -> MasqueSession`, `connect() -> WarpNet`; `won_door`, `with_won_door`, `mark_door_bad`, `endpoint_in_use`. |
| `WarpNet` | `Dial` (`label` `WARP`, `dial`, `dial_addrs`, `names_inside` true, `datagrams`); `start(session, v4, v6, mtu, report)`, `addresses`, `is_closed`, `close_reason`, `stack`, `with_resolvers`. |
| `api` | `Api::new(pod_proxy::Client)`, `with_base(url)` (podnet's `PODNET_WARP_API`, podssh's `PODSSH_WARP_API`), `with_bases`, `bases`, `request`, `send` (each base in turn; a `POST` goes on only after no answer); `ApiError`, `api_status`, `bases(value)`, `API_TIMEOUT`. |
| `register` | `create_device`, `create_device_with_key`, `enrol_masque`, `get_device`, `get_account` (license hidden), `set_license`, `delete_device`, `registration_error` (error 1015). |
| `device` | `Device` (`ipv4`, `ipv6`, `local_address`, `masque_endpoint`, `masque_endpoints`, `peer_public_key_pem`, `reserved`; `Debug` hides the token and the license), `Account`, `DeviceConfig`, `Peer`, `Endpoint`, `Interface`, `Addresses`, `Services`, `Policy`, `redact_account`. |
| `hints` | `DeviceHints { name, model, locale }`, `check`. |
| `identity` | `Identity` (`generate`, `of`, `key`, `spki_b64`, `spki_der`, `certified(now)`, `to_pem`, `from_pem`), `spki_from_pem`, `spki_from_cert`, `warp_plus_label`, `license_class`. |
| `key`, `der` | `DeviceKey` (P-256 in pure Rust: `generate`, `from_scalar`, `from_pkcs8`, `pkcs8`, `point`, `spki`, `sign`, `certificate`, `certified`, `signer`); the DER of a certificate, an SPKI, PEM and PKCS #8 (`der::pkcs8`). |
| `store` | `IdentityFile { path, foreign, lock_wait }`, `Stored`, `register_or_load`, `load`, `save`, `LOCK_WAIT`. |
| `files` | `Foreign` (`Refuse`, `Allow`), `check_private`, `write_private`, `lock`, `lock_path`, `Held`. |
| `door` | `Door` (`Record`, `Pinned(Target)`), `Routes`, `endpoint_order`, `doors`, `door_from`, `door_text`, `DOOR_TTL`. |
| `tls` | `client_config(provider, identity, pinned_spki, alpn)`, `handshake_failure`, `is_pin_mismatch`, `PIN_MISMATCH`, `ALPN_H2`. |
| `session` | `Way { dial, provider, sni, authority, timeout }`, `MasqueSession` (`connect`, `split`), `MasqueTx` (`send_packet`, `send_packets`, `waits`, `keepalive`, `close`), `MasqueRx` (`recv_packet`, `take_assigned`, `progress`, `is_closed`), `Keepalive`, `h2_builder`; the windows, the budget of DATA frames, `SEND_RESERVE`, `SEND_STALL`, `CONNECT_TIMEOUT`, `MAX_MISSED_PONGS`. |
| `capsule` | The capsules of RFC 9297 and the types of RFC 9484: `put_varint`, `get_varint`, `put_capsule`, `CapsuleDecoder`, `parse_address_assign`, `MAX_CAPSULE_LEN`. |
| `dns` | `lookup(net, resolvers, name, limit)` (DNS over TCP inside the tunnel), `RESOLVERS`. |
| `rules` | `Action`, `Rule` (`parse(spec, place)`, `matcher_text`), `Table` (`build(specs, file, default, lint, allow)`, `for_name`, `for_ip`, `for_addresses`, `decide`, `decide_static`), `Host`, `Lint`, `LintItem`, `NoLint`, `Rules` (`set`, `current`, `counters`), `Counters`, `Refused`, `load_file`, `matcher` (`RuleMatch`, `Net`, `load_nets`, `domain_matches`). |
| `service` | The facts of `cloudflare-warp` of `pod-services`: `api_base`, `api_second`, `api_version`, `client_version`, `user_agent`, `masque_sni`, `masque_port`, `masque_doors`, `connect_uri`, `terms_url`. |

## Settings

Each variable of a consumer is a field (`docs/design.md`, section 1, rule
6):

| podnet | podssh | `pod-warp` |
| --- | --- | --- |
| `PODNET_WARP_API` | `PODSSH_WARP_API` | `Api::with_base` |
| `PODNET_WARP_DEVICE_NAME`, `_MODEL`, `_LOCALE` | | `Settings::hints` |
| `PODNET_FOREIGN_IDENTITY=allow` (with `--identity`) | | `IdentityFile::foreign = Foreign::Allow` |
| `--masque-endpoint` | | `Settings::door = Door::Pinned(target)` |
| `--masque-sni` | | `Way::sni` |
| `has_default_route` | | `Settings::routes` |
| `HTTPS_PROXY`, `--transport`, the relay | `HTTPS_PROXY` | `Way::dial`, and the dial of the `pod_proxy::Client` of `Api` |
| `--allow-shadowed-rules` | | the `allow` of `Table::build` |
| the state directory (`warp.json`, `warp-door.json`) | the device's private file | `IdentityFile::path`; `door_from` and `door_text` for the caller's door file |

## The tests

Loopback only: the stand-in API (`pod_testkit::stand::warp`), the HTTP/2
and capsule part of the stand-in edge (`pod_testkit::masque`), and its TLS
and far end in `tests/common/edge.rs` (call C135).

| Approach of T-028 | Test |
| --- | --- |
| The registration on the loopback | `tests/register.rs`: `the_second_api_host_answers_after_a_failure`, `a_503_on_the_post_is_not_sent_to_the_second_host`, `a_device_registers_enrols_and_is_resumed_from_its_file`, `a_429_on_registration_names_the_remedy`, `the_account_calls_hide_the_license_and_rotate_the_key`, `the_hints_of_a_device_go_into_the_bodies` |
| The identity of another uid | `files::tests`: `a_foreign_owner_is_a_config_error`, `a_foreign_owner_reads_with_the_consent_of_the_caller`; `tests/register.rs`: `a_loose_identity_leads_to_no_registration`, `a_refused_identity_leads_to_no_registration` (as root) |
| The pinned door | `door::tests::a_pinned_door_skips_the_order`; `tests/net.rs`: `a_pinned_door_is_the_only_door`, `the_doors_of_the_record_go_in_order` |
| A session as a `Dial` and `Datagrams` | `tests/net.rs`: `a_session_carries_tcp_to_an_address_and_to_a_name`, `a_session_carries_udp_as_datagrams`, `a_new_key_of_the_front_door_reads_the_record_again`, `a_key_that_the_edge_did_not_enroll_is_refused` |
| The lint as a function of the caller | `rules::tests::the_lint_of_the_caller_judges_the_table` |
| The live check | `tests/live.rs`: `live_register_connect_and_trace` (ignored; its step in `live.yml` is T-339) |
| The proxy, the forward, nc | T-338 |

## The parity list

| Capability | podnet | podssh | `pod-warp` |
| --- | --- | --- | --- |
| Registration (`POST`, `PATCH`), the version pair of `pod-services` | yes | yes | yes |
| A second base of the API (PodUtils/podnet@T-241: a `POST` is final) | yes | routes (PodUtils/podssh@T-562) | yes (`Api::with_bases` takes podssh's routes) |
| Error 1015 names the remedy | yes | | yes |
| Account, license, rotation, removal | yes | | yes |
| Device hints | yes | | yes |
| The identity file, its lock, the record `created` | yes | a private file of its own | yes (podnet's form) |
| A foreign identity with consent | ask 9 | | yes |
| P-256 key and certificate in pure Rust | rcgen | yes | yes |
| Key of the front door pinned; the record read again on a new key | yes | yes (PodUtils/podssh@T-399) | yes |
| The order of the doors; the door that worked first; a door with no packet last | yes | the endpoints in turn (PodUtils/podssh@T-561) | yes |
| A pinned door | address | | `HOST:PORT` |
| MASQUE over HTTP/2, the windows, the batch, the keepalive | yes | yes | yes |
| MASQUE over HTTP/3; WireGuard; the carried device | yes | | T-337 |
| The relay leg and the race | yes | | T-336 |
| The stack (congestion control, burst, counters, the check of the data path) | yes | smoltcp of its own | the stack of `pod-netstack`; the rest T-335 |
| Names inside the tunnel | through the proxy's resolver | yes | yes |
| UDP | yes | | yes (`Datagrams`) |
| The rules with the lint of shadowed rules | yes | | yes (the lint is the caller's) |
| The proxy, the forward, nc, the L4 door | yes | | T-338 |
| `Live`, TUN, routes | yes | | T-339 |
| A tunnel that takes TLS and relays nothing (the probe of podssh, PodUtils/podssh@T-560) | the check of the data path | yes | T-335 |

## The adoption map

| podnet | `pod-warp` |
| --- | --- |
| `warp::WarpClient` | `Warp` (with `Settings`) |
| `WarpClient::register_or_load(path)` | `Warp::register_or_load(settings, &IdentityFile::new(path))` |
| `WarpClient::connect()` (a `WarpStack`) | `Warp::connect()` (a `WarpNet`; podnet implements its `podnet_core::network` traits over it, PodUtils/podnet#452) |
| `warp::register::*` | `register::*` with `&Api`; `api::api_status`, `device::redact_account` |
| `warp::identity::Identity` | `identity::Identity` |
| `warp::door::endpoint_order(all, won, bad, ipv6, v4_route)` | `door::endpoint_order(all, won, bad, Routes { v4, v6 })` |
| `door_from`, the door file | `door::door_from`, `door::door_text`; the file stays podnet's |
| `warp::session::{MasqueSession, MasqueTx, MasqueRx, Keepalive}` | `session::*`, with `Way` in place of `session::Dial` |
| `warp::capsule` | `capsule` |
| `warp::rules` (global `TABLE`, `COUNTERS`, `watch`) | `rules::Rules` (a value), `Table::build(.., &lint, allow)`; podnet passes `shadow::check` and keeps the reload of the file |
| `exit::fault(Fault::X, e)` | `Failure { stage, class }`; `Class::exit_code` |
| `decisions::count("masque", ..)` | `Observer::decision("warp.door", ..)` |

| podssh (`crates/podssh-warp`) | `pod-warp` |
| --- | --- |
| `key::DeviceKey`, `der` | `key::DeviceKey`, `der` |
| `register::Api` with `routes::Route` | `Api::with_bases` |
| `road::Warp` (open the tunnel, a stream to `HOST:PORT`) | `Warp::connect` and `WarpNet::dial` |
| `dns::lookup` | `dns::lookup` |
| `stand_in::StandIn` | `pod_testkit::masque` and the edge of `tests/common/edge.rs` |
| `device::Device` (its private file) | `store` (podnet's file form) |
