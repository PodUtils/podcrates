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
doors, the session over HTTP/2, the network and the rules; and T-335: the
settings of podnet's stack of `src/warp/ip.rs` (the congestion controller,
the TCP burst, the counters of the link, the budget of a relay) and the
check of the data path; and T-336: the relay leg of the caller, the race
of the direct path and the relay, the leg file and the budget of a relay
session. The rest is T-337 to T-339, and T-393 to T-395 (the rest of
`ip.rs`).

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
| `Settings` | `api: Api`, `way: Way`, `relay: Option<Relay>`, `door: Door`, `routes: Routes`, `hints: DeviceHints`, `report: Report`, `net: NetSettings`; `Settings::new(api, way)` (no relay). |
| `Warp` | `register(settings)`, `register_or_load(settings, &IdentityFile)`, `from_parts`, `from_stored`, `save(path)`; `device`, `identity`, `settings`; `refresh`, `account`, `set_license`, `rotate_key`, `delete`; `open_session() -> MasqueSession` (with a relay: the race of the two legs), `connect() -> WarpNet` (with the check of the data path; a session that carries nothing marks its door bad); `won_door`, `with_won_door`, `mark_door_bad`, `endpoint_in_use`; `relay_first`, `with_relay_first`, `leg`, `take_leg_change`. |
| `WarpNet` | `Dial` (`label` `WARP`, `dial`, `dial_addrs`, `names_inside` true, `datagrams`); `start(session, v4, v6, &NetSettings, report)`, `addresses`, `is_closed`, `close_reason`, `close(why)` (podnet's `close_session`), `stack`, `with_resolvers`, `link_stats`, `renew_due` (the budget of the session, else `NetSettings::renew`), `check_data_path(&DataPathCheck) -> Duration`, `checked`. When the session closes (the edge ends it, a pump ends, the reactor of the stack stops, also by a panic, or `close`), each open stream and UDP socket fails at once with `ConnectionReset` and the first cause, and a new dial and a new UDP socket fail with it. |
| `net` | `NetSettings { mtu, tcp_buffer, congestion_control, tcp_burst, link_stats, check, renew }` (default: 1280, `TCP_BUFFER` 512 KiB, none, no limit, the counters of the session, `DataPathCheck::default()`, no relay), `stack_config(v4, v6)`; `parse_cc` (`none`, `reno`, `cubic`), `parse_tcp_burst` (1 to `TCP_BURST_MAX`, 430, or `none`), `tcp_burst_max(buffer, mtu)`; `Renew` (a function of the caller); `CongestionControl`, `DeviceStats`, `DeviceStatsSnapshot`. A bad value is `Config` (exit 78). |
| `check` | `DataPathCheck { servers, name, limit, resend }` (default: the resolver of the tunnel of the first family of the device, `cloudflare.com`, 5 s, 1.2 s), `servers_for`, `CHECK_LIMIT`, `CHECK_RESEND`. A session with no answer is `Unavailable` (stage `Warp`). |
| `api` | `Api::new(pod_proxy::Client)`, `with_base(url)` (podnet's `PODNET_WARP_API`, podssh's `PODSSH_WARP_API`), `with_bases`, `bases`, `request`, `send` (each base in turn; a `POST` goes on only after no answer); `ApiError`, `api_status`, `bases(value)`, `API_TIMEOUT`. |
| `register` | `create_device`, `create_device_with_key`, `enrol_masque`, `get_device`, `get_account` (license hidden), `set_license`, `delete_device`, `registration_error` (error 1015). |
| `device` | `Device` (`ipv4`, `ipv6`, `local_address`, `masque_endpoint`, `masque_endpoints`, `peer_public_key_pem`, `reserved`; `Debug` hides the token and the license), `Account`, `DeviceConfig`, `Peer`, `Endpoint`, `Interface`, `Addresses`, `Services`, `Policy`, `redact_account`. |
| `hints` | `DeviceHints { name, model, locale }`, `check`. |
| `identity` | `Identity` (`generate`, `of`, `key`, `spki_b64`, `spki_der`, `certified(now)`, `to_pem`, `from_pem`), `spki_from_pem`, `spki_from_cert`, `warp_plus_label`, `license_class`. |
| `key`, `der` | `DeviceKey` (P-256 in pure Rust: `generate`, `from_scalar`, `from_pkcs8`, `pkcs8`, `point`, `spki`, `sign`, `certificate`, `certified`, `signer`); the DER of a certificate, an SPKI, PEM and PKCS #8 (`der::pkcs8`). |
| `store` | `IdentityFile { path, foreign, lock_wait }`, `Stored`, `register_or_load`, `load`, `save`, `LOCK_WAIT`. |
| `files` | `Foreign` (`Refuse`, `Allow`), `check_private`, `write_private`, `lock`, `lock_path`, `Held`. |
| `door` | `Door` (`Record`, `Pinned(Target)`), `Routes`, `endpoint_order`, `doors`, `door_from`, `door_text`, `DOOR_TTL`. |
| `leg` | `Relay { dial, name, meter, delay, delay_known }` (`Relay::new(dial, name)`: no meter, `RELAY_DELAY` 3 s, `RELAY_DELAY_KNOWN` 0.5 s; `delay_for`), `Leg` (`Direct`, `Relay`; `as_str`), `relay_first_from(text, now, relay)`, `leg_text(leg, now, relay)`, `LEG_TTL` (6 h). The direct path starts first; the relay starts after its delay, or at once when the direct path failed; the first session that opens wins and the other try is dropped; a failure of both names both causes. A relay reaches both families (`Routes::default()`). The decision `warp.leg`: `direct`, `relay`, `relay-first`, `no-relay`. |
| `tls` | `client_config(provider, identity, pinned_spki, alpn)`, `handshake_failure`, `is_pin_mismatch`, `PIN_MISMATCH`, `ALPN_H2`. |
| `session` | `Way { dial, meter, provider, sni, authority, timeout }`, `Meter` (a function of the caller: a stream in, the metered stream and its `Renew` out), `MasqueSession` (`connect`, `split`, `relay_budget`), `MasqueTx` (`send_packet`, `send_packets`, `waits`, `keepalive`, `close`), `MasqueRx` (`recv_packet`, `take_assigned`, `progress`, `is_closed`), `Keepalive`, `h2_builder`; the windows, the budget of DATA frames, `SEND_RESERVE`, `SEND_STALL`, `CONNECT_TIMEOUT`, `MAX_MISSED_PONGS`. |
| `capsule` | The capsules of RFC 9297 and the types of RFC 9484: `put_varint`, `get_varint`, `put_capsule`, `CapsuleDecoder`, `parse_address_assign`, `MAX_CAPSULE_LEN`. |
| `dns` | `lookup(net, resolvers, name, limit)` (DNS over TCP inside the tunnel), `RESOLVERS`. |
| `rules` | `Action`, `Rule` (`parse(spec, place)`, `matcher_text`), `Table` (`build(specs, file, default, lint, allow)`, `for_name`, `for_ip`, `for_addresses`, `decide`, `decide_static`), `Host`, `Lint`, `LintItem`, `NoLint`, `Rules` (`set`, `current`, `counters`), `Counters`, `Refused`, `load_file`, `matcher` (`RuleMatch`, `Net`, `load_nets`, `domain_matches`). |
| `service` | The facts of `cloudflare-warp` of `pod-services`: `api_base`, `api_second`, `api_version`, `client_version`, `user_agent`, `masque_sni`, `masque_port`, `masque_doors`, `connect_uri`, `terms_url`; of `cloudflare-dns`: `tunnel_resolver` (`udp-1`), `check_name`. |

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
| `HTTPS_PROXY`, `--transport` | `HTTPS_PROXY` | `Way::dial`, and the dial of the `pod_proxy::Client` of `Api` |
| the WebSocket relay (`PODNET_RELAY`, `ws_relay::Relay`) | | `Settings::relay = Some(Relay::new(dial, url))`, the dial of the relay of `pod-relay` |
| `--no-relay`, HTTP/3 | | `Settings::relay = None` |
| `warp-leg.json` (`read_leg`, `write_leg`) | | `leg::relay_first_from` into `Warp::with_relay_first`; `Warp::take_leg_change` and `leg::leg_text` for the caller's file |
| `--allow-shadowed-rules` | | the `allow` of `Table::build` |
| the state directory (`warp.json`, `warp-door.json`) | the device's private file | `IdentityFile::path`; `door_from` and `door_text` for the caller's door file |
| `--cc`, `PODNET_WARP_CC` (`ip::set_cc`, `check_cc`) | | `NetSettings::congestion_control`, from `net::parse_cc` |
| `PODNET_WARP_TCP_BURST` (`ip::check_tcp_burst`) | | `NetSettings::tcp_burst`, from `net::parse_tcp_burst` |
| the counters of the links of a process (`ip::link_stats`) | | `NetSettings::link_stats` (one `Arc<DeviceStats>` for each session), `WarpNet::link_stats` |
| `PODNET_WARP_NO_CHECK` | | `NetSettings::check = None` |
| `--resolver`, `PODNET_WARP_RESOLVERS` (for the check) | | `DataPathCheck::servers` (DNS on UDP; DNS over TLS comes with the resolvers of the proxy, T-338) |
| the budget of a relay session (`ws_relay::meter`, `StackConfig::renew`) | | `Relay::meter` (or `Way::meter`); `NetSettings::renew` for a session with no meter |
| `CARRY_MTU` (the carried device) | | `NetSettings::mtu` |

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

| Approach of T-335 | Test |
| --- | --- |
| The congestion controller (`--cc`) | `net::settings::tests::the_congestion_controller`; `tests/stack.rs`: `a_session_with_each_controller_carries_a_large_echo`; in `pod-netstack`: `each_tcp_socket_takes_the_congestion_controller_of_the_config`, `a_download_with_each_controller_crosses_a_bounded_link` |
| The TCP burst | `net::settings::tests::the_tcp_burst_variable_is_checked`, `the_stack_takes_the_burst_the_buffer_and_the_counters`; `tests/stack.rs`: `the_tcp_burst_clamps_the_window_that_the_edge_sees`; in `pod-netstack`: `the_burst_limit_clamps_the_window_that_the_stack_advertises` |
| The counters of the link | `tests/stack.rs`: `the_counters_of_the_link_count_each_session`; in `pod-netstack`: `stacks_with_shared_counters_add_to_one_count`, `shared_counters_take_the_inbound_drops_of_each_link` |
| The check of the data path, and the door that carried nothing | `check::tests::the_servers_of_the_check_follow_the_families_of_the_device`; `tests/stack.rs`: `the_check_of_the_data_path_needs_an_answer`, `a_door_that_carries_nothing_goes_last`, `a_lost_check_question_is_sent_again`, `no_check_takes_a_session_that_carries_nothing` |
| The budget of a relay | `tests/stack.rs`: `the_budget_of_a_relay_says_when_to_renew` |
| The flows of a closed session fail at once (T-394) | `tests/net.rs`: `the_flows_of_a_session_that_the_edge_ends_fail_at_once` (`Edge::end_sessions`), `a_close_by_the_caller_fails_the_flows_of_the_session`; in `pod-netstack`: the tests of `reset.rs` (a panic of the reactor) |

| Approach of T-336 | Test |
| --- | --- |
| The direct path wins when it is fast | `tests/leg.rs`: `the_direct_path_wins_when_it_is_fast` |
| The relay wins when the direct path hangs; the loser is dropped; the budget goes to `WarpNet` | `tests/leg.rs`: `the_relay_wins_when_the_direct_path_hangs` |
| The relay goes first when the leg says so; a direct path that works takes the first place back | `tests/leg.rs`: `the_relay_goes_first_when_the_leg_says_so` |
| A failure of both legs names both causes; the relay starts at once after a failure of the direct path | `tests/leg.rs`: `a_failure_of_both_legs_names_both_causes` |
| The text of the leg file | `leg::tests`: `the_leg_file_puts_a_recent_relay_first`, `the_leg_text_reads_back`, `a_change_of_the_order_is_kept_for_the_leg_file` |

The stand-in edge answers a DNS question on UDP port 53 too, notes the
largest TCP window of the client and drops the first questions that a test
names (`Watch` of `tests/common/edge.rs`); `Seen::silent` of
`pod_testkit::masque` makes a front door that takes the handshake and
carries nothing. Plants: `warp-check-without-answer`,
`warp-check-door-kept`, `warp-burst-not-passed`,
`netstack-controller-not-set`, `netstack-burst-not-set`,
`netstack-shared-counters-lose-the-drops`, `warp-close-resets-nothing`,
`warp-race-relay-waits-for-the-direct-path`.

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
| The relay leg and the race, the leg file | yes | | yes (the relay is a dial of the caller) |
| The stack (congestion control, burst, counters, the check of the data path) | yes | smoltcp of its own | yes (`NetSettings` over the stack of `pod-netstack`; the check in `Warp::connect`) |
| The TCP timeout and keepalive, no Nagle (podnet issue 125, podssh) | yes | no Nagle | T-393 |
| The streams of a closed session fail at once; a close by the caller (podnet issues 125, 185) | yes | | T-394 |
| UDP to a family that the device lacks is refused (podnet issue 112) | yes | | T-395 |
| The budget of a relay session | yes | | yes (`Relay::meter`, `MasqueSession::relay_budget`, `WarpNet::renew_due`) |
| Names inside the tunnel | through the proxy's resolver | yes | yes |
| UDP | yes | | yes (`Datagrams`) |
| The rules with the lint of shadowed rules | yes | | yes (the lint is the caller's) |
| The proxy, the forward, nc, the L4 door | yes | | T-338 |
| `Live`, TUN, routes | yes | | T-339 |
| A tunnel that takes TLS and relays nothing (the probe of podssh, PodUtils/podssh@T-560) | the check of the data path | yes | yes (the check of the data path) |

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
| `warp::ip::{parse_cc, set_cc, check_cc}`, `CC_VARIABLE` | `net::parse_cc` into `NetSettings::congestion_control`; podnet keeps the flag and the variable |
| `warp::ip::check_tcp_burst`, `TCP_BURST_VARIABLE` | `net::parse_tcp_burst` into `NetSettings::tcp_burst` |
| `warp::ip::link_stats()` (a global) | one `Arc<DeviceStats>` of podnet in `NetSettings::link_stats`; its `snapshot()` |
| `WarpStack::renew_due`, `StackConfig::renew` | `WarpNet::renew_due`, `NetSettings::renew = Some(Arc::new(move \|\| budget.spent()))` |
| `WarpClient::relay()`, `open_session` (the race), `RELAY_DELAY`, `RELAY_DELAY_KNOWN` | `Settings::relay`, `Warp::open_session`, `Relay::delay`, `Relay::delay_known` |
| `ws_relay::meter(io)`, `MasqueSession::relay_budget` | `Relay::meter = Some(Arc::new(\|io\| { let (io, b) = RelayBudget::meter_for(&ctx, io); (Box::new(io), Arc::new(move \|\| b.spent())) }))`; `MasqueSession::relay_budget` |
| `read_leg`, `write_leg`, `relay_first_from`, `LEG_TTL` | `leg::relay_first_from`, `Warp::with_relay_first`, `Warp::take_leg_change`, `leg::leg_text`, `leg::LEG_TTL`; the file stays podnet's |
| `decisions::count("masque", "direct" \| "relay" \| "relay_first" \| "no_relay")` | `Observer::decision("warp.leg", "direct" \| "relay" \| "relay-first" \| "no-relay")` |
| `WarpStack::close_session(why)`, `Driver::fail_all`, `supervise` | `WarpNet::close(why)`; `NetStack::reset_all` at the first cause; the watch of `NetStack::stopped()` |
| `proxy::check_data_path(&stack, ipv4)` | `WarpNet::check_data_path(&DataPathCheck)`; `Warp::connect` runs it and marks the door bad |
| `proxy::NO_CHECK_VARIABLE` | `NetSettings::check = None` |
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
